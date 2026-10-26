# Testing the taskbar widget on Windows

## 1. Install (one line)

Open **PowerShell** from the Start menu, paste this and press Enter:

```powershell
irm https://raw.githubusercontent.com/tejamaddimsetty/pillkit/main/scripts/get.ps1 | iex
```

It downloads the latest code, sets everything up (installing Python for your
user if needed, after asking), starts the server and host, opens the portal and
registers a demo app. Within a few seconds the pill appears in the taskbar.

It also puts **Start Pill Kit** and **Stop Pill Kit** on your Desktop. Next time,
just double-click those. Run the one-liner again whenever you want the newest
version.

Prefer not to paste commands? Download the ZIP from GitHub, **Extract All**,
and double-click `install.cmd` instead.

If Windows asks "Do you want to allow this app?" or SmartScreen warns, choose
**More info → Run anyway**: the files aren't code-signed yet.

Options (from a terminal in the Pill Kit folder):

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
- `%LOCALAPPDATA%\pillkit\host.log` (it says which mode was used and why it fell back)
- Windows version: run `winver`
