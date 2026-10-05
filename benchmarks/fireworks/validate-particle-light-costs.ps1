#requires -Version 7.0
# Read-only F7E4B3A run-level matched-cost/public-allocation evidence gate.
[CmdletBinding()]
param([Parameter(Mandatory,ParameterSetName='Reports')][string]$ReportsDirectory,
    [Parameter(Mandatory,ParameterSetName='SelfTest')][switch]$SelfTest)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
function Hash([string]$Path) { (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
function Metric($Report, [string]$Suffix) {
    $matches = @($Report.metrics.PSObject.Properties | Where-Object { $_.Name.EndsWith($Suffix) })
    if ($matches.Count -ne 1 -or $matches[0].Value.samples -lt 100) { throw "Missing timing $Suffix" }
    $metric = $matches[0].Value
    foreach ($key in @('mean','p50','p95','p99','min','max','stddev')) {
        if (![double]::IsFinite($metric.$key) -or $metric.$key -lt 0) { throw "Invalid timing $Suffix/$key" }
    }
    $metric
}
function WorkSignature($Report) {
    # Stable authored paths/ids/seeds/capacities/policy and delivered event totals.
    # Do not require equality of asynchronous peaks, samples or ECS entity IDs.
    $entries = @($Report.instances.PSObject.Properties | ForEach-Object {
        $instance = $_.Value
        $work = $Report.work.($_.Name)
        # A composition root has no native event-link observation (null), unlike
        # an observed empty link list. Preserve that distinction, never fake zero.
        $links = $null
        if ($null -ne $work.links) {
            $links = @($work.links | ForEach-Object {
                [ordered]@{demand=$_.captured_demand; accepted=$_.accepted
                    omitted=$_.expansion_omitted; rejected=$_.destination_rejected}
            })
        }
        [pscustomobject][ordered]@{
            path=($instance.clip_path -join '/'); source=$instance.source_effect; seed=$instance.seed
            capacity=$instance.particle_capacity; history=$instance.history_policy
            links=$links
        }
    } | Sort-Object path,source,seed)
    ConvertTo-Json -InputObject $entries -Depth 12 -Compress
}
function Clusters($Report) {
    if ($Report.cluster_overwritten_results -ne 0) { throw 'Lost cluster observations' }
    $measured = @($Report.cluster_buffers | Where-Object { $_.tick.measured })
    if ($measured.Count -lt $Report.frames-4 -or !$Report.cluster_buffers_scope.Contains('Excludes private')) { throw 'Missing cluster evidence/qualification' }
    $previous = 0L
    $viewIds = @($measured.views.main_entity | Sort-Object -Unique)
    if ($viewIds.Count -ne 1) { throw 'Expected one native effect view' }
    $indices = @()
    $offsets = @()
    $lights = @()
    foreach ($observation in $Report.cluster_buffers) {
        if ($observation.sequence -le $previous) { throw 'Repeated/out-of-order cluster observation' }
        $previous = $observation.sequence
        if (!$observation.tick.measured) { continue }
        if ($observation.views.Count -ne 1 -or $null -eq $observation.global_light_bytes -or $observation.global_light_bytes -le 0) { throw 'Missing measured buffer' }
        $view = $observation.views[0]
        if ($null -ne $view.z_slice_bytes) { throw 'Unexpected claim of private Z-slice measurement' }
        foreach ($field in @('index_bytes','offsets_counts_bytes')) {
            if ($null -eq $view.$field -or $view.$field -le 0) { throw "Missing physical size $field" }
        }
        $indices += $view.index_bytes
        $offsets += $view.offsets_counts_bytes
        $lights += $observation.global_light_bytes
    }
    # Initial settings are capacities, not byte measurements; growth is detected
    # from the accessible physical lists and independently from native logs.
    if (@($indices | Sort-Object -Unique).Count -ne 1 -or @($offsets | Sort-Object -Unique).Count -ne 1) {
        throw 'Native public cluster-list sizes grew/changed during measured playback'
    }
    $target = $Report.presentation.particle_light_cluster_adaptive_index_target
    if ($null -eq $target -or $target -le 0 -or $indices[0] -ne 4*$target -or
        $Report.cluster_main.Count -ne $Report.frames) { throw 'Missing/mismatched native cluster target observations' }
    $demands = @()
    $grids = @()
    $sample = 0
    foreach ($observation in $Report.cluster_main) {
        if ($observation.sample -ne $sample++ -or !$observation.gpu_clustering_enabled -or
            $observation.adaptive_index_target -ne $target -or $observation.views.Count -ne 1 -or
            $observation.views[0].main_entity -ne $viewIds[0]) { throw 'Incorrect native cluster main-world observation' }
        $view = $observation.views[0]
        if ($view.dimensions.Count -ne 3 -or @($view.dimensions | Where-Object { $_ -le 0 }).Count -ne 0 -or
            $view.dimensions[0]*$view.dimensions[1]*$view.dimensions[2] -gt 4096) { throw 'Invalid native cluster grid' }
        $grids += $view.dimensions -join ','
        if ($null -ne $view.last_native_index_demand) {
            if ($view.last_native_index_demand -lt 0 -or $view.last_native_index_demand -gt $target) { throw 'Native index demand exceeds configured storage' }
            $demands += $view.last_native_index_demand
        }
    }
    if ($demands.Count -eq 0 -or @($grids | Sort-Object -Unique).Count -ne 1) { throw 'Missing native demand or changing measured grid' }
    if ($Report.presentation.particle_light_gpu_cap -gt 0 -and ($demands | Measure-Object -Maximum).Maximum -le 0) { throw 'Enabled native demand unobserved' }
    [ordered]@{observations=$measured.Count; index_bytes=$indices[0]; offsets_counts_bytes=$offsets[0]
        global_light_bytes_min=($lights | Measure-Object -Minimum).Minimum
        global_light_bytes_max=($lights | Measure-Object -Maximum).Maximum
        adaptive_index_target=$target; grid_dimensions=$grids[0]
        native_async_index_demand_observations=$demands.Count
        native_async_index_demand_max=($demands | Measure-Object -Maximum).Maximum
        native_async_index_demand_at_last_main_sample=$Report.cluster_main[-1].views[0].last_native_index_demand
        public_list_growth=$false; private_z_slice_bytes=$null; private_staging_bytes=$null}
}
if ($SelfTest) {
    function Fixture {
        $view = [pscustomobject]@{main_entity='camera'; z_slice_bytes=$null; index_bytes=256; offsets_counts_bytes=32}
        [pscustomobject]@{frames=5; cluster_overwritten_results=0; cluster_buffers_scope='Excludes private'
            presentation=[pscustomobject]@{particle_light_cluster_adaptive_index_target=64; particle_light_gpu_cap=1}
            cluster_main=@(0..4 | ForEach-Object { [pscustomobject]@{sample=$_; gpu_clustering_enabled=$true; adaptive_index_target=64
                views=@([pscustomobject]@{main_entity='camera'; dimensions=@(2,2,2); last_native_index_demand=12})} })
            cluster_buffers=@([pscustomobject]@{sequence=1; tick=[pscustomobject]@{measured=$true}
                global_light_bytes=80; views=@($view)})}
    }
    Clusters (Fixture) | Out-Null
    foreach ($failure in @('lost','unknown','private','duplicate','growth','no-view','target','demand','grid','main-gap','cpu-fallback','camera')) {
        $fixture = Fixture
        switch ($failure) {
            lost { $fixture.cluster_overwritten_results = 1 }
            unknown { $fixture.cluster_buffers[0].views[0].index_bytes = $null }
            private { $fixture.cluster_buffers[0].views[0].z_slice_bytes = 12 }
            duplicate { $fixture.cluster_buffers += $fixture.cluster_buffers[0] }
            growth {
                $second = (Fixture).cluster_buffers[0]
                $second.sequence = 2
                $second.views[0].index_bytes = 512
                $fixture.cluster_buffers += $second
            }
            no-view { $fixture.cluster_buffers[0].views = @() }
            target { $fixture.cluster_main[0].adaptive_index_target = 32 }
            demand { $fixture.cluster_main[0].views[0].last_native_index_demand = 65 }
            grid { $fixture.cluster_main[0].views[0].dimensions = @(1,2,2) }
            main-gap { $fixture.cluster_main[0].sample = 1 }
            cpu-fallback { $fixture.cluster_main[0].gpu_clustering_enabled = $false }
            camera { $fixture.cluster_main[0].views[0].main_entity = 'different' }
        }
        $rejected = $false
        try { Clusters $fixture | Out-Null } catch { $rejected = $true }
        if (!$rejected) { throw "Negative control incorrectly accepted: $failure" }
    }
    function WorkFixture([string]$Entity, [int]$Demand) {
        [pscustomobject]@{instances=[pscustomobject]@{$Entity=[pscustomobject]@{clip_path=@('clip'); source_effect='source'
            seed='seed'; particle_capacity=100; history_policy='playback-only'}}
            work=[pscustomobject]@{$Entity=[pscustomobject]@{links=@([pscustomobject]@{captured_demand=$Demand
                accepted=$Demand; expansion_omitted=0; destination_rejected=0})}}}
    }
    if ((WorkSignature (WorkFixture 'first' 5)) -ne (WorkSignature (WorkFixture 'second' 5))) { throw 'ECS identity polluted work signature' }
    if ((WorkSignature (WorkFixture 'first' 5)) -eq (WorkSignature (WorkFixture 'second' 6))) { throw 'Changed event work accepted' }
    $unobserved = WorkFixture 'root' 0
    $unobserved.work.root.links = $null
    $empty = WorkFixture 'root' 0
    $empty.work.root.links = @()
    if (!(WorkSignature $unobserved).Contains('"links":null') -or
        (WorkSignature $unobserved) -eq (WorkSignature $empty)) { throw 'Null root links became fake zero/empty observations' }
    'Public-allocation/work-signature positive and negative controls passed'
    return
}
$directory = (Resolve-Path -LiteralPath $ReportsDirectory).Path
$manifest = Get-Content -LiteralPath "$directory/manifest.json" -Raw | ConvertFrom-Json
if ($manifest.schema -ne 1 -or $manifest.repetitions -lt 3 -or $manifest.repetitions -gt 8 -or
    !$manifest.inputs_unchanged -or $manifest.asset_inputs.Count -eq 0 -or
    $manifest.tiers.Count -eq 0 -or @($manifest.tiers | Sort-Object -Unique).Count -ne $manifest.tiers.Count -or
    @($manifest.tiers | Where-Object { $_ -notin @('high','medium','low') }).Count -gt 0 -or
    $manifest.runs.Count -ne $manifest.repetitions*$manifest.tiers.Count*6) { throw 'Incomplete/invalid run manifest' }
$rows = @()
$position = 0
$adapterSignature = $null
for ($repeat = 1; $repeat -le $manifest.repetitions; $repeat++) {
    $pair = 'pair{0:00}' -f $repeat
    # Existing strict gates retain show admission/cleanup, all selection/proxy
    # budgets and normal transparent work. Tiers default still covers all presets.
    & "$PSScriptRoot/validate-particle-light-gpu-workloads.ps1" -ReportsDirectory "$directory/$pair" -Tiers $manifest.tiers | Out-Null
    $cell = 0
    foreach ($tier in $manifest.tiers) {
        foreach ($probe in @('hero','volley','show')) {
            $order = if (($repeat + $cell) % 2 -eq 1) { @('gpu','control') } else { @('control','gpu') }
            $cell++
            $reports = @{}
            $summary = [ordered]@{pair=$pair; probe=$probe; tier=$tier; order=$order}
            foreach ($mode in $order) {
                $entry = $manifest.runs[$position++]
                $stem = "$probe-$mode-$tier"
                if ($entry.pair -ne $pair -or $entry.probe -ne $probe -or $entry.tier -ne $tier -or
                    $entry.mode -ne $mode -or $entry.stem -ne $stem -or $entry.exit_code -ne 0) { throw 'Incorrect/duplicate run order or unsuccessful native exit' }
                $path = "$directory/$pair/$stem"
                if ((Hash "$path.json") -ne $entry.report_sha256 -or (Hash "$path.log") -ne $entry.log_sha256) { throw 'Report/log hash mismatch' }
                $log = Get-Content -LiteralPath "$path.log" -Raw
                if ($log -match '(?i)Resizing the view clustering|lighting may have been corrupted|ERROR|panicked|validation error') { throw "$stem native growth/error log gate failed" }
                $report = Get-Content -LiteralPath "$path.json" -Raw | ConvertFrom-Json
                $adapter = $report.adapter | ConvertTo-Json -Compress
                if ($null -eq $adapterSignature) { $adapterSignature = $adapter }
                if ($adapter -ne $adapterSignature) { throw 'Adapter changed across repeated runs' }
                $reports[$mode] = $report
                $summary[$mode] = [ordered]@{
                    report_sha256=$entry.report_sha256; log_sha256=$entry.log_sha256
                    adapter=$report.adapter; clusters=Clusters $report
                    render_gpu_ms=Metric $report 'aestra::bench::full_frame/elapsed_gpu'
                    render_cpu_ms=Metric $report 'aestra::bench::full_frame/elapsed_cpu'
                    selection_gpu_ms=Metric $report 'aestra::gpu::particle_lights/elapsed_gpu'
                    selection_cpu_ms=Metric $report 'aestra::gpu::particle_lights/elapsed_cpu'
                    clustering_gpu_ms=Metric $report 'clustering/elapsed_gpu'
                    clustering_cpu_ms=Metric $report 'clustering/elapsed_cpu'
                    inject_gpu_ms=if ($mode -eq 'gpu') { Metric $report 'aestra::gpu::particle_light_inject/elapsed_gpu' } else { $null }
                    inject_cpu_ms=if ($mode -eq 'gpu') { Metric $report 'aestra::gpu::particle_light_inject/elapsed_cpu' } else { $null }
                }
            }
            if ((WorkSignature $reports.gpu) -ne (WorkSignature $reports.control)) { throw "$pair/$probe/$tier authored work differs" }
            foreach ($key in @('render_gpu_ms','render_cpu_ms','selection_gpu_ms','selection_cpu_ms','clustering_gpu_ms','clustering_cpu_ms')) {
                $summary["${key}_run_mean_difference"] = $summary.gpu[$key].mean - $summary.control[$key].mean
            }
            $summary.work_signature_matched = $true
            $rows += $summary
        }
    }
}
[ordered]@{schema=1; milestone='F7E4B3A'; accepted=$true
    scope='Repeated alternating run-level on/off pairs, matched authored identity/compiled capacity/seeds/event admission. Independent timing distributions, not frame-paired subtraction or statistical significance. Outer render graph, not whole app/game. Public physical native buffer sizes/no resize-warning gate and public asynchronous native index-demand acknowledgments, not paired with timing/buffer frames. Private Z-slice demand/allocations/staging and in-flight memory remain unmeasured. One GPU/headless target/default layers, not finale certification.'
    binary_sha256=$manifest.binary_sha256; rows=$rows} | ConvertTo-Json -Depth 16
