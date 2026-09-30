@echo off
setlocal EnableExtensions
cd /d "%~dp0.."

rem Publish a GitHub release of this repo from the files scripts\build-installer.bat
rem put in release\<version>\:
rem   OLS-<version>-setup.exe                 uploaded as-is
rem   OLS.exe + ols-helper.exe + *.dll        zipped as portable
rem   OLS-<version>-SHA256SUMS.txt            checksums for both
rem Release notes come from release-notes\<tag>.md, the commit message from
rem commit-message.txt (git-ignored, rewrite it for each release).
rem
rem Artifact names all start with OLS, matching the app name (docs/BRAND.md
rem 4.4 R3). The two release/ globs below still accept the pre-rename
rem Open-Local-Server-* name so a dist folder built before the rename can
rem still be published; a fresh build only ever produces OLS-*.
rem
rem Usage: scripts\upload-release.bat [tag] [mode]   (e.g. scripts\upload-release.bat v1.0.0 full)
rem Modes: full = binaries + release notes, notes-only = release notes only,
rem full-force-tag = full, plus move tag %TAG% onto local master and force-push
rem it, so the release points at the latest source code.
rem No mode given = ask 1, 2 or 3. No tag given = the newest release\<version>\
rem folder wins and the tag is v<version>.
set "DISTROOT=release"
set "TAG=%~1"
set "MODE=%~2"
if "%MODE%"=="" call :ask_mode || exit /b 1
if /i "%MODE%"=="1" set "MODE=full"
if /i "%MODE%"=="2" set "MODE=notes-only"
if /i "%MODE%"=="3" set "MODE=full-force-tag"
if /i "%MODE%"=="notes" set "MODE=notes-only"
if /i "%MODE%"=="full" goto :mode_ok
if /i "%MODE%"=="notes-only" goto :mode_ok
if /i "%MODE%"=="full-force-tag" goto :mode_ok
echo Unknown mode "%MODE%". Use full, notes-only or full-force-tag.
exit /b 1

:mode_ok
if not "%TAG%"=="" goto :have_tag

rem No tag: take the newest release\<version>\ folder that has a setup exe.
rem /o-d sorts by date descending, so the first hit is the newest build. The
rem folder name IS the version, so nothing has to be parsed out of a filename.
set "DIST="
for /f "delims=" %%D in ('dir /b /a-d /o-d /ad "%DISTROOT%\*" 2^>nul') do (
  if not defined DIST if exist "%DISTROOT%\%%D\OLS-*-setup.exe" (
    set "DIST=%DISTROOT%\%%D"
    set "VERSION=%%D"
  )
)
if not defined DIST for /f "delims=" %%D in ('dir /b /a-d /o-d /ad "%DISTROOT%\*" 2^>nul') do (
  if not defined DIST if exist "%DISTROOT%\%%D\Open-Local-Server-*-setup.exe" (
    set "DIST=%DISTROOT%\%%D"
    set "VERSION=%%D"
  )
)
if not defined DIST (
  echo No %DISTROOT%\<version>\OLS-*-setup.exe found and no tag given.
  echo Run scripts\build-installer.bat first.
  exit /b 1
)
set "SETUP=OLS-%VERSION%-setup.exe"
set "TAG=v%VERSION%"
goto :tag_ready

:have_tag
set "VERSION=%TAG%"
if /i "%VERSION:~0,1%"=="v" set "VERSION=%VERSION:~1%"
set "VERSION=%VERSION: =%"
set "VERSION=%VERSION:"=%"
set "TAG=v%VERSION%"
set "DIST=%DISTROOT%\%VERSION%"
rem Find the actual setup file on disk. Both spellings are tried because a
rem dist folder built before the rename holds the Open-Local-Server-* one and
rem there is no reason to refuse to publish it -- the tag is what identifies
rem the release, not the filename.
set "SETUP=OLS-%VERSION%-setup.exe"
if /i "%MODE%"=="notes-only" goto :tag_ready
if not exist "%DIST%\%SETUP%" (
  set "SETUP="
  for /f "delims=" %%F in ('dir /b /a-d /o-d "%DIST%\OLS-*-setup.exe" 2^>nul') do if not defined SETUP set "SETUP=%%F"
)
if not defined SETUP (
  set "SETUP=Open-Local-Server-%VERSION%-setup.exe"
  if not exist "%DIST%\%SETUP%" (
    set "SETUP="
    for /f "delims=" %%F in ('dir /b /a-d /o-d "%DIST%\Open-Local-Server-*-setup.exe" 2^>nul') do if not defined SETUP set "SETUP=%%F"
  )
)
if not defined SETUP (
  echo No %DIST%\OLS-*-setup.exe found.
  echo Run scripts\build-installer.bat first.
  exit /b 1
)

:tag_ready
echo Using tag %TAG% (version %VERSION%, mode %MODE%)

set "SETUPPATH=%DIST%\%SETUP%"
set "ZIP=OLS-%VERSION%-portable-win-x64.zip"
set "ZIPPATH=%TEMP%\%ZIP%"
set "STAGE=%TEMP%\ols-portable"
set "SUMS=OLS-%VERSION%-SHA256SUMS.txt"
set "SUMSPATH=%DIST%\%SUMS%"
rem The app exe, then the one it shipped as before the rename. A dist folder
rem built by an older build-installer.bat still has the legacy name and is
rem still publishable; the check below accepts whichever is present, because
rem refusing to release a build that is otherwise complete helps nobody.
set "MAIN_EXE=OLS.exe"
set "LEGACY_EXE=Open Local Server.exe"
set "HELPER_EXE=ols-helper.exe"

where gh >nul 2>&1 || (echo GitHub CLI "gh" not found. & exit /b 1)
where git >nul 2>&1 || (echo git not found. & exit /b 1)
if /i "%MODE%"=="notes-only" goto :commit_step
if not exist "%SETUPPATH%" (echo Missing %SETUPPATH% - run scripts\build-installer.bat first. & exit /b 1)
if not exist "%DIST%\%MAIN_EXE%" if not exist "%DIST%\%LEGACY_EXE%" (echo Missing %DIST%\%MAIN_EXE% - run scripts\build-installer.bat first. & exit /b 1)
if not exist "%DIST%\%HELPER_EXE%" (echo Missing %DIST%\%HELPER_EXE% - run scripts\build-installer.bat first. & exit /b 1)

:commit_step
rem Commit and push first, so a new release tag points at the commit these
rem builds came from.
set "MSGFILE=commit-message.txt"
set "DIRTY="
for /f "delims=" %%L in ('git status --porcelain') do set "DIRTY=1"
if not defined DIRTY goto :nothing_to_commit
if exist "%MSGFILE%" goto :do_commit
if /i "%MODE%"=="notes-only" goto :push_it
echo Uncommitted changes left as-is - no %MSGFILE%, skipping commit.
goto :push_it

:do_commit
echo Committing with message from %MSGFILE%...
git add -A || (echo git add failed. & exit /b 1)
git commit -F "%MSGFILE%" || (echo Commit failed. & exit /b 1)
goto :push_it

:nothing_to_commit
echo Nothing new to commit.

:push_it
echo Pushing...
git push origin HEAD || (echo Push failed. & exit /b 1)

rem Mode 3: the release must point at the newest source, so re-point the tag
rem at local master and force-push it. --force only moves the tag, never
rem rewrites history on the branch.
if /i not "%MODE%"=="full-force-tag" goto :no_force_tag
echo Re-pointing %TAG% at master and force-pushing the tag...
git tag -f "%TAG%" master || (echo git tag -f failed. & exit /b 1)
git push origin "%TAG%" --force || (echo Force-push of %TAG% failed. & exit /b 1)
:no_force_tag

if /i "%MODE%"=="notes-only" goto :notes_only
rem Only the portable files go in the zip, not the setup exe next to them.
echo Zipping portable version...
if exist "%STAGE%" rmdir /s /q "%STAGE%"
mkdir "%STAGE%" || (echo Could not create %STAGE%. & exit /b 1)
rem Whichever name this dist was built under, the zip carries one main exe.
rem Copying both would be wrong: two app exes in a portable zip is the same
rem two-copies problem the installer's two-install guard exists for.
if exist "%DIST%\%MAIN_EXE%" (
  copy /y "%DIST%\%MAIN_EXE%" "%STAGE%\" >nul || (echo Could not copy %MAIN_EXE%. & exit /b 1)
) else (
  copy /y "%DIST%\%LEGACY_EXE%" "%STAGE%\" >nul || (echo Could not copy %LEGACY_EXE%. & exit /b 1)
)
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

rem Update manifest for the in-app updater (latest.json + latest.json.minisig).
rem Needs minisign and the signing key; without them binaries still upload.
set "LATEST=%DIST%\latest.json"
set "LATESTSIG=%LATEST%.minisig"
del /f /q "%LATEST%" "%LATESTSIG%" >nul 2>&1
set "MINISIGN="
for /f "delims=" %%m in ('where minisign 2^>nul') do if not defined MINISIGN set "MINISIGN=%%m"
if not defined MINISIGN_KEY set "MINISIGN_KEY=%USERPROFILE%\.minisign\ols-update.key"
if not defined MINISIGN (
  echo [!] minisign not found - skipping latest.json ^(updater will 404^).
  echo     Fix: cargo install minisign, then re-run this script.
  goto :manifest_done
)
if not exist "%MINISIGN_KEY%" (
  echo [!] No signing key at %MINISIGN_KEY% - skipping latest.json.
  echo     Fix: minisign -G -W -p "%MINISIGN_KEY%.pub" -s "%MINISIGN_KEY%", bake the RW.. line as OLS_UPDATE_PUBKEY at build time, then re-run.
  goto :manifest_done
)
echo Writing latest.json...
set "UP_VERSION=%VERSION%"
set "UP_TAG=%TAG%"
set "UP_SETUP=%SETUPPATH%"
set "UP_NOTES=release-notes\%TAG%.md"
set "UP_OUT=%LATEST%"
powershell -NoProfile -Command "$sha=[BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash([IO.File]::ReadAllBytes($env:UP_SETUP))).Replace('-','').ToLower(); $size=(Get-Item $env:UP_SETUP).Length; $notes=''; if (($env:UP_NOTES) -and (Test-Path $env:UP_NOTES)) { $notes=[IO.File]::ReadAllText($env:UP_NOTES) }; $url='https://github.com/kz370/OpenLocalServer/releases/download/'+$env:UP_TAG+'/'+[IO.Path]::GetFileName($env:UP_SETUP); $o=[ordered]@{version=$env:UP_VERSION; notes=$notes; pub_date=(Get-Date -Format yyyy-MM-dd); platforms=[ordered]@{'windows-x86_64'=[ordered]@{url=$url; sha256=$sha; size=$size}}}; $o | ConvertTo-Json -Depth 5 | Out-String | ForEach-Object { [IO.File]::WriteAllText($env:UP_OUT, $_) }"
if errorlevel 1 (echo Manifest failed. & exit /b 1)
"%MINISIGN%" -Sm "%LATEST%" -s "%MINISIGN_KEY%"
if errorlevel 1 (echo Sign failed. & exit /b 1)
echo       signed latest.json + latest.json.minisig
echo       Public key for builds ^(OLS_UPDATE_PUBKEY^):
type "%MINISIGN_KEY%.pub"
:manifest_done

:notes_only
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
  echo Release %TAG% exists - updating it.
  if exist "%NOTES%" (
    echo Updating notes from %NOTES%
    gh release edit "%TAG%" --notes-file "%NOTES%" || (echo Notes update failed. & exit /b 1)
  )
)

if /i "%MODE%"=="notes-only" goto :notes_done
set "EXTRA="
if exist "%DIST%\latest.json" set "EXTRA="%DIST%\latest.json" "%DIST%\latest.json.minisig""
if defined EXTRA (echo Uploading update manifest too.) else (echo No latest.json - updater will 404 until a run signs one.)
gh release upload "%TAG%" "%SETUPPATH%" "%ZIPPATH%" "%SUMSPATH%" %EXTRA% --clobber || (echo Upload failed. & exit /b 1)

del /f /q "%ZIPPATH%" >nul 2>&1
echo Done. Uploaded %SETUP%, %ZIP% and %SUMS% to %TAG%.
endlocal & exit /b 0

:notes_done
echo Done. Updated notes for %TAG% - no binaries uploaded.
endlocal & exit /b 0

rem Prompt for upload mode when the second argument is missing.
:ask_mode
echo Select upload mode:
echo   1 - full: binaries + release notes
echo   2 - notes-only: release notes only
echo   3 - full-force-tag: binaries + notes, tag re-pointed at master ^(force^)
set "CHOICE="
set /p "CHOICE=Enter 1, 2 or 3 [1]: "
if "%CHOICE%"=="" set "CHOICE=1"
if "%CHOICE%"=="1" set "MODE=full" & exit /b 0
if "%CHOICE%"=="2" set "MODE=notes-only" & exit /b 0
if "%CHOICE%"=="3" set "MODE=full-force-tag" & exit /b 0
if /i "%CHOICE%"=="full" set "MODE=full" & exit /b 0
if /i "%CHOICE%"=="notes-only" set "MODE=notes-only" & exit /b 0
if /i "%CHOICE%"=="full-force-tag" set "MODE=full-force-tag" & exit /b 0
echo Invalid choice. & exit /b 1

rem Append SHA-256 of %1 to %2. No parens in echoes: this file uses
rem single-line ( ... ) blocks elsewhere.
:sum_one
set "SH="
for /f "skip=1 delims=" %%h in ('certutil -hashfile "%~1" SHA256 2^>nul') do if not defined SH set "SH=%%h"
if not defined SH exit /b 1
>> "%~2" echo %SH%  %~nx1
set "SH="
exit /b 0
