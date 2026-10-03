#requires -Version 7.0
# Read-only per-clip admission gate. Missing observations fail, never become zero.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$ReportsDirectory)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$sources = @('f8000', 'f9000', 'fa000', 'fb000', 'f8000', 'f8000', 'f8000',
    'fa000', 'f9000', 'f8000', 'f9000', 'fb000', 'fa000')
$previousMemory = [long]::MaxValue
foreach ($tier in @('high', 'medium', 'low')) {
    $report = Get-Content -LiteralPath (Join-Path $ReportsDirectory "f6a-show-$tier.json") -Raw | ConvertFrom-Json
    if ($report.effect -ne 'f6-show' -or $report.history_policy -ne 'playback-only' -or
        $report.presentation.quality_tier -ne $tier -or $report.presentation.transparent_order -ne 'fast' -or
        $report.presentation.seed -ne '0xf1e0000000000001' -or
        $null -eq $report.presentation.fixed_simulation_step_seconds -or
        [Math]::Abs($report.presentation.fixed_simulation_step_seconds - (1.0 / 60.0)) -gt 0.00000001 -or
        $report.warmup -ne 120 -or $report.frames -ne 1680 -or
        $report.peak_active_clip_instances -ne 6 -or $report.active_clip_instances_at_end -ne 0) {
        throw "$tier unexpected workload, history, clocks or clip lifecycle"
    }
    $entries = @($report.instances.PSObject.Properties)
    $roots = @($entries | Where-Object { $_.Value.clip_path.Count -eq 0 })
    $children = @($entries | Where-Object { $_.Value.clip_path.Count -gt 0 })
    if ($roots.Count -ne 1 -or $children.Count -ne 13) { throw "$tier expected one carrier and thirteen clip owners" }
    $root = $roots[0].Name
    if ($roots[0].Value.particle_capacity -ne 0 -or
        $roots[0].Value.history_policy -ne 'playback-only' -or
        $roots[0].Value.seed -ne '0xf1e0000000000001' -or
        $roots[0].Value.source_effect -ne 'a3574a00-0000-4000-8000-0000000fd000') { throw "$tier unexpected root" }
    if (@($children.Value.seed | Sort-Object -Unique).Count -ne 13) { throw "$tier reused child seeds" }
    $divisor = switch ($tier) { high { 1 }; medium { 2 }; low { 4 } }
    $multi = switch ($tier) { high { 8 }; medium { 6 }; low { 4 } }
    $crackle = switch ($tier) { high { 12 }; medium { 8 }; low { 4 } }
    $capacities = switch ($tier) {
        high { @(690, 1458, 274, 306) }
        medium { @(314, 570, 170, 186) }
        low { @(158, 222, 118, 126) }
    }
    $totalAdmitted = 0L
    for ($i = 0; $i -lt 13; $i++) {
        $path = 'a3574a00-0000-4000-8000-0000000' + (0xfd010 + $i).ToString('x')
        $matches = @($children | Where-Object { $_.Value.clip_path.Count -eq 1 -and $_.Value.clip_path[0] -eq $path })
        if ($matches.Count -ne 1) { throw "$tier missing/duplicate path $path" }
        $owner = $matches[0].Name
        $instance = $matches[0].Value
        if ($instance.root -ne $root -or $instance.history_policy -ne 'playback-only' -or
            $instance.source_effect -ne ('a3574a00-0000-4000-8000-0000000' + $sources[$i])) { throw "$tier wrong clip identity/policy: $path" }
        $sourceIndex = @('f8000', 'f9000', 'fa000', 'fb000').IndexOf($sources[$i])
        if ($instance.particle_capacity -ne $capacities[$sourceIndex]) { throw "$tier unexpected compiled capacity: $path" }
        if (@($report.effect_backends.$owner).Count -ne 1 -or $report.effect_backends.$owner[0] -ne 'native GPU') {
            throw "$tier fallback/unobserved backend: $path"
        }
        $work = $report.work.$owner
        if ($null -eq $work.event_readback_samples -or $work.event_readback_samples -le 0 -or
            $null -eq $work.peak_live_particles -or $work.peak_live_particles -le 0 -or
            $null -eq $work.peak_occupied_trails -or $work.peak_occupied_trails -le 0) {
            throw "$tier missing event/live/history observations: $path"
        }
        foreach ($counter in @('source_event_overflow', 'max_trail_evictions', 'max_truncated_trails',
                'last_live_particles', 'last_occupied_trails')) {
            if ($null -eq $work.$counter -or $work.$counter -ne 0) { throw "$tier $path $counter must be measured zero" }
        }
        $expected = switch ($sources[$i]) {
            f8000 { @((64 / $divisor), 1, (48 / $divisor), (64 / $divisor * $multi)) }
            f9000 { @((96 / $divisor), 1, (48 / $divisor), (96 / $divisor), (96 / $divisor * $crackle)) }
            fa000 { @((32 / $divisor), 1, (48 / $divisor), (32 / $divisor), (32 / $divisor), (32 / $divisor), (32 / $divisor)) }
            fb000 { @((192 / $divisor), 1, (48 / $divisor)) }
        }
        if ($null -eq $work.links -or $work.links.Count -ne $expected.Count) { throw "$tier wrong links: $path" }
        for ($linkIndex = 0; $linkIndex -lt $expected.Count; $linkIndex++) {
            $link = $work.links[$linkIndex]
            if ($link.captured_demand -ne $expected[$linkIndex] -or $link.accepted -ne $expected[$linkIndex] -or
                $link.expansion_omitted -ne 0 -or $link.destination_rejected -ne 0) {
                throw "$tier $path link $linkIndex failed admission of $($expected[$linkIndex])"
            }
            $totalAdmitted += $link.accepted
        }
        $frames = @($report.simulation_frames.$owner)
        if ($frames.Count -lt 100 -or $frames[-1].requested_time -lt 5.5 -or
            @($frames | Where-Object { $null -eq $_.checkpoint_capture_bytes -or $_.checkpoint_capture_bytes -ne 0 }).Count -gt 0) {
            throw "$tier missing full-lifecycle playback-only paired observations: $path"
        }
        for ($frameIndex = 1; $frameIndex -lt $frames.Count; $frameIndex++) {
            if ($frames[$frameIndex].sequence -le $frames[$frameIndex - 1].sequence -or
                $frames[$frameIndex].requested_time -lt $frames[$frameIndex - 1].requested_time) {
                throw "$tier repeated/backward simulation observations: $path"
            }
        }
    }
    $project = $report.project_work.$root
    foreach ($counter in @('last_live_particles', 'last_occupied_trails', 'max_trail_evictions', 'max_truncated_trails')) {
        if ($null -eq $project.$counter -or $project.$counter -ne 0) { throw "$tier project $counter must be measured zero" }
    }
    $memory = $project.peak_estimated_buffer_memory_bytes
    if ($null -eq $memory -or $memory -le 0 -or $memory -ge $previousMemory) { throw "$tier concurrent estimated buffers must decrease across tiers" }
    $previousMemory = $memory
    [pscustomobject]@{
        tier = $tier
        adapter = $report.adapter.name
        clips = $children.Count
        accepted = $totalAdmitted
        peak_live = $project.peak_live_particles
        peak_histories = $project.peak_occupied_trails
        peak_concurrent_estimated_buffer_bytes = $memory
        status = 'bounded forward admission/cleanup passed; artistic and speed certification not performed'
    }
}
