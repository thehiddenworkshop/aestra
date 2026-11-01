param(
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [string]$Binary = 'target/debug/fireworks.exe',
    [string]$ProjectRoot = '',
    [ValidateSet('host-overlap','host-spaced','host-finale','host-variety','pair')][string]$Mode = 'host-overlap',
    [ValidateSet('audience','sea','close','wide','side','near','water','orbit')][string]$Camera = 'close',
    [ValidateSet('calibration','sea')][string]$Scene = 'calibration',
    [ValidateSet('calm','rough')][string]$Water = 'rough',
    [switch]$WaterReflections,
    [ValidateSet('control','tuned')][string[]]$Variants = @('control','tuned'),
    [ValidateSet('high','medium','low')][string[]]$Tiers = @('high'),
    [ValidateSet('default','ordinary','shared')][string]$Transparency = 'default',
    [switch]$MatchMsaa,
    [Nullable[double]]$FragmentsPerPixel = $null,
    [ValidateSet(0,1,2,4,8)][int]$Partitions = 0,
    [switch]$RequireLossFree,
    [string]$SampleTimes = '1.8,2.8,4,5,6.9,8,12,16,23,36',
    [switch]$Cost,
    [Alias('Profile')][switch]$Instrumented,
    [switch]$SimulationPhases,
    [switch]$HideStarSmoke,
    [switch]$NoStarLights,
    [switch]$NoLaunchLights,
    [switch]$NoFlashLights
)
$ErrorActionPreference = 'Stop'
if ($Instrumented -and -not $Cost) { throw '-Instrumented requires -Cost (no screenshots or capacity scans).' }
if ($SimulationPhases -and -not $Instrumented) { throw '-SimulationPhases requires -Instrumented (separate diagnostic lane).' }
if ($Mode -eq 'host-variety' -and ($Variants.Count -ne 1 -or $Variants[0] -ne 'tuned')) { throw 'The variety sequence uses its authored repertoire; select -Variants tuned (no control comparison is defined).' }
if ($Scene -eq 'sea' -and $Mode -eq 'pair') { throw '-Scene sea requires a host rocket mode.' }
if ($NoLaunchLights -and $Scene -ne 'sea') { throw '-NoLaunchLights requires -Scene sea.' }
if ($WaterReflections -and $Scene -ne 'sea') { throw '-WaterReflections requires -Scene sea.' }
if ($PSBoundParameters.ContainsKey('Water') -and $Scene -ne 'sea') { throw '-Water requires -Scene sea.' }
if ($Cost -and $HideStarSmoke) { throw 'Smoke visibility controls require the image lane.' }
if ($Cost -and $Camera -eq 'orbit') { throw 'Orbit review requires the image lane; ordinary costs keep the audience view.' }
if ($null -ne $FragmentsPerPixel -and -not ($Transparency -eq 'shared' -or ($Transparency -eq 'default' -and $Mode -eq 'pair'))) { throw '-FragmentsPerPixel requires shared transparency.' }
if ($Partitions -ne 0 -and ($Transparency -ne 'shared' -or $Mode -eq 'pair')) { throw '-Partitions requires -Transparency shared with host rockets.' }
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$output = [IO.Path]::GetFullPath($OutputDirectory,$repo)
$exe = [IO.Path]::GetFullPath($Binary,$repo)
$project = if ([string]::IsNullOrWhiteSpace($ProjectRoot)) { Join-Path $repo 'assets/test' } else { [IO.Path]::GetFullPath($ProjectRoot,$repo) }
if (-not (Test-Path -LiteralPath (Join-Path $project 'effects') -PathType Container)) { throw 'ProjectRoot must contain an effects folder.' }
if (Test-Path -LiteralPath $output) { throw 'Use a fresh output directory.' }
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw 'Build the fireworks demo first.' }
New-Item -ItemType Directory -Path $output | Out-Null
$variables = @('AESTRA_EXAMPLE_REPORT','AESTRA_EXAMPLE_REPORT_IMAGES','AESTRA_EXAMPLE_REPORT_CAMERA',
    'AESTRA_EXAMPLE_REPORT_SAMPLE_TIMES','AESTRA_EXAMPLE_REPORT_STAR_SMOKE','AESTRA_EXAMPLE_REPORT_FLASHES',
    'AESTRA_EXAMPLE_PROFILE','AESTRA_EXAMPLE_CAPTURE','AESTRA_EXAMPLE_REPORT_MSAA','AESTRA_EXAMPLE_SIMULATION_PHASES')
$saved = @{}
foreach ($name in $variables) { $saved[$name] = [Environment]::GetEnvironmentVariable($name,'Process') }
Push-Location $repo
try {
    Remove-Item Env:AESTRA_EXAMPLE_PROFILE,Env:AESTRA_EXAMPLE_CAPTURE -ErrorAction SilentlyContinue
    if ($Instrumented) { $env:AESTRA_EXAMPLE_PROFILE = '1' }
    $env:AESTRA_EXAMPLE_SIMULATION_PHASES = if ($SimulationPhases) { '1' } else { '0' }
    $env:AESTRA_EXAMPLE_REPORT_IMAGES = if ($Cost) { '0' } else { '1' }
    $env:AESTRA_EXAMPLE_REPORT_CAMERA = if ($Scene -eq 'sea' -and -not $PSBoundParameters.ContainsKey('Camera')) { 'sea' } elseif ($Cost -and -not $PSBoundParameters.ContainsKey('Camera')) { 'audience' } else { $Camera }
    if ($MatchMsaa) { $env:AESTRA_EXAMPLE_REPORT_MSAA = 'off' }
    else { Remove-Item Env:AESTRA_EXAMPLE_REPORT_MSAA -ErrorAction SilentlyContinue }
    $env:AESTRA_EXAMPLE_REPORT_STAR_SMOKE = if ($HideStarSmoke) { '0' } else { '1' }
    $env:AESTRA_EXAMPLE_REPORT_FLASHES = '1'
    if ($Cost) { Remove-Item Env:AESTRA_EXAMPLE_REPORT_SAMPLE_TIMES -ErrorAction SilentlyContinue }
    else { $env:AESTRA_EXAMPLE_REPORT_SAMPLE_TIMES = if ($Mode -eq 'pair') { ($SampleTimes -split ',' | Where-Object { [double]$_ -le 24 }) -join ',' } else { $SampleTimes } }
    $manifest = [ordered]@{ binary_sha256=(Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash;
        project_root=$project; project_inputs=@(); scene=$Scene; water=if ($Scene -eq 'sea') { $Water } else { $null }; water_reflections=[bool]$WaterReflections; no_launch_lights=[bool]$NoLaunchLights;
        mode=$Mode; camera=$env:AESTRA_EXAMPLE_REPORT_CAMERA; transparency=$Transparency; match_msaa=[bool]$MatchMsaa; lane=if ($SimulationPhases) { 'simulation-phases' } elseif ($Instrumented) { 'instrumented' } elseif ($Cost) { 'ordinary' } else { 'images' }; no_flash_lights=[bool]$NoFlashLights; no_star_lights=[bool]$NoStarLights; hide_star_smoke=[bool]$HideStarSmoke; inputs=@(); runs=@() }
    $inputs = @(Get-ChildItem demos/fireworks/src -Recurse -File | ForEach-Object { [IO.Path]::GetRelativePath($repo,$_.FullName) }) +
        @(Get-ChildItem assets/test/materials -File | ForEach-Object { [IO.Path]::GetRelativePath($repo,$_.FullName) }) +
        @(Get-ChildItem assets/test/effects -Filter '*smoke.aestra.ron' -File | ForEach-Object { [IO.Path]::GetRelativePath($repo,$_.FullName) }) +
        @(Get-ChildItem bevy/aestra-bevy-render/src/experimental_oit -Filter 'partition*.rs' -File | ForEach-Object { [IO.Path]::GetRelativePath($repo,$_.FullName) }) +
        @(Get-ChildItem bevy/aestra-bevy-render/src/preview/planar -Recurse -File | ForEach-Object { [IO.Path]::GetRelativePath($repo,$_.FullName) }) +
        @('assets/test/effects/fireworks_shell_star_lights.aestra.ron',
          'bevy/aestra-bevy/src/playback.rs','bevy/aestra-bevy/src/presentation_cache.rs',
          'bevy/aestra-bevy/src/host_plugin.rs','bevy/aestra-bevy/src/lib.rs',
          'bevy/aestra-bevy-render/src/presented_effect.rs','bevy/aestra-bevy-render/src/material.rs',
          'bevy/aestra-bevy-render/src/gpu/shader_composition.rs','bevy/aestra-bevy-render/src/experimental_oit.rs',
          'bevy/aestra-bevy-render/src/gpu/alpha_sort.rs','bevy/aestra-bevy-render/src/gpu/alpha_sort.wgsl',
          'bevy/aestra-bevy-render/src/gpu/view_phases.rs','bevy/aestra-bevy-render/src/gpu/render.rs',
          'bevy/aestra-bevy-render/src/gpu/pipeline.rs',
          'bevy/aestra-bevy-render/src/preview/planar.rs','bevy/aestra-bevy-render/src/gpu/pipeline_preparation.rs',
          'bevy/aestra-bevy-render/src/gpu/trail_culling.rs',
          'crates/aestra-gpu/src/shaders/advected_smoke.wesl',
          'bevy/aestra-bevy-render/src/preview/water.rs','bevy/aestra-bevy-render/src/preview/water.wesl',
          'bevy/aestra-bevy-render/src/experimental_oit/tests/partition.rs',
          'bevy/aestra-bevy-render/src/experimental_oit/sort.rs','bevy/aestra-bevy-render/src/experimental_oit/sort.wesl',
          'bevy/aestra-bevy-render/src/experimental_oit/resolve.wesl')
    foreach ($path in $inputs) { $manifest.inputs += @{path=$path; sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash} }
    foreach ($file in Get-ChildItem -LiteralPath $project -File -Recurse) {
        $manifest.project_inputs += @{path=[IO.Path]::GetRelativePath($project,$file.FullName); sha256=(Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash}
    }
    foreach ($tier in $Tiers) {
        foreach ($variant in $Variants) {
            $name = "$variant-$tier"
            $report = Join-Path $output "$name.json"
            $env:AESTRA_EXAMPLE_REPORT = $report
            $launchArguments = @('--tier',$tier,'--no-audio','--project',$project)
            if ($Scene -eq 'sea') { $launchArguments += @('--scene','sea','--water',$Water) }
            if ($WaterReflections) { $launchArguments += '--water-reflections' }
            if (-not $NoStarLights) { $launchArguments += '--star-lights' }
            if ($NoFlashLights) { $launchArguments += '--no-flash-lights' }
            if ($NoLaunchLights) { $launchArguments += '--no-launch-lights' }
            if ($Mode -ne 'pair') {
                $pattern = switch ($Mode) { 'host-variety' { 'variety' } 'host-spaced' { 'spaced' } 'host-finale' { 'finale' } default { 'overlap' } }
                $launchArguments += @('--launch-pattern',$pattern)
                # Ordinary 12-shell benchmark keeps its historical explicit budget.
                # The finale measures all 32 requests without admission throttling.
                if ($Mode -in @('host-overlap','host-spaced')) { $launchArguments += @('--max-owners','24') }
                if ($variant -eq 'tuned') { $launchArguments += '--tuned-smoke' }
            } else {
                $effect = if ($variant -eq 'tuned') { 'effects/fireworks_shell_tuned_smoke.aestra.ron' } else { 'effects/fireworks_shell_star_lights.aestra.ron' }
                $launchArguments += @('--smoke-art-show','--effect',$effect)
            }
            if ($Transparency -eq 'shared' -or ($Transparency -eq 'default' -and $Mode -eq 'pair')) {
                $launchArguments += '--experimental-oit'
                if ($Partitions -ne 0) { $launchArguments += @('--oit-partitions', "$Partitions") }
                if ($null -ne $FragmentsPerPixel) { $launchArguments += @('--oit-fragments-per-pixel', ([double]$FragmentsPerPixel).ToString([Globalization.CultureInfo]::InvariantCulture)) }
            }
            & $exe @launchArguments *> (Join-Path $output "$name.log")
            $code = $LASTEXITCODE
            $result = Get-Content -LiteralPath $report -Raw | ConvertFrom-Json
            $lossFree = if ($result.experimental_oit -and $result.capacity) { [bool]$result.capacity.loss_free } else { $null }
            $manifest.runs += @{name=$name; exit_code=$code; arguments=$launchArguments; report=$report; fragment_capacity_passed=$lossFree}
            $manifest | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $output 'manifest.json')
            if ($code -ne 0 -or -not $result.completed) { throw "Native run failed: $name ($code)" }
            if ($WaterReflections -and ($null -eq $result.planar_reflection -or $null -ne $result.planar_reflection.rejection -or $result.planar_reflection.peak_queued_additive_draws -eq 0)) {
                throw "No accepted additive reflection view: $name"
            }
            if ($result.event_link_admission_truncated) { throw "Event audit was truncated: $name" }
            foreach ($route in $result.event_link_admission.PSObject.Properties.Value) {
                if ($route.source_overflow -ne 0 -or $route.counts.destination_rejected -ne 0 -or $route.counts.expansion_omitted -ne 0) {
                    throw "Particle event admission lost work: $name ($($route.source) -> $($route.target))"
                }
            }
            if ($Mode -ne 'pair') {
                $expected = switch ($Mode) { 'host-variety' { 22 } 'host-finale' { 32 } 'host-spaced' { 5 } default { 12 } }
                if ($result.host_state.launches -ne $expected -or $result.host_state.detonations -ne $expected -or
                    $result.host_state.current_gameplay_rockets -ne 0 -or $result.host_state.current_vfx_owners -ne 0 -or
                    $result.last_async_alive -ne 0) { throw "Incomplete workload or cleanup: $name (expected $expected launches/detonations and zero final owners/particles)" }
                if ($Mode -in @('host-finale','host-variety') -and ($null -ne $result.host_state.owner_limit -or $result.host_state.capacity_wait_updates -ne 0)) {
                    throw "Host sequence was admission-throttled: $name"
                }
                if ($Mode -eq 'host-variety' -and @($result.host_state.families_launched.PSObject.Properties).Count -ne 8) { throw "Variety sequence did not launch all eight families: $name" }
            }
            if ($RequireLossFree -and $result.experimental_oit -and $lossFree -ne $true) { throw "Shared transparency qualification failed: $name (fragment loss or no capacity scan); raw report retained." }
            if ($lossFree -eq $false) { Write-Output "CAPACITY FAILED: $name dropped up to $($result.capacity.max_dropped) fragments in an observed frame; completion is not acceptance." }
            Write-Output "$Mode/$name completed; host launches=$($result.host_state.launches), flashes=$($result.flashes_accepted), final alive=$($result.last_async_alive)"
        }
    }
} finally {
    foreach ($name in $variables) { [Environment]::SetEnvironmentVariable($name,$saved[$name],'Process') }
    Pop-Location
}
