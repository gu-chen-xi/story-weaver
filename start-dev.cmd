@echo off
setlocal
REM ============================================================
REM  DreamWeaver (dev launcher)
REM
REM  !! IMPORTANT !!
REM  This is the DEBUG build. It loads the UI from
REM  http://localhost:5173, so it NEEDS THE VITE DEV SERVER:
REM      npm run dev
REM  Without that server the window opens and closes at once.
REM  If you just want to USE the app, run start-release.cmd instead.
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
