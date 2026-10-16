@echo off
setlocal
REM ============================================================
REM  DreamWeaver (dev launcher)
REM
REM  WHY THIS EXISTS:
REM    The app needs a WRITABLE data folder. On some machines a
REM    security product blocks writes to %%APPDATA%%, so the data
REM    has to live inside the project folder.
REM    This script pins the data folder to  <project>\data
REM    (that folder already holds your stories).
REM
REM  TO CHANGE THE LOCATION: edit the SET line below.
REM ============================================================

set "DREAMWEAVER_DATA_DIR=%~dp0data"

echo ============================================================
echo   Data dir : %DREAMWEAVER_DATA_DIR%
echo   App      : %~dp0src-tauri\target\debug\app.exe
echo ============================================================
echo   Starting... (this window prints startup logs)
echo.

"%~dp0src-tauri\target\debug\app.exe"

echo.
echo ============================================================
echo   App exited with code %ERRORLEVEL%
echo     (0 = normal close, 1 = startup failed - see above)
echo ============================================================
pause
