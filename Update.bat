@echo off
title VoxLink Updater
echo ╔══════════════════════════════════════╗
echo ║        VoxLink Updater v1.0          ║
echo ╚══════════════════════════════════════╝
echo.

echo [1/2] Checking for updates...
git pull
if %ERRORLEVEL% neq 0 (
    echo.
    echo WARNING: Could not check for updates. Continuing with local version...
    echo.
)

echo.
echo [2/2] Building VoxLink (Release)...
cargo build --bin voxlink --release

if %ERRORLEVEL% neq 0 (
    echo.
    echo ════════════════════════════════════════
    echo   BUILD FAILED! Check errors above.
    echo ════════════════════════════════════════
    pause
    exit /b %ERRORLEVEL%
)

echo.
echo ════════════════════════════════════════
echo   Update complete!
echo   Run VoxLink from: target\release\voxlink.exe
echo ════════════════════════════════════════
pause
