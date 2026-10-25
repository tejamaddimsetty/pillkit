# Testing the taskbar widget on Windows

The Windows code compiles in CI but has not yet run on real hardware. This is
the checklist for the first hands-on test.

## 1. Get the host

1. Open the repository on GitHub → **Actions** → the latest green **ci** run.
2. Download the artifact **`barkit-host-dev-Windows-X64`** and unzip it. It contains `barkit-host.exe`.
   (Use the *dev* build: it trusts your local test server. The non-dev build only trusts production.)

Windows SmartScreen may warn because the file isn't code-signed yet: choose **More info → Run anyway**.

## 2. Get the code and tools

- Install **Python 3.10+** (tick "Add to PATH") and **Node 18+**.
- `git clone https://github.com/tejamaddimsetty/barkit` (or download the ZIP).

## 3. Run it

```powershell
cd barkit
powershell -ExecutionPolicy Bypass -File scripts\windows-test.ps1 -HostExe C:\path\to\barkit-host.exe
```

This starts a local server, the host and a demo app. Within a few seconds the
pill should appear in the taskbar and rotate between "Acme Cloud" and
"Build passed". Add your own items at http://localhost:5050 (`demo@barkit.dev` / `demo-password`).

## 4. What to check

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
| 11 | `-Mode overlay` and `-Mode tray` | Overlay: same spot, floats on top. Tray: an icon with a tooltip |

## 5. Send back

- Screenshots of the taskbar for checks 1–3
- `%LOCALAPPDATA%\barkit\host.log` (it says which mode was used and why it fell back)
- Windows version: run `winver`

## Cleaning up

```powershell
C:\path\to\barkit-host.exe uninstall
```
