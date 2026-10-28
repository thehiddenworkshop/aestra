param(
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [string]$Binary = 'target/debug/fireworks.exe',
    [ValidateSet('host-overlap','host-spaced','pair')][string]$Mode = 'host-overlap',
    [ValidateSet('audience','close','wide','side','near','orbit')][string]$Camera = 'close',
    [ValidateSet('control','tuned')][string[]]$Variants = @('control','tuned'),
    [ValidateSet('high','medium','low')][string[]]$Tiers = @('high'),
    [ValidateSet('default','ordinary','shared')][string]$Transparency = 'default',
    [switch]$MatchMsaa,
    [Nullable[double]]$FragmentsPerPixel = $null,
    [switch]$RequireLossFree,
    [string]$SampleTimes = '1.8,2.8,4,5,6.9,8,12,16,23,36',
    [switch]$Cost,
    [Alias('Profile')][switch]$Instrumented,
    [switch]$HideStarSmoke,
    [switch]$NoStarLights,
    [switch]$NoFlashLights
)
$ErrorActionPreference = 'Stop'
if ($Instrumented -and -not $Cost) { throw '-Instrumented requires -Cost (no screenshots or capacity scans).' }
if ($Cost -and $HideStarSmoke) { throw 'Smoke visibility controls require the image lane.' }
if ($Cost -and $Camera -eq 'orbit') { throw 'Orbit review requires the image lane; ordinary costs keep the audience view.' }
if ($null -ne $FragmentsPerPixel -and -not ($Transparency -eq 'shared' -or ($Transparency -eq 'default' -and $Mode -eq 'pair'))) { throw '-FragmentsPerPixel requires shared transparency.' }
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$output = [IO.Path]::GetFullPath($OutputDirectory,$repo)
$exe = [IO.Path]::GetFullPath($Binary,$repo)
if (Test-Path -LiteralPath $output) { throw 'Use a fresh output directory.' }
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw 'Build the fireworks demo first.' }
New-Item -ItemType Directory -Path $output | Out-Null
$variables = @('AESTRA_EXAMPLE_REPORT','AESTRA_EXAMPLE_REPORT_IMAGES','AESTRA_EXAMPLE_REPORT_CAMERA',
    'AESTRA_EXAMPLE_REPORT_SAMPLE_TIMES','AESTRA_EXAMPLE_REPORT_STAR_SMOKE','AESTRA_EXAMPLE_REPORT_FLASHES',
    'AESTRA_EXAMPLE_PROFILE','AESTRA_EXAMPLE_CAPTURE','AESTRA_EXAMPLE_REPORT_MSAA')
$saved = @{}
foreach ($name in $variables) { $saved[$name] = [Environment]::GetEnvironmentVariable($name,'Process') }
Push-Location $repo
try {
    Remove-Item Env:AESTRA_EXAMPLE_PROFILE,Env:AESTRA_EXAMPLE_CAPTURE -ErrorAction SilentlyContinue
    if ($Instrumented) { $env:AESTRA_EXAMPLE_PROFILE = '1' }
    $env:AESTRA_EXAMPLE_REPORT_IMAGES = if ($Cost) { '0' } else { '1' }
    $env:AESTRA_EXAMPLE_REPORT_CAMERA = if ($Cost -and -not $PSBoundParameters.ContainsKey('Camera')) { 'audience' } else { $Camera }
    if ($MatchMsaa) { $env:AESTRA_EXAMPLE_REPORT_MSAA = 'off' }
    else { Remove-Item Env:AESTRA_EXAMPLE_REPORT_MSAA -ErrorAction SilentlyContinue }
    $env:AESTRA_EXAMPLE_REPORT_STAR_SMOKE = if ($HideStarSmoke) { '0' } else { '1' }
    $env:AESTRA_EXAMPLE_REPORT_FLASHES = '1'
    if ($Cost) { Remove-Item Env:AESTRA_EXAMPLE_REPORT_SAMPLE_TIMES -ErrorAction SilentlyContinue }
    else { $env:AESTRA_EXAMPLE_REPORT_SAMPLE_TIMES = if ($Mode -eq 'pair') { ($SampleTimes -split ',' | Where-Object { [double]$_ -le 24 }) -join ',' } else { $SampleTimes } }
    $manifest = [ordered]@{ binary_sha256=(Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash;
        mode=$Mode; camera=$env:AESTRA_EXAMPLE_REPORT_CAMERA; transparency=$Transparency; match_msaa=[bool]$MatchMsaa; lane=if ($Instrumented) { 'instrumented' } elseif ($Cost) { 'ordinary' } else { 'images' }; no_flash_lights=[bool]$NoFlashLights; no_star_lights=[bool]$NoStarLights; hide_star_smoke=[bool]$HideStarSmoke; inputs=@(); runs=@() }
    $inputs = @(Get-ChildItem demos/fireworks/src -Recurse -File | ForEach-Object { [IO.Path]::GetRelativePath($repo,$_.FullName) }) +
        @(Get-ChildItem assets/test/materials -File | ForEach-Object { [IO.Path]::GetRelativePath($repo,$_.FullName) }) +
        @(Get-ChildItem assets/test/effects -Filter '*smoke.aestra.ron' -File | ForEach-Object { [IO.Path]::GetRelativePath($repo,$_.FullName) }) +
        @('assets/test/effects/fireworks_shell_star_lights.aestra.ron',
          'bevy/aestra-bevy/src/playback.rs','bevy/aestra-bevy/src/presentation_cache.rs',
          'bevy/aestra-bevy/src/host_plugin.rs','bevy/aestra-bevy/src/lib.rs',
          'bevy/aestra-bevy-render/src/presented_effect.rs','bevy/aestra-bevy-render/src/material.rs',
          'bevy/aestra-bevy-render/src/gpu/shader_composition.rs','bevy/aestra-bevy-render/src/experimental_oit.rs',
          'bevy/aestra-bevy-render/src/gpu/alpha_sort.rs','bevy/aestra-bevy-render/src/gpu/alpha_sort.wgsl',
          'bevy/aestra-bevy-render/src/gpu/view_phases.rs','bevy/aestra-bevy-render/src/gpu/render.rs',
          'bevy/aestra-bevy-render/src/experimental_oit/sort.rs','bevy/aestra-bevy-render/src/experimental_oit/sort.wesl',
          'bevy/aestra-bevy-render/src/experimental_oit/resolve.wesl')
    foreach ($path in $inputs) { $manifest.inputs += @{path=$path; sha256=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash} }
    foreach ($tier in $Tiers) {
        foreach ($variant in $Variants) {
            $name = "$variant-$tier"
            $report = Join-Path $output "$name.json"
            $env:AESTRA_EXAMPLE_REPORT = $report
            $launchArguments = @('--tier',$tier,'--no-audio')
            if (-not $NoStarLights) { $launchArguments += '--star-lights' }
            if ($NoFlashLights) { $launchArguments += '--no-flash-lights' }
            if ($Mode -ne 'pair') {
                $pattern = if ($Mode -eq 'host-spaced') { 'spaced' } else { 'overlap' }
                $launchArguments += @('--launch-pattern',$pattern,'--max-owners','24')
                if ($variant -eq 'tuned') { $launchArguments += '--tuned-smoke' }
            } else {
                $effect = if ($variant -eq 'tuned') { 'effects/fireworks_shell_tuned_smoke.aestra.ron' } else { 'effects/fireworks_shell_star_lights.aestra.ron' }
                $launchArguments += @('--smoke-art-show','--effect',$effect)
            }
            if ($Transparency -eq 'shared' -or ($Transparency -eq 'default' -and $Mode -eq 'pair')) {
                $launchArguments += '--experimental-oit'
                if ($null -ne $FragmentsPerPixel) { $launchArguments += @('--oit-fragments-per-pixel', ([double]$FragmentsPerPixel).ToString([Globalization.CultureInfo]::InvariantCulture)) }
            }
            & $exe @launchArguments *> (Join-Path $output "$name.log")
            $code = $LASTEXITCODE
            $result = Get-Content -LiteralPath $report -Raw | ConvertFrom-Json
            $lossFree = if ($result.experimental_oit -and $result.capacity) { [bool]$result.capacity.loss_free } else { $null }
            $manifest.runs += @{name=$name; exit_code=$code; arguments=$launchArguments; report=$report; fragment_capacity_passed=$lossFree}
            $manifest | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $output 'manifest.json')
            if ($code -ne 0 -or -not $result.completed) { throw "Native run failed: $name ($code)" }
            if ($RequireLossFree -and $result.experimental_oit -and $lossFree -ne $true) { throw "Shared transparency qualification failed: $name (fragment loss or no capacity scan); raw report retained." }
            if ($lossFree -eq $false) { Write-Output "CAPACITY FAILED: $name dropped up to $($result.capacity.max_dropped) fragments in an observed frame; completion is not acceptance." }
            Write-Output "$Mode/$name completed; host launches=$($result.host_state.launches), flashes=$($result.flashes_accepted), final alive=$($result.last_async_alive)"
        }
    }
} finally {
    foreach ($name in $variables) { [Environment]::SetEnvironmentVariable($name,$saved[$name],'Process') }
    Pop-Location
}
