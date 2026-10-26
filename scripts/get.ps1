# One-line setup for Windows. Paste into PowerShell:
#
#   irm https://raw.githubusercontent.com/tejamaddimsetty/pillkit/main/scripts/get.ps1 | iex
#
# Downloads the latest pillkit code to %LOCALAPPDATA%\pillkit\src (keeping the
# local database and settings from earlier runs), adds "Start Pill Kit" and
# "Stop Pill Kit" shortcuts to the Desktop, and runs the installer.
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$repo = "tejamaddimsetty/pillkit"
$base = Join-Path $env:LOCALAPPDATA "pillkit\src"
$root = Join-Path $base "pillkit-main"

Write-Host "Pill Kit: downloading the latest code..." -ForegroundColor Green
New-Item -ItemType Directory -Force -Path $base | Out-Null
if (Test-Path (Join-Path $root "scripts\stop.ps1")) {
  & (Join-Path $root "scripts\stop.ps1") -Quiet
}
$zip = Join-Path $env:TEMP "pillkit-main.zip"
Invoke-WebRequest -Uri "https://github.com/$repo/archive/refs/heads/main.zip" -OutFile $zip -UseBasicParsing
# Overwrites code files but leaves server\.env, server\storage and server\.venv alone.
Expand-Archive -Path $zip -DestinationPath $base -Force
Remove-Item $zip -ErrorAction SilentlyContinue
Get-ChildItem -Path $root -Recurse -File | Unblock-File

$desktop = [Environment]::GetFolderPath("Desktop")
$shell = New-Object -ComObject WScript.Shell
foreach ($s in @(@("Start Pill Kit", "install.cmd"), @("Stop Pill Kit", "stop.cmd"))) {
  $lnk = $shell.CreateShortcut((Join-Path $desktop "$($s[0]).lnk"))
  $lnk.TargetPath = Join-Path $root $s[1]
  $lnk.WorkingDirectory = $root
  $lnk.Save()
}
Write-Host "Pill Kit: added 'Start Pill Kit' and 'Stop Pill Kit' to your Desktop." -ForegroundColor Green

& (Join-Path $root "scripts\install.ps1") -Update
