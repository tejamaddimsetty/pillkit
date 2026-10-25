# Stops the server and host started by install.ps1.
param([switch]$Quiet)
$Run = Join-Path $env:LOCALAPPDATA "barkit\run"
$pids = Join-Path $Run "pids.txt"
if (Test-Path $pids) {
  Get-Content $pids | ForEach-Object { if ($_) { Stop-Process -Id ([int]$_) -Force -ErrorAction SilentlyContinue } }
  Remove-Item $pids -ErrorAction SilentlyContinue
}
Get-Process -Name "barkit-host*" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
if (-not $Quiet) { Write-Host "barkit stopped." -ForegroundColor Green }
