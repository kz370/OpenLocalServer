@echo off
setlocal EnableExtensions
rem Regenerates ui\src\components\faIconsData.json, the offline icon set used by
rem the site and project icon pickers, from scripts\build-fa-icons\icons.json.
rem
rem The SVG paths come from the Font Awesome Free npm packages in ui\node_modules,
rem read at build time only. The app never downloads icons: it ships the JSON.
rem
rem To add an icon: add an entry to scripts\build-fa-icons\icons.json, run this,
rem then rebuild the UI.
rem
rem Requires: Node.js and `npm install` run once inside ui\.

set "SCRIPT=%~dp0build-fa-icons\build-fa-icons.mjs"

echo.
echo === Font Awesome icon set ===
echo.

if not exist "%SCRIPT%" (
  echo [x] Problem: scripts\build-fa-icons\build-fa-icons.mjs is missing.
  echo     Cause: incomplete checkout, or the script was renamed.
  echo     Fix: restore the file and run this again.
  exit /b 1
)

where node >nul 2>nul
if errorlevel 1 (
  echo [x] Problem: Node.js was not found.
  echo     Cause: node is not installed or not on PATH.
  echo     Fix: install Node.js LTS, reopen the terminal and run this again.
  exit /b 1
)

node "%SCRIPT%"
exit /b %errorlevel%
