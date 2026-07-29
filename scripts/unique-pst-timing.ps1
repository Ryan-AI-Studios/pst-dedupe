# unique-pst-timing.ps1 — operator timing harness for Series L unique-export (track 0081).
#
# Measures wall time for unique-pst (and optionally a prior scan) with no hardcoded
# client or case paths. Complements summary.json phase_timings; does not replace them.
#
# Usage examples:
#   .\scripts\unique-pst-timing.ps1 `
#     -InputPaths @('C:\evidence\a.pst','C:\evidence\b.pst') `
#     -Out 'C:\work\unique.pst' `
#     -ReportDir 'C:\work\unique_report' `
#     -TimingJson 'C:\work\timing.json'
#
#   .\scripts\unique-pst-timing.ps1 -InputPaths @('.\a.pst') -Out '.\out\u.pst' `
#     -ReportDir '.\out\u_report' -RunScanFirst -ExtraArgs @('--no-attachments','--overwrite')
#
# Parameters:
#   -InputPaths     One or more source PST paths (required)
#   -Out            Primary unique PST output path (required)
#   -ReportDir      Report pack directory (required)
#   -PstDedupExe    Path to pst-dedup.exe (default: target\release\pst-dedup.exe under repo root)
#   -RunScanFirst   When set, time a scan --json pass over the same inputs first
#   -ExtraArgs      Additional unique-pst arguments (array of strings)
#   -TimingJson     Optional path to write a timing.json sidecar
#   -WorkingDirectory Optional process working directory
#
# PowerShell-native only (no bashisms). Exit code is that of the last unique-pst run.

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string[]]$InputPaths,

    [Parameter(Mandatory = $true)]
    [string]$Out,

    [Parameter(Mandatory = $true)]
    [string]$ReportDir,

    [Parameter(Mandatory = $false)]
    [string]$PstDedupExe = '',

    [Parameter(Mandatory = $false)]
    [switch]$RunScanFirst,

    [Parameter(Mandatory = $false)]
    [string[]]$ExtraArgs = @(),

    [Parameter(Mandatory = $false)]
    [string]$TimingJson = '',

    [Parameter(Mandatory = $false)]
    [string]$WorkingDirectory = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Resolve-RepoRoot {
    $here = $PSScriptRoot
    if ([string]::IsNullOrWhiteSpace($here)) {
        return (Get-Location).Path
    }
    return (Resolve-Path (Join-Path $here '..')).Path
}

function Resolve-PstDedupExe {
    param([string]$Explicit, [string]$RepoRoot)
    if (-not [string]::IsNullOrWhiteSpace($Explicit)) {
        if (-not (Test-Path -LiteralPath $Explicit)) {
            throw "PstDedupExe not found: $Explicit"
        }
        return (Resolve-Path -LiteralPath $Explicit).Path
    }
    $candidate = Join-Path $RepoRoot 'target\release\pst-dedup.exe'
    if (Test-Path -LiteralPath $candidate) {
        return (Resolve-Path -LiteralPath $candidate).Path
    }
    $debugCandidate = Join-Path $RepoRoot 'target\debug\pst-dedup.exe'
    if (Test-Path -LiteralPath $debugCandidate) {
        return (Resolve-Path -LiteralPath $debugCandidate).Path
    }
    throw "pst-dedup.exe not found under target\release or target\debug. Build with: cargo build -p pst-dedup-cli --release"
}

function Invoke-TimedProcess {
    param(
        [string]$Exe,
        [string[]]$ArgumentList,
        [string]$Label,
        [string]$WorkDir
    )
    Write-Host "=== stage=$Label ==="
    Write-Host ("  {0} {1}" -f $Exe, ($ArgumentList -join ' '))
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $Exe
    $psi.UseShellExecute = $false
    $psi.RedirectStandardOutput = $false
    $psi.RedirectStandardError = $false
    # ProcessStartInfo.Arguments is a single string; quote paths with spaces.
    $quoted = @()
    foreach ($a in $ArgumentList) {
        if ($a -match '\s') {
            $quoted += ('"{0}"' -f ($a -replace '"', '\"'))
        } else {
            $quoted += $a
        }
    }
    $psi.Arguments = ($quoted -join ' ')
    if (-not [string]::IsNullOrWhiteSpace($WorkDir)) {
        $psi.WorkingDirectory = $WorkDir
    }
    $proc = [System.Diagnostics.Process]::Start($psi)
    if ($null -eq $proc) {
        throw "Failed to start process: $Exe"
    }
    $proc.WaitForExit()
    $sw.Stop()
    [pscustomobject]@{
        Stage      = $Label
        ExitCode   = $proc.ExitCode
        ElapsedMs  = [int64]$sw.ElapsedMilliseconds
        ElapsedSec = [math]::Round($sw.Elapsed.TotalSeconds, 3)
    }
}

$repoRoot = Resolve-RepoRoot
$exe = Resolve-PstDedupExe -Explicit $PstDedupExe -RepoRoot $repoRoot

if ($InputPaths.Count -lt 1) {
    throw 'InputPaths must contain at least one PST path.'
}
foreach ($p in $InputPaths) {
    if (-not (Test-Path -LiteralPath $p)) {
        throw "Input path not found: $p"
    }
}

$workDir = $WorkingDirectory
if ([string]::IsNullOrWhiteSpace($workDir)) {
    $workDir = (Get-Location).Path
}

$overall = [System.Diagnostics.Stopwatch]::StartNew()
$stages = New-Object System.Collections.Generic.List[object]
$lastExit = 0

if ($RunScanFirst) {
    $scanArgs = @('scan') + $InputPaths + @('--json')
    $scanResult = Invoke-TimedProcess -Exe $exe -ArgumentList $scanArgs -Label 'scan' -WorkDir $workDir
    $stages.Add($scanResult) | Out-Null
    $lastExit = $scanResult.ExitCode
    Write-Host ("  scan exit={0} elapsed_ms={1}" -f $scanResult.ExitCode, $scanResult.ElapsedMs)
}

$uniqueArgs = @('unique-pst') + $InputPaths + @(
    '--out', $Out,
    '--report-dir', $ReportDir
)
if ($ExtraArgs -and $ExtraArgs.Count -gt 0) {
    $uniqueArgs = $uniqueArgs + $ExtraArgs
}

$uniqueResult = Invoke-TimedProcess -Exe $exe -ArgumentList $uniqueArgs -Label 'unique-pst' -WorkDir $workDir
$stages.Add($uniqueResult) | Out-Null
$lastExit = $uniqueResult.ExitCode
Write-Host ("  unique-pst exit={0} elapsed_ms={1}" -f $uniqueResult.ExitCode, $uniqueResult.ElapsedMs)

$overall.Stop()
$overallMs = [int64]$overall.ElapsedMilliseconds

Write-Host ''
Write-Host '=== timing summary ==='
foreach ($s in $stages) {
    Write-Host ("  {0,-12} exit={1,-4} {2} ms ({3} s)" -f $s.Stage, $s.ExitCode, $s.ElapsedMs, $s.ElapsedSec)
}
Write-Host ("  {0,-12}           {1} ms ({2} s)" -f 'overall', $overallMs, [math]::Round($overall.Elapsed.TotalSeconds, 3))
Write-Host "  report_dir=$ReportDir"
Write-Host '  (Also inspect summary.json phase_timings after the run.)'

if (-not [string]::IsNullOrWhiteSpace($TimingJson)) {
    $parent = Split-Path -Parent $TimingJson
    if (-not [string]::IsNullOrWhiteSpace($parent) -and -not (Test-Path -LiteralPath $parent)) {
        New-Item -ItemType Directory -Path $parent -Force | Out-Null
    }
    $payload = [ordered]@{
        schema        = 'unique_pst_timing_v1'
        generated_utc = (Get-Date).ToUniversalTime().ToString('o')
        exe           = $exe
        out           = $Out
        report_dir    = $ReportDir
        input_count   = $InputPaths.Count
        overall_ms    = $overallMs
        last_exit     = $lastExit
        stages        = @($stages | ForEach-Object {
                [ordered]@{
                    stage      = $_.Stage
                    exit_code  = $_.ExitCode
                    elapsed_ms = $_.ElapsedMs
                }
            })
    }
    $json = $payload | ConvertTo-Json -Depth 6
    Set-Content -LiteralPath $TimingJson -Value $json -Encoding utf8
    Write-Host "  timing_json=$TimingJson"
}

exit $lastExit
