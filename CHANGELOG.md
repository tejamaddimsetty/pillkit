# Changelog

All notable changes to this project. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
semantic versioning.

## [0.2.0] - 2026-10-27

### Changed
- **Renamed to Pill Kit** (`pillkit` in code): crates `pillkit-core` and
  `pillkit-host`, SDK `@pillkit/sdk` (class `PillKit`), environment variables
  `PILLKIT_*`, data folder `pillkit`, signed envelopes `pillkit1.`.

### Added
- `pillkit-preview` download: double-click to see sample pills in the menu
  bar or taskbar, with no server, licence or setup.
- One-line Windows setup: `irm .../scripts/get.ps1 | iex`, with Start / Stop
  shortcuts on the Desktop.

### Fixed
- Windows: the host crashed right after placing the pill. Windows sends our
  window messages during SetParent / SetWindowPos while we hold its state;
  those nested messages now skip instead of re-borrowing (which panicked and
  aborted the process).
- CI's Windows smoke test now also requires the pill to be placed and the
  host to stay alive.

## [0.1.4] - 2026-10-27

### Fixed
- Windows: apps couldn't register ("pillkit-host closed the connection"). The
  named pipe used synchronous I/O, where Windows serialises a pending read
  and a reply on the same handle. It now uses overlapped I/O.
- The host logs crashes and dropped connections to host.log.

### Added
- CI runs the real host on Windows and registers an app against a local
  server (scripts/smoke_test.py).

## [0.1.3] - 2026-10-27

### Fixed
- Host: a failed first licence request was retried only after 10 minutes,
  so the pill could stay missing. It now retries after 15 s, 30 s, 1 min...
  (up to 10 min), and immediately when an app connects.
- Local setup talks to 127.0.0.1 instead of "localhost", which Windows
  resolves to IPv6 first while the dev server listens on IPv4 only.
- host.log records each step: app registered, licence requested, result.

## [0.1.2] - 2026-10-27

### Fixed
- Windows: the pill never appeared. The host now ships a Windows 8+ manifest
  (required for a see-through window inside the taskbar), and when embedding
  isn't possible it falls back to a real top-most overlay instead of an
  invisible window, then to the tray.
- Windows: the log now records where the pill was placed and why a mode failed.
- Installers fetch a newer host automatically when a new release exists.
- Server dependencies install on any Python 3.10+ without a compiler.

## [0.1.1] - 2026-10-26

### Added
- One-step local setup: double-click `install.cmd` (Windows) or run
  `./install.sh` (macOS / Linux). Installs Python on Windows if needed,
  downloads the host from the latest release, starts everything and opens
  the portal. `stop.cmd` / `./stop.sh` to stop.
- Release workflow: tagged versions publish host binaries to GitHub Releases.
- `scripts/demo_app.py`: a dependency-free demo app (no Node needed).

### Fixed
- macOS: the pill no longer shrinks on a false "no room" signal; it now only
  steps down when it's really pushed off screen or under the notch.
- Windows: release builds no longer open a console window at login.

## [0.1.0] - 2026-10-26

First public release. `pillkit` is a working codename.

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
- `@pillkit/sdk` for Node and Electron.
