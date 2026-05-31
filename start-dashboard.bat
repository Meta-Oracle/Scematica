@echo off
REM Scematica v1.11.0 Dashboard Launcher
REM This script starts the dashboard in full mode (requires SCEMA tokens)

echo ========================================
echo Scematica v1.11.0 Dashboard
echo ========================================
echo.
echo Starting dashboard...
echo Press Ctrl+C to stop
echo.

cargo run --release --bin dashboard

pause
