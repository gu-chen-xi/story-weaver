@echo off
setlocal
REM ============================================================
REM  DreamWeaver - Startup Self-Check
REM  IMPORTANT: keep this file ASCII-only.
REM  (cmd.exe reads .cmd as GBK; Chinese comments get shredded and
REM   fragments are executed as commands -- learned the hard way.)
REM
REM  Runs a PowerShell script with -ExecutionPolicy Bypass, because
REM  running a .ps1 directly may be blocked by the execution policy.
REM ============================================================

set "PS1=%~dp0selfcheck.ps1"
set "PSEXE=%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe"

if not exist "%PS1%" (
  echo [ERROR] selfcheck.ps1 not found: %PS1%
  pause
  exit /b 1
)

if not exist "%PSEXE%" (
  echo [ERROR] Windows PowerShell not found: %PSEXE%
  pause
  exit /b 1
)

"%PSEXE%" -NoProfile -ExecutionPolicy Bypass -File "%PS1%"
set "RC=%ERRORLEVEL%"

echo.
echo ============================================================
echo   Self-check finished (exit code %RC%)
echo   The report was also saved under:  debug-diagnostics\
echo ============================================================
pause
exit /b %RC%
