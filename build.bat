@echo off
REM Scematica v1.11.0 Build Script
REM Compiles all binaries in release mode.
REM Run init.bat first if this is a fresh checkout (fetches dependencies).

echo ========================================
echo Scematica v1.11.0 Build
echo ========================================
echo.
echo Building all binaries in release mode...
echo This may take 5-10 minutes on first run
echo.

cargo build --release

if %ERRORLEVEL% EQU 0 (
    echo.
    echo ========================================
    echo Build completed successfully!
    echo ========================================
    echo.
    echo Trading bot binaries:
    echo   target\release\dashboard.exe
    echo   target\release\sniper.exe
    echo   target\release\arb.exe
    echo   target\release\scematica-protocol.exe
    echo.
    echo ScemaDEX SDK binaries:
    echo   target\release\sdk-dashboard.exe
    echo   target\release\scemadex-relay.exe
    echo.
) else (
    echo.
    echo ========================================
    echo Build FAILED - check errors above
    echo ========================================
    echo.
)

pause
