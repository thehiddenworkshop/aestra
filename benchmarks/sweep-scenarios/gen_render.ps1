# Render/overdraw ablation: fixed 1M particles, vary only sprite SIZE. Fragment
# count scales with sprite area while vertex count (6/particle) stays fixed, so
# correlating render_ms vs fragments isolates fill/overdraw from vertex/setup cost.
# Constant size curve (two equal keys). UTF-8 no BOM.
$ErrorActionPreference = 'Stop'
$outDir = 'C:\Users\flore\Documents\GitHub\TheHiddenWorkshop\aestra\benchmarks\sweep-scenarios'
$enc = New-Object System.Text.UTF8Encoding($false)
$cap = 1000000

# name, size, id-prefix
$variants = @(
    @('render_1m_tiny',  0.3,  'f0000030'),
    @('render_1m_med',   3.0,  'f0000300'),
    @('render_1m_huge', 12.0,  'f0001200')
)
foreach ($v in $variants) {
    $name = $v[0]; $size = $v[1]; $p = $v[2]
    $ron = @"
// $name - render/overdraw ablation. 1,000,000 particles, constant sprite size $size.
// Fill knob for isolating overdraw from vertex/setup in the transparent pass.
// GENERATED - see scratchpad/gen_render.ps1.
(
    format_version: 3,
    id: "$p-0000-4000-8000-000000000000",
    name: "Render 1M size $size",
    duration: 120.0,
    playback_mode: Once,
    materials: [
        (id: "a3574a00-0000-4000-8000-000000000101", name: "Bench Sprite", blend: Additive, properties: Sprite(softness: Constant(0.5), color: ParticleColor)),
    ],
    emitters: [
        (
            id: "$p-0000-4000-8000-000000000001",
            name: "Bench Emitter",
            enabled: true,
            transform: (translation: (0.0, 0.0, 0.0), rotation: (0.0, 0.0, 0.0, 1.0), scale: (1.0, 1.0, 1.0)),
            start_time: 0.0,
            duration: 120.0,
            max_particles: $cap,
            simulation_domain: Particle,
            modules: [
                (id: "$p-0000-4000-8000-000000000010", module_type: "aestra.emission.rate", stage: EmitterUpdate, enabled: true, parameters: Emission(spawn_rate: 0.0, burst_count: $cap)),
                (id: "$p-0000-4000-8000-000000000011", module_type: "aestra.spawn.shape", stage: ParticleSpawn, enabled: true, parameters: Shape(shape: Sphere(radius: 40.0))),
                (id: "$p-0000-4000-8000-000000000012", module_type: "aestra.spawn.initialize", stage: ParticleSpawn, enabled: true, parameters: Initialize(lifetime: (min: 120.0, max: 120.0), speed: (min: 10.0, max: 30.0), direction: (0.0, 1.0, 0.0), spread_degrees: 360.0, angular_velocity: (min: -1.0, max: 1.0))),
                (id: "$p-0000-4000-8000-000000000013", module_type: "aestra.update.motion", stage: ParticleUpdate, enabled: true, parameters: Motion(gravity: (0.0, 0.0, 0.0), drag: 0.5, turbulence: 3.0)),
                (id: "$p-0000-4000-8000-000000000014", module_type: "aestra.update.appearance", stage: ParticleUpdate, enabled: true, parameters: Appearance(size: (id: "$p-0000-4000-8000-0000000000c1", keys: [(time: 0.0, value: $size), (time: 1.0, value: $size)]), opacity: (id: "$p-0000-4000-8000-0000000000c2", keys: [(time: 0.0, value: 1.0), (time: 1.0, value: 0.0)]), color: (id: "$p-0000-4000-8000-0000000000c3", keys: [(time: 0.0, color: (1.0, 1.0, 1.0, 1.0)), (time: 1.0, color: (0.3, 0.1, 0.8, 0.0))]))),
            ],
            renderers: [
                (id: "$p-0000-4000-8000-0000000000a0", renderer_type: "aestra.renderer.sprite", enabled: true, material: "a3574a00-0000-4000-8000-000000000101", properties: Sprite),
            ],
        ),
    ],
)
"@
    [System.IO.File]::WriteAllText((Join-Path $outDir "$name.ron"), $ron, $enc)
    Write-Output "wrote $name.ron (size $size)"
}
