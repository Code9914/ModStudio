@echo off
rem ModStudio : produit un installateur Windows sur ce PC.
title ModStudio (installateur)
cd /d "%~dp0"
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
call npm install --no-audit --no-fund
call npm run tauri build
if errorlevel 1 (
  echo.
  echo La compilation a echoue. Copie ce message a Claude.
  pause
  exit /b 1
)
explorer "%~dp0src-tauri\target\release\bundle\nsis"
pause
