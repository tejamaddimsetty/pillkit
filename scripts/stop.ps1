# Stops the Pill Kit host (it starts again at the next login; install.cmd starts it now).
param([switch]$Quiet)
Get-Process -Name "pillkit-host*" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
if (-not $Quiet) { Write-Host "Pill Kit stopped." -ForegroundColor Green }
