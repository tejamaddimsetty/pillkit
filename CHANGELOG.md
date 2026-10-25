# Changelog

All notable changes to this project. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
semantic versioning.

## [0.1.0] - 2026-10-26

First public release. `barkit` is a working codename.

### Host (Rust)
- macOS menu bar item: real menu bar height detection (notch-aware), 224 pt
  slot with automatic step-down when there's no room, native hover card.
- Windows taskbar: embedded, overlay and tray modes with automatic fallback,
  Explorer-restart recovery, per-monitor DPI, full-screen awareness.
  Compiles in CI; first hands-on test pending (docs/WINDOWS_TESTING.md).
- One shared host per user with fair rotation between apps, offline cache,
  login auto-start without admin rights, newest-version-wins upgrades.
- Ed25519-signed licences and feeds; URL allowlist enforced on every click.

### Server and portal (Flask)
- Apply → manual review → lifetime licence, with manual invoices and a
  "dev mode" watermark until paid. Separate admin console.
- Apps with keys, allowed URL prefixes and bundle ID.
- Item editor with live menu bar, hover card and taskbar previews; icon
  upload; scheduling; edit, duplicate, pause.
- Per-app and per-item stats (shown, clicks, click rate, 30-day charts).
- Secret-key API for managing items from a developer's own backend.
- Docs at /docs.

### SDK
- `@barkit/sdk` for Node and Electron.
