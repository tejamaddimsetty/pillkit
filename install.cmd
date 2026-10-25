@echo off
rem Double-click to set up and run barkit locally. Options: install.cmd -Mode overlay ^| -Update
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\install.ps1" %*
echo.
pause
