@echo off
setlocal
REM ============================================================
REM  DreamWeaver launcher  --  IMPORTANT: keep this file ASCII-only
REM
REM  WHY IT COPIES THE EXE:
REM    Tauri / WebView2 fails to create the main window when the exe
REM    lives in a folder literally named "ai<CJK>" (verified: the same
REM    exe works from "ai<CJK>x", "a<CJK>", "<CJK>", or plain ASCII).
REM    So we copy the exe into an ASCII folder and run it from there.
REM    The DATA stays inside the project:  <project>\data
REM
REM  This is a RELEASE build: the web UI is embedded, so no dev
REM  server and no Rust toolchain is needed.
REM ============================================================

set "PROJ=%~dp0"
set "SRC=%PROJ%src-tauri\target\release\app.exe"
set "DATA=%PROJ%data"
set "RUNDIR=%LOCALAPPDATA%\DreamWeaverRun"
if not defined LOCALAPPDATA set "RUNDIR=D:\DreamWeaverRun"

if not exist "%SRC%" (
  echo [ERROR] Release build not found:
  echo         %SRC%
  echo.
  echo   Build it first:
  echo       npm run build
  echo       cd src-tauri ^&^& cargo build --release
  echo.
  pause
  exit /b 1
)

if not exist "%RUNDIR%" mkdir "%RUNDIR%" 2>nul
if not exist "%RUNDIR%" (
  set "RUNDIR=D:\DreamWeaverRun"
  mkdir "%RUNDIR%" 2>nul
)

echo Copying the app to a plain-ASCII folder...
copy /y "%SRC%" "%RUNDIR%\app.exe" >nul
if errorlevel 1 (
  echo [ERROR] Could not copy to %RUNDIR%
  echo         Close any running DreamWeaver window and retry.
  echo.
  pause
  exit /b 1
)

if not exist "%DATA%\library" (
  echo [WARN] No data found at %DATA%
  echo        The app will start with an empty library.
  echo.
)

set "DREAMWEAVER_DATA_DIR=%DATA%"

echo ============================================================
echo   Run dir  : %RUNDIR%
echo   Data dir : %DATA%
echo ============================================================
echo   Starting...
echo.

start "" "%RUNDIR%\app.exe"
exit /b 0
