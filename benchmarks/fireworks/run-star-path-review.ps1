param(
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [string]$Binary = 'target/debug/examples/fireworks.exe',
    [string]$Effect = 'effects/fireworks_show_star_path_smoke.aestra.ron',
    [switch]$HideStarSmoke,
    [ValidateSet('high', 'medium', 'low')][string[]]$Tiers = @('high', 'medium', 'low'),
    [ValidateSet('audience-lit', 'close-lit', 'close-muted')][string[]]$Cases = @('audience-lit', 'close-lit', 'close-muted')
)
$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$output = [IO.Path]::GetFullPath($OutputDirectory, $repo)
$exe = [IO.Path]::GetFullPath($Binary, $repo)
if (Test-Path -LiteralPath $output) { throw 'Use a fresh output directory.' }
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw 'Build the fireworks example first.' }
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
    $env:AESTRA_EXAMPLE_REPORT_IMAGES = '1'
    $env:AESTRA_EXAMPLE_REPORT_SAMPLE_TIMES = '2.8,3.8,9.3,12,18.4,22,30,33'
    $env:AESTRA_EXAMPLE_REPORT_STAR_SMOKE = if ($HideStarSmoke) { '0' } else { '1' }
    $manifest = [ordered]@{ binary_sha256 = (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash; runs = @() }
    foreach ($tier in $Tiers) {
        foreach ($case in @(@{camera = 'audience'; flashes = '1'},
                @{camera = 'close'; flashes = '1'}, @{camera = 'close'; flashes = '0'})) {
            $label = if ($case.flashes -eq '1') { 'lit' } else { 'muted' }
            $name = "$tier-$($case.camera)-$label"
            if ("$($case.camera)-$label" -notin $Cases) { continue }
            $report = Join-Path $output "$name.json"
            $log = Join-Path $output "$name.log"
            $env:AESTRA_EXAMPLE_REPORT = $report
            $env:AESTRA_EXAMPLE_REPORT_CAMERA = $case.camera
            $env:AESTRA_EXAMPLE_REPORT_FLASHES = $case.flashes
            $launchArguments = @('--smoke-art-show', '--effect', $Effect,
                '--experimental-oit', '--tier', $tier, '--no-audio')
            & $exe @launchArguments *> $log
            $code = $LASTEXITCODE
            $result = Get-Content -LiteralPath $report -Raw | ConvertFrom-Json
            $manifest.runs += @{ name = $name; exit_code = $code; report = $report; arguments = $launchArguments }
            $manifest | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $output 'manifest.json')
            if ($code -ne 0 -or -not $result.completed) { throw "Native run failed: $name ($code)" }
            Write-Output "$name completed; flashes=$($result.flashes_accepted), alive=$($result.last_async_alive), drops=$($result.capacity.max_dropped)"
        }
    }
} finally {
    foreach ($name in $variables) { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') }
    Pop-Location
}
