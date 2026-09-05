# Generates high-scale dense stress scenarios (100% occupancy) for the AAA-scale
# GPU sweep. Burst = capacity, lifetime = duration = 120s so every particle stays
# alive across the whole 720-frame capture. Written UTF-8 no BOM (RON parser rejects BOM).
$ErrorActionPreference = 'Stop'
$outDir = 'C:\Users\flore\Documents\GitHub\TheHiddenWorkshop\aestra\benchmarks\sweep-scenarios'
$enc = New-Object System.Text.UTF8Encoding($false)

# name, capacity, id-prefix (8 hex)
$scales = @(
    @('scale_0050k', 50000,   'e0000050'),
    @('scale_0100k', 100000,  'e0000100'),
    @('scale_0250k', 250000,  'e0000250'),
    @('scale_0500k', 500000,  'e0000500'),
    @('scale_1000k', 1000000, 'e0001000'),
    @('scale_2000k', 2000000, 'e0002000'),
    @('scale_4000k', 4000000, 'e0004000')
)

foreach ($s in $scales) {
    $name = $s[0]; $cap = $s[1]; $p = $s[2]
    $ron = @"
// $name - AAA-scale dense stress. One emitter, $cap capacity, 100% occupancy.
// Burst = capacity with lifetime = duration = 120s, so all $cap particles stay
// alive across the whole capture. Isolates how the analytical kernel's O(capacity)
// per-frame reconstruction (and the transparent render pass) scale toward the
// ~4.19M single-dispatch ceiling. GENERATED - see scratchpad/gen_scale.ps1.
(
    format_version: 3,
    id: "$p-0000-4000-8000-000000000000",
    name: "Scale $cap",
    duration: 120.0,
    playback_mode: Once,
    materials: [
        (
            id: "a3574a00-0000-4000-8000-000000000101",
            name: "Bench Sprite",
            blend: Additive,
            properties: Sprite(softness: Constant(0.5), color: ParticleColor),
        ),
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
                (id: "$p-0000-4000-8000-000000000014", module_type: "aestra.update.appearance", stage: ParticleUpdate, enabled: true, parameters: Appearance(size: (id: "$p-0000-4000-8000-0000000000c1", keys: [(time: 0.0, value: 5.0), (time: 1.0, value: 1.0)]), opacity: (id: "$p-0000-4000-8000-0000000000c2", keys: [(time: 0.0, value: 1.0), (time: 1.0, value: 0.0)]), color: (id: "$p-0000-4000-8000-0000000000c3", keys: [(time: 0.0, color: (1.0, 1.0, 1.0, 1.0)), (time: 1.0, color: (0.3, 0.1, 0.8, 0.0))]))),
            ],
            renderers: [
                (id: "$p-0000-4000-8000-0000000000a0", renderer_type: "aestra.renderer.sprite", enabled: true, material: "a3574a00-0000-4000-8000-000000000101", properties: Sprite),
            ],
        ),
    ],
)
"@
    $path = Join-Path $outDir "$name.ron"
    [System.IO.File]::WriteAllText($path, $ron, $enc)
    Write-Output "wrote $path ($cap particles)"
}
