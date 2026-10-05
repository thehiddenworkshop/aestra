#requires -Version 7.0
# Build first. Sequential native subprocesses only; fresh output directory required.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$ReportsDirectory,
    [string]$ViewerBinary = 'target/debug/aestra-viewer.exe',
    [ValidateRange(3,8)][int]$Repetitions = 3,
    [ValidateSet('high','medium','low')][string[]]$Tiers = @('high'),
    [switch]$RequireRetirement)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($Tiers.Count -eq 0 -or @($Tiers | Sort-Object -Unique).Count -ne $Tiers.Count) { throw 'Choose nonempty unique tiers' }
$Tiers = @(@('high','medium','low') | Where-Object { $_ -in $Tiers })
$binary = (Resolve-Path -LiteralPath $ViewerBinary).Path
if (Test-Path -LiteralPath $ReportsDirectory) { throw 'Use a fresh reports directory; do not overwrite prior attempts' }
New-Item -ItemType Directory -Path $ReportsDirectory | Out-Null
$directory = (Resolve-Path -LiteralPath $ReportsDirectory).Path
function Inputs {
    @(& rg --files assets | Sort-Object | ForEach-Object {
        [ordered]@{path=$_; sha256=(Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash.ToLowerInvariant()}
    })
}
$inputs = Inputs
$manifest = [ordered]@{schema=1; accepted=$false; repetitions=$Repetitions; tiers=$Tiers; runs=@(); failure=$null
    asset_inputs=$inputs; inputs_unchanged=$false
    binary_sha256=(Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant()
    scope='Alternating run-level GPU adapter on/off pairs; identical authored setup and cluster preallocation. Unpaced forward playback-only. Not frame-paired timestamps, whole-app timings or total resident cluster memory.'}
$manifest | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath "$directory/manifest.json" -Encoding utf8NoBOM
$probes = @{hero='f4-reference-hero'; volley='f5-secondary-volley'; show='f6-show'}
for ($repeat = 1; $repeat -le $Repetitions; $repeat++) {
    $pair = 'pair{0:00}' -f $repeat
    $pairDirectory = New-Item -ItemType Directory -Path "$directory/$pair"
    $cell = 0
    foreach ($tier in $Tiers) {
        foreach ($probe in @('hero','volley','show')) {
            $order = if (($repeat + $cell) % 2 -eq 1) { @('gpu','control') } else { @('control','gpu') }
            $cell++
            foreach ($mode in $order) {
                $stem = "$probe-$mode-$tier"
                $extra = @()
                if ($mode -eq 'control') { $extra += @('--particle-light-gpu-cap','0') }
                if ($probe -eq 'show') { $extra += '--transient-lights' }
                Write-Host "$pair $stem"
                & $binary --fireworks-f0 --fireworks-f0-probe $probes[$probe] --camera audience `
                    --backend gpu --history playback-only --hdr --particle-light-bench `
                    --particle-light-realization --particle-light-mode gpu --headless-bench --tier $tier `
                    --gpu-bench "$($pairDirectory.FullName)/$stem.json" @extra 2>&1 |
                    Tee-Object -FilePath "$($pairDirectory.FullName)/$stem.log" | Out-Null
                if ($LASTEXITCODE -ne 0) {
                    $manifest.failure = [ordered]@{pair=$pair; stem=$stem; exit_code=$LASTEXITCODE
                        log_sha256=(Get-FileHash -LiteralPath "$($pairDirectory.FullName)/$stem.log" -Algorithm SHA256).Hash.ToLowerInvariant()
                        report_sha256=if (Test-Path -LiteralPath "$($pairDirectory.FullName)/$stem.json") {
                            (Get-FileHash -LiteralPath "$($pairDirectory.FullName)/$stem.json" -Algorithm SHA256).Hash.ToLowerInvariant()
                        } else { $null }}
                    $manifest | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath "$directory/manifest.json" -Encoding utf8NoBOM
                    throw "$pair $stem native process failed: $LASTEXITCODE"
                }
                $manifest.runs += [ordered]@{pair=$pair; probe=$probe; tier=$tier; mode=$mode; stem=$stem; exit_code=0
                    report_sha256=(Get-FileHash -LiteralPath "$($pairDirectory.FullName)/$stem.json" -Algorithm SHA256).Hash.ToLowerInvariant()
                    log_sha256=(Get-FileHash -LiteralPath "$($pairDirectory.FullName)/$stem.log" -Algorithm SHA256).Hash.ToLowerInvariant()}
                $manifest | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath "$directory/manifest.json" -Encoding utf8NoBOM
            }
        }
    }
}
if ((ConvertTo-Json -InputObject $inputs -Depth 5 -Compress) -ne
    (ConvertTo-Json -InputObject (Inputs) -Depth 5 -Compress) -or
    (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash.ToLowerInvariant() -ne $manifest.binary_sha256) {
    throw 'Executable or asset inputs changed during the repeated measurements'
}
$manifest.inputs_unchanged = $true
$manifest | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath "$directory/manifest.json" -Encoding utf8NoBOM
# Read-only validator performs workload/resource/log/observable-growth gates first.
$summary = & "$PSScriptRoot/validate-particle-light-costs.ps1" -ReportsDirectory $directory -RequireRetirement:$RequireRetirement
$summary | Set-Content -LiteralPath "$directory/summary.json" -Encoding utf8NoBOM
$manifest.accepted = $true
$manifest | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath "$directory/manifest.json" -Encoding utf8NoBOM
Write-Host "Retained repeated-cost evidence: $directory"
