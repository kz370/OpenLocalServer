@echo off
rem The command line is ols-cli.exe, because the app itself is OLS.exe and Windows
rem does not tell two file names apart by case alone. This shim keeps `ols <command>`
rem working wherever the install folder is on PATH, and keeps it pointing at the CLI
rem rather than at the app.
"%~dp0ols-cli.exe" %*
exit /b %ERRORLEVEL%
