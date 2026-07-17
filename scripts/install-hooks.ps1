# Install / repair git hooks for pst-dedupe on Windows.
#
# Aligns with `ledgerful init` hook templates from the Ledgerful product
# (see ledgerful src/commands/init.rs):
#   - pre-commit  : ledger status gate + cargo hygiene (scripts/pre-commit.ps1)
#   - pre-push    : ledger status gate + ledgerful verify --scope fast
#   - commit-msg  : ledgerful intent / sidecar prep
#   - post-commit : ledgerful post-commit promotion
#
# Safe to re-run (overwrites managed hook files with this canonical content).

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
$HooksDir = Join-Path $RepoRoot ".git\hooks"

if (-not (Test-Path (Join-Path $RepoRoot ".git"))) {
    Write-Host "ERROR: Not a git repository: $RepoRoot" -ForegroundColor Red
    exit 1
}

if (-not (Get-Command ledgerful -ErrorAction SilentlyContinue)) {
    Write-Host "WARNING: ledgerful not on PATH. Hooks will no-op until ledgerful is installed." -ForegroundColor Yellow
}

New-Item -ItemType Directory -Force -Path $HooksDir | Out-Null

# LF-only content: Git for Windows runs hooks via bash, which expects Unix line endings.
function Write-HookFile {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Content
    )
    $path = Join-Path $HooksDir $Name
    $normalized = $Content -replace "`r`n", "`n" -replace "`r", "`n"
    # Write without BOM; UTF8 no BOM
    $utf8NoBom = New-Object System.Text.UTF8Encoding $false
    [System.IO.File]::WriteAllText($path, $normalized, $utf8NoBom)
    Write-Host "  installed $Name" -ForegroundColor Green
}

$ledgerGateCommit = @'
#!/usr/bin/env bash
# Managed by scripts/install-hooks.ps1 (Ledgerful-compatible)

# ledgerful-ledger-gate: auto-installed by `ledgerful init` / install-hooks.ps1
if command -v ledgerful &>/dev/null; then
    if ! ledgerful ledger status --compact --exit-code --verify-signatures; then
        echo "[Ledgerful] Blocked by ledger state."
        echo "[Ledgerful] Resolve with:"
        echo "[Ledgerful]   Pending tx:  ledgerful ledger commit <tx-id> --summary '...' --reason '...'"
        echo "[Ledgerful]   Drift:       ledgerful ledger reconcile --all --reason '...'"
        echo "[Ledgerful] Fix the issues or bypass with: git commit --no-verify"
        exit 1
    fi
fi

# Hygiene gate (fmt / clippy / test) via scripts/pre-commit.ps1
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
HYGIENE_PS1="$REPO_ROOT/scripts/pre-commit.ps1"
if [ -f "$HYGIENE_PS1" ]; then
    if command -v pwsh &>/dev/null; then
        pwsh -NoProfile -File "$HYGIENE_PS1" || exit 1
    elif command -v powershell &>/dev/null; then
        powershell -NoProfile -ExecutionPolicy Bypass -File "$HYGIENE_PS1" || exit 1
    else
        echo "[hooks] PowerShell not found; running cargo hygiene inline..."
        cargo fmt --all -- --check || exit 1
        cargo clippy --workspace --all-targets -- -D warnings || exit 1
        cargo test --workspace || exit 1
    fi
fi
'@

$ledgerGatePush = @'
#!/usr/bin/env bash
# Managed by scripts/install-hooks.ps1 (Ledgerful-compatible)

# ledgerful-ledger-gate: auto-installed by `ledgerful init` / install-hooks.ps1
if command -v ledgerful &>/dev/null; then
    if ! ledgerful ledger status --compact --exit-code --verify-signatures; then
        echo "[Ledgerful] Blocked by ledger state."
        echo "[Ledgerful] Resolve with:"
        echo "[Ledgerful]   Pending tx:  ledgerful ledger commit <tx-id> --summary '...' --reason '...'"
        echo "[Ledgerful]   Drift:       ledgerful ledger reconcile --all --reason '...'"
        echo "[Ledgerful] Fix the issues or bypass with: git push --no-verify"
        exit 1
    fi
fi

# ledgerful-verify-gate: fast scoped verification (pre-push only)
if command -v ledgerful &>/dev/null; then
    if ! ledgerful verify --scope fast; then
        echo "[Ledgerful] Push blocked by verification failure."
        echo "[Ledgerful] Fix the issues or bypass with: git push --no-verify"
        exit 1
    fi
fi
'@

$commitMsg = @'
#!/usr/bin/env bash
# Managed by scripts/install-hooks.ps1 (Ledgerful-compatible)

# ledgerful-intent-gate: auto-installed by `ledgerful init` / install-hooks.ps1
if command -v ledgerful &>/dev/null; then
    ledgerful internal hook-commit-msg "$1"
fi
'@

$postCommit = @'
#!/usr/bin/env bash
# Managed by scripts/install-hooks.ps1 (Ledgerful-compatible)

# ledgerful-post-commit-gate: auto-installed by `ledgerful init` / install-hooks.ps1
if command -v ledgerful &>/dev/null; then
    ledgerful internal hook-post-commit "$@"
fi
'@

Write-Host "Installing git hooks into $HooksDir ..." -ForegroundColor Cyan
Write-HookFile -Name "pre-commit" -Content $ledgerGateCommit
Write-HookFile -Name "pre-push" -Content $ledgerGatePush
Write-HookFile -Name "commit-msg" -Content $commitMsg
Write-HookFile -Name "post-commit" -Content $postCommit

Write-Host ""
Write-Host "Hooks installed." -ForegroundColor Green
Write-Host "  pre-commit  -> ledgerful ledger status + scripts/pre-commit.ps1 (fmt/clippy/test)"
Write-Host "  pre-push    -> ledgerful ledger status + ledgerful verify --scope fast"
Write-Host "  commit-msg  -> ledgerful internal hook-commit-msg"
Write-Host "  post-commit -> ledgerful internal hook-post-commit"
Write-Host ""
Write-Host "Re-run after cloning:"
Write-Host "  powershell -NoProfile -ExecutionPolicy Bypass -File scripts\install-hooks.ps1"
