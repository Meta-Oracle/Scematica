@echo off
setlocal
REM ============================================================================
REM Scematica v1.11.0 - One-Time Initialization
REM ----------------------------------------------------------------------------
REM Run this ONCE on a fresh checkout. It prepares everything the other batch
REM files (build.bat, verify-setup.bat, start-*.bat) rely on:
REM   - verifies the Rust toolchain
REM   - installs the rustfmt + clippy components (for fmt / clippy / lint)
REM   - fetches every workspace dependency (so build.bat can run offline/fast)
REM   - scaffolds a .env template if one does not exist
REM   - confirms config.toml is present
REM It does NOT compile binaries - that is build.bat. It does NOT touch secrets.
REM ============================================================================

echo ========================================
echo Scematica v1.11.0 Initialization
echo ========================================
echo.

REM ---------------------------------------------------------------------------
echo [1/5] Verifying Rust toolchain...
cargo --version >nul 2>&1
if not %ERRORLEVEL% EQU 0 goto :no_rust
for /f "delims=" %%v in ('cargo --version') do echo   [OK] %%v
rustc --version >nul 2>&1
if %ERRORLEVEL% EQU 0 (
    for /f "delims=" %%v in ('rustc --version') do echo   [OK] %%v
)
echo.

REM ---------------------------------------------------------------------------
echo [2/5] Ensuring rustfmt + clippy components...
rustup --version >nul 2>&1
if not %ERRORLEVEL% EQU 0 (
    echo   [WARN] rustup not found - skipping component install.
    echo          If 'cargo fmt' / 'cargo clippy' fail later, install rustup
    echo          from https://rustup.rs/ and re-run this script.
    goto :fetch
)
rustup component add rustfmt >nul 2>&1
if %ERRORLEVEL% EQU 0 (echo   [OK] rustfmt) else (echo   [WARN] could not add rustfmt)
rustup component add clippy >nul 2>&1
if %ERRORLEVEL% EQU 0 (echo   [OK] clippy) else (echo   [WARN] could not add clippy)
echo.

:fetch
REM ---------------------------------------------------------------------------
echo [3/5] Fetching all workspace dependencies...
echo   This downloads every crate the workspace needs and may take a few
echo   minutes on first run. Subsequent builds reuse this cache.
cargo fetch
if not %ERRORLEVEL% EQU 0 goto :fetch_failed
echo   [OK] Dependencies fetched.
echo.

REM ---------------------------------------------------------------------------
echo [4/5] Checking .env file...
if exist ".env" (
    echo   [OK] .env already exists - leaving it untouched.
) else (
    echo   [INFO] No .env found - writing a template. EDIT IT before live mode.
    (
        echo # Scematica environment - fill these in before running in full/live mode.
        echo # Demo and SIM modes do NOT require any of these.
        echo.
        echo # Solana RPC ^(Helius/QuickNode/etc. websocket-capable endpoint^):
        echo RPC_ENDPOINT=https://api.mainnet-beta.solana.com
        echo.
        echo # Path to your wallet keypair JSON:
        echo KEYPAIR_PATH=%USERPROFILE%\.config\solana\id.json
        echo.
        echo # Optional LLM keys for the AI agents ^(Groq / OpenRouter / xAI^):
        echo # GROQ_API_KEY=
        echo # OPENROUTER_API_KEY=
        echo.
        echo # Set to 1 ONLY during RPC outages to bypass the 250k SCEMA token gate:
        echo # SCEMATICA_SKIP_GATE=0
    ) > .env
    echo   [OK] Wrote .env template - open it and set RPC_ENDPOINT / KEYPAIR_PATH.
)
echo.

REM ---------------------------------------------------------------------------
echo [5/5] Checking config.toml...
if exist "config.toml" (
    echo   [OK] config.toml is present.
) else (
    echo   [WARN] config.toml not found. The bot needs one; copy a sample
    echo          ^(e.g. config-fibonacci-recovery.toml^) to config.toml.
)
echo.

echo ========================================
echo Initialization complete!
echo ========================================
echo.
echo Next steps:
echo   1. Edit .env  (RPC_ENDPOINT, KEYPAIR_PATH) for live/full mode.
echo   2. Run build.bat to compile all binaries (5-10 min first time).
echo   3. Run verify-setup.bat to confirm prerequisites.
echo   4. Try it without funds:
echo        start-dashboard-demo.bat     (bot dashboard, demo)
echo        start-sdk-dashboard.bat      (ScemaDEX SDK TUI, SIM)
echo   5. Full mode (needs 250k SCEMA + RPC): start-dashboard.bat
echo.
goto :end

:no_rust
echo   [FAIL] Rust/cargo not found on PATH.
echo          Install the Rust toolchain from https://rustup.rs/ then re-run
echo          this script. (Restart your terminal after installing.)
echo.
goto :end

:fetch_failed
echo   [FAIL] 'cargo fetch' failed - check your network/proxy and Cargo.toml.
echo          Re-run init.bat once the issue is resolved.
echo.

:end
pause
endlocal
