// Experimental Bevy 0.19 bridge: no private Rust-field offsets are assumed.
// Import the exact Bevy shader record consumed by ordinary StandardMaterial.
#import bevy_pbr::mesh_view_types::ClusteredLight

struct SelectedLight {
    position_range: vec4<f32>,
    color_intensity: vec4<f32>,
    radius: f32,
    priority: u32,
    source_token: u32,
    particle_index: u32,
};
struct Params { destination: vec4<u32>, limits: vec4<f32> };
@group(0) @binding(0) var<storage, read> selected: array<SelectedLight>;
@group(0) @binding(1) var<storage, read> counts: vec4<u32>;
@group(0) @binding(2) var<storage, read_write> lights: array<ClusteredLight>;
@group(0) @binding(3) var<uniform> params: Params;

@compute @workgroup_size(1)
fn inject() {
    let slot = params.destination.x;
    if slot >= arrayLength(&lights) { return; }
    // Bevy's main-world placeholder is zero-lumen each frame. Explicitly clear
    // the GPU override too; zero candidates must never keep yesterday's light.
    lights[slot].color_inverse_square_range = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    lights[slot].position_radius = vec4<f32>(0.0);
    lights[slot].range = 0.0;
    lights[slot].flags = 0u;
    lights[slot].soft_shadow_size = 0.0;
    lights[slot].decal_index = 0xffffffffu;
    if counts.z == 0u || counts.z > params.destination.y || arrayLength(&selected) == 0u { return; }
    let record = selected[0];
    if record.source_token != 0u || record.color_intensity.w <= 0.0 || record.position_range.w <= 0.0 { return; }
    let range = min(record.position_range.w, params.limits.y);
    // Bevy PointLight intensity is authored lumens, its GPU record is candela.
    let candela = min(record.color_intensity.w, params.limits.x) / 12.566370614359172;
    lights[slot].color_inverse_square_range = vec4<f32>(record.color_intensity.rgb * candela, 1.0 / (range * range));
    lights[slot].position_radius = vec4<f32>(record.position_range.xyz, min(record.radius, range));
    lights[slot].range = range;
}
