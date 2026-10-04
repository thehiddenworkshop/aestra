#requires -Version 7.0
# Read-only F7E4B1 authored-load resource/cost gate, not artistic/finale acceptance.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$ReportsDirectory)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
foreach ($mode in @('gpu','control')) {
    & "$PSScriptRoot/validate-show.ps1" -ReportsDirectory $ReportsDirectory -ReportPrefix "show-$mode" | Out-Null
}
function Maximum($Samples, [string]$Property) {
    if ($Samples.Count -eq 0) { throw "Missing observations: $Property" }
    ($Samples | Measure-Object -Property $Property -Maximum).Maximum
}
function Metric($Report, [string]$Suffix) {
    $found = @($Report.metrics.PSObject.Properties | Where-Object { $_.Name.EndsWith($Suffix) })
    if ($found.Count -ne 1 -or $found[0].Value.samples -lt 100 -or $found[0].Value.p95 -le 0) {
        throw "Missing/insufficient timing: $Suffix"
    }
    $found[0].Value
}
function Distribution($Values) {
    $sorted = @($Values | Sort-Object)
    if ($sorted.Count -eq 0) { throw 'Missing CPU observations' }
    foreach ($v in $sorted) { if ($null -eq $v -or ![double]::IsFinite($v) -or $v -lt 0) { throw 'Invalid CPU observation' } }
    [ordered]@{samples=$sorted.Count; p50=$sorted[[Math]::Ceiling($sorted.Count*.5)-1]
        p95=$sorted[[Math]::Ceiling($sorted.Count*.95)-1]; p99=$sorted[[Math]::Ceiling($sorted.Count*.99)-1]
        max=$sorted[-1]}
}
$runs = @()
foreach ($probe in @('hero','volley','show')) {
    foreach ($tier in @('high','medium','low')) {
        $cap = switch ($tier) {high {96}; medium {48}; low {24}}
        $cluster = switch ($tier) {high {@(4096,524288)}; medium {@(2048,262144)}; low {@(1024,131072)}}
        $frames = if ($probe -eq 'show') {1680} else {600}
        $effect = switch ($probe) {hero {'f4-reference-hero'}; volley {'f5-secondary-volley'}; show {'f6-show'}}
        $flashCap = if ($probe -eq 'show') {switch ($tier) {high {8}; medium {4}; low {2}}} else {0}
        $summary = [ordered]@{probe=$probe; tier=$tier; global_cap=$cap; cluster_initial_capacities=$cluster}
        foreach ($mode in @('gpu','control')) {
            $path = Join-Path $ReportsDirectory "$probe-$mode-$tier.json"
            $r = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
            $gpuCap = if ($mode -eq 'gpu') {$cap} else {0}
            if ($r.effect -ne $effect -or $r.history_policy -ne 'playback-only' -or $r.presentation.quality_tier -ne $tier -or
                !$r.presentation.particle_light_benchmark_fixture -or !$r.presentation.particle_light_realization -or
                $r.presentation.camera -ne 'audience' -or $r.presentation.render_mode -ne 'rendered' -or
                $r.presentation.transparent_order -ne 'fast' -or $r.presentation.seed -ne '0xf1e0000000000001' -or
                $r.presentation.particle_light_mode -ne 'gpu' -or $r.presentation.global_particle_light_cap -ne $cap -or
                $r.presentation.particle_light_gpu_cap -ne $gpuCap -or $r.presentation.particle_light_memory_mib -ne 64 -or
                ($r.presentation.particle_light_cluster_initial_capacities -join ',') -ne ($cluster -join ',') -or
                ($r.presentation.headless_target -join ',') -ne '960,540' -or !$r.presentation.response.hdr -or
                $r.presentation.representative_lights -ne ($probe -eq 'show') -or $r.warmup -ne 120 -or $r.frames -ne $frames -or
                $null -eq $r.presentation.fixed_simulation_step_seconds -or
                [Math]::Abs($r.presentation.fixed_simulation_step_seconds - 1.0/60.0) -gt .00000001) {
                throw "$probe/$tier/$mode incorrect setup"
            }
            if (@($r.effect_backends.PSObject.Properties).Count -eq 0) {throw 'Backend unobserved'}
            foreach ($owner in $r.effect_backends.PSObject.Properties) {
                if (($owner.Value -join ',') -ne 'native GPU') {throw 'Backend fallback'}
            }
            if ($r.particle_light_gpu.Count -ne $frames -or $r.particle_light_pool.Count -ne $frames -or
                $r.light_skipped_busy -ne 0 -or $r.light_overwritten_results -ne 0) {throw 'Missing/lost observations'}
            foreach ($s in $r.particle_light_gpu) {
                foreach ($field in @('dispatches','sequence','reserved_slots','written_capacity','buffer_bytes','invalid_sources','reserve_cpu_ms','authorize_cpu_ms')) {
                    if ($null -eq $s.$field -or ![double]::IsFinite($s.$field) -or $s.$field -lt 0) {throw "Missing/invalid GPU observation: $field"}
                }
                if ($null -ne $s.rejection -or $s.reserved_slots -gt $gpuCap -or $s.written_capacity -gt $s.reserved_slots -or
                    $s.buffer_bytes -gt 1MB -or $s.invalid_sources -ne 0) {throw "$probe/$tier/$mode adapter budget/qualification failure"}
            }
            foreach ($s in $r.particle_light_pool) {
                foreach ($field in @('active','allocated','submitted','pending','staging_bytes','copied_bytes','failed','representative_active','representative_allocated')) {
                    if ($null -eq $s.$field -or ![double]::IsFinite($s.$field) -or $s.$field -lt 0) {throw "Missing/invalid pool observation: $field"}
                }
                if ($s.active -ne 0 -or $s.allocated -ne 0 -or $s.submitted -ne 0 -or $s.pending -ne 0 -or
                    $s.staging_bytes -ne 0 -or $s.copied_bytes -ne 0 -or $s.failed -ne 0 -or $null -ne $s.rejection -or
                    $s.representative_active -gt $s.representative_allocated -or $s.representative_allocated -gt $flashCap) {
                    throw "$probe/$tier/$mode selected-position readback/proxy work or flash budget failure"
                }
            }
            $selected = @($r.particle_lights | Where-Object counters)
            if ($selected.Count -lt 100 -or @($selected.sequence | Sort-Object -Unique).Count -ne $selected.Count) {throw 'Missing/duplicate selected observations'}
            foreach ($s in $r.particle_lights) {
                if (!$s.tick.measured -or $s.rejected_outputs -ne 0 -or $s.status -notin @('selected','empty_or_unprepared')) {throw 'Selection rejected'}
                if (($s.status -eq 'selected') -ne ($null -ne $s.counters)) {throw 'Inconsistent selected publication'}
                if ($null -ne $s.counters) {
                    $c = $s.counters
                    if ($c.Count -ne 4 -or $c[0] -lt $c[1] -or $c[1] -lt $c[2] -or $c[3] -ne $c[1]-$c[2] -or
                        $c[2] -gt $cap -or $c[2] -gt $s.selected_capacity -or $s.source_runs -le 0 -or
                        $null -eq $s.reserved_bytes -or $s.reserved_bytes -le 0 -or $s.reserved_bytes -gt 64MB -or
                        $null -eq $s.scratch_bytes -or $s.scratch_bytes -gt $s.reserved_bytes) {throw 'Selection invariant/budget failure'}
                }
            }
            foreach ($work in $r.work.PSObject.Properties.Value) {
                foreach ($field in @('source_event_overflow','max_trail_evictions','max_truncated_trails')) {
                    if ($null -ne $work.$field -and $work.$field -ne 0) {throw "Produced work lost: $field"}
                }
                foreach ($link in $work.links) {
                    if ($link.accepted -ne $link.captured_demand -or $link.expansion_omitted -ne 0 -or $link.destination_rejected -ne 0) {throw 'Event admission changed'}
                }
            }
            $inject = @($r.metrics.PSObject.Properties | Where-Object { $_.Name.Contains('particle_light_inject/') })
            if ($mode -eq 'control') {
                if ((Maximum $r.particle_light_gpu dispatches) -ne 0 -or (Maximum $r.particle_light_gpu buffer_bytes) -ne 0 -or $inject.Count -ne 0) {throw 'Disabled adapter generated work'}
            } elseif ((Maximum $r.particle_light_gpu reserved_slots) -ne $cap -or
                (Maximum $r.particle_light_gpu written_capacity) -ne $cap -or
                @($r.particle_light_gpu.dispatches | Sort-Object -Unique).Count -lt 100) {throw 'Adapter dispatch/slot workload unproven'}
            if ($probe -eq 'show' -and ($r.particle_light_gpu[-1].written_capacity -ne 0 -or
                $r.particle_light_pool[-1].representative_active -ne 0 -or
                (Maximum $r.particle_light_pool representative_active) -le 0)) {throw 'Show lighting coexistence/lifecycle unproven'}
            $peakSelected = ($selected | ForEach-Object { $_.counters[2] } | Measure-Object -Maximum).Maximum
            if ($peakSelected -le 0) {throw 'Empty selected workload'}
            $summary[$mode] = [ordered]@{
                adapter=$r.adapter; selected_peak=$peakSelected; selected_observations=$selected.Count
                max_source_runs=Maximum $selected source_runs
                max_selector_reserved_bytes=Maximum $selected reserved_bytes
                max_adapter_logical_bytes=Maximum $r.particle_light_gpu buffer_bytes
                max_reserved_slots=Maximum $r.particle_light_gpu reserved_slots
                max_representative_active=Maximum $r.particle_light_pool representative_active
                reserve_cpu_ms=Distribution $r.particle_light_gpu.reserve_cpu_ms
                authorize_cpu_ms=Distribution $r.particle_light_gpu.authorize_cpu_ms
                selection_cpu_ms=Metric $r 'aestra::gpu::particle_lights/elapsed_cpu'
                selection_gpu_ms=Metric $r 'aestra::gpu::particle_lights/elapsed_gpu'
                inject_cpu_ms=$(if ($mode -eq 'gpu') {Metric $r 'aestra::gpu::particle_light_inject/elapsed_cpu'} else {$null})
                inject_gpu_ms=$(if ($mode -eq 'gpu') {Metric $r 'aestra::gpu::particle_light_inject/elapsed_gpu'} else {$null})
                clustering_cpu_ms=Metric $r 'clustering/elapsed_cpu'
                clustering_gpu_ms=Metric $r 'clustering/elapsed_gpu'
                frame_cpu_ms=Metric $r 'aestra::bench::full_frame/elapsed_cpu'
                frame_gpu_ms=Metric $r 'aestra::bench::full_frame/elapsed_gpu'
                transparent_fragments=Metric $r 'main_transparent_pass_3d/fragment_shader_invocations'
                raw_sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
            }
        }
        if ($summary.gpu.adapter.name -ne $summary.control.adapter.name -or $summary.gpu.adapter.backend -ne $summary.control.adapter.backend) {throw 'Adapter mismatch'}
        $runs += $summary
    }
}
[ordered]@{schema=1; date='2026-10-04'
    scope='F7E4B1: one sequential unpaced fixed-60Hz run per matrix cell, fresh selection-only controls with identical host cluster preallocation/representative flashes. Independent asynchronous observations, not frame-paired timings or exact active GPU-light counts. Full frame means outer render-graph window, not whole app/game. Adapter bytes are logical records + metadata; selector budget is separate; cluster capacities are requested initial sizes, not measured total memory. Not final-image authored receiver/registration, perspective art or production finale certification.'
    runs=$runs} | ConvertTo-Json -Depth 12
