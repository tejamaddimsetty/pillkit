# Pill Kit for Windows. Run it by double-clicking install.cmd.
#
# Downloads the host from the latest GitHub release, starts it (and at every
# login), and shows a demo pill. No server, account or Python needed.
#
#   install.cmd                  normal run
#   install.cmd -Mode overlay    force a taskbar mode: auto | embedded | overlay | tray
param(
  [ValidateSet("auto", "embedded", "overlay", "tray")][string]$Mode = "auto",
  [string]$HostExe = "",
  [switch]$Update
)
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$Repo = "tejamaddimsetty/pillkit"
$Data = Join-Path $env:LOCALAPPDATA "pillkit"
$Bin = Join-Path $Data "bin"
New-Item -ItemType Directory -Force -Path $Bin | Out-Null
$Log = Join-Path $Data "install.log"
try { Start-Transcript -Path $Log -Force | Out-Null } catch {}
function Step($n, $text) { Write-Host ""; Write-Host "[$n/3] $text" -ForegroundColor Cyan }
function Fail($text) {
  Write-Host ""; Write-Host "  $text" -ForegroundColor Red
  Write-Host "  Full log: $Log" -ForegroundColor Yellow
  try { Stop-Transcript | Out-Null } catch {}
  exit 1
}
trap { Fail "Something went wrong: $($_.Exception.Message)" }

Write-Host "Pill Kit installer" -ForegroundColor Green

Step 1 "Getting pillkit-host"
if (-not $HostExe) {
  $HostExe = Join-Path $Bin "pillkit-host.download.exe"
  $url = "https://github.com/$Repo/releases/latest/download/pillkit-host-windows-x64.exe"
  Write-Host "  Downloading $url"
  try {
    Invoke-WebRequest -Uri $url -OutFile $HostExe -UseBasicParsing
    Unblock-File $HostExe
  } catch {
    Fail "Couldn't download the host ($($_.Exception.Message)). Check your internet connection."
  }
}

Step 2 "Starting it (and at every login)"
& (Join-Path $PSScriptRoot "stop.ps1") -Quiet
Start-Process -FilePath $HostExe -ArgumentList "install" -Wait -WindowStyle Hidden
if ($HostExe -like "*.download.exe") { Remove-Item $HostExe -ErrorAction SilentlyContinue }
$Installed = Join-Path $Bin "pillkit-host.exe"
if (-not (Test-Path $Installed)) { Fail "The host didn't install. See $Data\host.log" }
if ($Mode -ne "auto") { $env:PILLKIT_WIN_MODE = $Mode } else { Remove-Item Env:PILLKIT_WIN_MODE -ErrorAction SilentlyContinue }
Start-Process -FilePath $Installed -ArgumentList "run" -WindowStyle Hidden
Start-Sleep -Seconds 2

Step 3 "Showing a demo pill. Look at your taskbar!"
$showArgs = @("show", "--app", "pillkit-demo", "--title", "Pill Kit is running", "--subtitle", "Hover me, then try the SDK",
          "--label", "Demo", "--url", "https://github.com/$Repo#readme")
Start-Process -FilePath $Installed -ArgumentList $showArgs -Wait -WindowStyle Hidden
Write-Host ""
Write-Host "  Try it yourself:   & `"$Installed`" show --title Hello --subtitle `"from PowerShell`""
Write-Host "  Remove the demo:   & `"$Installed`" clear --app pillkit-demo"
Write-Host "  Stop:              double-click stop.cmd    Uninstall: & `"$Installed`" uninstall"
Write-Host "  Log:               $Data\host.log"
try { Stop-Transcript | Out-Null } catch {}
