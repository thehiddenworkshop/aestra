# Requires PowerShell 7. Native GPU tests are opt-in and run in separate processes.
[CmdletBinding()]
param(
    [string]$ReportsDirectory,
    [string]$Toolchain = '1.98.1-x86_64-pc-windows-msvc',
    [switch]$Native
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$manifest = Join-Path $PSScriptRoot 'patches-020/Cargo.toml'
$target = Join-Path $root 'target/bevy-020-qualification'
if (!$ReportsDirectory) {
    $ReportsDirectory = Join-Path $target ('runs/' + [Guid]::NewGuid().ToString('N'))
}
$reports = [IO.Path]::GetFullPath($ReportsDirectory)
if (Test-Path -LiteralPath $reports) { throw 'Use a fresh report directory; evidence is never overwritten.' }
New-Item -ItemType Directory -Path $reports -Force | Out-Null

function Hash([string]$Path) {
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}
function Inputs {
    $paths = @('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml')
    # A direct Cargo/format invocation may create a nested target directory.
    # Generated compiler metadata is not source input; retain all actual fixture files.
    $fixtureTarget = (Join-Path $PSScriptRoot 'patches-020/target') + [IO.Path]::DirectorySeparatorChar
    $paths += @(Get-ChildItem -LiteralPath (Join-Path $PSScriptRoot 'patches-020') -File -Recurse |
        Where-Object { !$_.FullName.StartsWith($fixtureTarget, [StringComparison]::OrdinalIgnoreCase) } |
        ForEach-Object { [IO.Path]::GetRelativePath($root, $_.FullName).Replace('\', '/') })
    $paths += 'benchmarks/bevy-upgrade/keyboard-020.rs', 'benchmarks/bevy-upgrade/run-020-patches.ps1'
    $paths += @(
        'bevy/aestra-bevy-render/src/gpu.rs',
        'bevy/aestra-bevy-render/src/lib.rs',
        'bevy/aestra-bevy-render/src/presented_effect.rs',
        'bevy/aestra-bevy-render/src/gpu/output_context.rs',
        'bevy/aestra-bevy-render/src/gpu/particle_output_readback.rs',
        'bevy/aestra-bevy-render/src/gpu/particle_outputs.rs',
        'bevy/aestra-bevy-render/src/capabilities.rs',
        'bevy/aestra-bevy-render/src/render_settings.rs',
        'bevy/aestra-bevy-render/src/gpu/catchup_pacing.rs',
        'bevy/aestra-bevy-render/src/gpu/capability_publication.rs',
        'bevy/aestra-bevy-render/src/gpu/extraction_systems.rs',
        'bevy/aestra-bevy-render/src/gpu/extraction_control_tests.rs',
        'bevy/aestra-bevy-render/src/gpu/render.rs',
        'bevy/aestra-bevy-render/src/gpu/draw_commands.rs',
        'bevy/aestra-bevy-render/src/gpu/draw_preparation.rs',
        'bevy/aestra-bevy-render/src/gpu/draw_resources.rs',
        'bevy/aestra-bevy-render/src/gpu/scene_depth_019.rs',
        'bevy/aestra-bevy-render/src/gpu/scene_depth_020.rs',
        'bevy/aestra-bevy-render/src/gpu/alpha_sort.rs',
        'bevy/aestra-bevy-render/src/gpu/alpha_sort_tests.rs',
        'bevy/aestra-bevy-render/src/gpu/alpha_sort.wgsl',
        'bevy/aestra-bevy-render/src/gpu/trail_compaction.rs',
        'bevy/aestra-bevy-render/src/gpu/trail_culling.rs',
        'bevy/aestra-bevy-render/src/gpu/preparation_context.rs',
        'bevy/aestra-bevy-render/src/gpu/preparation_timing.rs',
        'bevy/aestra-bevy-render/src/gpu/simulation_timing.rs',
        'bevy/aestra-bevy-render/src/gpu/simulation_pipeline.rs',
        'bevy/aestra-bevy-render/src/gpu/stateful_simulation.rs',
        'bevy/aestra-bevy-render/src/gpu/coupled_simulation.rs',
        'bevy/aestra-bevy-render/src/execution.rs',
        'bevy/aestra-bevy-render/src/gpu/paged_trails.rs',
        'bevy/aestra-bevy-render/src/gpu/timestamp_transport.rs',
        'bevy/aestra-bevy-render/src/gpu/mapped_readback_019.rs',
        'bevy/aestra-bevy-render/src/gpu/mapped_readback_020.rs',
        'bevy/aestra-bevy-render/src/gpu/geometry_statistics.rs',
        'bevy/aestra-bevy-render/src/gpu/pipeline.rs',
        'bevy/aestra-bevy-render/src/material.rs',
        'bevy/aestra-bevy-render/src/material_layout.rs',
        'bevy/aestra-bevy-render/src/gpu/view_phases.rs',
        'bevy/aestra-bevy-render/src/gpu/shader_composition.rs',
        'bevy/aestra-bevy-render/src/gpu/storage_encoding.rs',
        'bevy/aestra-bevy-render/src/gpu/storage_buffers.rs',
        'bevy/aestra-bevy-render/src/gpu/storage_buffers_020.rs',
        'bevy/aestra-bevy-render/src/gpu/draw_instance.rs',
        'bevy/aestra-bevy-render/src/gpu/extraction.rs',
        'bevy/aestra-bevy-render/src/gpu/extraction_020.rs',
        'bevy/aestra-bevy-render/src/gpu/extraction_cleanup.rs',
        'bevy/aestra-bevy-render/src/gpu/extraction_tests.rs',
        'bevy/aestra-bevy-render/src/gpu/extraction_state_tests.rs',
        'bevy/aestra-bevy-render/src/gpu/clone_extraction.rs',
        'bevy/aestra-bevy-render/src/gpu/effect_inputs.rs',
        'bevy/aestra-bevy-render/src/gpu/stage_inputs.rs',
        'bevy/aestra-bevy/src/playback.rs',
        'bevy/aestra-bevy/src/project.rs',
        'bevy/aestra-bevy/src/bindings.rs',
        'bevy/aestra-bevy/src/lib.rs',
        'bevy/aestra-bevy-render/src/gpu/stage_runtimes.rs',
        'bevy/aestra-bevy-render/src/gpu/stage_output_delivery.rs',
        'bevy/aestra-bevy-render/src/gpu/particle_light_inputs.rs',
        'bevy/aestra-bevy-render/src/gpu/particle_light_transport.rs',
        'bevy/aestra-bevy-render/src/gpu/particle_lights.rs',
        'bevy/aestra-bevy-render/src/gpu/particle_light_readback.rs',
        'bevy/aestra-bevy-render/src/gpu/trail_checkpoints.rs',
        'bevy/aestra-bevy-render/src/gpu/trail_context.rs',
        'bevy/aestra-bevy-render/src/gpu/stateful_trails.rs',
        'bevy/aestra-bevy-render/src/gpu/stateful_trails_tests.rs',
        'bevy/aestra-bevy-render/src/gpu/volume.rs',
        'bevy/aestra-bevy-render/src/gpu/mesh_inputs.rs',
        'bevy/aestra-bevy-render/src/gpu/world_sdf.rs',
        'bevy/aestra-bevy-render/src/gpu/extension_stages.rs',
        'bevy/aestra-bevy-render/src/gpu/sprite_culling.rs',
        'bevy/aestra-bevy-render/src/gpu/wireframe.rs',
        'bevy/aestra-bevy-render/src/gpu/material_lighting.wgsl',
        'bevy/aestra-bevy-render/src/gpu/volume_lighting.wgsl',
        'extensions/aestra-fluid/src/volume.wgsl',
        'extensions/aestra-fluid/src/liquid_look.wgsl',
        'apps/aestra-editor/src/feathers/shaders/node_graph_grid.wgsl',
        'apps/aestra-editor/src/feathers/shaders/node_graph_wire.wgsl',
        'assets/test/materials/fireworks_lit_smoke.aestra.material.ron'
    )
    # The shader fixture compiles real engine-neutral Aestra path dependencies too.
    foreach ($crate in @('aestra-core', 'aestra-compiler', 'aestra-gpu', 'aestra-runtime', 'aestra-extension', 'aestra-project')) {
        $directory = Join-Path $root "crates/$crate"
        $paths += "crates/$crate/Cargo.toml"
        $paths += @(Get-ChildItem -LiteralPath "$directory/src" -File -Recurse |
            ForEach-Object { [IO.Path]::GetRelativePath($root, $_.FullName).Replace('\', '/') })
    }
    $paths += 'bevy/aestra-bevy-render/src/host_transform.rs', 'bevy/aestra-bevy-render/src/gpu/trail_replay.rs', 'extensions/aestra-fluid/Cargo.toml'
    $paths += @(Get-ChildItem -LiteralPath (Join-Path $root 'extensions/aestra-fluid/src') -File -Recurse |
        ForEach-Object { [IO.Path]::GetRelativePath($root, $_.FullName).Replace('\', '/') })
    foreach ($directory in @('vendor/bevy_input_focus_020', 'vendor/bevy_pbr_020')) {
        $paths += @(Get-ChildItem -LiteralPath (Join-Path $root $directory) -File -Recurse -Force |
            ForEach-Object { [IO.Path]::GetRelativePath($root, $_.FullName).Replace('\', '/') })
    }
    $result = [ordered]@{}
    foreach ($path in ($paths | Sort-Object -Unique)) { $result[$path] = Hash (Join-Path $root $path) }
    $result
}
function RunCargo([string]$Name, [string[]]$Arguments) {
    $log = Join-Path $reports "$Name.log"
    & cargo "+$Toolchain" @Arguments 2>&1 | Tee-Object -FilePath $log | ForEach-Object {
        # Retain machine-readable evidence without flooding the console with it.
        if ($Name -in @('native-build', 'shader-native-build', 'storage-native-build', 'extraction-native-build', 'async-native-build') -and $_.ToString().StartsWith('{')) {
            $message = $_ | ConvertFrom-Json
            if ($message.reason -eq 'compiler-message') { Write-Host $message.message.rendered }
        } elseif ($Name -ne 'metadata') { $_ | Out-Host }
    }
    if ($LASTEXITCODE -ne 0) { throw "$Name failed ($LASTEXITCODE); retained $log" }
}

Push-Location $root
try {
    # Verify every untouched published file, and reject unlisted source additions.
    $allowed = @{
        bevy_input_focus_020 = @('src/lib.rs', 'src/keyboard_dispatch.rs')
        bevy_pbr_020 = @('src/cluster/gpu.rs', 'src/cluster/mod.rs')
    }
    foreach ($crate in $allowed.Keys) {
        $directory = Join-Path $root "vendor/$crate"
        $provenance = Get-Content -LiteralPath "$directory/UPSTREAM_SHA256.json" -Raw | ConvertFrom-Json -AsHashtable
        if ($provenance.version -ne '0.20.0' -or $provenance.registry_package_checksum -notmatch '^[a-f0-9]{64}$') {
            throw "Invalid published provenance: $crate"
        }
        foreach ($entry in $provenance.files.GetEnumerator()) {
            if ($entry.Key -notin $allowed[$crate] -and (Hash "$directory/$($entry.Key)") -ne $entry.Value) {
                throw "Unexpected upstream modification: $crate/$($entry.Key)"
            }
        }
        Get-ChildItem -LiteralPath $directory -File -Recurse -Force | ForEach-Object {
            $relative = [IO.Path]::GetRelativePath($directory, $_.FullName).Replace('\', '/')
            if (!$provenance.files.ContainsKey($relative) -and $relative -notin $allowed[$crate] -and
                $relative -notin @('AESTRA_PATCH.md', 'UPSTREAM_SHA256.json', '.gitattributes')) {
                throw "Unexpected added file: $crate/$relative"
            }
        }
    }
    $before = Inputs
    $before | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath "$reports/inputs-before.json" -Encoding utf8NoBOM
    RunCargo 'versions' @('--version')
    & rustc "+$Toolchain" -vV 2>&1 | Set-Content -LiteralPath "$reports/rustc.txt" -Encoding utf8NoBOM
    if ($LASTEXITCODE -ne 0) { throw 'rustc version failed' }
    RunCargo 'metadata' @('metadata', '--manifest-path', $manifest, '--locked', '--offline', '--format-version', '1', '--filter-platform', 'x86_64-pc-windows-msvc')
    $metadata = Get-Content -LiteralPath "$reports/metadata.log" -Raw | ConvertFrom-Json
    $bevyVersions = @($metadata.packages | Where-Object { $_.name -match '^bevy($|_)' -and $_.name -notin @('bevy_mikktspace') } | Select-Object -ExpandProperty version -Unique)
    if ($bevyVersions.Count -ne 1 -or $bevyVersions[0] -ne '0.20.0') { throw "Mixed Bevy graph: $bevyVersions" }
    foreach ($crate in @('bevy_input_focus', 'bevy_pbr')) {
        $package = $metadata.packages | Where-Object name -EQ $crate
        $expected = [IO.Path]::GetFullPath((Join-Path $root "vendor/$crate`_020/Cargo.toml"))
        if ($package.source -or $package.manifest_path -ne $expected) { throw "Candidate patch not selected: $crate" }
    }
    $common = @('--manifest-path', $manifest, '--locked', '--offline', '--target-dir', $target)
    RunCargo 'keyboard' (@('test') + $common + @('--test', 'keyboard', '--', '--test-threads=1'))
    RunCargo 'upstream-input-focus' (@('test') + $common + @('-p', 'bevy_input_focus', '--lib', '--', '--test-threads=1'))
    RunCargo 'cluster-units' (@('test') + $common + @('-p', 'bevy_pbr', '--lib', 'aestra_reuse_tests', '--', '--test-threads=1'))
    RunCargo 'shader-composition' (@('test') + $common + @('--test', 'shader_composition', '--', '--test-threads=1'))
    RunCargo 'storage-buffers' (@('test') + $common + @('--test', 'storage_buffers', '--', '--test-threads=1'))
    RunCargo 'extraction' (@('test') + $common + @('--test', 'extraction', '--', '--test-threads=1'))
    $binary = $null
    $shaderBinary = $null
    $storageBinary = $null
    $extractionBinary = $null
    $asyncBinary = $null
    if ($Native) {
        RunCargo 'native-build' (@('test') + $common + @('--test', 'cluster_buffer_reuse', '--no-run', '--message-format=json'))
        $artifacts = Get-Content -LiteralPath "$reports/native-build.log" | ForEach-Object {
            try { $_ | ConvertFrom-Json } catch { $null }
        }
        $binary = @($artifacts | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'cluster_buffer_reuse' -and $_.executable } | Select-Object -ExpandProperty executable -Unique)
        if ($binary.Count -ne 1) { throw 'Expected exactly one native test executable.' }
        $binary = $binary[0]
        $binaryBefore = Hash $binary
        foreach ($test in @('native_cluster_buffers_reuse_grow_reset_and_retire_per_view', 'native_private_generations_overflow_recover_and_retire')) {
            $log = Join-Path $reports "$test.log"
            & $binary $test --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
            if ($LASTEXITCODE -ne 0) { throw "$test failed ($LASTEXITCODE); retained $log" }
            if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        }
        if ((Hash $binary) -ne $binaryBefore) { throw 'Native binary changed during qualification.' }
        # Run after both cluster tests have exited: never contend for the native GPU.
        RunCargo 'shader-native-build' (@('test') + $common + @('--test', 'shader_composition', '--no-run', '--message-format=json'))
        $artifacts = Get-Content -LiteralPath "$reports/shader-native-build.log" | ForEach-Object {
            try { $_ | ConvertFrom-Json } catch { $null }
        }
        $shaderBinary = @($artifacts | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'shader_composition' -and $_.executable } | Select-Object -ExpandProperty executable -Unique)
        if ($shaderBinary.Count -ne 1) { throw 'Expected exactly one shader test executable.' }
        $shaderBinary = $shaderBinary[0]
        $shaderBinaryBefore = Hash $shaderBinary
        $log = Join-Path $reports 'native-shader-pipelines.log'
        & $shaderBinary native_020_shader_modules_and_render_pipelines_validate --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native shader test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $shaderBinary) -ne $shaderBinaryBefore) { throw 'Shader binary changed during qualification.' }
        # Shader asset upload/resize qualification is another separate native process.
        RunCargo 'storage-native-build' (@('test') + $common + @('--test', 'storage_buffers', '--no-run', '--message-format=json'))
        $artifacts = Get-Content -LiteralPath "$reports/storage-native-build.log" | ForEach-Object {
            try { $_ | ConvertFrom-Json } catch { $null }
        }
        $storageBinary = @($artifacts | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'storage_buffers' -and $_.executable } | Select-Object -ExpandProperty executable -Unique)
        if ($storageBinary.Count -ne 1) { throw 'Expected exactly one storage test executable.' }
        $storageBinary = $storageBinary[0]
        $storageBinaryBefore = Hash $storageBinary
        $log = Join-Path $reports 'native-storage-buffers.log'
        & $storageBinary native_020_storage_uploads_reuse_resize_and_invalidate_bindings --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native storage test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $storageBinary) -ne $storageBinaryBefore) { throw 'Storage binary changed during qualification.' }
        # Device publication accesses the actual main world during extraction, on hardware.
        # It runs only after all preceding native processes have exited.
        RunCargo 'extraction-native-build' (@('test') + $common + @('--test', 'extraction', '--no-run', '--message-format=json'))
        $artifacts = Get-Content -LiteralPath "$reports/extraction-native-build.log" | ForEach-Object {
            try { $_ | ConvertFrom-Json } catch { $null }
        }
        $extractionBinary = @($artifacts | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'extraction' -and $_.executable } | Select-Object -ExpandProperty executable -Unique)
        if ($extractionBinary.Count -ne 1) { throw 'Expected exactly one extraction test executable.' }
        $extractionBinary = $extractionBinary[0]
        $extractionBinaryBefore = Hash $extractionBinary
        $log = Join-Path $reports 'native-device-publication.log'
        & $extractionBinary native_020_device_capabilities_publish_through_real_extract_schedule --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native extraction test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Extraction binary changed during qualification.' }
        $log = Join-Path $reports 'native-production-pipelines.log'
        & $extractionBinary pipeline_native::native_020_production_pipeline_descriptors_validate_with_explicit_layouts --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native pipeline test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Pipeline binary changed during qualification.' }
        $log = Join-Path $reports 'native-draw-bindings.log'
        & $extractionBinary draw_native::native_020_commands_submit_with_real_mesh_view_and_prepass_bindings --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native draw command test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Draw command binary changed during qualification.' }
        $log = Join-Path $reports 'native-queue-schedule.log'
        & $extractionBinary queue_native::native_020_installed_queues_render_and_retire_through_real_schedule --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native queue schedule test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Queue schedule binary changed during qualification.' }
        $log = Join-Path $reports 'native-alpha-compute-schedule.log'
        & $extractionBinary alpha_sort_native::native_020_alpha_compute_feeds_installed_queues_and_gates_stale_indices --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native alpha compute schedule test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Alpha compute schedule binary changed during qualification.' }
        $log = Join-Path $reports 'native-alpha-sparse-pools.log'
        & $extractionBinary alpha_sort::tests::native_alpha_sort_handles_large_sparse_pools_and_opposite_views --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native alpha sparse-pool test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Alpha sparse-pool binary changed during qualification.' }
        $log = Join-Path $reports 'native-trail-compute-schedule.log'
        & $extractionBinary trail_native::native_020_trail_producers_feed_installed_queues_and_fail_open --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native trail producer test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Trail producer binary changed during qualification.' }
        $log = Join-Path $reports 'native-analytic-simulation.log'
        & $extractionBinary simulation_native::native_020_analytic_simulation_feeds_alpha_and_installed_queues --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native analytic simulation test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Analytic simulation binary changed during qualification.' }
        $log = Join-Path $reports 'native-trail-simulation.log'
        & $extractionBinary trail_simulation_native::native_020_actual_trail_history_feeds_installed_producers_and_queues --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native trail simulation test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Trail simulation binary changed during qualification.' }
        $log = Join-Path $reports 'native-stateful-simulation.log'
        & $extractionBinary stateful_native::native_020_stateful_simulation_feeds_alpha_and_installed_queues --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native stateful simulation test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Stateful simulation binary changed during qualification.' }
        $log = Join-Path $reports 'native-coupled-simulation.log'
        & $extractionBinary coupled_native::native_020_coupled_routes_feed_alpha_and_installed_queues --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native coupled simulation test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Coupled simulation binary changed during qualification.' }
        $log = Join-Path $reports 'native-fluid-joint-trails.log'
        & $extractionBinary coupled_trails_native::native_020_fluid_coupling_and_joint_trails_feed_installed_queues --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native fluid/joint trail test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Fluid/joint trail binary changed during qualification.' }
        $log = Join-Path $reports 'native-domain-births.log'
        & $extractionBinary coupled_trails_native::native_020_domain_births_feed_particles_trails_and_output_rings --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native domain birth test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Domain birth binary changed during qualification.' }
        $log = Join-Path $reports 'native-async-host-outputs.log'
        & $extractionBinary coupled_trails_native::native_020_async_particle_outputs_reach_routed_host_messages_once --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native asynchronous host output test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Asynchronous host output binary changed during qualification.' }
        $log = Join-Path $reports 'native-stage-outputs.log'
        & $extractionBinary stage_outputs_native::native_020_stage_outputs_reach_host_values_and_impact_messages --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native stage output test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Stage output binary changed during qualification.' }
        $log = Join-Path $reports 'native-host-stage-schedule.log'
        & $extractionBinary host_stage_native::native_020_host_clock_drives_stage_reset_and_suppressed_seek_outputs --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native host stage schedule test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Host stage schedule binary changed during qualification.' }
        $log = Join-Path $reports 'native-project-host-schedule.log'
        & $extractionBinary host_stage_native::native_020_nested_project_bindings_drive_independent_stage_timelines --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native project host schedule test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Project host schedule binary changed during qualification.' }
        $log = Join-Path $reports 'native-timestamp-transport.log'
        & $extractionBinary trail_native::native_020_timestamp_transport_bounds_and_recycles_in_flight_batches --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "Native timestamp test failed ($LASTEXITCODE); retained $log" }
        if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
        if ((Hash $extractionBinary) -ne $extractionBinaryBefore) { throw 'Timestamp binary changed during qualification.' }
        # Keep the local synchronous gates unchanged. Only these two explicit
        # gates enable multithreaded shader compilation / real render pipelining.
        RunCargo 'async-native-build' (@('test') + $common + @('--test', 'extraction', '--features', 'async-qualification', '--no-run', '--message-format=json'))
        $artifacts = Get-Content -LiteralPath "$reports/async-native-build.log" | ForEach-Object {
            try { $_ | ConvertFrom-Json } catch { $null }
        }
        $asyncBinary = @($artifacts | Where-Object { $_.reason -eq 'compiler-artifact' -and $_.target.name -eq 'extraction' -and $_.executable } | Select-Object -ExpandProperty executable -Unique)
        if ($asyncBinary.Count -ne 1) { throw 'Expected exactly one asynchronous test executable.' }
        $asyncBinary = $asyncBinary[0]
        $asyncBinaryBefore = Hash $asyncBinary
        foreach ($test in @(
            'stateful_native::native_020_delayed_shader_recovers_stateful_simulation_and_draws',
            'host_stage_native::native_020_pipelined_project_outputs_survive_restart_seek_and_teardown'
        )) {
            $log = Join-Path $reports (($test -replace '::', '-') + '.log')
            & $asyncBinary $test --exact --ignored --nocapture --test-threads=1 2>&1 | Tee-Object -FilePath $log | Out-Host
            if ($LASTEXITCODE -ne 0) { throw "$test failed ($LASTEXITCODE); retained $log" }
            if (Select-String -LiteralPath $log -Pattern '\bERROR\b|panicked at' -Quiet) { throw "Native errors in $log" }
            if (!(Select-String -LiteralPath $log -Pattern 'test result: ok\. 1 passed' -Quiet)) { throw "Native gate did not execute exactly one test: $log" }
            if ((Hash $asyncBinary) -ne $asyncBinaryBefore) { throw 'Asynchronous binary changed during qualification.' }
        }
    }
    $after = Inputs
    $after | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath "$reports/inputs-after.json" -Encoding utf8NoBOM
    if (($before | ConvertTo-Json -Compress) -ne ($after | ConvertTo-Json -Compress)) { throw 'Qualification inputs changed during run.' }
    [ordered]@{
        accepted = $true; bevy = $bevyVersions; toolchain = $Toolchain
        native_executed = [bool]$Native; binary = $binary
        binary_sha256 = $(if ($Native) { $binaryBefore } else { $null })
        shader_binary = $shaderBinary
        shader_binary_sha256 = $(if ($Native) { $shaderBinaryBefore } else { $null })
        storage_binary = $storageBinary
        storage_binary_sha256 = $(if ($Native) { $storageBinaryBefore } else { $null })
        extraction_binary = $extractionBinary
        extraction_binary_sha256 = $(if ($Native) { $extractionBinaryBefore } else { $null })
        async_binary = $asyncBinary
        async_binary_sha256 = $(if ($Native) { $asyncBinaryBefore } else { $null })
        source_hashes_unchanged = $true; shipping_lock_sha256 = $after['Cargo.lock']
    } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath "$reports/summary.json" -Encoding utf8NoBOM
} finally { Pop-Location }
Write-Output $reports
