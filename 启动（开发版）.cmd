@echo off
REM 织梦机 · 开发版启动脚本
REM
REM 作用：把数据根固定到 D:\织梦机数据，避免"受限环境下自动兜底到 target\debug\data"
REM      （target 被 cargo clean 清掉时数据会一起没）。
REM
REM 想换位置：改下面这一行的路径即可（用绝对路径，不要带引号里的转义）。

set DREAMWEAVER_DATA_DIR=D:\织梦机数据

echo 数据目录：%DREAMWEAVER_DATA_DIR%
echo 启动中...
"%~dp0src-tauri\target\debug\app.exe"
