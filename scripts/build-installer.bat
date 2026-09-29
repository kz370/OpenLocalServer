@echo off
setlocal EnableExtensions
rem Build the OpenLocalServer frontend, desktop executable, and installer.
rem Usage: build-installer.bat [full|inno]  (no arg = ask).
rem Handles: tool checks, stale deps, running-app locks (retry), version
rem parsing, staging, optional Inno Setup (v6/v7), artifact verification.

set "ROOT=%~dp0..\"
set "APPNAME=Open Local Server"
set "CARGO_BIN=openlocalserver"
set "HELPER_BIN=ols-helper"
set "UI=%ROOT%ui"
set "TARGET=%ROOT%target\release"
set "EXE=%TARGET%\%CARGO_BIN%.exe"
set "HELPER_EXE=%TARGET%\%HELPER_BIN%.exe"
set "DIST=%ROOT%release"
set "STAGED=%DIST%\%APPNAME%.exe"
set "STAGED_HELPER=%DIST%\%HELPER_BIN%.exe"

for /f "tokens=2 delims==" %%v in ('findstr /b /c:"version = " "%ROOT%Cargo.toml"') do if not defined VERSION set "VERSION=%%~v"
rem Strip spaces/quotes (tokens=2 leaves a leading space, breaking %%~v).
set "VERSION=%VERSION: =%"
set "VERSION=%VERSION:"=%"
if not defined VERSION set "VERSION=0.0.1"


echo.
echo === %APPNAME% %VERSION% ===
echo.

rem Usage: build-installer.bat [full|inno]  (no arg = ask).
rem full = rebuild UI + exes + installer. inno = skip the rebuild,
rem stage the prebuilt target\release exes and compile the installer only.
set "MODE=%~1"
if "%MODE%"=="" call :ask_mode || goto :fail
if /i "%MODE%"=="1" set "MODE=full"
if /i "%MODE%"=="2" set "MODE=inno"
if /i "%MODE%"=="full" goto :mode_ok
if /i "%MODE%"=="inno" goto :mode_ok
echo Unknown mode "%MODE%". Use full or inno.
goto :fail

:mode_ok
if /i "%MODE%"=="inno" goto :inno_only


echo [0/5] Checking tools...
where cargo >nul 2>nul
if errorlevel 1 (
  echo [x] Problem: cargo not found. Cause: Rust toolchain missing.
  echo     Fix: install Rust from https://rustup.rs and run this again.
  goto :fail
)
where node >nul 2>nul
if errorlevel 1 (
  echo [x] Problem: node not found. Cause: Node.js missing.
  echo     Fix: install Node.js LTS from https://nodejs.org and run this again.
  goto :fail
)
if not exist "%UI%\package.json" (
  echo [x] Problem: ui\package.json missing. Cause: wrong working copy.
  echo     Fix: run from a full repo checkout.
  goto :fail
)
rem Refuse to build over a running app: Windows locks the exe and the
rem copy below would silently (or loudly) fail.
tasklist /FI "IMAGENAME eq %APPNAME%.exe" 2>nul | find /I "%APPNAME%.exe" >nul
if not errorlevel 1 (
  echo [x] Problem: %APPNAME%.exe is running - check the system tray.
  echo     Fix: right-click the tray icon, Quit, then run this again.
  goto :fail
)
tasklist /FI "IMAGENAME eq %CARGO_BIN%.exe" 2>nul | find /I "%CARGO_BIN%.exe" >nul
if not errorlevel 1 (
  echo [x] Problem: %CARGO_BIN%.exe is running.
  echo     Fix: stop it via tray icon Quit or Task Manager, then run this again.
  goto :fail
)
echo       tools ok (cargo, node).


rem Cap the build at half the CPU threads so the PC stays usable.
set /a JOBS=%NUMBER_OF_PROCESSORS% / 2
if %JOBS% LSS 1 set JOBS=1
rem CI sets BUILD_JOBS to use every core of its build machine.
if defined BUILD_JOBS set JOBS=%BUILD_JOBS%


rem Cargo never deletes old build output, so target\ only grows
rem (it reached 146 GB once). Wipe it when it passes the limit;
rem the next build is then a full ~10-15 minute rebuild. Set
rem CACHE_LIMIT_GB beforehand to change the limit.
if not defined CACHE_LIMIT_GB set CACHE_LIMIT_GB=40
set "CACHE_GB=0"
for /f %%s in ('powershell -NoProfile -Command "if (Test-Path '%TARGET%\..') { [int]((Get-ChildItem '%TARGET%\..' -Recurse -Force -File -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum / 1GB) } else { 0 }"') do set "CACHE_GB=%%s"
echo       Rust build cache: %CACHE_GB% GB (limit %CACHE_LIMIT_GB% GB)
if %CACHE_GB% GTR %CACHE_LIMIT_GB% (
  echo       Over the limit - cleaning it, this build starts from scratch...
  pushd "%ROOT%"
  cargo clean
  popd
)


echo [1/5] Building frontend...
rem Always install: no-op (~1s) when up to date, fixes stale node_modules.
pushd "%UI%"
call npm.cmd install
set "NPM_ERR=%ERRORLEVEL%"
if not "%NPM_ERR%"=="0" (
  popd
  echo [x] Problem: npm install failed. Cause: see npm output above.
  echo     Fix: delete ui\node_modules and run this again.
  goto :fail
)
call npm.cmd run build
set "BUILD_ERR=%ERRORLEVEL%"
popd
if not "%BUILD_ERR%"=="0" (
  echo [x] Problem: frontend build failed. Cause: see tsc/vite output above.
  echo     Fix: fix the reported error and run this again.
  goto :fail
)
if not exist "%UI%\dist\index.html" (
  echo [x] Problem: ui\dist\index.html missing after build.
  echo     Fix: delete ui\dist and run this again.
  goto :fail
)

echo [2/5] Building release executables (%JOBS% build jobs)...
rem Update trust baked into every build: the PUBLIC key is safe to ship.
rem A pre-set OLS_UPDATE_PUBKEY in the environment always wins.
if not defined OLS_UPDATE_PUBKEY set "OLS_UPDATE_PUBKEY=RWQTa5rn3AFu8SRjdSvz7VsUi/pRNNdk2FPuCbmgSZMV+veJMB9XCiys"
pushd "%ROOT%"
cargo build -p %CARGO_BIN% -p %HELPER_BIN% --release --jobs %JOBS%
set "BUILD_ERR=%ERRORLEVEL%"
popd
if not "%BUILD_ERR%"=="0" (
  echo [x] Problem: cargo build failed. Cause: see rustc output above.
  echo     Fix: fix the reported error and run this again.
  goto :fail
)
if not exist "%EXE%" (
  echo [x] Problem: %CARGO_BIN%.exe missing from target\release after build.
  echo     Fix: run cargo build -p %CARGO_BIN% --release manually to see why.
  goto :fail
)
if not exist "%HELPER_EXE%" (
  echo [x] Problem: %HELPER_BIN%.exe missing from target\release after build.
  echo     Fix: run cargo build -p %HELPER_BIN% --release manually to see why.
  goto :fail
)

if not exist "%DIST%" mkdir "%DIST%"
rem Copy with retries: antivirus or a just-exited app can hold the file briefly.
call :copy_retry "%EXE%" "%STAGED%" || goto :fail
call :copy_retry "%HELPER_EXE%" "%STAGED_HELPER%" || goto :fail
for %%d in ("%TARGET%\*.dll") do if exist "%%~d" (
  call :copy_retry "%%~d" "%DIST%\%%~nxd" || goto :fail
)
echo       portable exe: "%STAGED%"

echo [3/5] Looking for Inno Setup...
set "ISCC="
for %%p in ("%ProgramFiles%\Inno Setup 7\ISCC.exe" "%ProgramFiles(x86)%\Inno Setup 7\ISCC.exe" "%ProgramFiles%\Inno Setup 6\ISCC.exe" "%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe" "%LocalAppData%\Programs\Inno Setup 7\ISCC.exe") do (
  if not defined ISCC if exist "%%~p" set "ISCC=%%~p"
)
if not defined ISCC for /f "delims=" %%p in ('where iscc 2^>nul') do if not defined ISCC set "ISCC=%%p"
if not defined ISCC (
  echo [!] Inno Setup not found, so no setup file was made.
  echo     Install it from https://jrsoftware.org/isdl.php and run this again.
  echo     The portable executable is still available in release\.
  goto :verify
)


echo [4/5] Creating installer...
"%ISCC%" /Q "/DAppVersion=%VERSION%" "/DSourceExe=%STAGED%" "/DLibDir=%TARGET%" "/DOutputDir=%DIST%" "%ROOT%installer\open-local-server.iss"
if errorlevel 1 (
  echo [x] Problem: Inno Setup failed. Cause: see ISCC output above.
  echo     Fix: fix the reported error and run this again.
  goto :fail
)

:verify
echo [5/5] Verifying artifacts...
set "VERIFY_FAIL="
if not exist "%STAGED%" (
  echo [x] Missing "%STAGED%"
  set "VERIFY_FAIL=1"
)
if not exist "%STAGED_HELPER%" (
  echo [x] Missing "%STAGED_HELPER%"
  set "VERIFY_FAIL=1"
)
set "SETUP=%DIST%\Open-Local-Server-%VERSION%-setup.exe"
if defined ISCC (
  if not exist "%SETUP%" (
    echo [x] Missing "%SETUP%" - ISCC ran but produced no setup file.
    set "VERIFY_FAIL=1"
  )
) else (
  echo       no installer - Inno Setup not installed, portable only
)
if defined VERIFY_FAIL goto :fail
echo.
echo       Artifacts:
for %%f in ("%STAGED%" "%STAGED_HELPER%") do echo       %%~nxf - %%~zf bytes
if exist "%SETUP%" for %%f in ("%SETUP%") do echo       %%~nxf - %%~zf bytes
echo.
echo       SHA-256:
for %%f in ("%STAGED%" "%STAGED_HELPER%") do call :show_hash "%%~f"
if exist "%SETUP%" call :show_hash "%SETUP%"
echo.
echo === Done: %APPNAME% %VERSION% ===
echo     Run:      "%STAGED%"
if exist "%SETUP%" echo     Installer: "%SETUP%"
echo     Publish:  scripts\upload-release.bat v%VERSION%
echo.
exit /b 0


rem Inno-only mode: no rebuild. Stage prebuilt exes, compile installer.
:inno_only
echo [inno] Skipping rebuild - staging prebuilt exes...
tasklist /FI "IMAGENAME eq %APPNAME%.exe" 2>nul | find /I "%APPNAME%.exe" >nul
if not errorlevel 1 (
  echo [x] Problem: %APPNAME%.exe is running - check the system tray.
  echo     Fix: right-click the tray icon, Quit, then run this again.
  goto :fail
)
tasklist /FI "IMAGENAME eq %CARGO_BIN%.exe" 2>nul | find /I "%CARGO_BIN%.exe" >nul
if not errorlevel 1 (
  echo [x] Problem: %CARGO_BIN%.exe is running.
  echo     Fix: stop it via tray icon Quit or Task Manager, then run this again.
  goto :fail
)
if not exist "%EXE%" (
  echo [x] Problem: %CARGO_BIN%.exe missing from target\release.
  echo     Fix: run scripts\build-installer.bat full first to build it.
  goto :fail
)
if not exist "%HELPER_EXE%" (
  echo [x] Problem: %HELPER_BIN%.exe missing from target\release.
  echo     Fix: run scripts\build-installer.bat full first to build it.
  goto :fail
)
if not exist "%DIST%" mkdir "%DIST%"
call :copy_retry "%EXE%" "%STAGED%" || goto :fail
call :copy_retry "%HELPER_EXE%" "%STAGED_HELPER%" || goto :fail
for %%d in ("%TARGET%\*.dll") do if exist "%%~d" (
  call :copy_retry "%%~d" "%DIST%\%%~nxd" || goto :fail
)
echo       portable exe: "%STAGED%"
set "ISCC="
for %%p in ("%ProgramFiles%\Inno Setup 7\ISCC.exe" "%ProgramFiles(x86)%\Inno Setup 7\ISCC.exe" "%ProgramFiles%\Inno Setup 6\ISCC.exe" "%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe" "%LocalAppData%\Programs\Inno Setup 7\ISCC.exe") do (
  if not defined ISCC if exist "%%~p" set "ISCC=%%~p"
)
if not defined ISCC for /f "delims=" %%p in ('where iscc 2^>nul') do if not defined ISCC set "ISCC=%%p"
if not defined ISCC (
  echo [!] Inno Setup not found, so no setup file was made.
  echo     Install it from https://jrsoftware.org/isdl.php and run this again.
  echo     The portable executable is still available in release\.
  goto :verify
)
echo [inno] Creating installer...
"%ISCC%" /Q "/DAppVersion=%VERSION%" "/DSourceExe=%STAGED%" "/DLibDir=%TARGET%" "/DOutputDir=%DIST%" "%ROOT%installer\open-local-server.iss"
if errorlevel 1 (
  echo [x] Problem: Inno Setup failed. Cause: see ISCC output above.
  echo     Fix: fix the reported error and run this again.
  goto :fail
)
goto :verify


rem Prompt for build mode when the first argument is missing.
:ask_mode
echo Select build mode:
echo   1 - full: rebuild UI + exes + installer
echo   2 - inno: installer only from prebuilt exes
set "CHOICE="
set /p "CHOICE=Enter 1 or 2 [1]: "
if "%CHOICE%"=="" set "CHOICE=1"
if "%CHOICE%"=="1" set "MODE=full" & exit /b 0
if "%CHOICE%"=="2" set "MODE=inno" & exit /b 0
if /i "%CHOICE%"=="full" set "MODE=full" & exit /b 0
if /i "%CHOICE%"=="inno" set "MODE=inno" & exit /b 0
echo Invalid choice. & exit /b 1


rem Print SHA-256 via certutil (present on every Windows; Get-FileHash is not).
:show_hash
set "SH_HASH="
for /f "skip=1 delims=" %%h in ('certutil -hashfile "%~1" SHA256 2^>nul') do if not defined SH_HASH set "SH_HASH=%%h"
if defined SH_HASH (echo       %SH_HASH%  %~nx1) else (echo       hash failed for %~nx1)
set "SH_HASH="
exit /b 0


rem Retry copy: %1 = source, %2 = dest. Survives brief AV locks.
:copy_retry
set "CR_SRC=%~1"
set "CR_DST=%~2"
for /l %%r in (1,1,4) do (
  copy /y "%CR_SRC%" "%CR_DST%" >nul 2>&1
  if not errorlevel 1 exit /b 0
  if "%%r"=="4" (
    echo [x] Problem: could not write "%CR_DST%".
    echo     Cause: file locked - app still running or antivirus hold.
    echo     Fix: quit the app via tray icon, pause AV, run this again.
    exit /b 1
  )
  rem ping wait works headless; timeout.exe needs a console and dies redirected.
  ping -n 4 127.0.0.1 >nul 2>&1
)
exit /b 1


:fail
echo.
exit /b 1
