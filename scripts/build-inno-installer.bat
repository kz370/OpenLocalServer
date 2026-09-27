@echo off
setlocal EnableExtensions
rem Stage already-built release files and create the Inno Setup installer.
rem Contract: cargo build --release (and the frontend it embeds) has already
rem run. For a full clean build use build-installer.bat instead.

set "ROOT=%~dp0..\"
set "APPNAME=Open Local Server"
set "CARGO_BIN=openlocalserver"
set "HELPER_BIN=ols-helper"

set "TARGET=%ROOT%target\release"
set "DIST=%ROOT%release"

set "EXE=%TARGET%\%CARGO_BIN%.exe"
set "HELPER_EXE=%TARGET%\%HELPER_BIN%.exe"

set "STAGED_EXE=%DIST%\%APPNAME%.exe"
set "STAGED_HELPER=%DIST%\%HELPER_BIN%.exe"

rem Read version from Cargo.toml
for /f "tokens=2 delims==" %%v in ('findstr /b /c:"version = " "%ROOT%Cargo.toml"') do if not defined VERSION set "VERSION=%%~v"
rem Strip spaces/quotes (tokens=2 leaves a leading space, breaking %%~v).
set "VERSION=%VERSION: =%"
set "VERSION=%VERSION:"=%"
if not defined VERSION set "VERSION=0.0.1"

echo.
echo === %APPNAME% %VERSION% - Installer Build ===
echo.

echo [1/3] Checking release files...

rem Refuse to stage over a running app: Windows locks the exe and the
rem copy below would fail with a misleading error.
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
  echo [x] Problem: "%EXE%" missing. Cause: release was never built.
  echo     Fix: run build-installer.bat or cargo build -p %CARGO_BIN% --release first.
  goto :fail
)

if not exist "%HELPER_EXE%" (
  echo [x] Problem: "%HELPER_EXE%" missing. Cause: helper was never built.
  echo     Fix: run build-installer.bat or cargo build -p %HELPER_BIN% --release first.
  goto :fail
)

if not exist "%ROOT%installer\open-local-server.iss" (
  echo [x] Problem: installer\open-local-server.iss missing. Cause: wrong working copy.
  echo     Fix: run from a full repo checkout.
  goto :fail
)

echo       Found openlocalserver.exe
echo       Found ols-helper.exe

echo.
echo [2/3] Copying release files...

if not exist "%DIST%" mkdir "%DIST%"

rem Rename openlocalserver.exe to Open Local Server.exe (retry: AV can hold it).
call :copy_retry "%EXE%" "%STAGED_EXE%" || goto :fail

rem Stage ols-helper.exe beside it for the portable folder.
call :copy_retry "%HELPER_EXE%" "%STAGED_HELPER%" || goto :fail

rem Stage every release DLL the same way.
for %%d in ("%TARGET%\*.dll") do if exist "%%~d" (
  call :copy_retry "%%~d" "%DIST%\%%~nxd" || goto :fail
)

echo       %APPNAME%.exe
echo       ols-helper.exe
echo       *.dll from target\release

echo.
echo [3/3] Creating Inno Setup installer...

rem Find Inno Setup 6/7
set "ISCC="
for %%p in ("%ProgramFiles%\Inno Setup 7\ISCC.exe" "%ProgramFiles(x86)%\Inno Setup 7\ISCC.exe" "%ProgramFiles%\Inno Setup 6\ISCC.exe" "%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe" "%LocalAppData%\Programs\Inno Setup 7\ISCC.exe") do (
  if not defined ISCC if exist "%%~p" set "ISCC=%%~p"
)

if not defined ISCC (
  for /f "delims=" %%p in ('where iscc 2^>nul') do if not defined ISCC set "ISCC=%%p"
)

if not defined ISCC (
  echo [x] Problem: Inno Setup was not found. Cause: ISCC.exe not installed.
  echo     Fix: install it from https://jrsoftware.org/isdl.php and run this again.
  echo     The portable executables are still available in release\.
  goto :fail
)

echo       Inno Setup: "%ISCC%"
echo.

rem LibDir points at target\release (like build-installer.bat) so the
rem installer picks up ols-helper.exe plus every release DLL.
"%ISCC%" /Q "/DAppVersion=%VERSION%" "/DSourceExe=%STAGED_EXE%" "/DLibDir=%TARGET%" "/DOutputDir=%DIST%" "%ROOT%installer\open-local-server.iss"

if errorlevel 1 (
  echo.
  echo [x] Problem: Inno Setup failed. Cause: see ISCC output above.
  echo     Fix: fix the reported error and run this again.
  goto :fail
)

set "SETUP=%DIST%\Open-Local-Server-%VERSION%-setup.exe"

if not exist "%SETUP%" (
  echo.
  echo [x] Problem: installer was not created. Cause: ISCC ran but produced no file.
  echo     Fix: check the ISCC output above and run this again.
  goto :fail
)

echo.
echo       Artifacts:
for %%f in ("%STAGED_EXE%" "%STAGED_HELPER%") do echo       %%~nxf - %%~zf bytes
for %%f in ("%SETUP%") do echo       %%~nxf - %%~zf bytes
echo.
echo       SHA-256:
for %%f in ("%STAGED_EXE%" "%STAGED_HELPER%") do call :show_hash "%%~f"
call :show_hash "%SETUP%"
echo.
echo === Installer created successfully ===
echo.
echo     "%SETUP%"
echo.

exit /b 0


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
