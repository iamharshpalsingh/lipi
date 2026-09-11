@echo off
rem Double-click to install LiPi for the current user (see install.ps1).
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" %*
echo.
echo Done. You can close this window.
pause
