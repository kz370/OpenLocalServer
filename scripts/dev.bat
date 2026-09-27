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

rem Update trust baked into every build: the PUBLIC key is safe to ship.
rem A pre-set OLS_UPDATE_PUBKEY in the environment always wins.
if not defined OLS_UPDATE_PUBKEY set "OLS_UPDATE_PUBKEY=RWQTa5rn3AFu8SRjdSvz7VsUi/pRNNdk2FPuCbmgSZMV+veJMB9XCiys"

rem The hosts-file helper must sit beside the app, and `cargo tauri dev` only builds the app itself.
cargo build -p ols-helper
if errorlevel 1 exit /b 1

cargo tauri dev
endlocal
