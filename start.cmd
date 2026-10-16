@echo off
setlocal
REM ============================================================
REM  DreamWeaver launcher  --  IMPORTANT: keep this file ASCII-only
REM  (cmd.exe reads .cmd as GBK; Chinese comments get shredded and
REM   fragments are executed as commands -- verified the hard way.)
REM
REM  WHY IT COPIES THE EXE:
REM    Tauri / WebView2 cannot create the main window when the exe
REM    lives in a folder literally named "ai<CJK>" (verified: the same
REM    exe works from "ai<CJK>x", "a<CJK>", "<CJK>", or plain ASCII).
REM    So we copy the exe into an ASCII folder and run it from there.
REM    The DATA stays inside the project:  <project>\data
REM
REM  NEEDS A "PACKAGED" BUILD:
REM    The exe must be built WITH the custom-protocol feature, otherwise
REM    Tauri thinks it is a dev build and loads the UI from
REM    http://localhost:5173 -> the window shows "localhost refused to
REM    connect". See the [features] section in src-tauri\Cargo.toml.
REM ============================================================

set "PROJ=%~dp0"
set "SRC=%PROJ%src-tauri\target\release\app.exe"
set "DATA=%PROJ%data"
set "RUNDIR=%LOCALAPPDATA%\DreamWeaverRun"
if not defined LOCALAPPDATA set "RUNDIR=D:\DreamWeaverRun"

if not exist "%SRC%" (
  echo [ERROR] Packaged build not found:
  echo         %SRC%
  echo.
  echo   Build it first ^(one time^):
  echo       npm run build
  echo       cd src-tauri
  echo       cargo build --release --features custom-protocol
  echo.
  echo   NOTE: the --features custom-protocol part is REQUIRED.
  echo         Without it the app loads the UI from localhost:5173
  echo         and shows "localhost refused to connect".
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
