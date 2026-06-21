@echo off
echo Building VoxLink (Release Profile)...
cargo build --release

if %ERRORLEVEL% neq 0 (
    echo.
    echo Build failed!
    pause
    exit /b %ERRORLEVEL%
)

echo.
echo Packaging VoxLink...
if exist VoxLink-Release rmdir /s /q VoxLink-Release
if exist VoxLink.zip del /q VoxLink.zip

mkdir VoxLink-Release
copy target\release\voxlink.exe VoxLink-Release\ >nul

powershell -Command "Compress-Archive -Path VoxLink-Release\* -DestinationPath VoxLink.zip -Force"

echo.
echo Done! You can now send VoxLink.zip to your friends.
echo They just need to extract it and run voxlink.exe.
pause
