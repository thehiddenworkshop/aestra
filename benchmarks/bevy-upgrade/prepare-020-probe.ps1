#requires -Version 7.0
<#
Creates a dependency-resolution-only copy of every workspace manifest under target.
Target source files are placeholders: NEVER use this workspace as build/test evidence.
The real manifests, lockfile, vendored patches and Rust sources are not modified.
#>
[CmdletBinding()]
param([switch]$Resolve, [switch]$Offline)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '../..')).Path
$metadataText = & cargo metadata --manifest-path "$repoRoot/Cargo.toml" --locked --offline --no-deps --format-version 1
if ($LASTEXITCODE -ne 0) { throw 'Could not inspect the current workspace' }
$metadata = $metadataText | ConvertFrom-Json
$packages = @($metadata.packages | Where-Object { $_.id -in $metadata.workspace_members })
$inputs = @("$repoRoot/Cargo.toml", "$repoRoot/Cargo.lock", "$repoRoot/rust-toolchain.toml",
    "$PSScriptRoot/prepare-020-probe.ps1", "$PSScriptRoot/keyboard-020.rs") +
    @($packages | ForEach-Object { $_.manifest_path })
$inputs = @($inputs | Sort-Object -Unique)
$hashesBefore = @($inputs | ForEach-Object { (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash })

# Fresh directories only; no cleanup, recursive deletion or changes to registry sources.
$probeRoot = Join-Path $repoRoot ("target/bevy-0.20-preflight/" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $probeRoot | Out-Null

function WriteGenerated([string]$RelativePath, [string]$Contents) {
    $destination = [IO.Path]::GetFullPath((Join-Path $probeRoot $RelativePath))
    if (!$destination.StartsWith($probeRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Generated path escapes the probe: $RelativePath"
    }
    [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($destination)) | Out-Null
    [IO.File]::WriteAllText($destination, $Contents, [Text.UTF8Encoding]::new($false))
}

function ReplaceExpected([string]$Contents, [string]$Before, [string]$After) {
    if (!$Contents.Contains($Before)) { throw "Baseline changed; review the probe transformation: $Before" }
    return $Contents.Replace($Before, $After)
}

$rootManifest = [IO.File]::ReadAllText("$repoRoot/Cargo.toml")
$rootManifest = ReplaceExpected $rootManifest 'bevy = { version = "0.19.1"' 'bevy = { version = "=0.20.0"'
$rootManifest = ReplaceExpected $rootManifest ', "shader_format_wesl"' ''
$rootManifest = ReplaceExpected $rootManifest 'glam = { version = "0.32.1"' 'glam = { version = "0.33.2"'
# The portable compiler is already migrated. Assert the reviewed baseline; do not
# silently re-resolve a different compiler while probing the remaining engine graph.
if (!$rootManifest.Contains('naga = { version = "=30.0.1"') -or
    !$rootManifest.Contains('wesl = "=0.6.0"')) { throw 'Portable compiler baseline changed; review the probe' }
$rootManifest = ReplaceExpected $rootManifest 'wgpu = "29.0.4"' 'wgpu = "30.0.0"'
if ($rootManifest -notmatch '(?m)^\[patch\.crates-io\]') { throw 'Expected Bevy patch table is missing' }
# This is an unpatched-upstream experiment, not a decision to remove the real patches.
$rootManifest = [regex]::Replace($rootManifest, '(?ms)^\[patch\.crates-io\]\r?\n.*?(?=^\[|\z)', '')
WriteGenerated 'Cargo.toml' $rootManifest
WriteGenerated 'rust-toolchain.toml' ([IO.File]::ReadAllText("$repoRoot/rust-toolchain.toml"))

foreach ($package in $packages) {
    $relativeManifest = [IO.Path]::GetRelativePath($repoRoot, $package.manifest_path)
    $contents = [IO.File]::ReadAllText($package.manifest_path)
    if ($package.name -eq 'aestra-editor') {
        $contents = ReplaceExpected $contents 'bevy_winit = "0.19.1"' 'bevy_winit = "=0.20.0"'
    }
    WriteGenerated $relativeManifest $contents
    foreach ($target in $package.targets) {
        $relativeSource = [IO.Path]::GetRelativePath($repoRoot, $target.src_path)
        WriteGenerated $relativeSource "// Dependency resolution placeholder. Not an implementation or build fixture.`n"
    }
}

$hashesAfter = @($inputs | ForEach-Object { (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash })
if (($hashesBefore -join ',') -ne ($hashesAfter -join ',')) { throw 'Source inputs changed during probe preparation' }
$record = [ordered]@{
    schema = 1
    purpose = 'Full workspace dependency resolution only; source targets are placeholders'
    baseline_revision = (& git -C $repoRoot rev-parse HEAD)
    created_utc = [DateTime]::UtcNow.ToString('o')
    workspace_members = @($packages.name | Sort-Object)
    source_inputs = @(
        for ($i = 0; $i -lt $inputs.Count; $i++) {
            [ordered]@{ path = [IO.Path]::GetRelativePath($repoRoot, $inputs[$i]); sha256 = $hashesBefore[$i].ToLowerInvariant() }
        }
    )
    source_inputs_unchanged = $true
    retained_ecosystem_dependencies = @('resvg 0.47 (engine-neutral)', 'tiny-skia 0.12 (engine-neutral)')
    suspended_integrations = @('aestra-bevy-avian', 'aestra-bevy-rapier')
    unpatched_bevy = '0.20.0'
}
WriteGenerated 'probe-inputs.json' ($record | ConvertTo-Json -Depth 8)
WriteGenerated 'README.txt' "DEPENDENCY RESOLUTION ONLY. All Rust targets are placeholders. Do not run cargo build/test here or claim source compatibility. See probe-inputs.json for provenance.`n"
# A separate, real headless regression fixture. It uses published 0.20 only and
# deliberately checks the required behavior; an upstream failure is audit evidence.
$keyboardSource = (Join-Path $PSScriptRoot 'keyboard-020.rs').Replace('\', '/')
WriteGenerated 'keyboard-dispatch/Cargo.toml' @"
[workspace]
[package]
name = "aestra-bevy-020-keyboard-audit"
version = "0.0.0"
edition = "2024"
publish = false
[lib]
path = "$keyboardSource"
[dependencies]
bevy_app = "=0.20.0"
bevy_ecs = "=0.20.0"
bevy_input = "=0.20.0"
bevy_input_focus = "=0.20.0"
bevy_window = "=0.20.0"
"@
Write-Host "Prepared $($packages.Count) workspace members; original manifests and lockfile unchanged."
if ($Resolve) {
    $arguments = @('metadata', '--manifest-path', "$probeRoot/Cargo.toml", '--format-version', '1',
        '--all-features', '--filter-platform', 'x86_64-pc-windows-msvc')
    if ($Offline) { $arguments += '--offline' }
    $resolvedText = & cargo @arguments 2> "$probeRoot/resolve.log"
    $resolveExit = $LASTEXITCODE
    if ($resolveExit -ne 0) {
        Write-Output $probeRoot
        throw "Dependency resolution failed ($resolveExit); see resolve.log"
    }
    WriteGenerated 'metadata.json' ($resolvedText -join "`n")
    $resolved = $resolvedText | ConvertFrom-Json
    $criticalNames = @('bevy', 'bevy_app', 'bevy_ecs', 'bevy_render', 'bevy_input_focus', 'bevy_pbr',
        'bevy_winit', 'wgpu', 'naga', 'wesl', 'encase', 'glam', 'winit',
        'resvg', 'usvg', 'tiny-skia', 'bevy_resvg', 'avian3d', 'bevy_rapier3d',
        'bevy_transform_interpolation', 'bevy_heavy')
    $inventory = @($resolved.packages | Where-Object { $_.name -in $criticalNames } |
        Sort-Object name, version | Select-Object name, version)
    $bevyVersions = @($inventory | Where-Object name -EQ 'bevy' | ForEach-Object { $_.version })
    $incompatible = @(
        foreach ($name in @('bevy', 'bevy_app', 'bevy_ecs', 'bevy_render', 'bevy_input_focus',
                'bevy_pbr', 'bevy_winit', 'wgpu')) {
            $versions = @($inventory | Where-Object name -EQ $name | ForEach-Object { $_.version })
            $expectedPrefix = if ($name -eq 'wgpu') { '30.' } else { '0.20.' }
            if ($versions.Count -ne 1 -or !$versions[0].StartsWith($expectedPrefix)) {
                [ordered]@{ name = $name; versions = $versions; expected = $expectedPrefix + '*' }
            }
        }
    )
    $singleEngine = $incompatible.Count -eq 0
    $summary = [ordered]@{
        schema = 1
        resolution_succeeded = $true
        compatible_single_engine_graph = $singleEngine
        source_compatibility_tested = $false
        feature_scope = 'All workspace features, resolution only; Windows MSVC platform filter'
        incompatible_critical_packages = $incompatible
        package_inventory = $inventory
        metadata_sha256 = (Get-FileHash -LiteralPath "$probeRoot/metadata.json" -Algorithm SHA256).Hash.ToLowerInvariant()
        lockfile_sha256 = (Get-FileHash -LiteralPath "$probeRoot/Cargo.lock" -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    WriteGenerated 'resolution-summary.json' ($summary | ConvertTo-Json -Depth 8)
    if (!$singleEngine) { Write-Warning "Resolved but NOT migration-ready: Bevy versions $($bevyVersions -join ', ')" }
    $hashesAfterResolve = @($inputs | ForEach-Object { (Get-FileHash -LiteralPath $_ -Algorithm SHA256).Hash })
    if (($hashesBefore -join ',') -ne ($hashesAfterResolve -join ',')) { throw 'Source inputs changed during resolution' }
}
Write-Output $probeRoot
