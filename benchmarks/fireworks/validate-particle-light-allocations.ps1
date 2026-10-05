#requires -Version 7.0
# Read-only allocation observability and no-churn gate, not total resident VRAM.
[CmdletBinding()]
param([Parameter(Mandatory,ParameterSetName='Reports')][string]$ReportsDirectory,
    [Parameter(Mandatory,ParameterSetName='SelfTest')][switch]$SelfTest,
    [switch]$MeasureOnly,
    [ValidateRange(1,64)][int]$MaxNamedPrivateBuffers = 8)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
function Census($Report) {
    if ($null -eq $Report) { throw 'Unavailable allocator census' }
    if ($Report.total_allocated_bytes -le 0 -or $Report.total_reserved_bytes -lt $Report.total_allocated_bytes -or
        $Report.allocations.Count -eq 0 -or $Report.blocks.Count -eq 0) { throw 'Invalid allocator totals' }
    $allocated = 0L
    $reserved = 0L
    $cursor = 0
    foreach ($block in $Report.blocks) {
        if ($block.size -le 0 -or $block.allocation_range.Count -ne 2 -or
            $block.allocation_range[0] -ne $cursor -or $block.allocation_range[1] -lt $cursor -or
            $block.allocation_range[1] -gt $Report.allocations.Count) { throw 'Invalid allocator block/range' }
        $entries = @()
        for ($i = $cursor; $i -lt $block.allocation_range[1]; $i++) {
            $a = $Report.allocations[$i]
            if ($null -eq $a.name -or $a.size -le 0 -or $a.offset -lt 0 -or
                $a.size -gt $block.size -or $a.offset -gt $block.size-$a.size) { throw 'Allocation outside its block' }
            $allocated += $a.size
            $entries += $a
        }
        $end = 0L
        foreach ($a in @($entries | Sort-Object offset)) {
            if ($a.offset -lt $end) { throw 'Overlapping suballocator allocations' }
            $end = $a.offset + $a.size
        }
        $cursor = $block.allocation_range[1]
        $reserved += $block.size
    }
    if ($cursor -ne $Report.allocations.Count -or $allocated -ne $Report.total_allocated_bytes -or
        $reserved -ne $Report.total_reserved_bytes) { throw 'Allocator census accounting mismatch' }
    $metadata = @($Report.allocations | Where-Object name -eq 'clustering Z slicing metadata buffer')
    $staging = @($Report.allocations | Where-Object name -eq 'clustering metadata staging buffer')
    $metadataBytes = 0L
    $stagingBytes = 0L
    foreach ($a in $metadata) { $metadataBytes += $a.size }
    foreach ($a in $staging) { $stagingBytes += $a.size }
    [ordered]@{allocated_bytes=$allocated; reserved_bytes=$reserved; allocations=$Report.allocations.Count; blocks=$Report.blocks.Count
        named_metadata_count=$metadata.Count; named_metadata_bytes=$metadataBytes
        named_staging_count=$staging.Count; named_staging_bytes=$stagingBytes
        # Unlabeled allocations stay unattributed, not guessed from sizes/offsets.
        private_z_slice_bytes=$null; private_scratchpad_bytes=$null}
}
function Replacements($Values) {
    if ($Values.Count -lt 2 -or @($Values | Where-Object { $null -eq $_ -or $_ -notmatch '^[0-9a-f]{16}$' }).Count -gt 0) {
        throw 'Unknown/invalid public buffer identity fingerprint'
    }
    $changes = 0
    for ($i=1; $i -lt $Values.Count; $i++) { if ($Values[$i] -ne $Values[$i-1]) { $changes++ } }
    $changes
}
function NamedCeiling($Censuses) {
    @($Censuses | Where-Object {
        $_.named_metadata_count -lt 1 -or $_.named_staging_count -lt 1 -or
        $_.named_metadata_count -gt $MaxNamedPrivateBuffers -or $_.named_staging_count -gt $MaxNamedPrivateBuffers
    }).Count -eq 0 -and $Censuses.Count -gt 0
}
if ($SelfTest) {
    function Fixture {
        [pscustomobject]@{total_allocated_bytes=96; total_reserved_bytes=128
            allocations=@([pscustomobject]@{name='clustering Z slicing metadata buffer'; offset=0; size=48},
                [pscustomobject]@{name='clustering metadata staging buffer'; offset=48; size=48})
            blocks=@([pscustomobject]@{size=128; allocation_range=@(0,2)})}
    }
    Census (Fixture) | Out-Null
    $cold = Fixture
    $cold.allocations | ForEach-Object { $_.name = '' }
    $coldCensus = Census $cold
    if ($coldCensus.named_metadata_count -ne 0 -or $coldCensus.named_metadata_bytes -ne 0 -or
        $coldCensus.named_staging_count -ne 0 -or $coldCensus.named_staging_bytes -ne 0 -or
        (NamedCeiling @($coldCensus))) { throw 'Cold census absence was not preserved' }
    foreach ($failure in @('unknown','total','range','outside','overlap','reserved','unattributed')) {
        $f = Fixture
        switch ($failure) {
            unknown { $f = $null }
            total { $f.total_allocated_bytes = 95 }
            range { $f.blocks[0].allocation_range = @(0,3) }
            outside { $f.allocations[1].offset = 120 }
            overlap { $f.allocations[1].offset = 20 }
            reserved { $f.total_reserved_bytes = 129 }
            unattributed { $f.blocks[0].allocation_range = @(0,1); $f.total_allocated_bytes = 48 }
        }
        $rejected = $false
        try { Census $f | Out-Null } catch { $rejected = $true }
        if (!$rejected) { throw "Census negative control passed: $failure" }
    }
    if ((Replacements @('0000000000000001','0000000000000001')) -ne 0 -or
        (Replacements @('0000000000000001','0000000000000002','0000000000000002')) -ne 1) {
        throw 'Same-sized replacement detector failed'
    }
    $rejected = $false
    try { Replacements @('0000000000000001',$null) | Out-Null } catch { $rejected=$true }
    if (!$rejected) { throw 'Unknown identity passed' }
    if (!(NamedCeiling @([pscustomobject]@{named_metadata_count=1; named_staging_count=1})) -or
        (NamedCeiling @([pscustomobject]@{named_metadata_count=1; named_staging_count=0})) -or
        (NamedCeiling @([pscustomobject]@{named_metadata_count=1; named_staging_count=$MaxNamedPrivateBuffers+1})) -or
        (NamedCeiling @())) { throw 'Named private census ceiling/unknown controls failed' }
    'Allocator accounting/unknown and same-sized identity controls passed'
    return
}
$directory = (Resolve-Path -LiteralPath $ReportsDirectory).Path
$manifest = Get-Content "$directory/manifest.json" -Raw | ConvertFrom-Json
if (!$manifest.allocator_snapshots) { throw 'Matrix did not request opt-in allocation snapshots' }
# Preserve work matching, hashes, exits, resource/overflow logs and retirement.
# Its timing distributions are instrumented here, not ordinary matched-cost evidence.
$qualified = & "$PSScriptRoot/validate-particle-light-costs.ps1" -ReportsDirectory $directory -RequireRetirement | ConvertFrom-Json
$rows = @()
foreach ($entry in $manifest.runs) {
    $report = Get-Content "$directory/$($entry.pair)/$($entry.stem).json" -Raw | ConvertFrom-Json
    if (!$report.presentation.particle_light_allocation_snapshots -or !$report.allocator_scope.Contains('not cluster-only')) {
        throw 'Missing allocation instrumentation/scope'
    }
    $measured = @($report.cluster_buffers | Where-Object {$_.tick.measured})
    if (@($report.cluster_buffers | Where-Object allocation_sampling_exhausted).Count -ne 0) { throw 'Allocator sample limit exhausted' }
    $samples = @($measured | Where-Object {$null -ne $_.allocation_sample})
    if ($samples.Count -lt [Math]::Floor(($report.frames-4)/60)) { throw 'Missing allocator sample cadence' }
    $censuses = @()
    foreach ($s in @($report.cluster_buffers | Where-Object {$null -ne $_.allocation_sample})) {
        if (($s.sequence-1)%60 -ne 0 -or ![double]::IsFinite($s.allocation_sample.cpu_ms) -or $s.allocation_sample.cpu_ms -lt 0) {
            throw 'Incorrect allocator observation schedule/time'
        }
        $c = Census $s.allocation_sample.report
        $c.sample = $s.tick.index
        $c.sequence = $s.sequence
        $c.measured = $s.tick.measured
        $c.cpu_ms = $s.allocation_sample.cpu_ms
        $censuses += $c
    }
    $indices = @($measured | ForEach-Object { $_.views[0].index_buffer_fingerprint })
    $offsets = @($measured | ForEach-Object { $_.views[0].offsets_counts_buffer_fingerprint })
    $indexChanges = Replacements $indices
    $offsetChanges = Replacements $offsets
    $activeCensuses = @($censuses | Where-Object measured)
    $named = NamedCeiling $activeCensuses
    $rows += [ordered]@{pair=$entry.pair; probe=$entry.probe; mode=$entry.mode; tier=$entry.tier
        measured_binding_observations=$measured.Count
        index_buffer_replacements=$indexChanges; offsets_counts_buffer_replacements=$offsetChanges
        global_light_buffer_replacements=Replacements @($measured.global_light_buffer_fingerprint)
        stable_public_lists=($indexChanges -eq 0 -and $offsetChanges -eq 0)
        named_private_snapshot_ceiling_passed=$named; private_buffer_snapshot_ceiling=$MaxNamedPrivateBuffers
        sampled_device_allocated_bytes_peak=($activeCensuses.allocated_bytes | Measure-Object -Maximum).Maximum
        sampled_device_reserved_bytes_peak=($activeCensuses.reserved_bytes | Measure-Object -Maximum).Maximum
        censuses=$censuses; report_sha256=$entry.report_sha256; log_sha256=$entry.log_sha256}
}
$accepted = @($rows | Where-Object {!$_.stable_public_lists -or !$_.named_private_snapshot_ceiling_passed}).Count -eq 0
[ordered]@{schema=1; milestone='F7E4B3B2A'; measurements_valid=$true; accepted=$accepted
    scope='Opt-in backend suballocator snapshots and public hashed handle replacements. Whole-device live suballocations/reserved blocks, not total VRAM, exact in-flight retirement, complete create/free history, private unlabeled-buffer attribution or an enforced production budget. No-churn gate fails on observed index/offset replacements even with constant sizes. Instrumented timing distributions are not ordinary cost evidence.'
    binary_sha256=$manifest.binary_sha256; workload_retirement_and_log_gates_passed=$qualified.accepted
    rows=$rows} | ConvertTo-Json -Depth 12
if (!$accepted -and !$MeasureOnly) { throw 'Native cluster allocation qualification FAILED: observed churn or named-private snapshot ceiling' }
