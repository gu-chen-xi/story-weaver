#!/usr/bin/env pwsh
# Clean / report Rust build cache.
#
# Why this exists (2026-10-18): src-tauri\target\debug grew to 58.8 GB
# (deps 42 GB + incremental 12.8 GB) -- every `cargo test` run links a new test
# binary with sqlx/tauri inside, and old ones accumulate. Nothing under target/ is
# precious: cargo rebuilds it on demand.
#
# Usage:
#   .\clean-build.cmd            -> delete target\debug (keeps target\release + installers)
#   .\clean-build.cmd report     -> only print sizes, delete nothing
#   .\clean-build.cmd all        -> delete the whole target folder (also release)

param([string]$Mode = "debug")

$ErrorActionPreference = "SilentlyContinue"
$proj = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
if (-not (Test-Path (Join-Path $proj "src-tauri"))) { $proj = (Get-Location).Path }
$target = Join-Path $proj "src-tauri\target"

function Get-Size($p) {
    if (-not (Test-Path $p)) { return -1 }
    $s = (Get-ChildItem $p -Recurse -File -Force -ErrorAction SilentlyContinue |
          Measure-Object Length -Sum).Sum
    if ($null -eq $s) { return 0 }
    return $s
}

Write-Host "============================================================"
Write-Host " Build cache report"
Write-Host "============================================================"
$total = Get-Size $target
if ($total -lt 0) {
    Write-Host "  target folder does not exist: $target"
    exit 0
}
Write-Host ("  total      : {0,8:N2} GB" -f ($total / 1GB))
foreach ($sub in @("debug", "release")) {
    $s = Get-Size (Join-Path $target $sub)
    if ($s -ge 0) {
        Write-Host ("  {0,-10} : {1,8:N2} GB" -f $sub, ($s / 1GB))
    }
}
foreach ($sub in @("debug\deps", "debug\incremental")) {
    $s = Get-Size (Join-Path $target $sub)
    if ($s -ge 0) {
        Write-Host ("    {0,-14}: {1,8:N2} GB" -f $sub, ($s / 1GB))
    }
}

if ($Mode -eq "report") {
    Write-Host ""
    Write-Host "  (report only -- nothing deleted)"
    exit 0
}

if ($Mode -eq "all") {
    Write-Host ""
    Write-Host "  Deleting the WHOLE target folder (cargo rebuilds everything)..."
    Remove-Item $target -Recurse -Force
} else {
    Write-Host ""
    Write-Host "  Deleting target\debug ..."
    Remove-Item (Join-Path $target "debug") -Recurse -Force
}

$after = Get-Size $target
if ($after -lt 0) { $after = 0 }
Write-Host ""
Write-Host ("  freed      : {0,8:N2} GB" -f (($total - $after) / 1GB))
Write-Host ("  now        : {0,8:N2} GB" -f ($after / 1GB))
Write-Host ""
Write-Host "  Done. Next 'cargo test' / 'cargo build' rebuilds automatically."
