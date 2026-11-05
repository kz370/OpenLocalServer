@echo off
setlocal EnableExtensions EnableDelayedExpansion
rem ============================================================================
rem OpenLocalServer Pre-release Checks
rem
rem Runs the checks AGENTS.md asks for before a release, one after another, and
rem ends with a short summary. A failing step does not stop the others, so one
rem run shows everything that is wrong.
rem
rem   cargo fmt          formatting (checked, or applied with -fix)
rem   cargo clippy       lints for ols-core and ols-helper
rem   cargo test         ols-core and ols-helper
rem   cargo test         ols-mcp (the AI assistant tools)
rem   npm run lint       UI lint
rem   npm run build      UI build (also checks the TypeScript)
rem
rem Usage:
rem   scripts\run-checks.bat              everything
rem   scripts\run-checks.bat -quick       format check + Docker tests + MCP tests
rem   scripts\run-checks.bat -docker      only the Docker tests (about a second)
rem   scripts\run-checks.bat -noui        everything except the UI steps
rem   scripts\run-checks.bat -fix         apply cargo fmt instead of only checking
rem   scripts\run-checks.bat -force       run even if the app is running
rem   scripts\run-checks.bat -help
rem
rem Exit code: 0 all passed, 1 something failed, 2 could not start.
rem Full output of every step: target\ols-checks\
rem
rem Do NOT run this while "cargo tauri dev" is running: it rebuilds the same
rem files and the tests would fight the app over them (AGENTS.md).
rem ============================================================================

set "MODE=all"
set "NOUI=0"
set "FIX=0"
set "FORCE=0"

:args
if "%~1"=="" goto :argsdone
if /I "%~1"=="-quick"  set "MODE=quick"  & goto :nextarg
if /I "%~1"=="-docker" set "MODE=docker" & goto :nextarg
if /I "%~1"=="-noui"   set "NOUI=1"      & goto :nextarg
if /I "%~1"=="-fix"    set "FIX=1"       & goto :nextarg
if /I "%~1"=="-force"  set "FORCE=1"     & goto :nextarg
if /I "%~1"=="-help"   goto :help
if /I "%~1"=="-h"      goto :help
if /I "%~1"=="/?"      goto :help
echo.
echo [x] Problem: unknown option "%~1".
echo     Cause: this script only knows -quick, -docker, -noui, -fix, -force and -help.
echo     Fix: run scripts\run-checks.bat -help to see them.
echo.
exit /b 2
:nextarg
shift
goto :args

:help
echo.
echo OLS pre-release checks
echo   run-checks.bat           everything: format, clippy, all tests, UI lint and build
echo   run-checks.bat -quick    format check, Docker tests, MCP tests
echo   run-checks.bat -docker   only the Docker tests
echo   run-checks.bat -noui     skip the UI steps
echo   run-checks.bat -fix      apply cargo fmt instead of only checking
echo   run-checks.bat -force    run even if the app is running
echo Logs: target\ols-checks\
echo.
exit /b 0

:argsdone
cd /d "%~dp0"
if errorlevel 1 goto :badroot

where cargo >nul 2>nul
if errorlevel 1 goto :nocargo
if "%NOUI%"=="1" goto :uichecked
if /I "%MODE%"=="docker" goto :uichecked
where npm >nul 2>nul
if errorlevel 1 goto :nonpm
:uichecked

if "%FORCE%"=="1" goto :runcheck
tasklist /FI "IMAGENAME eq openlocalserver.exe" 2>nul | find /I "openlocalserver.exe" >nul
if not errorlevel 1 goto :appruns
tasklist /FI "IMAGENAME eq cargo-tauri.exe" 2>nul | find /I "cargo-tauri.exe" >nul
if not errorlevel 1 goto :appruns
:runcheck

set "LOGDIR=%CD%\target\ols-checks"
if not exist "%LOGDIR%" mkdir "%LOGDIR%"
del /q "%LOGDIR%\*.log" "%LOGDIR%\summary.txt" 2>nul
set "SUM=%LOGDIR%\summary.txt"
set "N=0"
set "FAILS=0"
call :now START

echo.
echo ===== OLS pre-release checks  ^(mode: %MODE%^) =====

if /I "%MODE%"=="docker" goto :m_docker
if /I "%MODE%"=="quick"  goto :m_quick

:m_all
call :fmt
call :step "Clippy" "cargo clippy -p ols-core -p ols-helper --all-targets" ""
call :step "Tests: ols-core and ols-helper" "cargo test -p ols-core -p ols-helper -- --skip ca:: --skip certs::" ""
call :step "Tests: ols-mcp" "cargo test -p ols-mcp" ""
if "%NOUI%"=="1" goto :summary
call :step "UI lint" "npm run lint" "ui"
call :step "UI build" "npm run build" "ui"
goto :summary

:m_quick
call :fmt
call :step "Tests: Docker" "cargo test -p ols-core docker" ""
call :step "Tests: ols-mcp" "cargo test -p ols-mcp" ""
goto :summary

:m_docker
call :step "Tests: Docker" "cargo test -p ols-core docker" ""
goto :summary

:summary
call :now END
set /a TOTAL=END-START
if %TOTAL% lss 0 set /a TOTAL+=86400
echo.
echo ===== Summary =====
type "%SUM%"
echo.
if "%FAILS%"=="0" goto :allgood
echo [x] %FAILS% of %N% step^(s^) failed in %TOTAL%s. Logs: target\ols-checks\
echo     Fix: open the log named above, fix the first error, and run this again.
echo     On Windows every step should pass. A failure here is real.
echo.
exit /b 1
:allgood
echo [ok] All %N% step^(s^) passed in %TOTAL%s.
echo.
exit /b 0

rem ---------------------------------------------------------------- steps

:fmt
if "%FIX%"=="1" goto :fmtfix
call :step "Format check" "cargo fmt --all -- --check" ""
exit /b 0
:fmtfix
call :step "Format (applying cargo fmt)" "cargo fmt --all" ""
exit /b 0

rem :step "label" "command" "folder or empty"
:step
set /a N+=1
set "LABEL=%~1"
set "CMD=%~2"
set "WD=%~3"
set "LOG=%LOGDIR%\%N%.log"
echo.
echo [%N%] %LABEL% ...
call :now T0
if not "%WD%"=="" pushd "%WD%"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0run-step.ps1" -Command "%CMD%" -Log "%LOG%"
set "RC=%errorlevel%"
if not "%WD%"=="" popd
call :now T1
set /a DT=T1-T0
if %DT% lss 0 set /a DT+=86400
if not "%RC%"=="0" goto :stepfail
echo     PASS  ^(%DT%s^)
>>"%SUM%" echo   PASS  %LABEL%  ^(%DT%s^)
for /f "usebackq delims=" %%L in (`findstr /C:"test result:" "%LOG%"`) do echo     %%L
exit /b 0
:stepfail
set /a FAILS+=1
echo     FAIL  ^(%DT%s, exit code %RC%^)  full log: %LOG%
>>"%SUM%" echo   FAIL  %LABEL%  ^(%DT%s^)  log: %LOG%
echo     --- last lines of the log ---
powershell -NoProfile -Command "Get-Content -LiteralPath '%LOG%' -Tail 25"
echo     -----------------------------
exit /b 0

rem :now VARNAME  -> seconds since midnight
:now
set "TT=%time: =0%"
set /a %~1=(1%TT:~0,2%-100)*3600+(1%TT:~3,2%-100)*60+(1%TT:~6,2%-100)
exit /b 0

rem ---------------------------------------------------------------- problems

:badroot
echo.
echo [x] Problem: could not open the project folder.
echo     Cause: scripts\run-checks.bat was moved out of the repository's scripts folder.
echo     Fix: restore it to scripts\ and run it again.
echo.
exit /b 2

:nocargo
echo.
echo [x] Problem: cargo was not found.
echo     Cause: Rust is not installed, or cargo is not on PATH in this window.
echo     Fix: install Rust from https://rustup.rs, open a new terminal, and run this again.
echo.
exit /b 2

:nonpm
echo.
echo [x] Problem: npm was not found.
echo     Cause: Node.js is not installed, or npm is not on PATH in this window.
echo     Fix: install Node.js, open a new terminal, or run this with -noui to skip the UI steps.
echo.
exit /b 2

:appruns
echo.
echo [x] Problem: OLS or "cargo tauri dev" is running.
echo     Cause: the tests rebuild the same files and fight the running app over them.
echo     Fix: close the app and "cargo tauri dev", then run this again. To run anyway, use -force.
echo.
exit /b 2
