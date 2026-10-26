@echo off
rem Double-click to set up and run barkit locally. Options: install.cmd -Mode overlay ^| -Update
if not exist "%~dp0scripts\install.ps1" (
  echo.
  echo   install.cmd is running on its own, without the rest of the barkit folder.
  echo   This happens when you open it from inside the ZIP.
  echo   Right-click the ZIP, choose "Extract All...", then run install.cmd from the extracted folder.
  echo.
  pause
  exit /b 1
)
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\install.ps1" %*
echo.
pause
