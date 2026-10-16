@echo off
setlocal
REM ============================================================
REM  DreamWeaver - Release launcher (no dev server needed)
REM
REM  This runs the RELEASE build, which has the frontend embedded,
REM  so it does NOT need a vite dev server or a Rust toolchain.
REM
REM  Data folder is pinned to <project>\data (holds your stories),
REM  because this machine's security product blocks writes to
REM  %%APPDATA%% for this program.
REM ============================================================

set "DREAMWEAVER_DATA_DIR=%~dp0data"
set "APP=%~dp0src-tauri\target\release\app.exe"

if not exist "%APP%" (
  echo [ERROR] Release build not found:
  echo         %APP%
  echo.
  echo   Build it first:   cd src-tauri ^&^& cargo build --release
  echo.
  pause
  exit /b 1
)

echo ============================================================
echo   Data dir : %DREAMWEAVER_DATA_DIR%
echo   App      : %APP%
echo ============================================================
echo   Starting... (this window prints startup logs)
echo.

"%APP%"

echo.
echo ============================================================
echo   App exited with code %ERRORLEVEL%
echo     ^(0 = normal close, 1 = startup failed - see above^)
echo ============================================================
pause
