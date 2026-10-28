param(
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [string]$Binary = 'target/debug/fireworks.exe',
    [ValidateSet('neither', 'flashes', 'stars', 'combined')][string[]]$Modes = @('neither', 'flashes', 'stars', 'combined'),
    [ValidateSet('high', 'medium', 'low')][string[]]$Tiers = @('high'),
    [ValidateSet('audience', 'close')][string]$Camera = 'close',
    [string]$SampleTimes = '1.8,2.8,4,5,6.7,6.9,7.4,8,9,12,16,23',
    [switch]$Cost,
    [switch]$HideStarSmoke
)
$ErrorActionPreference = 'Stop'
if ($Cost -and $HideStarSmoke) { throw 'Smoke visibility controls require the image lane.' }
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$output = [IO.Path]::GetFullPath($OutputDirectory, $repo)
$exe = [IO.Path]::GetFullPath($Binary, $repo)
if (Test-Path -LiteralPath $output) { throw 'Use a fresh output directory.' }
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw 'Build the fireworks demo first.' }
New-Item -ItemType Directory -Path $output | Out-Null
$variables = @('AESTRA_EXAMPLE_REPORT', 'AESTRA_EXAMPLE_REPORT_IMAGES', 'AESTRA_EXAMPLE_REPORT_CAMERA',
    'AESTRA_EXAMPLE_REPORT_SAMPLE_TIMES', 'AESTRA_EXAMPLE_REPORT_STAR_SMOKE', 'AESTRA_EXAMPLE_REPORT_FLASHES',
    'AESTRA_EXAMPLE_PROFILE', 'AESTRA_EXAMPLE_CAPTURE')
$saved = @{}
foreach ($name in $variables) { $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
Push-Location $repo
try {
    Remove-Item Env:AESTRA_EXAMPLE_PROFILE, Env:AESTRA_EXAMPLE_CAPTURE -ErrorAction SilentlyContinue
    $env:AESTRA_EXAMPLE_REPORT_IMAGES = if ($Cost) { '0' } else { '1' }
    $env:AESTRA_EXAMPLE_REPORT_CAMERA = $Camera
    $env:AESTRA_EXAMPLE_REPORT_STAR_SMOKE = if ($HideStarSmoke) { '0' } else { '1' }
    $env:AESTRA_EXAMPLE_REPORT_FLASHES = '1'
    if ($Cost) { Remove-Item Env:AESTRA_EXAMPLE_REPORT_SAMPLE_TIMES -ErrorAction SilentlyContinue }
    else { $env:AESTRA_EXAMPLE_REPORT_SAMPLE_TIMES = $SampleTimes }
    $manifest = [ordered]@{ binary_sha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash;
        lane = if ($Cost) { 'ordinary' } else { 'images' }; hide_star_smoke = [bool]$HideStarSmoke; inputs = @(); runs = @() }
    $inputs = @('assets/test/effects/fireworks_shell_star_lights.aestra.ron',
        'assets/test/effects/fireworks_point_star_lit_smoke.aestra.ron',
        'assets/test/effects/fireworks_gold_tail_lit_smoke.aestra.ron') +
        @(Get-ChildItem demos/fireworks/src -Recurse -File | ForEach-Object { [IO.Path]::GetRelativePath($repo, $_.FullName) }) +
        @(Get-ChildItem assets/test/materials -Filter 'fireworks*.ron' -File | ForEach-Object { [IO.Path]::GetRelativePath($repo, $_.FullName) })
    foreach ($path in $inputs) { $manifest.inputs += @{ path = $path; sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash } }
    foreach ($tier in $Tiers) {
        foreach ($mode in $Modes) {
            $name = "$mode-$tier-$Camera"
            $report = Join-Path $output "$name.json"
            $env:AESTRA_EXAMPLE_REPORT = $report
            $launchArguments = @('--smoke-art-show', '--effect', 'effects/fireworks_shell_star_lights.aestra.ron',
                '--experimental-oit', '--tier', $tier, '--no-audio')
            if ($mode -in @('stars', 'combined')) { $launchArguments += '--star-lights' }
            if ($mode -in @('neither', 'stars')) { $launchArguments += '--no-flash-lights' }
            & $exe @launchArguments *> (Join-Path $output "$name.log")
            $code = $LASTEXITCODE
            $result = Get-Content -LiteralPath $report -Raw | ConvertFrom-Json
            $manifest.runs += @{ name = $name; exit_code = $code; arguments = $launchArguments; report = $report }
            $manifest | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $output 'manifest.json')
            if ($code -ne 0 -or -not $result.completed) { throw "Native run failed: $name ($code)" }
            Write-Output "$name completed; flashes=$($result.flashes_accepted), selected capacity bound=$($result.selected_lighting.peak_capacity_bound), final alive=$($result.last_async_alive)"
        }
    }
} finally {
    foreach ($name in $variables) { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') }
    Pop-Location
}
