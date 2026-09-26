@echo off
setlocal EnableExtensions
rem Build the OpenLocalServer frontend, desktop executable, and installer.

set "ROOT=%~dp0"
set "APPNAME=Open Local Server"
set "BINNAME=openlocalserver"
set "UI=%ROOT%ui"
set "TARGET=%ROOT%target\release"
set "EXE=%TARGET%\%BINNAME%.exe"
set "DIST=%ROOT%release"

for /f "tokens=2 delims==" %%v in ('findstr /b /c:"version = " "%ROOT%Cargo.toml"') do if not defined VERSION set "VERSION=%%~v"
if not defined VERSION set "VERSION=0.0.1"


echo.
echo === %APPNAME% %VERSION% ===
echo.


where cargo >nul 2>nul
if errorlevel 1 (
  echo [x] cargo not found. Install Rust from https://rustup.rs and run this again.
  goto :fail
)


rem Cap the build at half the CPU threads so the PC stays usable.
set /a JOBS=%NUMBER_OF_PROCESSORS% / 2
if %JOBS% LSS 1 set JOBS=1
rem CI sets BUILD_JOBS to use every core of its build machine.
if defined BUILD_JOBS set JOBS=%BUILD_JOBS%


rem Cargo never deletes old build output, so target\ and target-test\ only
rem grow (they reached 146 GB once). Wipe them when they pass the limit;
rem the next build is then a full ~10-15 minute rebuild. Set
rem CACHE_LIMIT_GB beforehand to change the limit.
if not defined CACHE_LIMIT_GB set CACHE_LIMIT_GB=8
set "CACHE_GB=0"
for /f %%s in ('powershell -NoProfile -Command "$d = @('%ROOT%src-tauri\target','%ROOT%src-tauri\target-test') | Where-Object { Test-Path $_ }; if ($d) { [int]((Get-ChildItem $d -Recurse -Force -File -ErrorAction SilentlyContinue | Measure-Object Length -Sum).Sum / 1GB) } else { 0 }"') do set "CACHE_GB=%%s"
echo       Rust build cache: %CACHE_GB% GB (limit %CACHE_LIMIT_GB% GB)
if %CACHE_GB% GTR %CACHE_LIMIT_GB% (
  echo       Over the limit - cleaning it, this build starts from scratch...
  pushd "%ROOT%src-tauri"
  cargo clean
  if exist target-test cargo clean --target-dir target-test
  popd
)


echo [1/4] Building frontend...
if exist "%UI%node_modules" goto :skip_npm
echo       installing frontend dependencies...
pushd "%UI%"
call npm.cmd install
set "NPM_ERR=%ERRORLEVEL%"
popd
if not "%NPM_ERR%"=="0" (
  echo [x] npm install failed.
  goto :fail
)
:skip_npm
pushd "%UI%"
call npm.cmd run build
set "BUILD_ERR=%ERRORLEVEL%"
popd
if not "%BUILD_ERR%"=="0" (
  echo [x] Frontend build failed.
  goto :fail
)
if not exist "%UI%\dist\index.html" (
  echo [x] Frontend build finished but ui\dist\index.html is missing.
  goto :fail
)

echo [2/4] Building release exe (%JOBS% build jobs)...
pushd "%ROOT%"
cargo build -p openlocalserver --release --jobs %JOBS%
set "BUILD_ERR=%ERRORLEVEL%"
popd
if not "%BUILD_ERR%"=="0" (
  echo [x] Build failed.
  goto :fail
)
if not exist "%EXE%" (
  echo [x] Build finished but %BINNAME%.exe is missing from target\release.
  goto :fail
)

if not exist "%DIST%" mkdir "%DIST%"
copy /y "%EXE%" "%DIST%\%APPNAME%.exe" >nul
for %%d in ("%TARGET%\*.dll") do if exist "%%~d" (
  copy /y "%%~d" "%DIST%\" >nul
)
echo       portable exe: "%DIST%\%APPNAME%.exe"

echo [3/4] Looking for Inno Setup...
set "ISCC="
for %%p in ("%ProgramFiles%\Inno Setup 7\ISCC.exe" "%ProgramFiles(x86)%\Inno Setup 7\ISCC.exe" "%LocalAppData%\Programs\Inno Setup 7\ISCC.exe") do (
  if not defined ISCC if exist "%%~p" set "ISCC=%%~p"
)
if not defined ISCC for /f "delims=" %%p in ('where iscc 2^>nul') do if not defined ISCC set "ISCC=%%p"
if not defined ISCC (
  echo [!] Inno Setup 7 not found, so no setup file was made.
  echo     Install it from https://jrsoftware.org/isdl.php and run this again,
  echo     The portable executable is still available in release\.
  goto :done
)


echo [4/4] Creating installer...
"%ISCC%" /Q "/DAppVersion=%VERSION%" "/DSourceExe=%EXE%" "/DLibDir=%TARGET%" "/DOutputDir=%DIST%" "%ROOT%installer\open-local-server.iss"
if errorlevel 1 (
  echo [x] Inno Setup failed.
  goto :fail
)
:done
echo.
exit /b 0


:fail
echo.
exit /b 1
