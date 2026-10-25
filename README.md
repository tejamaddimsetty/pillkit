# barkit

Put your app next to the clock: the macOS menu bar, the Windows taskbar, and
(soon) Linux panels. A small, signed, labelled widget that developers control
and end users can always hide.

> `barkit` is a working codename. The product name is still to be decided.

```
 your app ──(SDK)──▶ barkit-host ──(HTTPS, signed)──▶ barkit server
                         │
                         └── draws the widget in the OS bar
```

- **barkit-host** is one small per-user process (Rust) that owns the bar slot.
  Every app that uses barkit shares it, taking turns, so five apps never fight
  over the taskbar. It installs per user, needs no admin rights, and keeps
  showing your server-managed items even when your app is closed.
- **SDKs** talk to the host over a local socket. Node/Electron is first;
  Swift, .NET, Python, Go and others follow the same small protocol.
- **The server** runs the developer portal (apply → manual review → lifetime
  licence), the admin console, and the API that hands hosts signed licences
  and feeds.

## Where the widget goes

| OS | Placement | Status |
|---|---|---|
| macOS | Menu bar status item | ✅ v0.1 |
| Windows 11 | Taskbar, the weather widget's spot (embedded → overlay → tray fallback) | ✅ v0.1, hands-on test pending: [docs/WINDOWS_TESTING.md](docs/WINDOWS_TESTING.md) |
| Windows 10 | Taskbar, left of the tray | ✅ v0.1, hands-on test pending |
| Linux (KDE, GNOME, …) | Tray item / GNOME Shell extension | 🗓 v0.3 |

## Quick start

Clone or download the repo, then:

- **Windows:** double-click `install.cmd`
- **macOS / Linux:** run `./install.sh`

It sets up the local server, gets the host (downloaded from the latest release,
or built with `cargo` if you have Rust), starts everything, opens the portal and
registers a demo app. The widget appears next to your clock within seconds.
Stop everything with `stop.cmd` / `./stop.sh`.

Logins on your local copy:

- `demo@barkit.dev` / `demo-password`: a developer, as your customers see it
- `admin@barkit.dev` / `admin-password`: the platform admin console

In production, create your own admin with `python app.py create-admin you@yourdomain.com`.
Full documentation is served by the portal at http://localhost:5050/docs.
Testing on Windows: [docs/WINDOWS_TESTING.md](docs/WINDOWS_TESTING.md).

## Using it in your app

```js
import { Barkit } from "@barkit/sdk";

const bar = await Barkit.start({ appId: "app_…", key: "pk_…", bundleId: "com.acme.clip" });
await bar.show({ id: "build", title: "Build passed", subtitle: "main · 2m ago", action: { type: "callback", name: "open-build" } });
bar.on("callback", (e) => openBuildWindow());
```

Items can open allowlisted web pages, deep-link into your app, call back into
your code, or show a card. Sponsored items are always labelled.

## Repository layout

```
crates/barkit-core   model, signature checks, URL policy, rotation, renderer (pure Rust, tested)
crates/barkit-host   the host: IPC, networking, controller, macOS + Windows bar code
server/              Flask portal, admin console and API (SQLite locally, MySQL in production)
sdks/node            @barkit/sdk for Node and Electron
docs/PROTOCOL.md     the contract between all of the above
rfcs/                design proposals
```

## Trust and safety

- Every licence and feed is signed (Ed25519); the host rejects anything else.
- Widgets can only open URLs the developer registered; `file:`, `javascript:`
  and similar are always blocked.
- Developers are reviewed by hand before they get a licence; revoking a
  licence switches their widgets off.
- End users can hide the widget from its menu, and see why it's there.
- No code is injected into Explorer or any other process.
- No personal data is collected; events are aggregate counts per item per day.

See [SECURITY.md](SECURITY.md) to report a vulnerability.

## License

Apache-2.0 for the SDKs, host and core. See [LICENSE](LICENSE).
