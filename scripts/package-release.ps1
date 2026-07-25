# Build Windows RC package: three exes + CycloneDX SBOMs + README-RELEASE + PDB symbols + ZIP.
# Track 0062. Does not Authenticode-sign; see docs/release-signing.md.
#
# Usage:
#   powershell -File scripts/package-release.ps1
#   powershell -File scripts/package-release.ps1 -Version 0.2.0-rc.1 -OutRoot dist
#
# Prerequisites: rustc/cargo; cargo-cyclonedx (`cargo install cargo-cyclonedx`).

[CmdletBinding()]
param(
    [string]$Version = "0.2.0-rc.1",
    [string]$OutRoot = "dist",
    [switch]$SkipBuild,
    [switch]$SkipSbom
)

$ErrorActionPreference = "Stop"
# Cargo writes progress to stderr; do not treat as terminating under Stop.
$PSNativeCommandUseErrorActionPreference = $false
$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $RepoRoot

$PkgDir = Join-Path $RepoRoot (Join-Path $OutRoot $Version)
$SymbolsDir = Join-Path $PkgDir "symbols"
$ZipPath = Join-Path $RepoRoot (Join-Path $OutRoot "dedupe-$Version-windows-x64.zip")
$SymbolsZipPath = Join-Path $RepoRoot (Join-Path $OutRoot "dedupe-$Version-windows-x64-symbols.zip")

Write-Host "==> Package dir: $PkgDir"

if (-not $SkipBuild) {
    Write-Host "==> cargo build --release (desk, cli, gui)"
    $prevEap = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    & cargo build --release -p dedupe-desk -p pst-dedup-cli -p pst-dedup-gui 2>&1 | ForEach-Object { "$_" }
    $buildExit = $LASTEXITCODE
    $ErrorActionPreference = $prevEap
    if ($buildExit -ne 0) { throw "cargo build failed with exit $buildExit" }
}

New-Item -ItemType Directory -Force -Path $PkgDir | Out-Null
New-Item -ItemType Directory -Force -Path $SymbolsDir | Out-Null

$ReleaseDir = Join-Path $RepoRoot "target\release"
# MSVC/rustc emit PDB basenames with underscores (crate package name), not exe hyphens.
$Binaries = @(
    @{ Name = "dedupe-desk.exe"; Pdb = "dedupe_desk.pdb"; Crate = "dedupe-desk"; BomName = "bom-desk.json" },
    @{ Name = "pst-dedup.exe"; Pdb = "pst_dedup.pdb"; Crate = "pst-dedup-cli"; BomName = "bom-cli.json" },
    @{ Name = "pst-dedup-gui.exe"; Pdb = "pst_dedup_gui.pdb"; Crate = "pst-dedup-gui"; BomName = "bom-gui.json" }
)

foreach ($b in $Binaries) {
    $src = Join-Path $ReleaseDir $b.Name
    if (-not (Test-Path $src)) { throw "Missing binary: $src" }
    Copy-Item -Force $src (Join-Path $PkgDir $b.Name)
    $pdbSrc = Join-Path $ReleaseDir $b.Pdb
    if (-not (Test-Path $pdbSrc)) {
        throw "Missing PDB for $($b.Name): expected $pdbSrc (profile.release debug=1 required)"
    }
    Copy-Item -Force $pdbSrc (Join-Path $SymbolsDir $b.Pdb)
    Write-Host "  PDB: $($b.Pdb)"
}

function Invoke-CargoCycloneDx {
    param(
        [string]$CrateDir,
        [string]$ManifestPath,
        [string]$DestBom
    )
    Push-Location $CrateDir
    try {
        $prevEap = $ErrorActionPreference
        $ErrorActionPreference = "Continue"
        & cargo cyclonedx --format json --manifest-path $ManifestPath --target x86_64-pc-windows-msvc --describe binaries 2>&1 | ForEach-Object { "$_" }
        $cdxExit = $LASTEXITCODE
        $ErrorActionPreference = $prevEap
        if ($cdxExit -ne 0) { throw "cargo cyclonedx failed for $CrateDir with exit $cdxExit" }

        $candidates = @(Get-ChildItem -Path $CrateDir -Filter "*_bin.cdx.json" -ErrorAction SilentlyContinue |
            Select-Object -ExpandProperty FullName) + @(
            Get-ChildItem -Path $CrateDir -Filter "*.cdx.json" -ErrorAction SilentlyContinue |
                Select-Object -ExpandProperty FullName
        )
        $found = $candidates | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
        if (-not $found) {
            throw "cargo cyclonedx produced no JSON under $CrateDir"
        }
        Copy-Item -Force $found $DestBom
        $raw = Get-Content -Raw -Path $DestBom
        if ($raw -notmatch '"bomFormat"\s*:\s*"CycloneDX"' -and $raw -notmatch '"components"') {
            throw "SBOM does not look like CycloneDX JSON: $DestBom"
        }
        # Desk/GUI graphs must mention eframe; CLI graph is large enough if components present.
        Write-Host "  SBOM: $DestBom ($((Get-Item $DestBom).Length) bytes) from $found"
    }
    finally {
        Pop-Location
    }
}

if (-not $SkipSbom) {
    Write-Host "==> CycloneDX SBOM for each shipped binary"
    foreach ($b in $Binaries) {
        $crateDir = Join-Path $RepoRoot "crates\$($b.Crate)"
        $manifest = Join-Path $crateDir "Cargo.toml"
        $dest = Join-Path $PkgDir $b.BomName
        Invoke-CargoCycloneDx -CrateDir $crateDir -ManifestPath $manifest -DestBom $dest
    }

    # Primary bom.json = CLI product surface (largest automation surface); per-binary files also ship.
    Copy-Item -Force (Join-Path $PkgDir "bom-cli.json") (Join-Path $PkgDir "bom.json")

    # Sanity: Desk SBOM should include egui stack (eframe) so package is not CLI-only inventory theater.
    $deskRaw = Get-Content -Raw (Join-Path $PkgDir "bom-desk.json")
    if ($deskRaw -notmatch 'eframe') {
        throw "bom-desk.json missing eframe - Desk dependency graph incomplete"
    }
    $guiRaw = Get-Content -Raw (Join-Path $PkgDir "bom-gui.json")
    if ($guiRaw -notmatch 'eframe') {
        throw "bom-gui.json missing eframe - GUI dependency graph incomplete"
    }
}

$readme = @"
Dedupe Desk / pst-dedupe — Release package $Version
================================================

Binaries
--------
- dedupe-desk.exe   — primary matter Desk UI
- pst-dedup.exe     — CLI (PST tools + matter automation + unique-pst)
- pst-dedup-gui.exe — scan GUI + Unique PST wizard

Also in this folder
-------------------
- bom.json          — CycloneDX SBOM for CLI (pst-dedup) graph
- bom-cli.json      — same as bom.json (explicit name)
- bom-desk.json     — CycloneDX SBOM for dedupe-desk graph
- bom-gui.json      — CycloneDX SBOM for pst-dedup-gui graph
- symbols/          — PDB files for field stacks (support); also packaged as sibling *-symbols.zip

Version / schema
----------------
- Product version: $Version
- Matter SCHEMA_VERSION: 39

Golden path
-----------
See docs in the source repo (or internal docs drop):
  docs/operator-golden-path.md
  docs/operator-rc-checklist.md
  docs/release-signing.md
  CHANGELOG.md

Signing
-------
Operator-facing handoff REQUIRES Authenticode signatures on the three .exe files.
Unsigned builds are engineering-only. See docs/release-signing.md (D-0062-codesign).
Do not present an unsigned ZIP as the official counsel RC.

Smoke
-----
  .\pst-dedup.exe --help
  .\pst-dedup.exe unique-pst --help
  .\dedupe-desk.exe
  .\pst-dedup-gui.exe
"@
Set-Content -Path (Join-Path $PkgDir "README-RELEASE.txt") -Value $readme -Encoding utf8

# Smoke CLI help (non-GUI)
Write-Host "==> Smoke: pst-dedup --help"
& (Join-Path $PkgDir "pst-dedup.exe") --help | Out-Null
if ($LASTEXITCODE -ne 0) { throw "pst-dedup --help failed" }
& (Join-Path $PkgDir "pst-dedup.exe") unique-pst --help | Out-Null
if ($LASTEXITCODE -ne 0) { throw "unique-pst --help failed" }

# Bounded Desk/GUI launch smoke: process starts and remains alive briefly, then we stop it.
function Test-GuiLaunch {
    param([string]$ExePath, [string]$Label, [int]$HoldSeconds = 3)
    Write-Host "==> Smoke: $Label launch ($HoldSeconds s)"
    $p = Start-Process -FilePath $ExePath -PassThru -WindowStyle Minimized
    Start-Sleep -Seconds $HoldSeconds
    if ($p.HasExited) {
        throw "$Label exited early with code $($p.ExitCode) (expected to stay up for smoke)"
    }
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 500
    Write-Host "  $Label launch OK (started PID $($p.Id), stopped)"
}

Test-GuiLaunch -ExePath (Join-Path $PkgDir "dedupe-desk.exe") -Label "dedupe-desk"
Test-GuiLaunch -ExePath (Join-Path $PkgDir "pst-dedup-gui.exe") -Label "pst-dedup-gui"

# Operator package ZIP (exes + SBOMs + README; symbols are separate)
Write-Host "==> ZIP: $ZipPath"
if (Test-Path $ZipPath) { Remove-Item -Force $ZipPath }
$zipItems = @(
    "dedupe-desk.exe", "pst-dedup.exe", "pst-dedup-gui.exe",
    "README-RELEASE.txt", "bom.json", "bom-cli.json", "bom-desk.json", "bom-gui.json"
) | ForEach-Object { Join-Path $PkgDir $_ }
Compress-Archive -Path $zipItems -DestinationPath $ZipPath -Force

Write-Host "==> Symbols ZIP: $SymbolsZipPath"
if (Test-Path $SymbolsZipPath) { Remove-Item -Force $SymbolsZipPath }
Compress-Archive -Path (Join-Path $SymbolsDir "*") -DestinationPath $SymbolsZipPath -Force

Write-Host "==> Package ready: $PkgDir"
Write-Host "    Operator ZIP: $ZipPath (unsigned - D-0062-codesign blocks counsel handoff until signed)"
Write-Host "    Symbols ZIP:  $SymbolsZipPath"
Get-ChildItem $PkgDir -Recurse | Select-Object FullName, Length | Format-Table -AutoSize
Get-Item $ZipPath, $SymbolsZipPath | Select-Object FullName, Length | Format-Table -AutoSize
