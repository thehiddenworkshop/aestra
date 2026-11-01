param(
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [string]$Binary = 'target/release/fireworks.exe',
    [ValidateRange(2,6)][int]$Repetitions = 3
)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$output = [IO.Path]::GetFullPath($OutputDirectory,$repo)
$exe = [IO.Path]::GetFullPath($Binary,$repo)
if (Test-Path -LiteralPath $output) { throw 'Use a fresh output directory.' }
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw 'Build the release demo first.' }
New-Item -ItemType Directory -Path $output | Out-Null
$variables = @('AESTRA_EXAMPLE_REPORT','AESTRA_EXAMPLE_REPORT_IMAGES','AESTRA_EXAMPLE_REPORT_CAMERA',
    'AESTRA_EXAMPLE_REPORT_SAMPLE_TIMES','AESTRA_EXAMPLE_REPORT_STAR_SMOKE','AESTRA_EXAMPLE_REPORT_FLASHES',
    'AESTRA_EXAMPLE_PROFILE','AESTRA_EXAMPLE_CAPTURE','AESTRA_EXAMPLE_REPORT_MSAA')
$saved = @{}
foreach ($name in $variables) {
    $saved[$name] = [Environment]::GetEnvironmentVariable($name,'Process')
}
$manifest = [ordered]@{ preset='fireworks-showcase-v1'; camera='audience'; size=@(1280,720);
    binary=$exe; binary_sha256=(Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash;
    head=(git -C $repo rev-parse HEAD); repetitions=$Repetitions;
    arguments=@('--showcase','--no-audio'); inputs=@(); runs=@();
    scope='Release candidate baseline. Separate image, ordinary and timestamp lanes. No performance/art adoption. Partition health runs in every lane. Warm-up excluded. Run alone without builds or other GPU workloads.' }
$inputs = @('Cargo.toml','Cargo.lock','rust-toolchain.toml','demos/fireworks/build.rs','benchmarks/fireworks/run-showcase-baseline.ps1') +
    @(Get-ChildItem (Join-Path $repo 'demos/fireworks/src') -File -Recurse | ForEach-Object { [IO.Path]::GetRelativePath($repo,$_.FullName) }) +
    @(Get-ChildItem (Join-Path $repo 'assets/test') -File -Recurse | ForEach-Object { [IO.Path]::GetRelativePath($repo,$_.FullName) }) +
    @(Get-ChildItem (Join-Path $repo 'bevy/aestra-bevy-render/src') -File -Recurse | ForEach-Object { [IO.Path]::GetRelativePath($repo,$_.FullName) }) +
    @('bevy/aestra-bevy/src/host_plugin.rs','bevy/aestra-bevy/src/playback.rs','bevy/aestra-bevy/src/presentation_cache.rs')
foreach ($path in $inputs) { $manifest.inputs += @{path=$path; sha256=(Get-FileHash -LiteralPath (Join-Path $repo $path) -Algorithm SHA256).Hash} }

function Run-Lane([string]$name, [bool]$profile, [bool]$images) {
    Remove-Item Env:AESTRA_EXAMPLE_CAPTURE,Env:AESTRA_EXAMPLE_PROFILE -ErrorAction SilentlyContinue
    $env:AESTRA_EXAMPLE_REPORT = Join-Path $output "$name.json"
    $env:AESTRA_EXAMPLE_REPORT_CAMERA = 'audience'
    $env:AESTRA_EXAMPLE_REPORT_IMAGES = if ($images) { '1' } else { '0' }
    $env:AESTRA_EXAMPLE_REPORT_STAR_SMOKE = '1'
    $env:AESTRA_EXAMPLE_REPORT_FLASHES = '1'
    if ($images) { $env:AESTRA_EXAMPLE_REPORT_SAMPLE_TIMES = '2.8,6.9,12,23,36' }
    else { Remove-Item Env:AESTRA_EXAMPLE_REPORT_SAMPLE_TIMES -ErrorAction SilentlyContinue }
    Remove-Item Env:AESTRA_EXAMPLE_REPORT_MSAA -ErrorAction SilentlyContinue
    if ($profile) { $env:AESTRA_EXAMPLE_PROFILE = '1' }
    & $exe --showcase --no-audio *> (Join-Path $output "$name.log")
    $code = $LASTEXITCODE
    $manifest.runs += @{name=$name; exit_code=$code; profile=$profile; images=$images; report=$env:AESTRA_EXAMPLE_REPORT}
    $manifest | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $output 'manifest.json')
    if ($code -ne 0 -or -not (Test-Path -LiteralPath $env:AESTRA_EXAMPLE_REPORT)) { throw "Native candidate failed: $name ($code); inspect $name.log" }
    $r = Get-Content -LiteralPath $env:AESTRA_EXAMPLE_REPORT -Raw | ConvertFrom-Json
    if ($code -ne 0 -or -not $r.completed) { throw "Native candidate failed: $name ($code)" }
    if ($r.build.profile -ne 'release' -or $r.build.opt_level -ne '3' -or $r.build.debug_assertions) { throw 'Expected a release opt-level 3 binary; development builds cannot populate this baseline.' }
    if ($r.showcase_preset -ne 'fireworks-showcase-v1' -or -not $r.native_gpu -or $r.history -ne 'PlaybackOnly' -or
        $r.audio_enabled -or $r.review_camera -ne 'audience' -or ($r.camera_physical_size -join ',') -ne '1280,720' -or
        $null -ne $r.host_state.owner_limit -or $r.node_head_allocated_bytes -ne 47923200) { throw "Candidate configuration changed: $name" }
    if ($r.fragment_capacity_passed -ne $true -or $r.capacity.observations -le 0 -or
        $r.capacity.max_dropped -ne 0 -or $r.capacity.max_invalid_links -ne 0 -or
        $r.capacity.observed_partition_loss) { throw "Missing/failed partition health: $name" }
    if ($r.host_state.launches -ne 12 -or $r.host_state.detonations -ne 12 -or $r.flashes_accepted -ne 12 -or
        $r.host_state.current_gameplay_rockets -ne 0 -or $r.host_state.current_vfx_owners -ne 0 -or $r.last_async_alive -ne 0) { throw "Incomplete workload or cleanup: $name" }
    if ($profile -and -not ($r.profile.gpu_phases_ms.PSObject.Properties.Name -contains 'render/fireworks_render_graph/elapsed_gpu')) { throw "Missing render-graph GPU timestamps: $name" }
    Write-Output "$name passed: release, native GPU, unchanged 12-rocket workload, healthy partitions and empty final state"
}
Push-Location $repo
try {
    Run-Lane 'images' $false $true
    Run-Lane 'warmup' $false $false
    foreach ($n in 1..$Repetitions) {
        $lanes = if ($n % 2 -eq 1) { @('ordinary','profile') } else { @('profile','ordinary') }
        foreach ($lane in $lanes) { Run-Lane "$lane-$n" ($lane -eq 'profile') $false }
    }
} finally {
    foreach ($name in $variables) {
        if ($null -eq $saved[$name]) { Remove-Item -LiteralPath "Env:$name" -ErrorAction SilentlyContinue }
        else { [Environment]::SetEnvironmentVariable($name,$saved[$name],'Process') }
    }
    Pop-Location
}
