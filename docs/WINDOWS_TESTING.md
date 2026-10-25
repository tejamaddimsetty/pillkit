# Testing the taskbar widget on Windows

## 1. Install (one double-click)

1. On GitHub, click **Code → Download ZIP** (or `git clone https://github.com/tejamaddimsetty/barkit`) and unzip it.
2. Double-click **`install.cmd`** in the folder.

That's it. The installer:

- installs Python for your user if it's missing (it asks first, using `winget`)
- sets up the local server and a demo account
- downloads `barkit-host` from the latest GitHub release
- starts everything, opens the portal, and registers a demo app

Within a few seconds the pill appears in the taskbar and rotates between your
items. Double-click **`stop.cmd`** to stop everything. Running `install.cmd`
again restarts it (and skips setup that's already done).

If Windows asks "Do you want to allow this app?" or SmartScreen warns, choose
**More info → Run anyway**: the files aren't code-signed yet.

Options (run from a terminal in the folder):

```powershell
install.cmd -Mode overlay     # force a mode: auto | embedded | overlay | tray
install.cmd -Update           # re-download the newest host
```

## 2. What to check

Note the result of each, with a screenshot if anything looks off.

| # | Check | Expected |
|---|---|---|
| 1 | Windows 11, icons centred (default) | Pill on the far left of the taskbar, where the weather widget sits (or just right of it if the widget is on) |
| 2 | Windows 11, Settings → Taskbar behaviours → alignment **Left** | Pill moves to just left of the tray icons |
| 3 | Windows 10 | Pill just left of the tray; task buttons shrink to make room |
| 4 | Left-click | Opens the item's link in the browser |
| 5 | Right-click | Menu with the card text, Hide for 12 hours, Why am I seeing this? |
| 6 | Restart Explorer (Task Manager → Windows Explorer → Restart) | Pill comes back within ~2 s |
| 7 | Full-screen video or game | Pill doesn't cover it |
| 8 | Display scaling 125 % / 150 % | Pill stays sharp and correctly sized |
| 9 | Second monitor | Note what happens (main taskbar only is expected for v0.1) |
| 10 | Light and dark mode | Pill uses its own colours; check it reads well on both |
| 11 | `install.cmd -Mode overlay` and `-Mode tray` | Overlay: same spot, floats on top. Tray: an icon with a tooltip |
| 12 | Add or edit an item in the portal | Shows in the taskbar within ~15 s |

## 3. Send back

- Screenshots of the taskbar for checks 1–3
- `%LOCALAPPDATA%\barkit\host.log` (it says which mode was used and why it fell back)
- Windows version: run `winver`
