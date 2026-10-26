"""Creates a ready-to-use developer (with a paid lifetime licence, an app and an
item) and a separate platform admin in the configured database, for local
demos. Prints the values the SDK needs.

The two are kept apart on purpose: developers never see the admin console.
For production, create your own admin with `python app.py create-admin EMAIL`.

    python dev_seed.py
"""
import secrets
import time

from sqlalchemy import insert, select, update
from werkzeug.security import generate_password_hash

import db
from app import new_id, sha256

engine = db.make_engine()
t = int(time.time())
with engine.begin() as c:
    dev = c.execute(select(db.developers).where(db.developers.c.email == "demo@pillkit.dev")).mappings().first()
    if dev:
        a = c.execute(select(db.apps).where(db.apps.c.developer_id == dev["id"])).mappings().first()
    else:
        dev_id, app_id = new_id("dev"), new_id("app")
        c.execute(insert(db.developers).values(
            id=dev_id, email="demo@pillkit.dev", password_hash=generate_password_hash("demo-password"), name="Demo Developer",
            company="Acme", website="https://acme.example/", country="", use_case="Local demo account", status="approved",
            is_admin=False, review_note="", created_at=t, reviewed_at=t))
        c.execute(insert(db.licenses).values(
            id=new_id("lic"), developer_id=dev_id, plan="lifetime", status="active", max_apps=3, max_items=20,
            amount="0", currency="USD", invoice_ref="DEMO", paid_at=t, created_at=t))
        c.execute(insert(db.apps).values(
            id=app_id, developer_id=dev_id, name="Acme Clip", bundle_id="", publishable_key="pk_" + secrets.token_urlsafe(24),
            secret_key_hash=sha256("sk_" + secrets.token_urlsafe(32)), allowed_url_prefixes="https://acme.example/\nhttps://pillkit.dev/",
            status="active", created_at=t))
        c.execute(insert(db.items).values(
            id=new_id("itm"), app_id=app_id, kind="sponsored", title="Acme Cloud", subtitle="Deploy in 30s. Try free.",
            label="Ad", background="#E8F04A", foreground="#2A2C05", action_type="open_url",
            action_value="https://acme.example/pricing", card_body="Deploy any repo in 30 seconds. First month free.",
            weight=1, duration_secs=20, active=True, created_at=t))
        a = c.execute(select(db.apps).where(db.apps.c.id == app_id)).mappings().first()
    # A plain developer, never an admin.
    c.execute(update(db.developers).where(db.developers.c.email == "demo@pillkit.dev").values(is_admin=False))
    if not c.execute(select(db.developers.c.id).where(db.developers.c.email == "admin@pillkit.dev")).first():
        c.execute(insert(db.developers).values(
            id=new_id("dev"), email="admin@pillkit.dev", password_hash=generate_password_hash("admin-password"), name="Platform Admin",
            company="pillkit", website="", country="", use_case="Platform administrator", status="approved",
            is_admin=True, review_note="", created_at=t, reviewed_at=t))
print("developer:  demo@pillkit.dev / demo-password")
print("admin:      admin@pillkit.dev / admin-password  (local only)")
print(f"APP_ID={a['id']}")
print(f"KEY={a['publishable_key']}")
