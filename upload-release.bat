@echo off
setlocal EnableExtensions
cd /d "%~dp0"

rem Publish a GitHub release of this repo from the files build-installer.bat
rem put in release\:
rem   release\Open-Local-Server-<version>-setup.exe   uploaded as-is
rem   release\Open Local Server.exe + ols-helper.exe + *.dll   zipped as portable
rem   release\Open-Local-Server-<version>-SHA256SUMS.txt       checksums for both
rem Release notes come from release-notes\<tag>.md, the commit message from
rem commit-message.txt (git-ignored, rewrite it for each release).
rem
rem Usage: upload-release.bat [tag]   (e.g. upload-release.bat v1.0.0)
rem No tag given = read the version from the setup exe name
rem (Open-Local-Server-<version>-setup.exe) and use tag v<version>.
set "DIST=release"
set "TAG=%~1"
if not "%TAG%"=="" goto :have_tag

rem Newest setup exe wins if there are several.
set "SETUP="
for /f "delims=" %%F in ('dir /b /a-d /o-d "%DIST%\Open-Local-Server-*-setup.exe" 2^>nul') do if not defined SETUP set "SETUP=%%F"
if not defined SETUP (
  echo No %DIST%\Open-Local-Server-*-setup.exe found and no tag given.
  echo Run build-installer.bat first.
  exit /b 1
)
rem Strip prefix/suffix, then all spaces + quotes (build-installer once
rem emitted "Open-Local-Server- 1.0.0-setup.exe" with a leading space).
set "VERSION=%SETUP:Open-Local-Server-=%"
set "VERSION=%VERSION:-setup.exe=%"
set "VERSION=%VERSION: =%"
set "VERSION=%VERSION:"=%"
set "TAG=v%VERSION%"
goto :tag_ready

:have_tag
set "VERSION=%TAG%"
if /i "%VERSION:~0,1%"=="v" set "VERSION=%VERSION:~1%"
set "VERSION=%VERSION: =%"
set "VERSION=%VERSION:"=%"
set "TAG=v%VERSION%"
rem Find the actual setup file on disk (tolerates the old leading-space name).
set "SETUP=Open-Local-Server-%VERSION%-setup.exe"
if not exist "%DIST%\%SETUP%" (
  set "SETUP="
  for /f "delims=" %%F in ('dir /b /a-d /o-d "%DIST%\Open-Local-Server-*-setup.exe" 2^>nul') do if not defined SETUP set "SETUP=%%F"
  if not defined SETUP (
    echo No %DIST%\Open-Local-Server-*-setup.exe found.
    echo Run build-installer.bat first.
    exit /b 1
  )
)

:tag_ready
echo Using tag %TAG% (version %VERSION%)

set "SETUPPATH=%DIST%\%SETUP%"
set "ZIP=Open-Local-Server-%VERSION%-portable-win-x64.zip"
set "ZIPPATH=%TEMP%\%ZIP%"
set "STAGE=%TEMP%\open-local-server-portable"
set "SUMS=Open-Local-Server-%VERSION%-SHA256SUMS.txt"
set "SUMSPATH=%DIST%\%SUMS%"
set "MAIN_EXE=Open Local Server.exe"
set "HELPER_EXE=ols-helper.exe"

where gh >nul 2>&1 || (echo GitHub CLI "gh" not found. & exit /b 1)
where git >nul 2>&1 || (echo git not found. & exit /b 1)
if not exist "%SETUPPATH%" (echo Missing %SETUPPATH% - run build-installer.bat first. & exit /b 1)
if not exist "%DIST%\%MAIN_EXE%" (echo Missing %DIST%\%MAIN_EXE% - run build-installer.bat first. & exit /b 1)
if not exist "%DIST%\%HELPER_EXE%" (echo Missing %DIST%\%HELPER_EXE% - run build-installer.bat first. & exit /b 1)

rem Commit and push first, so a new release tag points at the commit these
rem builds came from.
set "MSGFILE=commit-message.txt"
set "DIRTY="
for /f "delims=" %%L in ('git status --porcelain') do set "DIRTY=1"
if defined DIRTY (
  if not exist "%MSGFILE%" (echo Missing %MSGFILE% - write the commit message there first. & exit /b 1)
  echo Committing with message from %MSGFILE%...
  git add -A || (echo git add failed. & exit /b 1)
  git commit -F "%MSGFILE%" || (echo Commit failed. & exit /b 1)
) else (
  echo Nothing new to commit.
)
echo Pushing...
git push origin HEAD || (echo Push failed. & exit /b 1)

rem Only the portable files go in the zip, not the setup exe next to them.
echo Zipping portable version...
if exist "%STAGE%" rmdir /s /q "%STAGE%"
mkdir "%STAGE%" || (echo Could not create %STAGE%. & exit /b 1)
copy /y "%DIST%\%MAIN_EXE%" "%STAGE%\" >nul || (echo Could not copy %MAIN_EXE%. & exit /b 1)
copy /y "%DIST%\%HELPER_EXE%" "%STAGE%\" >nul || (echo Could not copy %HELPER_EXE%. & exit /b 1)
for %%d in ("%DIST%\*.dll") do copy /y "%%~d" "%STAGE%\" >nul || (echo Could not copy %%~nxd. & exit /b 1)
if exist "%ZIPPATH%" del /f /q "%ZIPPATH%"
powershell -NoProfile -Command "Compress-Archive -Path (Join-Path $env:STAGE '*') -DestinationPath $env:ZIPPATH -Force" || (echo Zip failed. & exit /b 1)
rmdir /s /q "%STAGE%"

rem Checksums so downloaders can verify (SHA-256 per repo safety rules).
rem certutil ships with every Windows; Get-FileHash does not.
echo Writing %SUMS%...
if exist "%SUMSPATH%" del /f /q "%SUMSPATH%"
for %%f in ("%SETUPPATH%" "%ZIPPATH%") do call :sum_one "%%~f" "%SUMSPATH%" || (echo Checksum failed. & exit /b 1)

rem Release notes: release-notes\<tag>.md if present, else GitHub's generated notes
rem (new releases only). An existing release keeps its notes unless the file exists.
set "NOTES=release-notes\%TAG%.md"

gh release view "%TAG%" >nul 2>&1
if errorlevel 1 (
  echo Release %TAG% not found - creating it...
  if exist "%NOTES%" (
    echo Using notes from %NOTES%
    gh release create "%TAG%" --title "%TAG%" --notes-file "%NOTES%" || (echo Create failed. & exit /b 1)
  ) else (
    echo No %NOTES% - using GitHub generated notes.
    gh release create "%TAG%" --title "%TAG%" --generate-notes || (echo Create failed. & exit /b 1)
  )
) else (
  echo Release %TAG% exists - replacing assets.
  if exist "%NOTES%" (
    echo Updating notes from %NOTES%
    gh release edit "%TAG%" --notes-file "%NOTES%" || (echo Notes update failed. & exit /b 1)
  )
)

gh release upload "%TAG%" "%SETUPPATH%" "%ZIPPATH%" "%SUMSPATH%" --clobber || (echo Upload failed. & exit /b 1)

del /f /q "%ZIPPATH%" >nul 2>&1
echo Done. Uploaded %SETUP%, %ZIP% and %SUMS% to %TAG%.
endlocal & exit /b 0

rem Append SHA-256 of %1 to %2. No parens in echoes: this file uses
rem single-line ( ... ) blocks elsewhere.
:sum_one
set "SH="
for /f "skip=1 delims=" %%h in ('certutil -hashfile "%~1" SHA256 2^>nul') do if not defined SH set "SH=%%h"
if not defined SH exit /b 1
>> "%~2" echo %SH%  %~nx1
set "SH="
exit /b 0
