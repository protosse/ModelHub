@echo off
REM Windows CMD entry → node scripts/dev-launch.mjs → dev.ps1
setlocal
cd /d "%~dp0.."
node "%~dp0dev-launch.mjs" %*
exit /b %ERRORLEVEL%
