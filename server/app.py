"""pillkit server: developer portal, admin console and the host API (port 5050).

    python app.py                       run the server
    python app.py create-admin EMAIL    make (or promote) an admin account
"""
import base64
import binascii
import getpass
import hashlib
import hmac
import json
import logging
import os
import re
import secrets
import sys
import time
from datetime import datetime, timezone
from functools import wraps

from dotenv import load_dotenv
from flask import Flask, abort, flash, g, jsonify, redirect, render_template, request, session, url_for
from sqlalchemy import func, insert, select, update, delete
from werkzeug.security import check_password_hash, generate_password_hash

import db
from signing import Signer

load_dotenv()
logging.basicConfig(level=logging.INFO, format="%(asctime)s [pillkit] %(message)s", datefmt="%H:%M:%S")
log = logging.getLogger("pillkit")

LICENSE_TTL = 30 * 86400
FEED_TTL = 3600
FEED_POLL = int(os.environ.get("FEED_POLL_SECS", 300))
ICON_PX = 128
BANNED_SCHEMES = ("file:", "javascript:", "data:", "vbscript:", "ms-msdt:", "search-ms:")
ACTION_TYPES = ("open_url", "deep_link", "callback", "card", "none")
HEX = re.compile(r"^#[0-9A-Fa-f]{6}$")
CALLBACK = re.compile(r"^[A-Za-z0-9_.-]{1,64}$")
EMAIL = re.compile(r"^[^@\s]+@[^@\s]+\.[^@\s]+$")


def now():
    return int(time.time())


def new_id(prefix, n=12):
    return f"{prefix}_{secrets.token_hex(n // 2 + 1)[:n]}"


def sha256(s):
    return hashlib.sha256(s.encode()).hexdigest()


def url_allowed(url, prefixes):
    """Same rule the host applies (pillkit-core policy::url_allowed)."""
    url = (url or "").strip()
    if not url or any(ord(c) < 32 for c in url):
        return False
    low = url.lower()
    if low.startswith(BANNED_SCHEMES):
        return False
    return any(p and low.startswith(p.lower()) for p in prefixes)


def normalize_url(url):
    """Accept links the way people type them: "stacks.ltd" and
    "http://stacks.ltd" become "https://stacks.ltd/" (the host only opens
    secure links). Custom app links ("myapp://page") are left as they are."""
    url = (url or "").strip()
    if not url:
        return url
    if "://" not in url and re.match(r"^[\w-]+(\.[\w-]+)+([/?#].*)?$", url):
        url = "https://" + url
    if re.match(r"^http://", url, re.I):
        url = "https://" + url[7:]
    if re.match(r"^https://[^/?#\s]+$", url, re.I):
        url += "/"
    return url


def suggest_prefix(url):
    """The allowlist entry that would cover `url`: https://host/ or scheme://."""
    m = re.match(r"^(https)://([^/?#\s]+)", (url or "").strip(), re.I)
    if m:
        return f"{m.group(1).lower()}://{m.group(2).lower()}/"
    m = re.match(r"^([a-z][a-z0-9+.-]*)://", (url or "").strip(), re.I)
    if m and not m.group(0).lower().startswith(BANNED_SCHEMES) and not m.group(1).lower() == "http":
        return f"{m.group(1).lower()}://"
    return None


def icon_from_upload(file):
    """Any PNG/JPEG/WebP/GIF upload -> square 128 px PNG (base64), transparent
    padding, under the 64 KB item limit. Returns (b64, error)."""
    from io import BytesIO

    from PIL import Image, UnidentifiedImageError

    data = file.read(5 * 1024 * 1024 + 1)
    if len(data) > 5 * 1024 * 1024:
        return None, "Icon must be under 5 MB"
    try:
        img = Image.open(BytesIO(data))
        img.load()
    except (UnidentifiedImageError, OSError):
        return None, "Icon must be a PNG, JPEG, WebP or GIF image"
    img = img.convert("RGBA")
    img.thumbnail((ICON_PX, ICON_PX), Image.LANCZOS)
    canvas = Image.new("RGBA", (ICON_PX, ICON_PX), (0, 0, 0, 0))
    canvas.paste(img, ((ICON_PX - img.width) // 2, (ICON_PX - img.height) // 2), img)
    out = BytesIO()
    canvas.save(out, "PNG", optimize=True)
    return base64.b64encode(out.getvalue()).decode(), None


def parse_prefixes(text):
    out = []
    for line in (text or "").splitlines():
        p = line.strip()
        if not p:
            continue
        low = p.lower()
        if low.startswith(BANNED_SCHEMES) or "://" not in p:
            raise ValueError(f"'{p}' is not a URL prefix (use https://site.com/ or yourapp://)")
        if low.startswith("http://"):
            raise ValueError(f"'{p}' must use https://")
        if low.startswith("https://") and p.count("/") < 3:
            # https://acme.com would also match https://acme.com.evil.io
            raise ValueError(f"'{p}' needs a trailing slash: {p}/")
        out.append(p)
    return out


def validate_item(data, prefixes):
    """Returns (row, error). `data` is a form or JSON dict."""
    get = lambda k, d="": (data.get(k) if data.get(k) is not None else d)
    kind = str(get("kind", "sponsored"))
    row = {
        "kind": kind,
        "title": str(get("title")).strip(),
        "subtitle": str(get("subtitle")).strip(),
        "label": str(get("label")).strip(),
        "background": str(get("background", "#E8F04A")).strip(),
        "foreground": str(get("foreground", "#2A2C05")).strip(),
        "action_type": str(get("action_type", "open_url")),
        "action_value": str(get("action_value")).strip(),
        "card_body": str(get("card_body")).strip(),
        "icon_png": (str(get("icon_png")).strip() or None),
    }
    if kind not in ("sponsored", "status"):
        return None, "kind must be sponsored or status"
    if not 1 <= len(row["title"]) <= 40:
        return None, "Title is required (up to 40 characters)"
    if len(row["subtitle"]) > 60:
        return None, "Subtitle can be up to 60 characters"
    if kind == "sponsored" and not row["label"]:
        row["label"] = "Sponsored"
    if len(row["label"]) > 16:
        return None, "Label can be up to 16 characters"
    if not HEX.match(row["background"]) or not HEX.match(row["foreground"]):
        return None, "Colours must be hex like #E8F04A"
    if row["action_type"] not in ACTION_TYPES:
        return None, f"action_type must be one of {', '.join(ACTION_TYPES)}"
    if row["action_type"] in ("open_url", "deep_link"):
        row["action_value"] = normalize_url(row["action_value"])
        if not url_allowed(row["action_value"], prefixes):
            if row["action_value"].lower().startswith("http://"):
                return None, "Links must use https://"
            allowed = ", ".join(prefixes) or "none yet"
            return None, (f"This app can only open URLs you've allowed. Add "
                          f"{suggest_prefix(row['action_value']) or 'its address'} to Allowed URL prefixes "
                          f"in Settings (allowed now: {allowed}).")
    elif row["action_type"] == "callback":
        if not CALLBACK.match(row["action_value"]):
            return None, "Callback names use letters, numbers, dot, dash and underscore"
    else:
        row["action_value"] = ""
    if len(row["card_body"]) > 280:
        return None, "Card text can be up to 280 characters"
    if row["icon_png"]:
        try:
            raw = base64.b64decode(row["icon_png"], validate=True)
        except (binascii.Error, ValueError):
            return None, "icon_png must be base64"
        if len(raw) > 64 * 1024 or not raw.startswith(b"\x89PNG\r\n\x1a\n"):
            return None, "icon_png must be a PNG up to 64 KB"
    try:
        row["weight"] = max(1, min(100, int(get("weight", 1) or 1)))
        row["duration_secs"] = max(5, min(300, int(get("duration_secs", 20) or 20)))
        row["starts_at"] = int(get("starts_at")) if str(get("starts_at")).strip() else None
        row["ends_at"] = int(get("ends_at")) if str(get("ends_at")).strip() else None
    except (TypeError, ValueError):
        return None, "weight, duration_secs, starts_at and ends_at must be numbers"
    return row, None


def item_to_wire(r):
    action = {"type": r["action_type"]}
    if r["action_type"] in ("open_url", "deep_link"):
        action["url"] = r["action_value"]
    elif r["action_type"] == "callback":
        action["name"] = r["action_value"]
    out = {
        "id": r["id"], "title": r["title"], "subtitle": r["subtitle"], "label": r["label"],
        "icon": {"type": "png", "data": r["icon_png"]} if r["icon_png"] else {"type": "monogram"},
        "theme": {"background": r["background"], "foreground": r["foreground"]},
        "action": action, "weight": r["weight"], "duration_secs": r["duration_secs"],
        "starts_at": r["starts_at"], "ends_at": r["ends_at"],
    }
    if r["card_body"]:
        out["card"] = {"body": r["card_body"], "buttons": []}
    return out


def create_app(config=None):
    app = Flask(__name__)
    cfg = {
        "DATABASE_URL": os.environ.get("DATABASE_URL"),
        "SECRET_KEY": os.environ.get("SECRET_KEY") or secrets.token_hex(32),
        "SIGNING_KEY_PATH": os.environ.get("SIGNING_KEY_PATH", "storage/signing.key"),
        "PUBLIC_BASE_URL": os.environ.get("PUBLIC_BASE_URL", "http://localhost:5050"),
        "LIFETIME_PRICE": os.environ.get("LIFETIME_PRICE", ""),
    }
    cfg.update(config or {})
    app.config.update(cfg, SESSION_COOKIE_HTTPONLY=True, SESSION_COOKIE_SAMESITE="Lax")
    if not os.environ.get("SECRET_KEY") and not (config or {}).get("SECRET_KEY"):
        log.warning("SECRET_KEY not set: sessions reset on every restart")
    engine = db.make_engine(app.config["DATABASE_URL"])
    signer = Signer.load_or_create(app.config["SIGNING_KEY_PATH"])
    app.extensions["engine"] = engine
    app.extensions["signer"] = signer
    log.info("public key: %s", signer.public_key_b64)

    def q(stmt):
        with engine.connect() as c:
            return c.execute(stmt)

    def one(stmt):
        with engine.connect() as c:
            row = c.execute(stmt).mappings().first()
            return dict(row) if row else None

    def many(stmt):
        with engine.connect() as c:
            return [dict(r) for r in c.execute(stmt).mappings().all()]

    def write(*stmts):
        with engine.begin() as c:
            for s in stmts:
                c.execute(s)

    def audit(action, target="", **detail):
        actor = g.user["id"] if getattr(g, "user", None) else "api"
        write(insert(db.audit).values(actor=actor, action=action, target=target, detail=json.dumps(detail), at=now()))

    def license_for(dev_id):
        return one(select(db.licenses).where(db.licenses.c.developer_id == dev_id).order_by(db.licenses.c.created_at.desc()))

    # ---------- session, CSRF, auth ----------

    @app.before_request
    def load_user():
        g.user = None
        uid = session.get("uid")
        if uid:
            g.user = one(select(db.developers).where(db.developers.c.id == uid))
        if "csrf" not in session:
            session["csrf"] = secrets.token_urlsafe(24)
        if request.method == "POST" and not request.path.startswith("/v1/"):
            sent = request.form.get("csrf", "")
            if not hmac.compare_digest(sent, session["csrf"]):
                abort(400, "Form expired. Go back, reload and try again.")

    @app.context_processor
    def inject():
        my_apps = []
        if g.user and not g.user["is_admin"]:
            my_apps = many(select(db.apps.c.id, db.apps.c.name).where(db.apps.c.developer_id == g.user["id"]).order_by(db.apps.c.created_at))
        # Changes whenever style.css does, so browsers never keep a stale copy.
        css_v = int(os.path.getmtime(os.path.join(app.static_folder, "style.css")))
        return {"user": g.user, "csrf": session.get("csrf", ""), "price": app.config["LIFETIME_PRICE"],
                "my_apps": my_apps, "css_v": css_v}

    def login_required(fn):
        @wraps(fn)
        def wrapper(*a, **kw):
            if not g.user:
                return redirect(url_for("login", next=request.path))
            return fn(*a, **kw)
        return wrapper

    def admin_required(fn):
        @wraps(fn)
        def wrapper(*a, **kw):
            if not g.user or not g.user["is_admin"]:
                abort(404)
            return fn(*a, **kw)
        return wrapper

    def own_app(app_id):
        a = one(select(db.apps).where(db.apps.c.id == app_id))
        if not a or (a["developer_id"] != g.user["id"] and not g.user["is_admin"]):
            abort(404)
        return a

    # ---------- public pages ----------

    @app.get("/")
    def index():
        return render_template("index.html")

    @app.route("/apply", methods=["GET", "POST"])
    def apply():
        if request.method == "GET":
            return render_template("apply.html", form={})
        f = request.form
        form = {k: f.get(k, "").strip() for k in ("name", "email", "company", "website", "country", "use_case")}
        err = None
        if not form["name"] or not EMAIL.match(form["email"]):
            err = "Enter your name and a valid email"
        elif len(f.get("password", "")) < 10:
            err = "Use a password of at least 10 characters"
        elif len(form["use_case"]) < 20:
            err = "Tell us a bit more about how you'll use pillkit (20+ characters)"
        elif one(select(db.developers.c.id).where(db.developers.c.email == form["email"].lower())):
            err = "An account with that email already exists. Sign in instead."
        if err:
            flash(err, "err")
            return render_template("apply.html", form=form), 400
        dev_id = new_id("dev")
        write(insert(db.developers).values(
            id=dev_id, email=form["email"].lower(), password_hash=generate_password_hash(f["password"]),
            name=form["name"], company=form["company"], website=form["website"], country=form["country"],
            use_case=form["use_case"], status="pending", is_admin=False, review_note="", created_at=now(),
        ))
        session.clear()
        session["uid"] = dev_id
        log.info("new application %s (%s)", dev_id, form["email"])
        return redirect(url_for("dashboard"))

    @app.route("/login", methods=["GET", "POST"])
    def login():
        if request.method == "GET":
            return render_template("login.html")
        dev = one(select(db.developers).where(db.developers.c.email == request.form.get("email", "").strip().lower()))
        if not dev or not check_password_hash(dev["password_hash"], request.form.get("password", "")):
            flash("That email and password don't match", "err")
            return render_template("login.html"), 401
        session.clear()
        session["uid"] = dev["id"]
        nxt = request.args.get("next", "")
        home = url_for("admin") if dev["is_admin"] else url_for("dashboard")
        return redirect(nxt if nxt.startswith("/") and not nxt.startswith("//") else home)

    @app.post("/logout")
    def logout():
        session.clear()
        return redirect(url_for("index"))

    @app.get("/docs")
    def docs():
        return render_template("docs.html")

    @app.get("/why")
    def why():
        return render_template("why.html")

    # ---------- developer portal ----------

    @app.get("/dashboard")
    @login_required
    def dashboard():
        lic = license_for(g.user["id"])
        my_apps = many(select(db.apps).where(db.apps.c.developer_id == g.user["id"]).order_by(db.apps.c.created_at))
        for a in my_apps:
            rows = many(select(db.items).where(db.items.c.app_id == a["id"]).order_by(db.items.c.created_at.desc()))
            live = [r for r in rows if item_status(r) == "live"]
            _, total, _ = series(a["id"])
            a.update(items=len(rows), live=len(live), top=live[0] if live else (rows[0] if rows else None), stats=total)
            # The one thing to do next, so a new developer is never stuck.
            if not rows:
                a["next"] = ("Add your first item", url_for("new_item", app_id=a["id"]))
            elif not total["imp_total"]:
                a["next"] = ("Add the SDK to your app", url_for("app_detail", app_id=a["id"], tab="integrate"))
            elif not live:
                a["next"] = ("Nothing is live. Turn an item on", url_for("app_detail", app_id=a["id"]))
            else:
                a["next"] = None
        pending = 0
        if g.user["is_admin"]:
            pending = q(select(func.count()).select_from(db.developers).where(db.developers.c.status == "pending")).scalar()
        return render_template("dashboard.html", lic=lic, apps=my_apps, pending=pending)

    @app.post("/apps")
    @login_required
    def create_app_route():
        lic = license_for(g.user["id"])
        if g.user["status"] != "approved" or not lic or lic["status"] == "revoked":
            abort(403)
        count = q(select(func.count()).select_from(db.apps).where(db.apps.c.developer_id == g.user["id"])).scalar()
        if count >= lic["max_apps"]:
            flash(f"Your licence covers {lic['max_apps']} apps", "err")
            return redirect(url_for("dashboard"))
        name = request.form.get("name", "").strip()[:120]
        if not name:
            flash("Give the app a name", "err")
            return redirect(url_for("dashboard"))
        app_id, secret = new_id("app"), "sk_" + secrets.token_urlsafe(32)
        write(insert(db.apps).values(
            id=app_id, developer_id=g.user["id"], name=name, bundle_id=request.form.get("bundle_id", "").strip()[:160],
            publishable_key="pk_" + secrets.token_urlsafe(24), secret_key_hash=sha256(secret),
            allowed_url_prefixes="", status="active", created_at=now(),
        ))
        audit("app.create", app_id, name=name)
        session["new_secret"] = {"app": app_id, "key": secret}
        return redirect(url_for("app_detail", app_id=app_id))

    # ---------- stats ----------

    def day_keys(days):
        start = now() - (days - 1) * 86400
        return [datetime.fromtimestamp(start + i * 86400, timezone.utc).strftime("%Y-%m-%d") for i in range(days)]

    def series(app_id, days=30):
        """Daily impressions/clicks for the app and for each item."""
        keys = day_keys(days)
        idx = {k: i for i, k in enumerate(keys)}
        empty = lambda: {"imp": [0] * days, "clk": [0] * days}
        total, per_item = empty(), {}
        for r in many(select(db.events.c.item_id, db.events.c.day, db.events.c.type, func.sum(db.events.c.count).label("n"))
                      .where(db.events.c.app_id == app_id, db.events.c.day >= keys[0])
                      .group_by(db.events.c.item_id, db.events.c.day, db.events.c.type)):
            i = idx.get(r["day"])
            if i is None:
                continue
            k = "imp" if r["type"] == "impression" else "clk"
            total[k][i] += int(r["n"])
            per_item.setdefault(r["item_id"], empty())[k][i] += int(r["n"])
        for s_ in [total, *per_item.values()]:
            s_["imp_total"], s_["clk_total"] = sum(s_["imp"]), sum(s_["clk"])
            s_["ctr"] = (100.0 * s_["clk_total"] / s_["imp_total"]) if s_["imp_total"] else 0.0
        return keys, total, per_item

    def item_status(r):
        t = now()
        if r["ends_at"] and r["ends_at"] <= t:
            return "ended"
        if not r["active"]:
            return "paused"
        if r["starts_at"] and r["starts_at"] > t:
            return "scheduled"
        return "live"

    # ---------- app pages ----------

    @app.get("/apps/<app_id>")
    @login_required
    def app_detail(app_id):
        a = own_app(app_id)
        tab = request.args.get("tab", "items")
        if tab not in ("items", "integrate", "settings"):
            tab = "items"
        rows = many(select(db.items).where(db.items.c.app_id == app_id).order_by(db.items.c.created_at.desc()))
        for r in rows:
            r["status"] = item_status(r)
        keys, total, per_item = series(app_id)
        secret = session.pop("new_secret", None)
        secret = secret["key"] if secret and secret["app"] == app_id else None
        if secret:
            tab = "settings"
        empty = {"imp": [0] * len(keys), "clk": [0] * len(keys), "imp_total": 0, "clk_total": 0, "ctr": 0.0}
        return render_template("app.html", a=a, tab=tab, items=rows, total=total, per_item=per_item, empty=empty,
                               days=keys, live=sum(1 for r in rows if r["status"] == "live"), secret=secret,
                               base=app.config["PUBLIC_BASE_URL"], lic=license_for(a["developer_id"]))

    @app.post("/apps/<app_id>/settings")
    @login_required
    def app_settings(app_id):
        own_app(app_id)
        back = url_for("app_detail", app_id=app_id, tab="settings")
        try:
            prefixes = parse_prefixes(request.form.get("allowed_url_prefixes", ""))
        except ValueError as e:
            flash(str(e), "err")
            return redirect(back)
        write(update(db.apps).where(db.apps.c.id == app_id).values(
            name=request.form.get("name", "").strip()[:120] or "Untitled",
            bundle_id=request.form.get("bundle_id", "").strip()[:160],
            allowed_url_prefixes="\n".join(prefixes),
        ))
        audit("app.settings", app_id, prefixes=prefixes)
        flash("Settings saved", "ok")
        return redirect(back)

    @app.post("/apps/<app_id>/rotate-secret")
    @login_required
    def rotate_secret(app_id):
        own_app(app_id)
        secret = "sk_" + secrets.token_urlsafe(32)
        write(update(db.apps).where(db.apps.c.id == app_id).values(secret_key_hash=sha256(secret)))
        audit("app.rotate_secret", app_id)
        session["new_secret"] = {"app": app_id, "key": secret}
        return redirect(url_for("app_detail", app_id=app_id, tab="settings"))

    # ---------- item editor (create and edit share one page) ----------

    def insert_item(a, data):
        lic = license_for(a["developer_id"])
        count = q(select(func.count()).select_from(db.items).where(db.items.c.app_id == a["id"])).scalar()
        if lic and count >= lic["max_items"]:
            return None, f"This app already has {lic['max_items']} items; delete one first"
        row, err = validate_item(data, a["allowed_url_prefixes"].splitlines())
        if err:
            return None, err
        row.update(id=new_id("itm"), app_id=a["id"], active=not data.get("paused"), created_at=now())
        write(insert(db.items).values(**row))
        return row, None

    def get_item(app_id, item_id):
        return one(select(db.items).where(db.items.c.id == item_id, db.items.c.app_id == app_id)) or abort(404)

    def parse_local(value, offset_min):
        """datetime-local from the browser + its UTC offset -> Unix seconds."""
        import calendar
        value = (value or "").strip()
        if not value:
            return ""
        try:
            dt = datetime.strptime(value[:16], "%Y-%m-%dT%H:%M")
            return str(calendar.timegm(dt.timetuple()) + int(offset_min or 0) * 60)
        except ValueError:
            return "bad"

    def auto_allow(a, data):
        """Adds the item's link to the app's allowed prefixes, so typing a link
        just works. Returns the prefix added, or None."""
        if data.get("action_type") not in ("open_url", "deep_link"):
            return None
        data["action_value"] = normalize_url(data.get("action_value"))
        prefix = suggest_prefix(data["action_value"])
        if not prefix:
            return None
        prefixes = a["allowed_url_prefixes"].splitlines()
        if url_allowed(data["action_value"], prefixes):
            return None
        prefixes.append(prefix)
        write(update(db.apps).where(db.apps.c.id == a["id"]).values(allowed_url_prefixes="\n".join(prefixes)))
        a["allowed_url_prefixes"] = "\n".join(prefixes)
        audit("app.allow_prefix", a["id"], prefix=prefix)
        return prefix

    def editor_form(a, existing=None):
        """Normalises the submitted editor form. Returns (data, suggest, error)."""
        f = request.form.to_dict()
        upload = request.files.get("icon")
        if f.get("icon_remove"):
            f["icon_png"] = ""
        elif upload and upload.filename:
            f["icon_png"], err = icon_from_upload(upload)
            if err:
                return f, None, err
        elif f.get("icon_keep"):
            f["icon_png"] = f["icon_keep"]
        elif existing:
            f["icon_png"] = existing["icon_png"] or ""
        for k in ("starts_at", "ends_at"):
            if f"{k}_local" in f:
                f[k] = parse_local(f.get(f"{k}_local"), f.get("tz_offset"))
        if "bad" in (f.get("starts_at"), f.get("ends_at")):
            return f, None, "Dates must look like 2026-11-01 09:00"
        if f.get("starts_at") and f.get("ends_at") and int(f["ends_at"]) <= int(f["starts_at"]):
            return f, None, "The end date must be after the start date"
        added = auto_allow(a, f)
        if added:
            flash(f"Added {added} to this app's allowed links.", "ok")
        suggest = None
        return f, suggest, None

    def render_editor(a, form, item=None, suggest=None, status=200):
        stats = None
        if item:
            keys, _, per_item = series(a["id"])
            stats = per_item.get(item["id"]) or {"imp": [0] * len(keys), "clk": [0] * len(keys), "imp_total": 0, "clk_total": 0, "ctr": 0.0}
            stats["days"] = keys
        if suggest and url_allowed(form.get("action_value"), own_app(a["id"])["allowed_url_prefixes"].splitlines()):
            suggest = None
        return render_template("item.html", a=own_app(a["id"]), form=form, item=item, stats=stats, suggest=suggest,
                               status_label=item_status(item) if item else None), status

    @app.get("/apps/<app_id>/items/new")
    @login_required
    def new_item(app_id):
        a = own_app(app_id)
        src = request.args.get("from")
        form = dict(get_item(app_id, src)) if src else {}
        if src:
            form["title"] = (form["title"] + " copy")[:40]
        return render_editor(a, form)

    @app.post("/apps/<app_id>/items")
    @login_required
    def add_item(app_id):
        a = own_app(app_id)
        f, suggest, err = editor_form(a)
        row = None
        if not err:
            row, err = insert_item(own_app(app_id), f)
        if err:
            flash(err, "err")
            return render_editor(a, f, suggest=suggest, status=400)
        audit("item.create", row["id"], app=app_id)
        flash("Published. It reaches menu bars within a few minutes." if row["active"] else "Saved as paused.", "ok")
        return redirect(url_for("app_detail", app_id=app_id))

    @app.get("/apps/<app_id>/items/<item_id>")
    @login_required
    def edit_item(app_id, item_id):
        a = own_app(app_id)
        item = get_item(app_id, item_id)
        return render_editor(a, dict(item), item=item)

    @app.post("/apps/<app_id>/items/<item_id>")
    @login_required
    def update_item(app_id, item_id):
        a = own_app(app_id)
        item = get_item(app_id, item_id)
        f, suggest, err = editor_form(a, existing=item)
        row = None
        if not err:
            row, err = validate_item(f, own_app(app_id)["allowed_url_prefixes"].splitlines())
        if err:
            flash(err, "err")
            return render_editor(a, f, item=item, suggest=suggest, status=400)
        write(update(db.items).where(db.items.c.id == item_id).values(**row))
        audit("item.update", item_id, app=app_id)
        flash("Changes saved. Menu bars update within a few minutes.", "ok")
        return redirect(url_for("app_detail", app_id=app_id))

    @app.post("/apps/<app_id>/items/<item_id>/<op>")
    @login_required
    def item_op(app_id, item_id, op):
        own_app(app_id)
        get_item(app_id, item_id)
        where = (db.items.c.id == item_id) & (db.items.c.app_id == app_id)
        if op == "toggle":
            write(update(db.items).where(where).values(active=~db.items.c.active))
        elif op == "delete":
            write(delete(db.items).where(where))
            flash("Item deleted", "ok")
        else:
            abort(404)
        audit(f"item.{op}", item_id, app=app_id)
        return redirect(url_for("app_detail", app_id=app_id))

    # ---------- admin console ----------

    @app.get("/admin")
    @admin_required
    def admin():
        devs = many(select(db.developers).order_by(db.developers.c.created_at.desc()))
        lics = {l["developer_id"]: l for l in many(select(db.licenses).order_by(db.licenses.c.created_at))}
        counts = {r["developer_id"]: r["n"] for r in many(
            select(db.apps.c.developer_id, func.count().label("n")).group_by(db.apps.c.developer_id))}
        recent = many(select(db.audit).order_by(db.audit.c.at.desc()).limit(30))
        return render_template("admin.html", devs=devs, lics=lics, counts=counts, recent=recent)

    @app.post("/admin/developers/<dev_id>/<op>")
    @admin_required
    def admin_dev(dev_id, op):
        dev = one(select(db.developers).where(db.developers.c.id == dev_id)) or abort(404)
        note = request.form.get("note", "").strip()[:500]
        status = {"approve": "approved", "reject": "rejected", "suspend": "suspended", "reinstate": "approved"}.get(op)
        if not status:
            abort(404)
        stmts = [update(db.developers).where(db.developers.c.id == dev_id).values(status=status, review_note=note, reviewed_at=now())]
        if op == "approve" and not license_for(dev_id):
            stmts.append(insert(db.licenses).values(
                id=new_id("lic"), developer_id=dev_id, plan="lifetime", status="awaiting_payment",
                max_apps=3, max_items=20, amount="", currency="USD", invoice_ref="", created_at=now()))
        write(*stmts)
        audit(f"developer.{op}", dev_id, note=note)
        flash(f"{dev['email']}: {status}", "ok")
        return redirect(url_for("admin"))

    @app.post("/admin/licenses/<lic_id>/<op>")
    @admin_required
    def admin_license(lic_id, op):
        one(select(db.licenses).where(db.licenses.c.id == lic_id)) or abort(404)
        f = request.form
        if op == "paid":
            values = dict(status="active", paid_at=now(), amount=f.get("amount", "").strip()[:32],
                          currency=(f.get("currency", "USD").strip() or "USD")[:8], invoice_ref=f.get("invoice_ref", "").strip()[:80])
        elif op == "revoke":
            values = dict(status="revoked")
        elif op == "limits":
            try:
                values = dict(max_apps=max(1, int(f.get("max_apps", 3))), max_items=max(1, int(f.get("max_items", 20))))
            except ValueError:
                abort(400)
        else:
            abort(404)
        write(update(db.licenses).where(db.licenses.c.id == lic_id).values(**values))
        audit(f"license.{op}", lic_id, **{k: v for k, v in values.items() if k != "paid_at"})
        flash(f"Licence {lic_id}: {op}", "ok")
        return redirect(url_for("admin"))

    # ---------- host API (docs/PROTOCOL.md) ----------

    def api_error(code, msg):
        return jsonify({"error": msg}), code

    def app_from_key():
        auth = request.headers.get("Authorization", "")
        key = auth[7:].strip() if auth.startswith("Bearer ") else ""
        app_id = (request.get_json(silent=True) or {}).get("app_id") or request.args.get("app_id", "")
        if not key.startswith("pk_") or not app_id:
            return None, api_error(401, "missing_key")
        a = one(select(db.apps).where(db.apps.c.id == app_id))
        if not a or not hmac.compare_digest(a["publishable_key"], key):
            return None, api_error(401, "bad_key")
        if a["status"] != "active":
            return None, api_error(403, "app_disabled")
        dev = one(select(db.developers).where(db.developers.c.id == a["developer_id"]))
        lic = license_for(a["developer_id"])
        if not dev or dev["status"] != "approved" or not lic or lic["status"] == "revoked":
            return None, api_error(403, "license_inactive")
        return (a, lic), None

    @app.get("/v1/public-key")
    def public_key():
        return jsonify({"alg": "Ed25519", "key": signer.public_key_b64})

    @app.post("/v1/license")
    def api_license():
        found, err = app_from_key()
        if err:
            return err
        a, lic = found
        body = request.get_json(silent=True) or {}
        if a["bundle_id"] and body.get("bundle_id", "") != a["bundle_id"]:
            return api_error(403, "bundle_id_mismatch")
        t = now()
        token = signer.seal({
            "typ": "license", "lic": lic["id"], "dev": a["developer_id"], "app_id": a["id"],
            "bundle_id": a["bundle_id"], "plan": lic["plan"],
            # Approved but unpaid: everything works, with a "Pill Kit dev" label.
            "entitlements": {"watermark": lic["status"] != "active", "max_items": lic["max_items"]},
            "iat": t, "exp": t + LICENSE_TTL,
        })
        return jsonify({"token": token})

    @app.get("/v1/feed")
    def api_feed():
        found, err = app_from_key()
        if err:
            return err
        a, _ = found
        t = now()
        rows = many(select(db.items).where(db.items.c.app_id == a["id"], db.items.c.active == True)  # noqa: E712
                    .order_by(db.items.c.created_at))
        live = [item_to_wire(r) for r in rows if (r["ends_at"] is None or r["ends_at"] > t)]
        token = signer.seal({
            "typ": "feed", "app_id": a["id"], "iat": t, "exp": t + FEED_TTL, "poll_secs": FEED_POLL,
            "allowed_url_prefixes": a["allowed_url_prefixes"].splitlines(), "items": live,
        })
        return jsonify({"token": token})

    @app.post("/v1/events")
    def api_events():
        found, err = app_from_key()
        if err:
            return err
        a, _ = found
        evs = (request.get_json(silent=True) or {}).get("events") or []
        day = datetime.now(timezone.utc).strftime("%Y-%m-%d")
        with engine.begin() as c:
            for e in evs[:200]:
                item_id, typ = str(e.get("item_id", ""))[:64], e.get("type")
                n = e.get("count", 1)
                if typ not in ("impression", "click") or not item_id or not isinstance(n, int) or not 0 < n <= 100000:
                    continue
                key = (db.events.c.app_id == a["id"]) & (db.events.c.item_id == item_id) & (db.events.c.day == day) & (db.events.c.type == typ)
                if c.execute(update(db.events).where(key).values(count=db.events.c.count + n)).rowcount == 0:
                    c.execute(insert(db.events).values(app_id=a["id"], item_id=item_id, day=day, type=typ, count=n))
        return jsonify({"ok": True})

    # Server-to-server API for developers' own backends (secret key).

    def app_from_secret():
        auth = request.headers.get("Authorization", "")
        key = auth[7:].strip() if auth.startswith("Bearer ") else ""
        if not key.startswith("sk_"):
            return None
        return one(select(db.apps).where(db.apps.c.secret_key_hash == sha256(key)))

    @app.get("/v1/items")
    def api_list_items():
        a = app_from_secret()
        if not a:
            return api_error(401, "bad_secret_key")
        rows = many(select(db.items).where(db.items.c.app_id == a["id"]).order_by(db.items.c.created_at))
        return jsonify({"items": [dict(item_to_wire(r), active=bool(r["active"]), kind=r["kind"]) for r in rows]})

    @app.post("/v1/items")
    def api_create_item():
        a = app_from_secret()
        if not a:
            return api_error(401, "bad_secret_key")
        data = request.get_json(silent=True) or {}
        action = data.get("action") or {}
        if isinstance(action, dict):
            data.setdefault("action_type", action.get("type", "open_url"))
            data.setdefault("action_value", action.get("url") or action.get("name") or "")
        theme = data.get("theme") or {}
        data.setdefault("background", theme.get("background", "#E8F04A"))
        data.setdefault("foreground", theme.get("foreground", "#2A2C05"))
        if isinstance(data.get("card"), dict):
            data.setdefault("card_body", data["card"].get("body", ""))
        added = auto_allow(a, data)
        row, err = insert_item(a, data)
        if err:
            return api_error(400, err)
        out = {"item": item_to_wire(row)}
        if added:
            out["allowed_prefix_added"] = added
        return jsonify(out), 201

    @app.delete("/v1/items/<item_id>")
    def api_delete_item(item_id):
        a = app_from_secret()
        if not a:
            return api_error(401, "bad_secret_key")
        n = q(delete(db.items).where(db.items.c.id == item_id, db.items.c.app_id == a["id"])).rowcount
        return (jsonify({"ok": True}), 200) if n else api_error(404, "not_found")

    @app.get("/healthz")
    def healthz():
        q(select(1))
        return jsonify({"ok": True})

    return app


def create_admin(email):
    engine = db.make_engine()
    email = email.strip().lower()
    with engine.begin() as c:
        row = c.execute(select(db.developers).where(db.developers.c.email == email)).first()
        if row:
            c.execute(update(db.developers).where(db.developers.c.email == email).values(is_admin=True, status="approved"))
            print(f"{email} is now an admin")
            return
        pw = getpass.getpass("Password (10+ characters): ")
        if len(pw) < 10:
            sys.exit("Password too short")
        c.execute(insert(db.developers).values(
            id=new_id("dev"), email=email, password_hash=generate_password_hash(pw), name="Admin", company="", website="",
            country="", use_case="pillkit administrator", status="approved", is_admin=True, review_note="", created_at=now()))
        print(f"created admin {email}")


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "create-admin":
        create_admin(sys.argv[2])
    else:
        create_app().run(host="127.0.0.1", port=int(os.environ.get("PORT", 5050)), debug=False)
