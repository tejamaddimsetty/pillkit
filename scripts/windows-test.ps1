# One-command local test on Windows: server + host + a demo app.
#
#   powershell -ExecutionPolicy Bypass -File scripts\windows-test.ps1 -HostExe C:\path\to\barkit-host.exe
#
# Needs Python 3.10+ and Node 18+. Use the *dev* host build from CI
# (artifact "barkit-host-dev-Windows-X64"); release builds only trust the
# production key.
param(
  [Parameter(Mandatory = $true)][string]$HostExe,
  [ValidateSet("auto", "embedded", "overlay", "tray")][string]$Mode = "auto"
)
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
$Server = Join-Path $Root "server"

Write-Host "1/4  Setting up the server..." -ForegroundColor Cyan
if (-not (Test-Path "$Server\.venv")) { python -m venv "$Server\.venv" }
& "$Server\.venv\Scripts\pip.exe" install -q -r "$Server\requirements.txt"
if (-not (Test-Path "$Server\.env")) {
  @"
DATABASE_URL=sqlite:///storage/barkit.db
SECRET_KEY=windows-test-only
SIGNING_KEY_PATH=storage/signing.key
PUBLIC_BASE_URL=http://localhost:5050
FEED_POLL_SECS=15
"@ | Set-Content "$Server\.env"
}
$seed = & "$Server\.venv\Scripts\python.exe" "$Server\dev_seed.py"
$seed | ForEach-Object { Write-Host "     $_" }
$AppId = ($seed | Select-String "APP_ID=(.*)").Matches[0].Groups[1].Value
$Key = ($seed | Select-String "KEY=(pk_.*)").Matches[0].Groups[1].Value

Write-Host "2/4  Starting the server on http://localhost:5050 ..." -ForegroundColor Cyan
$srv = Start-Process -FilePath "$Server\.venv\Scripts\python.exe" -ArgumentList "app.py" -WorkingDirectory $Server -PassThru -WindowStyle Minimized
for ($i = 0; $i -lt 30; $i++) {
  try { Invoke-RestMethod http://localhost:5050/healthz | Out-Null; break } catch { Start-Sleep -Milliseconds 500 }
}
$env:BARKIT_PUBLIC_KEY = (Invoke-RestMethod http://localhost:5050/v1/public-key).key

Write-Host "3/4  Starting barkit-host (mode: $Mode)..." -ForegroundColor Cyan
if ($Mode -ne "auto") { $env:BARKIT_WIN_MODE = $Mode } else { Remove-Item Env:BARKIT_WIN_MODE -ErrorAction SilentlyContinue }
$hst = Start-Process -FilePath $HostExe -ArgumentList "run" -PassThru -WindowStyle Hidden
Start-Sleep -Seconds 2

Write-Host "4/4  Registering the demo app. Look at your taskbar!" -ForegroundColor Green
Write-Host "     Log: $env:LOCALAPPDATA\barkit\host.log"
Write-Host "     Portal: http://localhost:5050  (demo@barkit.dev / demo-password)"
Write-Host "     Press Ctrl+C to stop everything."
try {
  node (Join-Path $Root "examples\node-demo.mjs") $AppId $Key "http://localhost:5050"
} finally {
  Stop-Process -Id $hst.Id -ErrorAction SilentlyContinue
  Stop-Process -Id $srv.Id -ErrorAction SilentlyContinue
}
