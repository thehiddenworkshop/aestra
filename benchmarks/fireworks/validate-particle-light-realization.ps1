#requires -Version 7.0
# Read-only F7E portable-baseline gate. Not production finale/perceptual certification.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$ReportsDirectory)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
& "$PSScriptRoot/validate-show.ps1" -ReportsDirectory $ReportsDirectory -ReportPrefix show | Out-Null

function Distribution($Values) {
    $sorted = @($Values | Sort-Object)
    if ($sorted.Count -eq 0) { throw 'Missing timing/latency observations' }
    foreach ($value in $sorted) { if (![double]::IsFinite($value) -or $value -lt 0) { throw 'Invalid timing' } }
    [ordered]@{ samples = $sorted.Count; p50 = $sorted[[Math]::Max(0,[Math]::Ceiling($sorted.Count*0.50)-1)]
        p95 = $sorted[[Math]::Max(0,[Math]::Ceiling($sorted.Count*0.95)-1)]
        p99 = $sorted[[Math]::Max(0,[Math]::Ceiling($sorted.Count*0.99)-1)]
        max = $sorted[-1]; mean = ($sorted | Measure-Object -Average).Average }
}
function Metric($Report, [string]$Suffix) {
    $found = @($Report.metrics.PSObject.Properties | Where-Object { $_.Name.EndsWith($Suffix) })
    if ($found.Count -ne 1 -or $found[0].Value.samples -lt 100 -or $found[0].Value.p95 -le 0) { throw "Missing GPU metric: $Suffix" }
    $found[0].Value
}
$results = @()
foreach ($tier in @('high','medium','low')) {
    $path = Join-Path $ReportsDirectory "show-$tier.json"
    $r = Get-Content -LiteralPath $path -Raw | ConvertFrom-Json
    $cap = switch ($tier) { high {96}; medium {48}; low {24} }
    $flashCap = switch ($tier) { high {8}; medium {4}; low {2} }
    if (!$r.presentation.particle_light_realization -or !$r.presentation.representative_lights -or
        !$r.presentation.particle_light_benchmark_fixture -or !$r.presentation.response.hdr -or
        $r.presentation.global_particle_light_cap -ne $cap -or $r.presentation.particle_light_memory_mib -ne 64 -or
        ($r.presentation.headless_target -join ',') -ne '960,540') { throw "$tier incorrect setup" }
    foreach ($owner in $r.effect_backends.PSObject.Properties) {
        if (($owner.Value -join ',') -ne 'native GPU') { throw "$tier backend fallback" }
    }
    $pool = @($r.particle_light_pool)
    if ($pool.Count -ne 1680) { throw "$tier missing pool observations" }
    $bytes = 48*$cap+16
    foreach ($s in $pool) {
        if ($s.active -gt $s.allocated -or $s.allocated -gt $cap -or $s.copied_bytes -gt $bytes -or
            $s.pending -gt 3 -or $s.staging_bytes -gt 3*$bytes -or $s.failed -ne 0 -or $s.rejected -ne 0 -or
            $null -ne $s.rejection -or $s.frame_lag -gt 8 -or $s.update_age_ms -gt 100 -or $s.expired -ne 0 -or
            $s.representative_active -gt $s.representative_allocated -or $s.representative_allocated -gt $flashCap) {
            throw "$tier violated resource/latency/validity budget at sample $($s.sample)"
        }
    }
    $accepted = @($pool | Where-Object { $_.sequence -gt 0 } | Sort-Object sequence -Unique)
    if ($accepted.Count -lt 100 -or ($pool.active | Measure-Object -Maximum).Maximum -ne $cap -or
        @($pool | Where-Object { $_.active -gt 0 -and $_.representative_active -gt 0 }).Count -eq 0 -or
        $pool[-1].active -ne 0 -or $pool[-1].pending -ne 0 -or $pool[-1].staging_bytes -ne 0 -or
        $pool[-1].representative_active -ne 0) { throw "$tier saturation/coexistence/cleanup unproven" }
    $results += [ordered]@{
        tier = $tier; cap = $cap; adapter = $r.adapter; observations = $pool.Count; accepted_sets = $accepted.Count
        max_active = ($pool.active | Measure-Object -Maximum).Maximum
        max_allocated = ($pool.allocated | Measure-Object -Maximum).Maximum
        max_staging_bytes = ($pool.staging_bytes | Measure-Object -Maximum).Maximum
        max_frame_lag = ($accepted.frame_lag | Measure-Object -Maximum).Maximum
        update_age_ms = Distribution $accepted.update_age_ms
        pool_update_ms = Distribution $pool.pool_update_ms
        active_pool_update_ms = Distribution @($pool | Where-Object active | ForEach-Object pool_update_ms)
        selection_gpu_ms = Metric $r 'aestra::gpu::particle_lights/elapsed_gpu'
        copy_gpu_ms = Metric $r 'aestra::gpu::particle_light_copy/elapsed_gpu'
        frame_gpu_ms = Metric $r 'aestra::bench::full_frame/elapsed_gpu'
        max_representative_active = ($pool.representative_active | Measure-Object -Maximum).Maximum
        coexistence_samples = @($pool | Where-Object { $_.active -gt 0 -and $_.representative_active -gt 0 }).Count
        stale_sources = $pool[-1].stale_sources; expired = $pool[-1].expired
        stale_callbacks = $pool[-1].stale_callbacks; skipped_busy = $pool[-1].skipped_busy
        overwritten = $pool[-1].overwritten; failed = $pool[-1].failed; rejected = $pool[-1].rejected
        final_active = $pool[-1].active; final_allocated = $pool[-1].allocated
        final_staging_bytes = $pool[-1].staging_bytes
        raw_sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}
$disabledPath = Join-Path $ReportsDirectory 'show-disabled.json'
$disabled = Get-Content -LiteralPath $disabledPath -Raw | ConvertFrom-Json
if (!$disabled.presentation.particle_light_realization -or $disabled.presentation.global_particle_light_cap -ne 0 -or
    $disabled.particle_light_pool.Count -ne 1680 -or
    @($disabled.particle_light_pool | Where-Object { $_.active -ne 0 -or $_.allocated -ne 0 -or $_.submitted -ne 0 -or $_.staging_bytes -ne 0 }).Count -ne 0 -or
    @($disabled.particle_lights | Where-Object { $null -ne $_.sequence }).Count -ne 0 -or
    @($disabled.metrics.PSObject.Properties | Where-Object { $_.Name.Contains('particle_light_copy/elapsed_gpu') }).Count -ne 0) { throw 'Global disable did not remove light work' }
[ordered]@{ date = '2026-10-04'; scope = 'F7E portable baseline; unpaced fixed 60Hz full show, one sequential run per tier. Prior-main-world pool observations, accepted-set readback latency and independent GPU timings; no frame-paired subtraction or final-display/production certification.'
    runs = $results; disabled_raw_sha256 = (Get-FileHash -LiteralPath $disabledPath -Algorithm SHA256).Hash.ToLowerInvariant() } | ConvertTo-Json -Depth 8
