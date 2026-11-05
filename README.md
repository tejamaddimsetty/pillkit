# Pill Kit

[![npm](https://img.shields.io/npm/v/pillkit)](https://www.npmjs.com/package/pillkit)
[![license](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

**Put your desktop app's notifications next to the clock.** Pill Kit shows a
small "pill" widget in the **macOS menu bar** and the **Windows taskbar** (the
spot where Windows shows "49°F Partly cloudy"), with a hover card and a click
action. Free, open source, and local only: no server, account or API key.

```bash
npm install pillkit
```

```js
import { PillKit } from "pillkit";

const pill = await PillKit.start({ appId: "my-app" });
await pill.show({ id: "build", title: "Build passed", subtitle: "main · 2m ago", url: "https://ci.example.com" });
```

Works with **Electron**, plain **Node.js** apps and CLI tools, and anything
that can run a command (`npx pillkit show --title "Hello"`).

## How it works

```
 your app ──(pillkit SDK)──▶ pillkit-host ──▶ draws the pill in the OS bar
            local socket      (one small per-user process, Rust)
```

- **pillkit-host** owns the bar slot. Every app that uses Pill Kit shares it,
  taking turns, so several apps never fight over the taskbar. It installs per
  user, needs no admin rights, uses under 1% CPU when idle, and never covers
  your taskbar icons: it finds free space and shrinks (full → compact →
  icon only) when the taskbar is crowded.
- **The SDK** (`pillkit` on npm) starts the host and talks to it over a local
  socket. npm installs only the host binary for your computer.
- Nothing leaves the machine: no telemetry, no network calls.

## Where the pill goes

| OS | Placement | Status |
|---|---|---|
| macOS (Apple silicon and Intel) | Menu bar, with a native hover card | ✅ |
| Windows 11 / 10 | Taskbar, in the weather widget's spot (falls back to the tray when there's no room) | ✅ |
| Linux (KDE, GNOME, …) | Tray item / GNOME Shell extension | 🗓 planned |

## Using it in your app

```js
import { PillKit } from "pillkit";

const pill = await PillKit.start({ appId: "my-app", linkSchemes: ["myapp"] });

await pill.show({
  id: "deploy",
  title: "Deploy finished",
  subtitle: "production · 41s",
  body: "All 12 checks passed.",        // shown in the hover card
  icon: "./icon.png",
  url: "myapp://deploys/latest",        // https:// links work out of the box
});

await pill.show({ id: "ask", title: "Review needed", onClick: "open-review" });
pill.on("callback", (e) => openReviewWindow(e.name));
```

Full guide, including Electron packaging (`asarUnpack`), the CLI, per-OS
behaviour and troubleshooting: **[docs/INTEGRATION.md](docs/INTEGRATION.md)**.
A runnable Electron example: [examples/electron-hello](examples/electron-hello).

## Try it without code

- **Windows:** download the repo and double-click `install.cmd`
- **macOS / Linux:** `./install.sh`

It installs the host, starts it at login, and shows a demo pill. Then try
`pillkit-host show --title "Hello"`. Stop with `stop.cmd` / `./stop.sh`.

## Repository layout

```
crates/pillkit-core   model, URL policy, rotation, renderer (pure Rust, tested)
crates/pillkit-host   the host: IPC, controller, macOS + Windows bar code, CLI
sdks/node            the `pillkit` npm package (Node and Electron)
npm/                 the per-platform pillkit-host-<os>-<arch> packages
examples/            integration examples
docs/                integration guide and protocol
server/              optional portal, not used by the SDK
```

## Safety

- A pill can only open `https://` links or the URL schemes your app declares;
  `file:`, `javascript:` and similar are always blocked.
- End users can hide the pill from its menu.
- No code is injected into Explorer or any other process.
- No personal data is collected and nothing is sent over the network.

See [SECURITY.md](SECURITY.md) to report a vulnerability.

## License

Apache-2.0. See [LICENSE](LICENSE).
