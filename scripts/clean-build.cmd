@echo off
REM Wrapper so this can be double-clicked (Windows blocks .ps1 by default).
REM   clean-build.cmd          -> delete target\debug
REM   clean-build.cmd report   -> only print sizes
REM   clean-build.cmd all      -> delete the whole target folder
setlocal
set "PS=powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0clean-build.ps1""
if "%~1"=="" (
  %PS%
) else (
  %PS% -Mode %~1
)
echo.
pause
endlocal
