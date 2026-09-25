@echo off
rem Starts the UI dev server in its own window, then launches the desktop app.
setlocal
cd /d "%~dp0"

if not exist "ui\node_modules" (
  echo Installing UI dependencies...
  pushd ui
  call npm install
  popd
)

start "OpenLocalServer UI (vite)" /D "%~dp0ui" cmd /k npm run dev

cargo tauri dev
endlocal
