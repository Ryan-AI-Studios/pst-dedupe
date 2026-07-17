# Pre-commit hygiene checks for pst-dedupe (PowerShell / Windows)
# Mirrors C:\dev\ledgerful\scripts\pre-commit.ps1 (fmt + clippy + test).
# Invoked by scripts\install-hooks.ps1 via .git\hooks\pre-commit, or run manually.

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot

Write-Host "Running engineering hygiene checks..." -ForegroundColor Cyan

# 1. Format check
Write-Host "Checking formatting..."
cargo fmt --all -- --check
if ($LASTEXITCODE -ne 0) {
    Write-Host "ERROR: Code is not formatted. Run 'cargo fmt --all' and try again." -ForegroundColor Red
    exit 1
}

# 2. Lint check
Write-Host "Running clippy..."
cargo clippy --workspace --all-targets -- -D warnings
if ($LASTEXITCODE -ne 0) {
    Write-Host "ERROR: Clippy found warnings/errors. Fix them and try again." -ForegroundColor Red
    exit 1
}

# 3. Test check
Write-Host "Running tests..."
cargo test --workspace
if ($LASTEXITCODE -ne 0) {
    Write-Host "ERROR: Tests failed. Fix them and try again." -ForegroundColor Red
    exit 1
}

Write-Host "Hygiene checks PASSED." -ForegroundColor Green
exit 0
