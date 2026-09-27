@echo off
rem Starts the UI dev server in its own window, then launches the desktop app.
setlocal
cd /d "%~dp0.."

if not exist "ui\node_modules" (
  echo Installing UI dependencies...
  pushd ui
  call npm install
  popd
)

rem The hosts-file helper must sit beside the app, and `cargo tauri dev` only builds the app itself.
cargo build -p ols-helper
if errorlevel 1 exit /b 1

start "OpenLocalServer UI (vite)" /D "%~dp0..\ui" cmd /k npm run dev

cargo tauri dev
endlocal
