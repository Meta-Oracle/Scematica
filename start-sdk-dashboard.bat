@echo off
REM Scematica v1.11.0 - ScemaDEX SDK Dashboard Launcher
REM Drives intents through the conviction / route / bond pipeline in a TUI.
REM SIM mode is the default (offline, no keypair/RPC). Pass --live for real
REM Jupiter quotes:  start-sdk-dashboard.bat --live

echo ========================================
echo ScemaDEX SDK Dashboard
echo ========================================
echo.
if "%~1"=="--live" (
    echo Mode: LIVE  ^(real Jupiter quotes - experimental^)
) else (
    echo Mode: SIM   ^(offline default; pass --live for real quotes^)
)
echo Keys: [q]/[Esc] quit  [space] pause  [s] step
echo Press Ctrl+C to stop
echo.

cargo run --release --bin sdk-dashboard -- %*

pause
