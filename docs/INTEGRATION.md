# Integrating Pill Kit into your app

Pill Kit shows your app's notifications as a small **pill next to the clock**:
in the macOS menu bar and the Windows taskbar. Hovering it opens a card;
clicking it opens a link, a page in your app, or runs your code.

It's free and runs entirely on the user's computer: no server, account or
API key.

- [Install](#install)
- [Electron apps](#electron-apps)
- [Node apps and CLI tools](#node-apps-and-cli-tools)
- [Shell scripts and other languages](#shell-scripts-and-other-languages)
- [Items](#items)
- [Clicks: links, deep links and callbacks](#clicks-links-deep-links-and-callbacks)
- [How it behaves on each OS](#how-it-behaves-on-each-os)
- [Shipping it](#shipping-it)
- [Troubleshooting](#troubleshooting)
- [Privacy](#privacy)

## Install

```bash
npm install pillkit
```

npm also installs the small host program for your platform
(`pillkit-host-darwin-arm64`, `-darwin-x64`, `-win32-x64` or `-linux-x64`).

> **Before the npm release:** install from the GitHub release instead. Pick
> the host file for your computer:
>
> ```bash
> V=0.3.0
> npm install \
>   https://github.com/tejamaddimsetty/pillkit/releases/download/v$V/pillkit-$V.tgz \
>   https://github.com/tejamaddimsetty/pillkit/releases/download/v$V/pillkit-host-darwin-arm64-$V.tgz
> ```
>
> Hosts: `pillkit-host-darwin-arm64` (Apple Silicon Mac), `pillkit-host-darwin-x64`
> (Intel Mac), `pillkit-host-win32-x64` (Windows), `pillkit-host-linux-x64`.

Requires Node 18+.

## Electron apps

Use Pill Kit from the **main process**:

```js
// main.mjs
import path from "node:path";
import { app } from "electron";
import { PillKit } from "pillkit";

app.whenReady().then(async () => {
  const pill = await PillKit.start({ appId: "acme-notes", linkSchemes: ["acmenotes"] });

  await pill.show({
    id: "sync",
    title: "All notes synced",
    subtitle: "Just now",
    icon: path.join(app.getAppPath(), "assets/pill-icon.png"),
    onClick: "open-sync",
  });

  pill.on("callback", (e) => {
    if (e.name === "open-sync") mainWindow.show();
  });
});
```

Packaging with electron-builder: keep the host outside the asar archive.

```json
"build": {
  "asarUnpack": ["**/node_modules/pillkit-host-*/**"]
}
```

A complete minimal app is in [examples/electron-hello](../examples/electron-hello).

## Node apps and CLI tools

Same API. For a short-lived script, call `close()` when you're done: the pill
stays up after your process exits.

```js
import { PillKit } from "pillkit";

const pill = await PillKit.start({ appId: "deploy-cli" });
await pill.show({ id: "deploy", title: "Deployed to production", subtitle: "v1.4.2 · 38 s", url: "https://dash.example.com" });
pill.close();
```

## Shell scripts and other languages

The SDK ships a `pillkit` command, and the host itself is a CLI, so any
language that can run a program can show a pill:

```bash
npx pillkit show --app backup --title "Backup done" --subtitle "312 files" --url https://example.com
npx pillkit show --app backup --id err --title "Backup failed" --bg "#B42318" --fg "#FFFFFF"
npx pillkit clear --app backup
npx pillkit status
```

| Flag | |
|---|---|
| `--app` | Your app's id (default `cli`) |
| `--id` | Item id; the same id updates the item (default `main`) |
| `--title`, `--subtitle`, `--label` | Text |
| `--bg`, `--fg` | Colours, like `#1F7A4D` |
| `--url` | `https://` page or `yourapp://` link |
| `--icon` | Path to a PNG |
| `--seconds` | Time on screen before the next item |

From Python, Go, Rust, .NET or anything else: run `pillkit-host show ...`
(found in `node_modules/pillkit-host-<os>-<arch>/bin/`), or speak the local
protocol directly ([docs/PROTOCOL.md](PROTOCOL.md) §5; there's a dependency-free
Python example in [scripts/demo_app.py](../scripts/demo_app.py)).

## Items

```js
await pill.show({
  id: "launch",                  // required; same id again = update
  title: "v2.1 is out",          // required, up to 40 characters
  subtitle: "See what's new",    // up to 60
  label: "New",                  // small tag, up to 16
  icon: "./logo.png",            // PNG path or base64; square works best
  background: "#111111",
  foreground: "#FFFFFF",
  url: "https://example.com/changelog",
  body: "Faster sync, offline mode and a new dark theme.", // card text on hover
  seconds: 20,                   // 5–300, time on screen before the next item
  weight: 1,                     // 1–100, higher shows more often
});

await pill.remove("launch");     // one item
await pill.clear();              // all of your items
await pill.status();             // { local_items, ... }
await pill.unregister();         // remove your app from the host entirely
```

Several items rotate in the slot. If several apps use Pill Kit on the same
computer, apps take turns.

Keep titles short and recognisable: on a crowded taskbar the pill shrinks to
the icon and title, or just the icon.

## Clicks: links, deep links and callbacks

| You want | Item | Notes |
|---|---|---|
| Open a web page | `url: "https://..."` | Works out of the box |
| Open a page in your app | `url: "yourapp://..."` | Declare the scheme once: `PillKit.start({ appId, linkSchemes: ["yourapp"] })` |
| Run code in your app | `onClick: "name"` | Handle `pill.on("callback", e => ...)`; your app must be running |
| Just show the card | neither | |

`file:`, `javascript:`, `data:` and plain `http:` links are refused.

## How it behaves on each OS

- **macOS:** a menu bar item sized to the menu bar. If the menu bar is full
  (especially beside the notch), the pill shrinks: compact, then icon only.
- **Windows 10/11:** inside the taskbar, only in free space (it never covers
  Start, Search or app icons), shrinking on busy taskbars and falling back to
  a tray icon only when there's no room at all.
- **Linux:** not yet; calls succeed but nothing is drawn.

The card opens on hover or right-click, with **Hide** (12 hours) and
**Why this?** for the user.

Footprint: one small host per user, about 0.1–0.6 % of one CPU core when
idle and 30–60 MB of memory.

## Shipping it

- **startAtLogin** (default `true`) installs the host in the user's data folder
  and starts it at login, so pills survive your app quitting and restarts.
  Pass `startAtLogin: false` if your pills only matter while your app runs.
- **Upgrades:** if several apps bundle different versions, the newest host wins.
- **Uninstall:** `pillkit-host uninstall` removes the host, its login item and
  saved items. Data folder: `~/Library/Application Support/pillkit` (macOS),
  `%LOCALAPPDATA%\pillkit` (Windows).
- **Code signing:** the host isn't code-signed yet. Inside a signed Electron
  app it's covered by your app's signature on macOS once notarised with your
  app; on Windows, SmartScreen may ask the first time.

## Troubleshooting

| Symptom | Fix |
|---|---|
| `no host for <os>-<arch>` | Reinstall `pillkit` so npm adds the host package; on Electron check `asarUnpack` |
| `link_not_allowed` | Use `https://`, or add your scheme to `linkSchemes` |
| Nothing appears on Windows | Look behind the **^** next to the clock; see `%LOCALAPPDATA%\pillkit\host.log` |
| Nothing appears on macOS | The menu bar may be full; quit an app with menu bar icons; see `host.log` |

`npx pillkit status` tells you whether the host is running and how many items
your app has.

## Privacy

Everything stays on the user's computer. Pill Kit makes no network requests
and collects nothing. The only links it opens are the ones you put in items,
when the user clicks them.
