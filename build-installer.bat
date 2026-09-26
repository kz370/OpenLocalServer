@echo off
setlocal EnableExtensions
rem ---------------------------------------------------------------------------
rem  Open Local Server - build the release .exe and an installer for
rem  C:\Program Files
rem
rem    build-installer.bat           build exe + setup (Inno Setup)
rem    build-installer.bat install   build exe, then install it directly into
rem                                  "C:\Program Files\Open Local Server"
rem                                  (asks for admin)
rem    build-installer.bat upload    build exe + setup, then upload them as a
rem                                  GitHub release without asking
rem    build-installer.bat noupload  build exe + setup, never ask to upload
rem
rem  Needs: Rust (https://rustup.rs) with the MSVC toolchain.
rem         Inno Setup 6 (https://jrsoftware.org/isdl.php) for the setup file.
rem ---------------------------------------------------------------------------


set "ROOT=%~dp0"
set "APPNAME=Open Local Server"
set "BINNAME=local-ai-server"
set "EXE=%ROOT%src-tauri\target\release\%BINNAME%.exe"
rem NOTE: outputs go to release\, never to dist\ (dist\ is the Tauri
rem frontend folder and gets embedded into the exe as-is).
set "DIST=%ROOT%release"


rem version = the "version" line of Cargo.toml
for /f "tokens=2 delims== " %%v in ('findstr /b /c:"version" "%ROOT%src-tauri\Cargo.toml"') do if not defined VERSION set "VERSION=%%~v"
if not defined VERSION set "VERSION=0.1.0"


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
if not exist "%ROOT%node_modules" goto :npm_install 
goto :skip_npm
:npm_install
echo       installing frontend dependencies...
pushd "%ROOT%"
call npm.cmd install
set "NPM_ERR=%ERRORLEVEL%"
popd
if not "%NPM_ERR%"=="0" (
  echo [x] npm install failed.
  goto :fail
)
:skip_npm
pushd "%ROOT%"
call npm.cmd run build
set "BUILD_ERR=%ERRORLEVEL%"
popd
if not "%BUILD_ERR%"=="0" (
  echo [x] Frontend build failed.
  goto :fail
)
if not exist "%ROOT%dist\index.html" (
  echo [x] Frontend build finished but dist\index.html is missing.
  goto :fail
)


rem sherpa-onnx (the speech engine) comes as a prebuilt archive that its build
rem script downloads into target\, so every cargo clean downloads it again.
rem Keep a copy in .build-cache\ and let the build script use that instead.
rem The version comes from Cargo.lock, so an upgrade downloads the new one.
set "SHERPA_CACHE=%ROOT%.build-cache\sherpa-onnx"
set "SHERPA_VER="
set "SHERPA_ONNX_ARCHIVE_DIR="
for /f %%v in ('powershell -NoProfile -Command "$m = [regex]::Match((Get-Content -Raw '%ROOT%src-tauri\Cargo.lock'), 'name = .sherpa-onnx-sys.\r?\nversion = .([0-9.]+)'); if ($m.Success) { $m.Groups[1].Value }"') do set "SHERPA_VER=%%v"
set "SHERPA_ARCHIVE=sherpa-onnx-v%SHERPA_VER%-win-x64-shared-MT-Release-lib.tar.bz2"
if defined SHERPA_VER if exist "%SHERPA_CACHE%\%SHERPA_ARCHIVE%" (
  set "SHERPA_ONNX_ARCHIVE_DIR=%SHERPA_CACHE%"
  echo       sherpa-onnx %SHERPA_VER%: using the cached copy, no download
)


echo [2/4] Building release exe (%JOBS% build jobs)...
pushd "%ROOT%src-tauri"
cargo build --release --jobs %JOBS%
set "BUILD_ERR=%ERRORLEVEL%"
popd
if not "%BUILD_ERR%"=="0" (
  echo [x] Build failed.
  goto :fail
)
if not exist "%EXE%" (
  echo [x] Build finished but %EXE% is missing.
  goto :fail
)

rem Save the sherpa-onnx archive for the next clean build.
set "SHERPA_BUILT=%ROOT%src-tauri\target\sherpa-onnx-prebuilt\%SHERPA_ARCHIVE%"
if defined SHERPA_VER if not exist "%SHERPA_CACHE%\%SHERPA_ARCHIVE%" if exist "%SHERPA_BUILT%" (
  if not exist "%SHERPA_CACHE%" mkdir "%SHERPA_CACHE%"
  copy /y "%SHERPA_BUILT%" "%SHERPA_CACHE%\" >nul && echo       sherpa-onnx %SHERPA_VER%: saved to .build-cache for next time
)


if not exist "%DIST%" mkdir "%DIST%"
copy /y "%EXE%" "%DIST%\%APPNAME%.exe" >nul
rem The portable exe needs the speech libraries beside it.
for %%d in (sherpa-onnx-c-api.dll sherpa-onnx-cxx-api.dll onnxruntime.dll onnxruntime_providers_shared.dll) do (
  copy /y "%ROOT%src-tauri\target\release\%%d" "%DIST%\%%d" >nul
)
echo       portable exe: "%DIST%\%APPNAME%.exe"


if /i "%~1"=="install" goto :direct_install


echo [3/4] Looking for Inno Setup...
set "ISCC="
for %%p in ("%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe" "%ProgramFiles%\Inno Setup 6\ISCC.exe" "%LocalAppData%\Programs\Inno Setup 6\ISCC.exe") do (
  if not defined ISCC if exist "%%~p" set "ISCC=%%~p"
)
if not defined ISCC for /f "delims=" %%p in ('where iscc 2^>nul') do if not defined ISCC set "ISCC=%%p"
if not defined ISCC (
  echo [!] Inno Setup 6 not found, so no setup file was made.
  echo     Install it from https://jrsoftware.org/isdl.php and run this again,
  echo     or run:  build-installer.bat install   to install straight into Program Files.
  goto :done
)


echo [4/4] Creating installer...
"%ISCC%" /Q "/DAppVersion=%VERSION%" "/DSourceExe=%EXE%" "/DLibDir=%ROOT%src-tauri\target\release" "/DOutputDir=%DIST%" "%ROOT%installer\open-local-server.iss"
if errorlevel 1 (
  echo [x] Inno Setup failed.
  goto :fail
)
echo.
echo Done. Installer: "%DIST%\Open-Local-Server-%VERSION%-setup.exe"
echo It installs to "C:\Program Files\%APPNAME%" with a Start menu entry and uninstaller.


rem Offer to publish this build as GitHub release v<version> (upload-release.bat).
rem Handy when release\ was lost and the same version just needs rebuilding.
if /i "%~1"=="noupload" goto :done
set "TAG=v%VERSION%"
set "UPLOAD=N"
if /i "%~1"=="upload" set "UPLOAD=Y"
if /i "%UPLOAD%"=="Y" goto :upload
where gh >nul 2>nul || (echo To publish later: run upload-release.bat & goto :done)
echo.
gh release view "%TAG%" >nul 2>nul
if errorlevel 1 (
  echo Release %TAG% does not exist on GitHub yet.
  if not exist "%ROOT%release-notes\%TAG%.md" echo Note: no release-notes\%TAG%.md, GitHub will generate the notes.
  choice /c YN /n /m "Create release %TAG% and upload this build? [Y/N] "
) else (
  echo Release %TAG% already exists on GitHub.
  choice /c YN /n /m "Replace its setup exe and portable zip with this build? [Y/N] "
)
if errorlevel 2 (
  echo Not uploaded. To publish later: run upload-release.bat
  goto :done
)

:upload
echo Uploading %TAG%...
call "%ROOT%upload-release.bat" %TAG%
if errorlevel 1 (
  echo [x] Upload failed.
  goto :fail
)
goto :done


:direct_install
echo [2/2] Installing into "%ProgramFiles%\%APPNAME%" (administrator rights needed)...
set "PS1=%TEMP%\openlocalserver-install.ps1"
> "%PS1%"  echo $ErrorActionPreference = 'Stop'
>> "%PS1%" echo $dir = Join-Path $env:ProgramFiles '%APPNAME%'
>> "%PS1%" echo New-Item -ItemType Directory -Force -Path $dir ^| Out-Null
>> "%PS1%" echo Get-Process -Name '%BINNAME%','%APPNAME%' -ErrorAction SilentlyContinue ^| Stop-Process -Force
>> "%PS1%" echo Copy-Item -Force '%EXE%' (Join-Path $dir '%APPNAME%.exe')
>> "%PS1%" echo Get-ChildItem '%ROOT%src-tauri\target\release\*.dll' ^| Where-Object { $_.Name -match 'sherpa-onnx|onnxruntime' } ^| ForEach-Object { Copy-Item -Force $_.FullName $dir }
>> "%PS1%" echo Copy-Item -Force '%ROOT%src-tauri\icons\icon.ico' (Join-Path $dir 'icon.ico')
>> "%PS1%" echo $shell = New-Object -ComObject WScript.Shell
>> "%PS1%" echo $lnk = $shell.CreateShortcut((Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\%APPNAME%.lnk'))
>> "%PS1%" echo $lnk.TargetPath = (Join-Path $dir '%APPNAME%.exe')
>> "%PS1%" echo $lnk.WorkingDirectory = $dir
>> "%PS1%" echo $lnk.Save()
powershell -NoProfile -ExecutionPolicy Bypass -Command "Start-Process powershell -Verb RunAs -Wait -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','\"%PS1%\"'"
del "%PS1%" >nul 2>nul
if exist "%ProgramFiles%\%APPNAME%\%APPNAME%.exe" (
  echo Installed: "%ProgramFiles%\%APPNAME%\%APPNAME%.exe"  ^(Start menu: %APPNAME%^)
) else (
  echo [x] Install was cancelled or failed.
  goto :fail
)


:done
echo.
pause
exit /b 0


:fail
echo.
pause
exit /b 1
