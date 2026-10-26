# Stops the server and host started by install.ps1.
param([switch]$Quiet)
$Run = Join-Path $env:LOCALAPPDATA "pillkit\run"
$pids = Join-Path $Run "pids.txt"
if (Test-Path $pids) {
  Get-Content $pids | ForEach-Object { if ($_) { Stop-Process -Id ([int]$_) -Force -ErrorAction SilentlyContinue } }
  Remove-Item $pids -ErrorAction SilentlyContinue
}
Get-Process -Name "pillkit-host*" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
if (-not $Quiet) { Write-Host "pillkit stopped." -ForegroundColor Green }
