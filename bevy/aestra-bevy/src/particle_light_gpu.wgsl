// Use Bevy's own clustered-light ABI, never private Rust offsets.
#import bevy_pbr::mesh_view_types::ClusteredLight
struct SelectedLight {
    position_range: vec4<f32>, color_intensity: vec4<f32>,
    radius: f32, priority: u32, source_token: u32, particle_index: u32,
};
struct Params { bounds: vec4<u32>, clamps: vec4<f32> };
@group(0) @binding(0) var<storage, read> selected: array<SelectedLight>;
@group(0) @binding(1) var<storage, read> counts: vec4<u32>;
@group(0) @binding(2) var<storage, read_write> lights: array<ClusteredLight>;
@group(0) @binding(3) var<storage, read> destinations: array<vec4<u32>>;
@group(0) @binding(4) var<storage, read> authorized: array<vec4<u32>>;
@group(0) @binding(5) var<uniform> params: Params;

@compute @workgroup_size(64)
fn inject(@builtin(global_invocation_id) invocation: vec3<u32>) {
    let ordinal = invocation.x;
    if ordinal >= params.bounds.x || ordinal >= arrayLength(&destinations) { return; }
    let slot = destinations[ordinal].x;
    if slot >= arrayLength(&lights) { return; }
    lights[slot].color_inverse_square_range = vec4<f32>(0.0,0.0,0.0,1.0);
    lights[slot].position_radius = vec4<f32>(0.0);
    lights[slot].range = 0.0;
    lights[slot].flags = 0u;
    lights[slot].soft_shadow_size = 0.0;
    lights[slot].decal_index = 0xffffffffu;
    if counts.z > params.bounds.y || ordinal >= counts.z || ordinal >= arrayLength(&selected) { return; }
    let record = selected[ordinal];
    if record.source_token >= params.bounds.z || record.source_token >= arrayLength(&authorized) { return; }
    if authorized[record.source_token].x == 0u { return; }
    // Canonical selection already validates candidates; fail closed here too.
    // Comparisons reject NaNs, and bounds reject infinities/unsafe magnitudes.
    if !all(abs(record.position_range.xyz) <= vec3<f32>(3.402823e38))
        || !all(record.color_intensity.rgb >= vec3<f32>(0.0))
        || !all(record.color_intensity.rgb <= vec3<f32>(3.402823e38))
        || !any(record.color_intensity.rgb > vec3<f32>(0.0))
        || !(record.color_intensity.w > 0.0 && record.color_intensity.w <= 3.402823e38)
        || !(record.position_range.w > 0.0 && record.position_range.w <= 3.402823e38)
        || !(record.radius >= 0.0 && record.radius <= 3.402823e38) { return; }
    let range = min(record.position_range.w, params.clamps.y);
    let candela = min(record.color_intensity.w, params.clamps.x)/12.566370614359172;
    let color = record.color_intensity.rgb*candela;
    let inverse_square_range = 1.0/(range*range);
    if !all(color <= vec3<f32>(3.402823e38)) || !(inverse_square_range <= 3.402823e38) { return; }
    lights[slot].color_inverse_square_range = vec4<f32>(color,inverse_square_range);
    lights[slot].position_radius = vec4<f32>(record.position_range.xyz,min(record.radius,range));
    lights[slot].range = range;
}
