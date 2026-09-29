@echo off
setlocal EnableExtensions
rem Regenerates every place the app shows its mark from the sources in assets\:
rem the four 1024 masters, the tauri bundle set, the three runtime size sets
rem (icons\red, icons\dark, icons\dark\red) and the four ui\public\favicon*.svg.
rem
rem The image work lives in build-app-icons.ps1; this is the entry point, so the
rem same double-click works without knowing PowerShell is involved.
rem
rem Sources:
rem   assets\icon.webp      light theme, services running
rem   assets\stop-icon.webp light theme, services stopped
rem   assets\icon-dark.webp dark theme, services running
rem The dark stopped twin is derived by rotating the dark running master's plate
rem hue onto the light red's measured hue, so both pairs stay one artwork in two
rem colourways. Replace a source, run this, and every icon follows.
rem
rem Requires: ImageMagick (magick on PATH) and the Tauri CLI (cargo tauri icon).

set "ROOT=%~dp0.."
set "SCRIPT=%~dp0build-app-icons.ps1"

echo.
echo === App icons ===
echo.

if not exist "%SCRIPT%" (
  echo [x] Problem: scripts\build-app-icons.ps1 is missing.
  echo     Cause: incomplete checkout, or the script was renamed.
  echo     Fix: restore the file and run this again.
  goto :fail
)

where magick >nul 2>nul
if errorlevel 1 (
  echo [x] Problem: magick not found. Cause: ImageMagick missing from PATH.
  echo     Fix: install ImageMagick from https://imagemagick.org and run this again.
  goto :fail
)
where cargo >nul 2>nul
if errorlevel 1 (
  echo [x] Problem: cargo not found. Cause: Rust toolchain missing from PATH.
  echo     Fix: install Rust from https://rustup.rs and run this again.
  goto :fail
)
if not exist "%ROOT%\assets\icon.webp" (
  echo [x] Problem: assets\icon.webp is missing. Cause: incomplete checkout.
  echo     Fix: run from a full repo checkout, or add the source and re-run.
  goto :fail
)

rem Set the code page so the PS1's box-drawing and arrow characters print as
rem intended instead of as mojibake in a legacy console.
chcp 65001 >nul
pwsh -NoProfile -ExecutionPolicy Bypass -File "%SCRIPT%"
if errorlevel 1 (
  echo.
  echo [x] Problem: icon generation failed. Cause: see the script output above.
  echo     Fix: fix the reported error and run this again.
  goto :fail
)

rem The tray and window marks are include_bytes!'d, so a Rust source change has
rem to be picked up by a rebuild for the new bytes to reach the running app.
echo.
echo       Done. Next:
echo         cargo build            - embed the new bytes into tray/taskbar/window icons
echo         cd ui ^&^& npm run build - refresh the compiled favicon copies
echo.
echo === Done ===
echo.
exit /b 0

:fail
echo.
exit /b 1
