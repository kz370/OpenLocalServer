@echo off
setlocal EnableExtensions
rem ============================================================================
rem OpenLocalServer Version Update Script
rem
rem Updates the version across all project manifests, configurations, lockfiles,
rem documentation, changelog, and release notes:
rem   - Cargo.toml ([workspace.package] version)
rem   - src-tauri/tauri.conf.json (version)
rem   - ui/package.json (version)
rem   - ui/package-lock.json (version in root and packages[""])
rem   - Cargo.lock (ols-cli, ols-core, ols-helper, openlocalserver)
rem   - README.md (version badge)
rem   - CHANGELOG.md (adds [version] section if missing)
rem   - release-notes/v<version>.md (draft template if missing)
rem
rem Usage:
rem   scripts\update-version.bat [version] [options]
rem   update-version.bat 2.3.0
rem   update-version.bat --dry-run 2.3.0
rem   update-version.bat (interactive mode)
rem ============================================================================

set "ROOT=%~dp0..\"
set "SCRIPT=%~dp0update-version.ps1"

if not exist "%SCRIPT%" (
  echo.
  echo [x] Problem: scripts\update-version.ps1 is missing.
  echo     Cause: incomplete checkout, or the script was moved or renamed.
  echo     Fix: restore scripts\update-version.ps1 and run this again.
  echo.
  exit /b 1
)

rem Find PowerShell executable (prefer pwsh 7+, fallback to powershell 5.1)
set "PS_CMD="
where pwsh >nul 2>nul
if %errorlevel% equ 0 (
  set "PS_CMD=pwsh"
) else (
  where powershell >nul 2>nul
  if %errorlevel% equ 0 (
    set "PS_CMD=powershell"
  )
)

if not defined PS_CMD (
  echo.
  echo [x] Problem: PowerShell not found on PATH.
  echo     Cause: neither pwsh nor powershell.exe is accessible in PATH.
  echo     Fix: install PowerShell or ensure powershell.exe is on your system PATH.
  echo.
  exit /b 1
)

rem Switch to UTF-8 code page so Unicode characters and emojis display cleanly
chcp 65001 >nul

rem Handle help flags cleanly
if "%~1"=="/?" goto :show_help
if "%~1"=="-h" goto :show_help
if "%~1"=="--help" goto :show_help
if "%~1"=="help" goto :show_help

rem Invoke PowerShell script with all supplied arguments
"%PS_CMD%" -NoProfile -ExecutionPolicy Bypass -File "%SCRIPT%" %*
set "EXIT_CODE=%errorlevel%"
exit /b %EXIT_CODE%

:show_help
"%PS_CMD%" -NoProfile -ExecutionPolicy Bypass -File "%SCRIPT%" -Help
exit /b 0
