# Deploy the on-chain programs built by tools/build-programs.ps1.
#
# Deploys are resumable but NOT atomic: a failure partway through leaves a funded buffer
# account holding real rent. This script prints any stranded buffers at the end rather
# than silently leaving them, because `solana program deploy` does not.
#
#   powershell -ExecutionPolicy Bypass -File tools/deploy-programs.ps1
#   powershell -ExecutionPolicy Bypass -File tools/deploy-programs.ps1 -Programs scemadex-vault
#
# The upgrade authority is the deploy keypair (solana config get). Making the vault
# non-custodial is a SEPARATE, IRREVERSIBLE step this script deliberately does not take:
#   solana program set-upgrade-authority <ID> --final
# See programs/scemadex-vault/DEPLOY.md sections 5 and 6.

param(
    [string[]]$Programs = @('scematica-swap', 'scemadex-escrow', 'scemadex-vault')
)

$ErrorActionPreference = 'Continue'

$solanaBin = "$env:USERPROFILE\.local\share\solana\install\active_release\bin"
if (Test-Path $solanaBin) { $env:PATH = "$solanaBin;$env:PATH" }
if (-not $env:HOME) { $env:HOME = $env:USERPROFILE }

$repoRoot = Split-Path -Parent $PSScriptRoot

# `exact` allocates no upgrade headroom, halving locked rent. Correct for a program that
# will be finalized (the vault) and wrong for one you may still need to grow.
$spec = @{
    'scematica-swap'  = @{ so = 'scematica_swap.so';  keypair = 'scematica_swap-keypair.json';  sizing = 'double' }
    'scemadex-escrow' = @{ so = 'scemadex_escrow.so'; keypair = 'scemadex_escrow-keypair.json'; sizing = 'double' }
    'scemadex-vault'  = @{ so = 'scemadex_vault.so';  keypair = 'scemadex_vault-keypair.json';  sizing = 'exact'  }
}

Write-Host "deployer : $(solana address)"
Write-Host "balance  : $(solana balance)"
Write-Host "rpc      : $((solana config get | Select-String 'RPC URL').ToString().Split(':',2)[1].Trim().Split('?')[0])"
Write-Host ""

$deployed = @()
$failed = @()

foreach ($name in $Programs) {
    $s = $spec[$name]
    if (-not $s) { Write-Host "unknown program $name" -ForegroundColor Red; $failed += $name; continue }

    $deployDir = Join-Path $repoRoot "programs\$name\target\deploy"
    $so = Join-Path $deployDir $s.so
    $kp = Join-Path $deployDir $s.keypair

    if (-not (Test-Path $so)) { Write-Host "missing $so - run tools/build-programs.ps1" -ForegroundColor Red; $failed += $name; continue }
    if (-not (Test-Path $kp)) { Write-Host "missing $kp" -ForegroundColor Red; $failed += $name; continue }

    $programId = (solana address -k $kp).Trim()
    $len = (Get-Item $so).Length

    Write-Host "==== deploying $name ====" -ForegroundColor Cyan
    Write-Host "  program id : $programId"
    Write-Host "  size       : $len bytes ($($s.sizing) sizing)"

    $args = @('program', 'deploy', $so, '--program-id', $kp)
    if ($s.sizing -eq 'exact') { $args += @('--max-len', "$len") }

    $out = & solana @args 2>&1 | ForEach-Object { "$_" }
    $code = $LASTEXITCODE
    $out | Write-Host

    if ($code -ne 0) {
        Write-Host "  DEPLOY FAILED (exit $code)" -ForegroundColor Red
        $failed += $name
    } else {
        Write-Host "  deployed" -ForegroundColor Green
        $deployed += [pscustomobject]@{ Name = $name; ProgramId = $programId }
    }
    Write-Host ""
}

Write-Host "==== on-chain state ====" -ForegroundColor Cyan
foreach ($d in $deployed) {
    Write-Host "---- $($d.Name) ----"
    solana program show $d.ProgramId 2>&1 | Write-Host
}

Write-Host ""
Write-Host "==== stranded buffers (reclaimable rent) ====" -ForegroundColor Cyan
solana program show --buffers 2>&1 | Write-Host

Write-Host ""
Write-Host "balance after : $(solana balance)"
if ($failed.Count -gt 0) {
    Write-Host "FAILED: $($failed -join ', ')" -ForegroundColor Red
    exit 1
}
exit 0
