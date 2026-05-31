@echo off
REM Scematica v1.11.0 Dashboard Launcher (Demo Mode)
REM This script starts the dashboard in demo mode (no tokens or RPC required)

echo ========================================
echo Scematica v1.11.0 Dashboard - DEMO MODE
echo ========================================
echo.
echo Starting dashboard in demo mode...
echo No SCEMA tokens or RPC connection required
echo Press Ctrl+C to stop
echo.

cargo run --release --bin dashboard -- --demo

pause
