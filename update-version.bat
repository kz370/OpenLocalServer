@echo off
setlocal EnableExtensions
rem Convenience root entry point for scripts\update-version.bat
"%~dp0scripts\update-version.bat" %*
exit /b %errorlevel%
