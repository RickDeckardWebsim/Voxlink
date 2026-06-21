@echo off
title VoxLink Updater

echo Checking for updates...
git pull

echo.
echo Building VoxLink (Release Profile)...
cargo build --bin voxlink --release

if %ERRORLEVEL% neq 0 (
    echo.
    echo Build failed! Please check the errors above.
    pause
    exit /b %ERRORLEVEL%
)

echo.
echo Build successful! Launching VoxLink...
start "" "target\release\voxlink.exe"

exit
