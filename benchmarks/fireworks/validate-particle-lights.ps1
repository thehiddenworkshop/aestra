#requires -Version 7.0
# F7D2B: selection instrumentation, not visible-light or finale certification.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$ReportsDirectory)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Keep the full per-clip admission, forward-history and cleanup gate.
& "$PSScriptRoot/validate-show.ps1" -ReportsDirectory $ReportsDirectory -ReportPrefix show-selection | Out-Null
& "$PSScriptRoot/validate-show.ps1" -ReportsDirectory $ReportsDirectory -ReportPrefix show-baseline | Out-Null

function Maximum($Samples, [string]$Property) {
    if ($Samples.Count -eq 0) { throw "Missing observations: $Property" }
    ($Samples | Measure-Object -Property $Property -Maximum).Maximum
}
function Metric($Report, [string]$Name) {
    $matches = @($Report.metrics.PSObject.Properties | Where-Object Name -eq $Name)
    if ($matches.Count -ne 1 -or $matches[0].Value.samples -lt 100 -or $matches[0].Value.p95 -le 0) {
        throw "Missing/insufficient GPU timing: $Name"
    }
    $matches[0].Value
}

$results = @()
foreach ($probe in @('hero', 'volley', 'show')) {
    $previousMemory = [long]::MaxValue
    foreach ($tier in @('high', 'medium', 'low')) {
        $cap = switch ($tier) { high { 96 }; medium { 48 }; low { 24 } }
        $selectedPath = Join-Path $ReportsDirectory "$probe-selection-$tier.json"
        $baselinePath = Join-Path $ReportsDirectory "$probe-baseline-$tier.json"
        $selected = Get-Content -LiteralPath $selectedPath -Raw | ConvertFrom-Json
        $baseline = Get-Content -LiteralPath $baselinePath -Raw | ConvertFrom-Json
        foreach ($report in @($selected, $baseline)) {
            if ($report.history_policy -ne 'playback-only' -or $report.presentation.quality_tier -ne $tier -or
                !$report.presentation.particle_light_benchmark_fixture -or $report.presentation.response.transient_lights -or
                !$report.presentation.response.hdr -or $report.presentation.render_mode -ne 'rendered' -or
                $report.presentation.particle_light_memory_mib -ne 64 -or
                ($report.presentation.headless_target -join ',') -ne '960,540' -or
                [Math]::Abs($report.presentation.fixed_simulation_step_seconds - 1.0 / 60.0) -gt 0.00000001 -or
                $report.warmup -ne 120 -or $report.frames -ne $(if ($probe -eq 'show') {1680} else {600})) {
                throw "$probe/$tier incorrect workload/presentation"
            }
            if (@($report.effect_backends.PSObject.Properties).Count -eq 0) { throw "$probe/$tier backend unobserved" }
            foreach ($owner in $report.effect_backends.PSObject.Properties) {
                if (($owner.Value -join ',') -ne 'native GPU') { throw "$probe/$tier backend fallback" }
            }
            $fragments = @($report.metrics.PSObject.Properties | Where-Object Name -Match 'main_transparent_pass_3d/fragment_shader_invocations$')
            if ($fragments.Count -ne 1 -or $fragments[0].Value.max -le 0) { throw "$probe/$tier normal transparent rendering unobserved" }
            foreach ($work in $report.work.PSObject.Properties.Value) {
                foreach ($field in @('source_event_overflow', 'max_trail_evictions', 'max_truncated_trails')) {
                    if ($null -ne $work.$field -and $work.$field -ne 0) { throw "$probe/$tier $field" }
                }
                foreach ($link in $work.links) {
                    if ($link.expansion_omitted -ne 0 -or $link.destination_rejected -ne 0 -or $link.accepted -ne $link.captured_demand) {
                        throw "$probe/$tier event admission changed"
                    }
                }
            }
        }
        if ($selected.adapter.name -ne $baseline.adapter.name -or $selected.adapter.backend -ne $baseline.adapter.backend -or
            $selected.presentation.global_particle_light_cap -ne $cap -or $baseline.presentation.global_particle_light_cap -ne 0) {
            throw "$probe/$tier baseline mismatch"
        }
        if (@($baseline.particle_lights | Where-Object {$null -ne $_.counters -or $null -ne $_.sequence}).Count -ne 0) {
            throw "$probe/$tier disabled selection generated a set"
        }
        $observed = @($selected.particle_lights | Where-Object counters)
        if ($observed.Count -lt 100) { throw "$probe/$tier missing async selected-set observations" }
        if (@($observed.sequence | Sort-Object -Unique).Count -ne $observed.Count) { throw "$probe/$tier reused sequence" }
        foreach ($sample in $selected.particle_lights) {
            if (!$sample.tick.measured -or $sample.rejected_outputs -ne 0 -or
                $sample.status -notin @('selected', 'empty_or_unprepared')) { throw "$probe/$tier invalid/rejected sample" }
            if (($sample.status -eq 'selected') -ne ($null -ne $sample.counters) -or
                ($null -ne $sample.counters) -ne ($null -ne $sample.sequence)) { throw "$probe/$tier inconsistent publication" }
            if ($null -ne $sample.counters) {
                $c = $sample.counters
                if ($c.Count -ne 4 -or $c[0] -lt $c[1] -or $c[1] -lt $c[2] -or $c[3] -ne $c[1] - $c[2] -or
                    $c[2] -gt $cap -or $c[2] -gt $sample.selected_capacity -or $sample.source_runs -le 0 -or
                    $null -eq $sample.reserved_bytes -or $sample.reserved_bytes -le 0 -or $sample.reserved_bytes -gt 64MB -or
                    $null -eq $sample.scratch_bytes -or $sample.scratch_bytes -gt $sample.reserved_bytes) { throw "$probe/$tier counter/resource invariant" }
            }
        }
        $peak = $observed | Sort-Object {$_.counters[1]} -Descending | Select-Object -First 1
        if ($peak.counters[1] -le $cap) { throw "$probe/$tier did not exercise budget drops" }
        $memory = Maximum $observed reserved_bytes
        if ($memory -gt $previousMemory) { throw "$probe selection memory did not scale down by quality" }
        $previousMemory = $memory
        $results += [pscustomobject]@{
            probe = $probe; tier = $tier; adapter = $selected.adapter; cap = $cap
            selection_gpu_ms = Metric $selected 'render/aestra::gpu::particle_lights/elapsed_gpu'
            frame_gpu_ms = Metric $selected 'render/aestra::bench::full_frame/elapsed_gpu'
            baseline_frame_gpu_ms = Metric $baseline 'render/aestra::bench::full_frame/elapsed_gpu'
            observations = $observed.Count
            counters_at_peak_candidates = $peak.counters
            max_source_runs = Maximum $observed source_runs
            max_reserved_bytes = $memory
            max_scratch_bytes = Maximum $observed scratch_bytes
            skipped_busy = $selected.light_skipped_busy
            overwritten_results = $selected.light_overwritten_results
            rejected_outputs = 0
            raw_selection_sha256 = (Get-FileHash -LiteralPath $selectedPath -Algorithm SHA256).Hash.ToLowerInvariant()
            raw_baseline_sha256 = (Get-FileHash -LiteralPath $baselinePath -Algorithm SHA256).Hash.ToLowerInvariant()
        }
    }
}
# JSON to stdout; callers can persist this generated evidence with their artifact tooling.
$results | ConvertTo-Json -Depth 8
