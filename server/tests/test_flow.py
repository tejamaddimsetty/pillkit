"""End-to-end developer journey against an in-memory SQLite database."""
import base64
import json
import re
import sys
from pathlib import Path

import pytest
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import app as server  # noqa: E402


def unb64(s):
    return base64.urlsafe_b64decode(s + "=" * (-len(s) % 4))


def open_envelope(token, public_key_b64):
    prefix, body, sig = token.split(".")
    assert prefix == "pillkit1"
    Ed25519PublicKey.from_public_bytes(unb64(public_key_b64)).verify(unb64(sig), f"{prefix}.{body}".encode())
    return json.loads(unb64(body))


@pytest.fixture
def client(tmp_path):
    a = server.create_app({
        "DATABASE_URL": f"sqlite:///{tmp_path}/t.db", "SECRET_KEY": "test", "SIGNING_KEY_PATH": str(tmp_path / "k.pem"),
    })
    a.testing = True
    return a.test_client()


def csrf(c):
    with c.session_transaction() as s:
        return s.get("csrf") or s.setdefault("csrf", "tok")


def post(c, path, **data):
    c.get("/")
    return c.post(path, data={"csrf": csrf(c), **data})


def make_admin(c):
    engine = c.application.extensions["engine"]
    from sqlalchemy import update
    import db
    post(c, "/apply", name="Admin", email="admin@pillkit.dev", password="adminpassword1", use_case="I run the pillkit platform itself")
    with engine.begin() as conn:
        conn.execute(update(db.developers).where(db.developers.c.email == "admin@pillkit.dev").values(is_admin=True, status="approved"))


def dev_id(c, email):
    from sqlalchemy import select
    import db
    with c.application.extensions["engine"].connect() as conn:
        return conn.execute(select(db.developers.c.id).where(db.developers.c.email == email)).scalar()


def test_full_journey(client):
    c = client
    pub = c.get("/v1/public-key").get_json()["key"]

    # 1. A developer applies and lands on the dashboard in review.
    r = post(c, "/apply", name="Ada Dev", email="ada@acme.com", password="averylongpass", company="Acme",
             use_case="Show our own release news in the Acme Clip menu bar app")
    assert r.status_code == 302
    assert b"In review" in c.get("/dashboard").data
    assert post(c, "/apps", name="Clip").status_code == 403  # can't register apps before approval
    ada = dev_id(c, "ada@acme.com")
    post(c, "/logout")

    # 2. Admin approves; a lifetime licence is created, awaiting payment.
    make_admin(c)
    assert post(c, f"/admin/developers/{ada}/approve").status_code == 302
    assert b"awaiting payment" in c.get("/admin").data
    post(c, "/logout")

    # 3. Developer registers an app, sets URL prefixes and adds an item.
    c.post("/login", data={"csrf": csrf(c), "email": "ada@acme.com", "password": "averylongpass"})
    r = post(c, "/apps", name="Clip", bundle_id="com.acme.clip")
    app_id = r.headers["Location"].rsplit("/", 1)[1]
    page = c.get(f"/apps/{app_id}").data.decode()
    pk = re.search(r"pk_[A-Za-z0-9_-]+", page).group(0)
    secret = re.search(r"sk_[A-Za-z0-9_-]+", page).group(0)
    assert "sk_" not in c.get(f"/apps/{app_id}").data.decode()  # shown once only

    assert b"trailing slash" in c.post(f"/apps/{app_id}/settings", data={"csrf": csrf(c), "name": "Clip", "bundle_id": "com.acme.clip",
                                       "allowed_url_prefixes": "https://acme.com"}, follow_redirects=True).data
    post(c, f"/apps/{app_id}/settings", name="Clip", bundle_id="com.acme.clip", allowed_url_prefixes="https://acme.com/\nacmeclip://")
    r = c.post(f"/apps/{app_id}/items", data={"csrf": csrf(c), "title": "Acme Cloud", "subtitle": "Try free",
               "action_type": "open_url", "action_value": "https://evil.io/"}, follow_redirects=True)
    assert r.status_code == 400 and b"https://evil.io/" in r.data and b"Acme Cloud" in r.data  # names the fix, keeps the form
    post(c, f"/apps/{app_id}/items", title="Acme Cloud", subtitle="Deploy in 30s", action_type="open_url",
         action_value="https://acme.com/pricing", background="#E8F04A", foreground="#2A2C05", weight="2", duration_secs="15")

    # 4. The host fetches a licence: works, but watermarked until paid.
    auth = {"Authorization": f"Bearer {pk}"}
    body = {"app_id": app_id, "bundle_id": "com.acme.clip", "platform": "macos"}
    assert c.post("/v1/license", json=dict(body, bundle_id="com.other"), headers=auth).status_code == 403
    assert c.post("/v1/license", json=body, headers={"Authorization": "Bearer pk_wrong"}).status_code == 401
    lic = open_envelope(c.post("/v1/license", json=body, headers=auth).get_json()["token"], pub)
    assert lic["typ"] == "license" and lic["plan"] == "lifetime" and lic["entitlements"]["watermark"] is True

    # 5. Admin marks the invoice paid: watermark goes away.
    post(c, "/logout")
    c.post("/login", data={"csrf": csrf(c), "email": "admin@pillkit.dev", "password": "adminpassword1"})
    lic_id = lic["lic"]
    post(c, f"/admin/licenses/{lic_id}/paid", amount="499", currency="USD", invoice_ref="INV-001")
    lic = open_envelope(c.post("/v1/license", json=body, headers=auth).get_json()["token"], pub)
    assert lic["entitlements"]["watermark"] is False

    # 6. Signed feed carries the item and the allowlist.
    feed = open_envelope(c.get(f"/v1/feed?app_id={app_id}", headers=auth).get_json()["token"], pub)
    assert feed["allowed_url_prefixes"] == ["https://acme.com/", "acmeclip://"]
    [item] = feed["items"]
    assert item["title"] == "Acme Cloud" and item["label"] == "Sponsored" and item["weight"] == 2
    assert item["action"] == {"type": "open_url", "url": "https://acme.com/pricing"}

    # 7. Events aggregate per day.
    for _ in range(2):
        c.post("/v1/events", json={"app_id": app_id, "events": [{"item_id": item["id"], "type": "impression", "count": 3}]}, headers=auth)
    from sqlalchemy import select
    import db
    with c.application.extensions["engine"].connect() as conn:
        assert conn.execute(select(db.events.c.count).where(db.events.c.item_id == item["id"])).scalar() == 6

    # 8. Secret-key API for the developer's backend.
    sauth = {"Authorization": f"Bearer {secret}"}
    r = c.post("/v1/items", json={"kind": "status", "title": "v2.1 is out", "action": {"type": "deep_link", "url": "acmeclip://changelog"}}, headers=sauth)
    assert r.status_code == 201, r.get_json()
    assert len(c.get("/v1/items", headers=sauth).get_json()["items"]) == 2
    assert c.get("/v1/items", headers=auth).status_code == 401  # publishable key can't manage items

    # 9. Revoking switches everything off.
    post(c, f"/admin/licenses/{lic_id}/revoke")
    assert c.post("/v1/license", json=body, headers=auth).status_code == 403
    assert c.get(f"/v1/feed?app_id={app_id}", headers=auth).status_code == 403


def test_forms_need_csrf(client):
    r = client.post("/apply", data={"name": "x", "email": "x@y.com", "password": "averylongpass", "use_case": "x" * 30})
    assert r.status_code == 400


def test_admin_hidden_from_developers(client):
    post(client, "/apply", name="Bob", email="bob@b.com", password="averylongpass", use_case="testing that admin is hidden")
    assert client.get("/admin").status_code == 404


def test_url_rules():
    assert server.url_allowed("https://acme.com/x", ["https://acme.com/"])
    assert not server.url_allowed("https://acme.com.evil.io/", ["https://acme.com/"])
    assert not server.url_allowed("javascript:alert(1)", ["javascript:"])
    with pytest.raises(ValueError):
        server.parse_prefixes("http://acme.com/")


def test_one_click_allow_domain(client):
    c = client
    post(c, "/apply", name="Ada", email="ada@a.com", password="averylongpass", use_case="testing the allow domain shortcut")
    ada = dev_id(c, "ada@a.com")
    post(c, "/logout")
    make_admin(c)
    post(c, f"/admin/developers/{ada}/approve")
    post(c, "/logout")
    c.post("/login", data={"csrf": csrf(c), "email": "ada@a.com", "password": "averylongpass"})
    app_id = post(c, "/apps", name="Clip").headers["Location"].rsplit("/", 1)[1]
    c.get(f"/apps/{app_id}")  # the landing page shows the one-time secret
    form = dict(title="Ramp", subtitle="Time is money. Save both.", action_type="open_url", action_value="https://ramp.com/x")
    r = c.post(f"/apps/{app_id}/items", data={"csrf": csrf(c), **form})
    assert r.status_code == 400 and b"https://ramp.com/" in r.data and b"Allow and save" in r.data
    r = c.post(f"/apps/{app_id}/items", data={"csrf": csrf(c), "allow_domain": "1", **form})
    assert r.status_code == 302
    assert "Ramp" in c.get(f"/apps/{app_id}").data.decode()
    assert "https://ramp.com/" in c.get(f"/apps/{app_id}?tab=settings").data.decode()


def test_suggest_prefix():
    assert server.suggest_prefix("https://Ramp.com/pricing?x=1") == "https://ramp.com/"
    assert server.suggest_prefix("acmeclip://settings") == "acmeclip://"
    assert server.suggest_prefix("javascript:alert(1)") is None
    assert server.suggest_prefix("http://insecure.com/") is None


def test_icon_upload_is_resized_to_png(client):
    import io
    from PIL import Image
    c = client
    post(c, "/apply", name="Ada", email="ada@i.com", password="averylongpass", use_case="testing icon uploads end to end")
    ada = dev_id(c, "ada@i.com")
    post(c, "/logout")
    make_admin(c)
    post(c, f"/admin/developers/{ada}/approve")
    post(c, "/logout")
    c.post("/login", data={"csrf": csrf(c), "email": "ada@i.com", "password": "averylongpass"})
    app_id = post(c, "/apps", name="Clip").headers["Location"].rsplit("/", 1)[1]
    buf = io.BytesIO()
    Image.new("RGB", (600, 300), (10, 20, 30)).save(buf, "JPEG")
    buf.seek(0)
    r = c.post(f"/apps/{app_id}/items", data={"csrf": csrf(c), "title": "Ramp", "action_type": "card",
               "icon": (buf, "logo.jpg")}, content_type="multipart/form-data")
    assert r.status_code == 302
    from sqlalchemy import select
    import db
    with c.application.extensions["engine"].connect() as conn:
        b64 = conn.execute(select(db.items.c.icon_png).where(db.items.c.app_id == app_id)).scalar()
    img = Image.open(io.BytesIO(base64.b64decode(b64)))
    assert img.format == "PNG" and img.size == (128, 128)

    r = c.post(f"/apps/{app_id}/items", data={"csrf": csrf(c), "title": "Bad", "action_type": "card",
               "icon": (io.BytesIO(b"not an image"), "x.png")}, content_type="multipart/form-data")
    assert r.status_code == 400 and b"PNG, JPEG" in r.data


def test_bare_domain_link_is_allowed():
    row, err = server.validate_item({"title": "Stacks", "action_type": "open_url", "action_value": "https://stacks.ltd"}, ["https://stacks.ltd/"])
    assert err is None and row["action_value"] == "https://stacks.ltd/"


def approved_app(c, email="ed@e.com"):
    post(c, "/apply", name="Ed", email=email, password="averylongpass", use_case="testing editing published items")
    dev = dev_id(c, email)
    post(c, "/logout")
    make_admin(c)
    post(c, f"/admin/developers/{dev}/approve")
    post(c, "/logout")
    c.post("/login", data={"csrf": csrf(c), "email": email, "password": "averylongpass"})
    app_id = post(c, "/apps", name="Clip").headers["Location"].rsplit("/", 1)[1]
    c.get(f"/apps/{app_id}")  # the landing page shows the one-time secret
    post(c, f"/apps/{app_id}/settings", name="Clip", bundle_id="", allowed_url_prefixes="https://stacks.ltd/")
    return app_id


def item_rows(c, app_id):
    from sqlalchemy import select
    import db
    with c.application.extensions["engine"].connect() as conn:
        return [dict(r) for r in conn.execute(select(db.items).where(db.items.c.app_id == app_id)).mappings()]


def test_edit_duplicate_and_stats(client):
    c = client
    app_id = approved_app(c)
    assert c.get(f"/apps/{app_id}/items/new").status_code == 200
    r = post(c, f"/apps/{app_id}/items", title="Stacks India", subtitle="Founders", action_type="open_url",
             action_value="https://stacks.ltd", background="#E8F04A", foreground="#2A2C05", kind="sponsored")
    assert r.status_code == 302
    [item] = item_rows(c, app_id)

    # Edit keeps the id, changes the content, and can schedule it.
    page = c.get(f"/apps/{app_id}/items/{item['id']}").data.decode()
    assert "Save changes" in page and "Stacks India" in page
    r = post(c, f"/apps/{app_id}/items/{item['id']}", title="Stacks India 2", subtitle="Apply now", action_type="open_url",
             action_value="https://stacks.ltd/apply", background="#111111", foreground="#FFFFFF", kind="sponsored",
             starts_at_local="2030-01-01T09:00", ends_at_local="", tz_offset="-330")
    assert r.status_code == 302
    [edited] = item_rows(c, app_id)
    assert edited["id"] == item["id"] and edited["title"] == "Stacks India 2" and edited["background"] == "#111111"
    assert edited["starts_at"] == 1893468600  # 2030-01-01 09:00 IST
    assert b"Scheduled" in c.get(f"/apps/{app_id}").data

    # End before start is rejected and the form is kept.
    r = post(c, f"/apps/{app_id}/items/{item['id']}", title="X", action_type="card", kind="status",
             starts_at_local="2030-01-02T09:00", ends_at_local="2030-01-01T09:00", tz_offset="0")
    assert r.status_code == 400 and b"after the start" in r.data

    # Duplicate pre-fills the editor.
    page = c.get(f"/apps/{app_id}/items/new?from={item['id']}").data.decode()
    assert "Stacks India 2 copy" in page and "Publish" in page

    # Save as paused.
    post(c, f"/apps/{app_id}/items", title="Draft", action_type="card", kind="status", paused="1")
    assert {r["title"]: bool(r["active"]) for r in item_rows(c, app_id)}["Draft"] is False

    # Stats show up per item.
    from sqlalchemy import select
    import db
    with c.application.extensions["engine"].connect() as conn:
        pk = conn.execute(select(db.apps.c.publishable_key).where(db.apps.c.id == app_id)).scalar()
    c.post("/v1/events", json={"app_id": app_id, "events": [{"item_id": item["id"], "type": "impression", "count": 40},
                                                          {"item_id": item["id"], "type": "click", "count": 2}]},
           headers={"Authorization": f"Bearer {pk}"})
    page = c.get(f"/apps/{app_id}").data.decode()
    assert ">40<" in page and "5.0%" in page
    assert "5.0%" in c.get(f"/apps/{app_id}/items/{item['id']}").data.decode()
    assert c.get(f"/apps/{app_id}?tab=settings").status_code == 200
    assert c.get(f"/apps/{app_id}?tab=integrate").status_code == 200


def test_developers_never_see_admin(client):
    c = client
    post(c, "/apply", name="Dee", email="dee@d.com", password="averylongpass", use_case="checking the nav has no admin link")
    page = c.get("/dashboard").data.decode()
    assert "Admin console" not in page and "Dashboard" in page
    assert c.get("/admin").status_code == 404
    post(c, "/logout")
    make_admin(c)
    post(c, "/logout")
    r = c.post("/login", data={"csrf": csrf(c), "email": "admin@pillkit.dev", "password": "adminpassword1"})
    assert r.headers["Location"].endswith("/admin")
    assert "Admin console" in c.get("/admin").data.decode()


def test_docs_page_is_public(client):
    r = client.get("/docs")
    assert r.status_code == 200 and b"Quickstart" in r.data and b"PillKit.start" in r.data
