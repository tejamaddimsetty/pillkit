# pillkit

Show your app's notifications as a small **pill next to the clock**: in the
macOS menu bar and the Windows taskbar. Build status, reminders, launches,
your own sponsors. One `npm install`, a few lines, no server or account.

```bash
npm install pillkit
```

```js
import { PillKit } from "pillkit";

const pill = await PillKit.start({ appId: "my-app" });

await pill.show({
  id: "build",
  title: "Build passed",
  subtitle: "main · 2 minutes ago",
  url: "https://ci.example.com/builds/42",
});
```

That's it. Within a second the pill appears next to the clock. Hovering it
opens a card; clicking it opens the link.

## Items

```js
await pill.show({
  id: "launch",                 // same id again = update it
  title: "v2.1 is out",         // up to 40 characters
  subtitle: "See what's new",   // up to 60
  label: "New",                 // small tag on the right, up to 16
  icon: "./assets/logo.png",    // a PNG path (or base64); square works best
  background: "#111111",
  foreground: "#FFFFFF",
  url: "myapp://changelog",     // https:// page, or a link into your app
  body: "Faster sync, offline mode and a new dark theme.", // card text on hover
  seconds: 20,                  // time on screen before the next item
});

await pill.remove("launch");    // one item
await pill.clear();             // all of this app's items
```

Run code in your app instead of opening a link:

```js
await pill.show({ id: "sync", title: "Sync paused", onClick: "resume-sync" });
pill.on("callback", (e) => {
  if (e.name === "resume-sync") resumeSync();
});
```

Links into your app need its scheme declared once:

```js
const pill = await PillKit.start({ appId: "my-app", linkSchemes: ["myapp"] });
```

`https://` links work out of the box; `file:`, `javascript:` and similar are
always refused.

## Options

| Option | Default | |
|---|---|---|
| `appId` | required | Names your app on this computer (letters, digits, `-`, `_`). |
| `linkSchemes` | `[]` | Your app's URL schemes for deep links, e.g. `["myapp"]`. |
| `startAtLogin` | `true` | Keep pills showing after your app quits and across restarts. |
| `hostPath` | bundled | Use a specific `pillkit-host` binary. |

## How it works

`pillkit` installs a small host program for your platform
(`pillkit-host-<os>-<arch>`, picked automatically by npm). One host runs per
user and owns the slot next to the clock; if several apps use Pill Kit,
their items take turns. It uses about 0.1–0.5 % of one CPU core when idle.

- **macOS:** a menu bar item, sized to the menu bar (with a notch-aware fallback).
- **Windows 10/11:** inside the taskbar, in free space only (never over Start,
  Search or app icons), shrinking to a compact pill or just the icon when the
  taskbar is busy.
- **Linux:** coming soon.

### Electron

Keep the host outside the asar archive:

```json
"build": { "asarUnpack": ["**/node_modules/pillkit-host-*/**"] }
```

### From a shell script

The SDK includes a `pillkit` command:

```bash
npx pillkit show --app my-script --title "Backup done" --url https://example.com
npx pillkit clear --app my-script
npx pillkit status
```

## License

Apache-2.0. Source: https://github.com/tejamaddimsetty/pillkit
