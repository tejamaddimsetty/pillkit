"""Schema and connection. SQLite for local development, MySQL (Hostinger) in production.

Tables are created on startup with SQLAlchemy Core, so the same definitions
work on both databases. All timestamps are Unix seconds (UTC).
"""
import os
from pathlib import Path

from sqlalchemy import (
    BigInteger, Boolean, Column, Index, Integer, MetaData, String, Table, Text, UniqueConstraint, create_engine,
)

metadata = MetaData()

developers = Table(
    "developers", metadata,
    Column("id", String(32), primary_key=True),
    Column("email", String(255), nullable=False, unique=True),
    Column("password_hash", String(255), nullable=False),
    Column("name", String(120), nullable=False),
    Column("company", String(160), nullable=False, default=""),
    Column("website", String(255), nullable=False, default=""),
    Column("country", String(80), nullable=False, default=""),
    Column("use_case", Text, nullable=False),
    # pending -> approved | rejected; approved -> suspended
    Column("status", String(16), nullable=False, default="pending"),
    Column("is_admin", Boolean, nullable=False, default=False),
    Column("review_note", Text, nullable=False, default=""),
    Column("created_at", BigInteger, nullable=False),
    Column("reviewed_at", BigInteger),
)

licenses = Table(
    "licenses", metadata,
    Column("id", String(32), primary_key=True),
    Column("developer_id", String(32), nullable=False, index=True),
    Column("plan", String(32), nullable=False, default="lifetime"),
    # awaiting_payment -> active -> revoked
    Column("status", String(24), nullable=False, default="awaiting_payment"),
    Column("max_apps", Integer, nullable=False, default=3),
    Column("max_items", Integer, nullable=False, default=20),
    Column("amount", String(32), nullable=False, default=""),
    Column("currency", String(8), nullable=False, default="USD"),
    Column("invoice_ref", String(80), nullable=False, default=""),
    Column("paid_at", BigInteger),
    Column("created_at", BigInteger, nullable=False),
)

apps = Table(
    "apps", metadata,
    Column("id", String(32), primary_key=True),
    Column("developer_id", String(32), nullable=False, index=True),
    Column("name", String(120), nullable=False),
    Column("bundle_id", String(160), nullable=False, default=""),
    Column("publishable_key", String(64), nullable=False, unique=True),
    Column("secret_key_hash", String(64), nullable=False),
    # One prefix per line, e.g. https://acme.com/ or acmeclip://
    Column("allowed_url_prefixes", Text, nullable=False),
    Column("status", String(16), nullable=False, default="active"),
    Column("created_at", BigInteger, nullable=False),
)

items = Table(
    "items", metadata,
    Column("id", String(32), primary_key=True),
    Column("app_id", String(32), nullable=False, index=True),
    Column("kind", String(16), nullable=False, default="sponsored"),
    Column("title", String(40), nullable=False),
    Column("subtitle", String(60), nullable=False, default=""),
    Column("label", String(16), nullable=False, default=""),
    Column("background", String(7), nullable=False, default="#E8F04A"),
    Column("foreground", String(7), nullable=False, default="#2A2C05"),
    Column("icon_png", Text),
    Column("action_type", String(16), nullable=False, default="open_url"),
    Column("action_value", String(500), nullable=False, default=""),
    Column("card_body", String(280), nullable=False, default=""),
    Column("weight", Integer, nullable=False, default=1),
    Column("duration_secs", Integer, nullable=False, default=20),
    Column("starts_at", BigInteger),
    Column("ends_at", BigInteger),
    Column("active", Boolean, nullable=False, default=True),
    Column("created_at", BigInteger, nullable=False),
)

events = Table(
    "events", metadata,
    Column("id", Integer, primary_key=True, autoincrement=True),
    Column("app_id", String(32), nullable=False),
    Column("item_id", String(64), nullable=False),
    Column("day", String(10), nullable=False),
    Column("type", String(16), nullable=False),
    Column("count", BigInteger, nullable=False, default=0),
    UniqueConstraint("app_id", "item_id", "day", "type", name="uq_event_bucket"),
)

audit = Table(
    "audit_log", metadata,
    Column("id", Integer, primary_key=True, autoincrement=True),
    Column("actor", String(64), nullable=False),
    Column("action", String(64), nullable=False),
    Column("target", String(64), nullable=False, default=""),
    Column("detail", Text, nullable=False),
    Column("at", BigInteger, nullable=False),
)

Index("ix_audit_at", audit.c.at)


def make_engine(url=None):
    url = url or os.environ.get("DATABASE_URL", "sqlite:///storage/pillkit.db")
    if url.startswith("sqlite:///") and not url.startswith("sqlite:////"):
        # Relative SQLite paths are relative to this folder, not the shell's cwd.
        path = Path(__file__).parent / url[len("sqlite:///"):]
        path.parent.mkdir(parents=True, exist_ok=True)
        url = f"sqlite:///{path}"
    kwargs = {"future": True}
    if url.startswith("mysql"):
        # Hostinger closes idle connections; recycle and ping before use.
        kwargs.update(pool_pre_ping=True, pool_recycle=280)
    engine = create_engine(url, **kwargs)
    metadata.create_all(engine)
    return engine
