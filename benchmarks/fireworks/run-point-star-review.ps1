param(
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [string]$Binary = 'target/debug/fireworks.exe',
    [ValidateSet('point_star', 'gold_tail', 'shell_smoke')][string]$Family = 'point_star',
    [string]$SampleTimes = '0.8,1.3,1.5,1.8,2.3,2.8,3.3,3.8,4.3,5,7,12,17',
    [switch]$Cost,
    [switch]$HideStarSmoke,
    [switch]$MuteFlashes,
    [ValidateSet('control', 'candidate')][string[]]$Variants = @('control', 'candidate'),
    [ValidateSet('high', 'medium', 'low')][string[]]$Tiers = @('high'),
    [ValidateSet('audience', 'close')][string[]]$Cameras = @('audience', 'close')
)
$ErrorActionPreference = 'Stop'
if ($Cost -and ($HideStarSmoke -or $MuteFlashes)) { throw 'Visibility controls require the image lane.' }
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$output = [IO.Path]::GetFullPath($OutputDirectory, $repo)
$exe = [IO.Path]::GetFullPath($Binary, $repo)
if (Test-Path -LiteralPath $output) { throw 'Use a fresh output directory.' }
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw 'Build the fireworks demo first.' }
New-Item -ItemType Directory -Path $output | Out-Null
$variables = @('AESTRA_EXAMPLE_REPORT', 'AESTRA_EXAMPLE_REPORT_IMAGES',
    'AESTRA_EXAMPLE_REPORT_CAMERA', 'AESTRA_EXAMPLE_REPORT_SAMPLE_TIMES',
    'AESTRA_EXAMPLE_REPORT_STAR_SMOKE', 'AESTRA_EXAMPLE_REPORT_FLASHES',
    'AESTRA_EXAMPLE_PROFILE', 'AESTRA_EXAMPLE_CAPTURE')
$saved = @{}
foreach ($name in $variables) { $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
Push-Location $repo
try {
    Remove-Item Env:AESTRA_EXAMPLE_PROFILE, Env:AESTRA_EXAMPLE_CAPTURE -ErrorAction SilentlyContinue
    $env:AESTRA_EXAMPLE_REPORT_IMAGES = if ($Cost) { '0' } else { '1' }
    if ($Cost) {
        Remove-Item Env:AESTRA_EXAMPLE_REPORT_SAMPLE_TIMES -ErrorAction SilentlyContinue
    } else {
        $env:AESTRA_EXAMPLE_REPORT_SAMPLE_TIMES = $SampleTimes
    }
    $env:AESTRA_EXAMPLE_REPORT_STAR_SMOKE = if ($HideStarSmoke) { '0' } else { '1' }
    $env:AESTRA_EXAMPLE_REPORT_FLASHES = if ($MuteFlashes) { '0' } else { '1' }
    $manifest = [ordered]@{ binary_sha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash; lane = if ($Cost) { 'ordinary' } else { 'images' }; hide_star_smoke = [bool]$HideStarSmoke; mute_flashes = [bool]$MuteFlashes; assets = @(); runs = @() }
    foreach ($variant in $Variants) {
        $effect = "effects/fireworks_$($Family)_$variant.aestra.ron"
        $manifest.assets += @{ path = "assets/test/$effect"; sha256 = (Get-FileHash -LiteralPath "assets/test/$effect" -Algorithm SHA256).Hash }
        foreach ($tier in $Tiers) {
            foreach ($camera in $Cameras) {
                $name = "$variant-$tier-$camera"
                $report = Join-Path $output "$name.json"
                $env:AESTRA_EXAMPLE_REPORT = $report
                $env:AESTRA_EXAMPLE_REPORT_CAMERA = $camera
                $launchArguments = @('--smoke-art-show', '--effect', $effect,
                    '--experimental-oit', '--tier', $tier, '--no-audio')
                & $exe @launchArguments *> (Join-Path $output "$name.log")
                $code = $LASTEXITCODE
                $result = Get-Content -LiteralPath $report -Raw | ConvertFrom-Json
                $manifest.runs += @{ name = $name; exit_code = $code; report = $report; arguments = $launchArguments }
                $manifest | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $output 'manifest.json')
                if ($code -ne 0 -or -not $result.completed) { throw "Native run failed: $name ($code)" }
                if ($Cost) {
                    Write-Output "$name completed; ordinary p95=$($result.host_intervals_including_vsync.p95_ms) ms, alive=$($result.last_async_alive)"
                } else {
                    Write-Output "$name completed; flashes=$($result.flashes_accepted), alive=$($result.last_async_alive), drops=$($result.capacity.max_dropped)"
                }
            }
        }
    }
} finally {
    foreach ($name in $variables) { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') }
    Pop-Location
}
