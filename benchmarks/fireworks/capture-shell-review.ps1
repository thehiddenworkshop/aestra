#requires -Version 7.0
# Capture evidence only: never automatically approve artistic/golden references.
[CmdletBinding()]
param(
    [ValidateSet('peony', 'chrysanthemum', 'pistil', 'willow', 'reference-hero', 'multi-break', 'crackle')]
    [ValidateNotNullOrEmpty()]
    [string[]]$Shell = @('peony', 'chrysanthemum', 'pistil', 'willow'),
    [ValidateSet('close', 'audience', 'wide')]
    [ValidateNotNullOrEmpty()]
    [string[]]$Camera = @('close', 'audience', 'wide'),
    [ValidateSet('authored', 'sampled')]
    [ValidateNotNullOrEmpty()]
    [string[]]$Response = @('authored', 'sampled'),
    [string]$OutputDirectory,
    [switch]$PlanOnly
)

$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
if (!$OutputDirectory) {
    $OutputDirectory = 'target/fireworks-f4/shell-review-' + (Get-Date -Format 'yyyyMMdd-HHmmss')
}
if (![IO.Path]::IsPathRooted($OutputDirectory)) {
    $OutputDirectory = Join-Path $repo $OutputDirectory
}
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
$cases = @(
    foreach ($shellName in ($Shell | Select-Object -Unique)) {
        $frames = if ($shellName -eq 'willow') {
            @(45, 80, 110, 150, 240, 330, 450, 540)
        } elseif ($shellName -eq 'reference-hero') {
            @(45, 80, 110, 150, 210, 270, 360, 480)
        } elseif ($shellName -eq 'multi-break') {
            @(45, 80, 110, 150, 180, 210, 270, 420)
        } elseif ($shellName -eq 'crackle') {
            @(45, 80, 110, 130, 145, 160, 210, 420)
        } else {
            @(45, 80, 110, 150, 210, 300, 390, 420)
        }
        foreach ($cameraName in ($Camera | Select-Object -Unique)) {
            foreach ($responseName in ($Response | Select-Object -Unique)) {
                $floor = if ($responseName -eq 'sampled') { 2 } else { 0 }
                $name = "$shellName-$cameraName-$responseName"
                [pscustomobject]@{
                    name = $name
                    shell = $shellName
                    camera = $cameraName
                    response = $responseName
                    frames = $frames
                    arguments = @(
                        '--fireworks-f0', '--fireworks-f0-probe', $(if ($shellName -eq 'reference-hero') { 'f4-reference-hero' } elseif ($shellName -eq 'multi-break' -or $shellName -eq 'crackle') { "f5-$shellName" } else { "f3-$shellName" }),
                        '--camera', $cameraName, '--semantic-materials',
                        '--backend', 'gpu', '--history', 'playback-only',
                        '--tier', 'high', '--seed', '0xf1e0000000000001',
                        '--stable-transparency', '--hdr', '--exposure', '0',
                        '--tonemapping', 'tony', '--bloom', '0.15',
                        '--sprite-min-pixels', "$floor", '--trail-min-pixels', "$floor",
                        '--sample-frames', ($frames -join ','),
                        '--capture', (Join-Path $OutputDirectory $name)
                    )
                }
            }
        }
    }
)
if ($PlanOnly) {
    $cases | ConvertTo-Json -Depth 5
    return
}
# Reject any existing directory, even an empty one: stale images/reports cannot pass a new run.
if (Test-Path -LiteralPath $OutputDirectory) {
    throw "Use a fresh output directory; refusing to overwrite $OutputDirectory"
}

function Assert-Capture($case, $report, $directory) {
    if ($report.status -ne 'succeeded' -or $report.runtime.active_backend -ne 'native GPU') {
        throw "$($case.name): capture failed or did not use the native GPU"
    }
    if ($report.runtime.compatibility.compatible -ne $true -or $report.errors.Count -ne 0) {
        throw "$($case.name): incompatible or erroneous capture"
    }
    if ($report.capture.seed -ne '0xf1e0000000000001' -or
        $report.capture.tick_rate -ne 60 -or $report.capture.frame_width -ne 960 -or
        $report.capture.frame_height -ne 540 -or
        (($report.capture.frames.simulation_frame -join ',') -ne ($case.frames -join ','))) {
        throw "$($case.name): capture seed, resolution or lifecycle frames differ from the plan"
    }
    $floor = if ($case.response -eq 'sampled') { 2 } else { 0 }
    $response = $report.capture.response
    if ($response.hdr -ne $true -or $response.exposure_stops -ne 0 -or
        $response.tonemapping -ne 'tony' -or $response.bloom_intensity -ne 0.15 -or
        $response.sprite_minimum_pixels -ne $floor -or $response.trail_minimum_pixels -ne $floor) {
        throw "$($case.name): photographic response differs from the plan"
    }
    foreach ($metric in @('alive_particles', 'occupied_trails', 'retired_trails',
                          'trail_evictions', 'truncated_trails')) {
        $value = $report.metrics.$metric
        if ($value.source -ne 'measured' -or $value.value -ne 0) {
            throw "$($case.name): final $metric must be measured zero, got $($value | ConvertTo-Json -Compress)"
        }
    }
    $stars = @($report.metrics.emitters | Where-Object { $_.name.EndsWith('/ Main stars') })
    $mainCount = if ($case.shell -eq 'reference-hero') { 384 } elseif ($case.shell -eq 'multi-break') { 64 } elseif ($case.shell -eq 'crackle') { 96 } else { 256 }
    if ($stars.Count -ne 1 -or $stars[0].peak_particles -ne $mainCount) {
        throw "$($case.name): missing the expected $mainCount-star main cohort"
    }
    if ($case.shell -eq 'pistil') {
        $inner = @($report.metrics.emitters | Where-Object { $_.name.EndsWith('/ Pistil stars') })
        if ($inner.Count -ne 1 -or $inner[0].peak_particles -ne 96) {
            throw "$($case.name): missing the expected 96-star inner cohort"
        }
    }
    if ($case.shell -eq 'reference-hero') {
        foreach ($expected in @(@('Gold inner stars', 96), @('Free cooling embers', 128), @('Burst smoke', 48))) {
            $cohort = @($report.metrics.emitters | Where-Object { $_.name.EndsWith('/ ' + $expected[0]) })
            if ($cohort.Count -ne 1 -or $cohort[0].peak_particles -ne $expected[1]) {
                throw "$($case.name): missing the expected $($expected[0]) cohort"
            }
        }
    }
    if ($case.shell -eq 'multi-break') {
        $secondary = @($report.metrics.emitters | Where-Object { $_.name.EndsWith('/ Secondary sparks') })
        $smoke = @($report.metrics.emitters | Where-Object { $_.name.EndsWith('/ Burst smoke') })
        # Staggered parent deaths and spark lifetimes need not give a peak of
        # 512. Total admission is checked separately in uninterrupted playback.
        if ($secondary.Count -ne 1 -or $secondary[0].peak_particles -le 0 -or
            $secondary[0].peak_particles -gt 512 -or $smoke.Count -ne 1 -or $smoke[0].peak_particles -ne 48) {
            throw "$($case.name): missing or over-budget secondary/smoke cohort"
        }
    }
    if ($case.shell -eq 'crackle') {
        foreach ($expected in @(@('Burning carriers', 96), @('Crackle sparks', 1152), @('Burst smoke', 48))) {
            $cohort = @($report.metrics.emitters | Where-Object { $_.name.EndsWith('/ ' + $expected[0]) })
            # Brief staggered flashes need not all be live at once; total admission
            # is verified by the uninterrupted event-link report, not seeking stills.
            if ($cohort.Count -ne 1 -or $cohort[0].peak_particles -le 0 -or
                $cohort[0].peak_particles -gt $expected[1]) {
                throw "$($case.name): missing or over-budget $($expected[0]) cohort"
            }
        }
    }
    foreach ($file in @('contact-sheet.png', 'capture-manifest.md') + @($report.capture.frames.image)) {
        if (!(Test-Path -LiteralPath (Join-Path $directory $file) -PathType Leaf)) {
            throw "$($case.name): missing $file"
        }
    }
}

$manifest = $null
$manifestPath = $null
Push-Location $repo
try {
    & cargo build --locked -p aestra-viewer
    if ($LASTEXITCODE -ne 0) { throw 'Viewer build failed' }
    $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
    $binary = if ($IsWindows) { 'debug/aestra-viewer.exe' } else { 'debug/aestra-viewer' }
    $executable = Join-Path $target $binary
    if (!(Test-Path -LiteralPath $executable)) { throw "Missing viewer: $executable" }
    New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
    $revision = (& git rev-parse HEAD)
    $dirty = @(& git status --porcelain)
    $sourcePaths = @('apps/aestra-viewer/src/main.rs', 'apps/aestra-viewer/src/fireworks_f3.rs',
                     'apps/aestra-viewer/src/fireworks_f0.rs', 'apps/aestra-viewer/src/fireworks_hero.rs',
                     'apps/aestra-viewer/src/fireworks_f5.rs') +
        @(Get-ChildItem assets/test/effects/fireworks_*.aestra.ron,
                       assets/test/materials/fireworks_*.aestra.material.ron | ForEach-Object FullName)
    $sourceHashes = @($sourcePaths | ForEach-Object {
        $sourcePath = if ([IO.Path]::IsPathRooted($_)) { $_ } else { Join-Path $repo $_ }
        [pscustomobject]@{ path = [IO.Path]::GetRelativePath($repo, $sourcePath);
                          sha256 = (Get-FileHash -LiteralPath $sourcePath).Hash }
    })
    $manifest = [ordered]@{
        schema_version = 1
        status = 'capturing'
        artistic_acceptance = 'pending human/reference-footage review'
        timing_certification = 'not performed; exact-frame captures are not live benchmarks'
        git_revision = $revision
        dirty_paths = $dirty
        source_hashes = $sourceHashes
        plan = $cases
        captures = @()
    }
    $manifestPath = Join-Path $OutputDirectory 'review-manifest.json'
    $manifest | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $manifestPath -Encoding utf8
    foreach ($case in $cases) {
        Write-Host "Capturing $($case.name)"
        & $executable @($case.arguments)
        if ($LASTEXITCODE -ne 0) { throw "$($case.name): viewer exited with code $LASTEXITCODE" }
        $directory = Join-Path $OutputDirectory $case.name
        $report = Get-Content -LiteralPath (Join-Path $directory 'preview-report.json') -Raw | ConvertFrom-Json
        Assert-Capture $case $report $directory
        $manifest.captures += [pscustomobject]@{
            name = $case.name
            status = 'technical checks passed; visual review pending'
            adapter = $report.runtime.adapter
            effect = $report.effect
            endpoint_metrics = $report.metrics
            images = @($report.capture.frames | ForEach-Object {
                [pscustomobject]@{ simulation_frame = $_.simulation_frame;
                                  image = "$($case.name)/$($_.image)";
                                  sha256 = (Get-FileHash -LiteralPath (Join-Path $directory $_.image)).Hash }
            })
        }
        $manifest | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $manifestPath -Encoding utf8
    }
    $manifest.status = 'captured; technical checks passed'
    $manifest | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $manifestPath -Encoding utf8
    Write-Host "Review evidence: $manifestPath"
} catch {
    if ($manifest -and $manifestPath) {
        $manifest.status = 'failed'
        $manifest.error = $_.Exception.Message
        $manifest | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $manifestPath -Encoding utf8
    }
    throw
} finally {
    Pop-Location
}
