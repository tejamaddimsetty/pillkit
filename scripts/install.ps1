# barkit local installer for Windows. Run it by double-clicking install.cmd.
#
# Sets up the server, downloads the host from the latest GitHub release,
# starts everything, registers a demo app and opens the portal. Running it
# again skips the setup that's already done and restarts everything.
#
#   install.cmd                  normal run
#   install.cmd -Mode overlay    force a taskbar mode: auto | embedded | overlay | tray
#   install.cmd -Update          re-download the host
param(
  [ValidateSet("auto", "embedded", "overlay", "tray")][string]$Mode = "auto",
  [string]$HostExe = "",
  [switch]$Update
)
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$Root = Split-Path -Parent $PSScriptRoot
$Server = Join-Path $Root "server"
$Data = Join-Path $env:LOCALAPPDATA "barkit"
$Run = Join-Path $Data "run"
$Repo = "tejamaddimsetty/barkit"
New-Item -ItemType Directory -Force -Path $Run, (Join-Path $Data "bin") | Out-Null
# Everything shown in this window is also saved here, to send if something fails.
$Log = Join-Path $Run "install.log"
try { Start-Transcript -Path $Log -Force | Out-Null } catch {}
trap {
  Write-Host ""
  Write-Host "  Something went wrong: $($_.Exception.Message)" -ForegroundColor Red
  Write-Host "  Full log: $Log" -ForegroundColor Yellow
  try { Stop-Transcript | Out-Null } catch {}
  exit 1
}

function Step($n, $text) { Write-Host ""; Write-Host "[$n/6] $text" -ForegroundColor Cyan }
function Fail($text) {
  Write-Host ""; Write-Host "  $text" -ForegroundColor Red
  Write-Host "  Full log: $Log" -ForegroundColor Yellow
  try { Stop-Transcript | Out-Null } catch {}
  exit 1
}

function Find-Python {
  $candidates = @()
  if (Get-Command py -ErrorAction SilentlyContinue) { $candidates += , @("py", "-3") }
  if (Get-Command python -ErrorAction SilentlyContinue) { $candidates += , @("python") }
  Get-ChildItem "$env:LOCALAPPDATA\Programs\Python\Python3*\python.exe" -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending | ForEach-Object { $candidates += , @($_.FullName) }
  foreach ($c in $candidates) {
    try {
      $exe = $c[0]; $rest = @($c | Select-Object -Skip 1)
      $out = & $exe @rest -c "import sys; print(sys.version_info >= (3, 10), sys.executable)" 2>$null
      if ($LASTEXITCODE -eq 0 -and $out -like "True *") { return $out.Substring(5).Trim() }
    } catch {}
  }
  return $null
}

Write-Host "barkit installer" -ForegroundColor Green
Write-Host "Repo: $Root"

# ---------------------------------------------------------------- 1. Python
Step 1 "Checking for Python 3.10+"
$py = Find-Python
if (-not $py) {
  Write-Host "  Python isn't installed."
  if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
    Fail "Install Python 3.12 from https://www.python.org/downloads/ (tick 'Add to PATH'), then run install.cmd again."
  }
  $answer = Read-Host "  Install Python 3.12 for your user with winget now? [Y/n]"
  if ($answer -and $answer -notmatch "^[Yy]") { Fail "Python is required. Install it, then run install.cmd again." }
  winget install -e --id Python.Python.3.12 --scope user --silent
  $py = Find-Python
  if (-not $py) { Fail "Python was installed but isn't visible yet. Close this window and run install.cmd again." }
}
Write-Host "  Using $py"

# ---------------------------------------------------------------- 2. Server
Step 2 "Setting up the server"
$venvPy = Join-Path $Server ".venv\Scripts\python.exe"
if (-not (Test-Path $venvPy)) {
  & $py -m venv (Join-Path $Server ".venv")
}
& $venvPy -m pip install -q --disable-pip-version-check -r (Join-Path $Server "requirements.txt")
if ($LASTEXITCODE -ne 0) { Fail "Installing the server's Python packages failed (see above)." }
$envFile = Join-Path $Server ".env"
if (-not (Test-Path $envFile)) {
  $secret = [guid]::NewGuid().ToString("N") + [guid]::NewGuid().ToString("N")
  @(
    "DATABASE_URL=sqlite:///storage/barkit.db",
    "SECRET_KEY=$secret",
    "SIGNING_KEY_PATH=storage/signing.key",
    "PUBLIC_BASE_URL=http://localhost:5050",
    "FEED_POLL_SECS=15"
  ) | Set-Content -Encoding ascii $envFile
}
Push-Location $Server
$seed = & $venvPy dev_seed.py
Pop-Location
$AppId = ($seed | Select-String "APP_ID=(.*)").Matches[0].Groups[1].Value.Trim()
$Key = ($seed | Select-String "KEY=(pk_.*)").Matches[0].Groups[1].Value.Trim()
Write-Host "  Demo app: $AppId"

# ---------------------------------------------------------------- 3. Host
Step 3 "Getting barkit-host"
if (-not $HostExe) {
  $HostExe = Join-Path $Data "bin\barkit-host-dev.exe"
  if ($Update -or -not (Test-Path $HostExe)) {
    $url = "https://github.com/$Repo/releases/latest/download/barkit-host-dev-windows-x64.exe"
    Write-Host "  Downloading $url"
    try {
      Invoke-WebRequest -Uri $url -OutFile "$HostExe.download" -UseBasicParsing
      Move-Item -Force "$HostExe.download" $HostExe
      Unblock-File $HostExe
    } catch {
      if (Get-Command cargo -ErrorAction SilentlyContinue) {
        Write-Host "  No release download available; building from source with cargo..."
        Push-Location $Root; cargo build -p barkit-host; Pop-Location
        Copy-Item -Force (Join-Path $Root "target\debug\barkit-host.exe") $HostExe
      } else {
        Fail "Couldn't download the host ($($_.Exception.Message)). Check your internet connection, or install Rust (https://rustup.rs) and run again."
      }
    }
  }
}
Write-Host "  Using $HostExe"

# ---------------------------------------------------------------- 4. Start
Step 4 "Starting the server and the host"
& (Join-Path $PSScriptRoot "stop.ps1") -Quiet
$venvPyw = Join-Path $Server ".venv\Scripts\pythonw.exe"
if (-not (Test-Path $venvPyw)) { $venvPyw = $venvPy }
$srv = Start-Process -FilePath $venvPyw -ArgumentList "app.py" -WorkingDirectory $Server -WindowStyle Hidden -PassThru `
  -RedirectStandardOutput (Join-Path $Run "server.log") -RedirectStandardError (Join-Path $Run "server.err.log")
$ok = $false
for ($i = 0; $i -lt 60; $i++) {
  try { Invoke-RestMethod http://localhost:5050/healthz | Out-Null; $ok = $true; break } catch { Start-Sleep -Milliseconds 500 }
}
if (-not $ok) { Fail "The server didn't start. See $Run\server.err.log" }
$env:BARKIT_PUBLIC_KEY = (Invoke-RestMethod http://localhost:5050/v1/public-key).key
if ($Mode -ne "auto") { $env:BARKIT_WIN_MODE = $Mode } else { Remove-Item Env:BARKIT_WIN_MODE -ErrorAction SilentlyContinue }
$hst = Start-Process -FilePath $HostExe -ArgumentList "run" -WindowStyle Hidden -PassThru
"$($srv.Id)`n$($hst.Id)" | Set-Content (Join-Path $Run "pids.txt")
Write-Host "  Server: http://localhost:5050   Host mode: $Mode"

# ---------------------------------------------------------------- 5. Portal
Step 5 "Opening the portal"
Start-Process "http://localhost:5050/login"
Write-Host "  Developer login: demo@barkit.dev / demo-password"
Write-Host "  Admin login:     admin@barkit.dev / admin-password"

# ---------------------------------------------------------------- 6. Demo
Step 6 "Registering the demo app. Look at your taskbar!"
Write-Host "  Host log: $Data\host.log"
Write-Host "  To stop everything later, double-click stop.cmd."
& $venvPy (Join-Path $PSScriptRoot "demo_app.py") $AppId $Key "http://localhost:5050"
