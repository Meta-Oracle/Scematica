# Build the on-chain Anchor programs (programs/*), which are excluded from the cargo
# workspace and built with the SBF toolchain rather than the host one.
#
# Use this instead of calling `cargo-build-sbf` directly. It exists for one reason:
#
#   cargo-build-sbf reports a stack-frame overflow as `Error: Function ... Stack offset
#   of N exceeded max offset of 4096` and then EXITS 0, emitting a .so.
#
# That binary deploys without complaint and fails at runtime with a stack access
# violation the first time the offending instruction is called. On a program holding
# other people's money, a silent exit-0 for that condition is not acceptable, so this
# script greps the build output and turns it into a hard failure.
#
#   powershell -ExecutionPolicy Bypass -File tools/build-programs.ps1
#
# Requires solana-cli 1.18.26 on PATH (see programs/scemadex-vault/DEPLOY.md).

param(
    [string[]]$Programs = @('scematica-swap', 'scemadex-escrow', 'scemadex-vault')
)

$ErrorActionPreference = 'Continue'

$solanaBin = "$env:USERPROFILE\.local\share\solana\install\active_release\bin"
if (Test-Path $solanaBin) { $env:PATH = "$solanaBin;$env:PATH" }

# cargo-build-sbf aborts with "Can't get home directory path" on Windows unless HOME is
# set. Windows sets USERPROFILE, not HOME.
if (-not $env:HOME) { $env:HOME = $env:USERPROFILE }

$repoRoot = Split-Path -Parent $PSScriptRoot
$failed = @()

foreach ($name in $Programs) {
    Write-Host "==== building $name ====" -ForegroundColor Cyan
    $manifest = Join-Path $repoRoot "programs\$name\Cargo.toml"
    if (-not (Test-Path $manifest)) {
        Write-Host "  no manifest at $manifest" -ForegroundColor Red
        $failed += "$name (missing manifest)"
        continue
    }

    $out = & cargo-build-sbf --manifest-path $manifest 2>&1 | ForEach-Object { "$_" }
    $code = $LASTEXITCODE
    $out | Write-Host

    if ($code -ne 0) {
        $failed += "$name (cargo-build-sbf exit $code)"
        continue
    }

    # The check this script exists for. Do not remove.
    $overflow = $out | Where-Object { $_ -match 'Stack offset of (\d+) exceeded max offset of (\d+)' }
    if ($overflow) {
        Write-Host "  STACK FRAME OVERFLOW - the .so is unusable at runtime:" -ForegroundColor Red
        $overflow | ForEach-Object { Write-Host "    $_" -ForegroundColor Red }
        $failed += "$name (stack frame overflow)"
        continue
    }

    $so = Get-ChildItem (Join-Path $repoRoot "programs\$name\target\deploy") -Filter *.so -ErrorAction SilentlyContinue
    if (-not $so) {
        $failed += "$name (no .so emitted)"
        continue
    }
    $hash = (Get-FileHash $so[0].FullName -Algorithm SHA256).Hash.ToLower()
    Write-Host ("  OK  {0}  {1} bytes" -f $so[0].Name, $so[0].Length) -ForegroundColor Green
    Write-Host ("  sha256 {0}" -f $hash)
}

Write-Host ""
if ($failed.Count -gt 0) {
    Write-Host "BUILD FAILED:" -ForegroundColor Red
    $failed | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    exit 1
}
Write-Host "All programs built clean." -ForegroundColor Green
exit 0
