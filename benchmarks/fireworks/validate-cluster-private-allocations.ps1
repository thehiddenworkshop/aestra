#requires -Version 7.0
# Narrow high-tier authored fixture gate, layered on unchanged allocation gates.
[CmdletBinding()]
param([Parameter(Mandatory,ParameterSetName='Reports')][string]$ReportsDirectory,
    [Parameter(Mandatory,ParameterSetName='SelfTest')][switch]$SelfTest)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
function PrivateCensus($Report, [long]$ZBytes, [long]$ScratchBytes) {
    if ($null -eq $Report) { throw 'Private allocator report unavailable' }
    $names = @('clustering Z slice buffer','clustering scratchpad buffer','clustering Z slicing metadata buffer')
    $bytes = @($ZBytes,$ScratchBytes,48L)
    for ($index=0; $index -lt $names.Count; $index++) {
        $allocations = @($Report.allocations | Where-Object name -eq $names[$index])
        if ($allocations.Count -ne 1 -or $allocations[0].size -ne $bytes[$index]) {
            throw "Missing, duplicated or incorrectly sized private allocation: $($names[$index])"
        }
    }
    $staging = @($Report.allocations | Where-Object name -eq 'clustering metadata staging buffer')
    if ($staging.Count -gt 8 -or @($staging | Where-Object size -ne 48).Count -ne 0) {
        throw 'Native staging ceiling/size failed'
    }
    [ordered]@{z_slice_count=1; z_slice_bytes=$ZBytes; scratchpad_count=1; scratchpad_bytes=$ScratchBytes;
        metadata_count=1; metadata_bytes=48; staging_count=$staging.Count; staging_bytes=$staging.Count*48}
}
if ($SelfTest) {
    $valid = [pscustomobject]@{allocations=@(
        [pscustomobject]@{name='clustering Z slice buffer'; size=49152L},
        [pscustomobject]@{name='clustering scratchpad buffer'; size=117504L},
        [pscustomobject]@{name='clustering Z slicing metadata buffer'; size=48L}) +
        @(1..8 | ForEach-Object { [pscustomobject]@{name='clustering metadata staging buffer'; size=48L} })}
    $positive = PrivateCensus $valid 49152 117504
    if ($positive.staging_count -ne 8) { throw 'Positive ceiling control failed' }
    $negative = @($null,
        [pscustomobject]@{allocations=@($valid.allocations | Where-Object name -ne 'clustering Z slice buffer')},
        [pscustomobject]@{allocations=$valid.allocations + $valid.allocations[0]},
        [pscustomobject]@{allocations=$valid.allocations + $valid.allocations[-1]})
    foreach ($invalid in $negative) {
        $rejected = $false
        try { PrivateCensus $invalid 49152 117504 | Out-Null } catch { $rejected = $true }
        if (!$rejected) { throw 'Missing/duplicate/overflow negative control accepted' }
    }
    $rejected = $false
    try { PrivateCensus $valid 49152 512 | Out-Null } catch { $rejected = $true }
    if (!$rejected) { throw 'Wrong-size negative control accepted' }
    Write-Host 'Private allocation positive/negative controls passed.'
    return
}
$directory = (Resolve-Path -LiteralPath $ReportsDirectory).Path
# Preserve binary/asset/log/workload/admission/retirement/public no-churn checks.
$public = & "$PSScriptRoot/validate-particle-light-allocations.ps1" -ReportsDirectory $directory | ConvertFrom-Json
if (!$public.accepted) { throw 'Existing allocation qualification failed' }
$manifest = Get-Content -LiteralPath "$directory/manifest.json" -Raw | ConvertFrom-Json
$rows = @()
foreach ($run in $manifest.runs) {
    $report = Get-Content -LiteralPath "$directory/$($run.pair)/$($run.stem).json" -Raw | ConvertFrom-Json
    if ($report.presentation.quality_tier -ne 'high' -or
        ($report.presentation.particle_light_cluster_initial_capacities -join ',') -ne '4096,524288' -or
        ($report.presentation.headless_target -join ',') -ne '960,540') { throw 'Private gate requires the unchanged high-tier fixture' }
    $samples = @($report.cluster_buffers | Where-Object { $_.tick.measured -and $null -ne $_.allocation_sample })
    if ($samples.Count -eq 0) { throw 'Missing measured private samples' }
    if ($report.cluster_main.Count -eq 0) { throw 'Missing native grid observations' }
    foreach ($main in $report.cluster_main) {
        if ($main.views.Count -ne 1 -or ($main.views[0].dimensions -join ',') -ne '17,9,24') {
            throw 'Private allocation gate requires one fixed 17x9x24 main-world view'
        }
    }
    $censuses = @()
    foreach ($sample in $samples) {
        if ($sample.views.Count -ne 1 -or $sample.views[0].main_entity -ne $report.cluster_main[0].views[0].main_entity) {
            throw 'Private allocation gate requires the same single render-world view'
        }
        $censuses += PrivateCensus $sample.allocation_sample.report 49152 117504
    }
    $rows += [ordered]@{pair=$run.pair; probe=$run.probe; mode=$run.mode; samples=$samples.Count;
        z_slice_count=1; z_slice_bytes=49152; scratchpad_count=1; scratchpad_bytes=117504;
        metadata_count=1; metadata_bytes=48;
        staging_count_min=($censuses.staging_count | Measure-Object -Minimum).Minimum;
        staging_count_max=($censuses.staging_count | Measure-Object -Maximum).Maximum;
        report_sha256=$run.report_sha256; log_sha256=$run.log_sha256}
}
[ordered]@{schema=1; milestone='F7E4B3B2C'; accepted=$true;
    scope='Measured named private live allocations in the unchanged single-view high-tier hero/volley/show matrix. Existing workload/retirement/hash/public no-churn gates unchanged. Snapshot counts/bytes are not exact allocation generation IDs, full driver free history, total VRAM, hard limits on arbitrary overload or timing evidence.';
    binary_sha256=$manifest.binary_sha256; rows=$rows} | ConvertTo-Json -Depth 8
