@echo off
REM Scematica v1.11.0 Setup Verification
REM Checks all prerequisites before running.
REM Run init.bat first to install/fetch anything this reports as missing.

echo ========================================
echo Scematica v1.11.0 Setup Verification
echo ========================================
echo.

REM Check Rust installation
echo [1/6] Checking Rust installation...
cargo --version >nul 2>&1
if %ERRORLEVEL% EQU 0 (
    echo   [OK] Rust is installed
    cargo --version
) else (
    echo   [FAIL] Rust not found - install from https://rustup.rs/
    goto :end
)
echo.

REM Check Solana CLI
echo [2/6] Checking Solana CLI...
solana --version >nul 2>&1
if %ERRORLEVEL% EQU 0 (
    echo   [OK] Solana CLI is installed
    solana --version
) else (
    echo   [WARN] Solana CLI not found - optional but recommended
)
echo.

REM Check wallet keypair
echo [3/6] Checking wallet keypair...
if exist "C:\Users\deads\.config\solana\id.json" (
    echo   [OK] Wallet keypair found at C:\Users\deads\.config\solana\id.json
) else (
    echo   [FAIL] Wallet keypair NOT found
    echo   Generate one with: solana-keygen new --outfile C:\Users\deads\.config\solana\id.json
    goto :end
)
echo.

REM Check .env file
echo [4/6] Checking .env file...
if exist ".env" (
    echo   [OK] .env file exists
    findstr /C:"RPC_ENDPOINT" .env >nul 2>&1
    if %ERRORLEVEL% EQU 0 (
        echo   [OK] RPC_ENDPOINT configured
    ) else (
        echo   [WARN] RPC_ENDPOINT not found in .env
    )
) else (
    echo   [FAIL] .env file not found
    goto :end
)
echo.

REM Check config.toml
echo [5/6] Checking config.toml...
if exist "config.toml" (
    echo   [OK] config.toml exists
) else (
    echo   [FAIL] config.toml not found
    goto :end
)
echo.

REM Check if binaries are built
echo [6/6] Checking compiled binaries...
if exist "target\release\dashboard.exe" (
    echo   [OK] dashboard.exe is built
) else (
    echo   [INFO] Bot binaries not built yet - run build.bat
)
if exist "target\release\sdk-dashboard.exe" (
    echo   [OK] sdk-dashboard.exe is built
) else (
    echo   [INFO] ScemaDEX SDK binaries not built yet - run build.bat
)
echo.

echo ========================================
echo Setup verification complete!
echo ========================================
echo.
echo Next steps:
echo   1. Run init.bat once on a fresh checkout (toolchain + dependencies)
echo   2. Run build.bat to compile binaries
echo   3. Run start-dashboard-demo.bat to test in demo mode
echo   4. Run start-sdk-dashboard.bat for the ScemaDEX SDK TUI (SIM mode)
echo   5. Ensure you have 250,000+ SCEMA tokens in your wallet
echo   6. Run start-dashboard.bat for full bot mode
echo.

:end
pause
