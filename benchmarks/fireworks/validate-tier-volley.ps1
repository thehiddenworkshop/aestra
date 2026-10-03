#requires -Version 7.0
# Read-only admission gate for three uninterrupted F5F native GPU bench reports.
# Timings are evidence, not a performance pass/fail or artistic acceptance.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$ReportsDirectory)
$ErrorActionPreference = 'Stop'
$previousMemory = [long]::MaxValue
foreach ($tier in @('high', 'medium', 'low')) {
    $report = Get-Content -LiteralPath (Join-Path $ReportsDirectory "f5f-volley-$tier.json") -Raw | ConvertFrom-Json
    if ($report.effect -ne 'f5-secondary-volley' -or $report.history_policy -ne 'playback-only' -or
        $report.presentation.quality_tier -ne $tier -or $report.presentation.transparent_order -ne 'fast' -or
        $report.presentation.seed -ne '0xf1e0000000000001' -or
        [Math]::Abs($report.presentation.fixed_simulation_step_seconds - (1.0 / 60.0)) -gt 0.00000001 -or
        $report.warmup -ne 120 -or $report.frames -ne 600) {
        throw "$tier`: unexpected workload, history, tier, seed or benchmark policy"
    }
    $owners = @($report.work.PSObject.Properties)
    if ($owners.Count -ne 1) { throw "$tier`: expected one measured effect" }
    $owner = $owners[0].Name
    $work = $owners[0].Value
    if (@($report.effect_backends.$owner).Count -ne 1 -or $report.effect_backends.$owner[0] -ne 'native GPU') {
        throw "$tier`: expected native GPU, not fallback"
    }
    if ($work.event_readback_samples -le 0 -or $work.peak_live_particles -le 0 -or $work.peak_occupied_trails -le 0) {
        throw "$tier`: missing event or live/history observations"
    }
    foreach ($counter in @('source_event_overflow', 'max_trail_evictions', 'max_truncated_trails')) {
        if ($work.$counter -ne 0) { throw "$tier`: $counter must be measured zero" }
    }
    $expected = switch ($tier) {
        high { @(256, 4, 192, 2048) }
        medium { @(128, 4, 96, 768) }
        low { @(64, 4, 48, 256) }
    }
    if ($work.links.Count -ne 4) { throw "$tier`: missing links" }
    for ($i = 0; $i -lt 4; $i++) {
        $link = $work.links[$i]
        if ($link.captured_demand -ne $expected[$i] -or $link.accepted -ne $expected[$i] -or
            $link.expansion_omitted -ne 0 -or $link.destination_rejected -ne 0) {
            throw "$tier`: link $i did not admit all $($expected[$i]) requested children"
        }
    }
    $frames = @($report.simulation_frames.$owner)
    if ($frames.Count -ne 600 -or @($frames | Where-Object { $_.checkpoint_capture_bytes -ne 0 }).Count -ne 0) {
        throw "$tier`: expected 600 paired simulation samples with measured zero checkpoint capture"
    }
    $memory = $work.estimated_buffer_memory_bytes
    if ($null -eq $memory -or $memory -le 0 -or $memory -ge $previousMemory) {
        throw "$tier`: estimated effect buffers must decrease across tiers"
    }
    $previousMemory = $memory
    [pscustomobject]@{
        tier = $tier
        adapter = $report.adapter
        admitted = $expected
        peak_live = $work.peak_live_particles
        peak_histories = $work.peak_occupied_trails
        estimated_buffer_bytes = $memory
        paired_simulation = $report.simulation_total.$owner
        status = 'bounded admission passed; timing and artistic certification not performed'
    }
}
