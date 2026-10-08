// View-local presentation permutation only: never mutate simulation or compact alive lists.
struct Particle {
    color: vec4<f32>, position: vec3<f32>, size: f32,
    rotation: f32, normalized_age: f32, packed_emitter_alive: u32, particle_index: u32,
}
struct Globals { world_from_effect: mat4x4<f32>, time: f32, seed: u32, padding: vec2<f32> }
struct Params { view_from_world: mat4x4<f32>, range: vec4<u32> }
@group(0) @binding(0) var<storage, read> particles: array<Particle>;
@group(0) @binding(1) var<storage, read> alive: array<u32>;
@group(0) @binding(2) var<storage, read> indirect: array<u32>;
@group(0) @binding(3) var<storage, read> globals: Globals;
// alive offset, capacity, emitter index, merge run width (0 for classification).
@group(0) @binding(4) var<uniform> params: Params;
@group(0) @binding(5) var<storage, read> source: array<vec4<u32>>;
@group(0) @binding(6) var<storage, read_write> destination: array<vec4<u32>>;
@group(0) @binding(7) var<storage, read_write> indices: array<u32>;
var<workgroup> page: array<vec4<u32>, 256>;

// valid-before-padding, signed view Z ascending (far to near), then birth ordinal/slot.
fn before(a: vec4<u32>, b: vec4<u32>) -> bool {
    if a.w != b.w { return a.w < b.w; }
    if a.x != b.x { return a.x < b.x; }
    if a.y != b.y { return a.y < b.y; }
    return a.z < b.z;
}
fn depth_key(z: f32) -> u32 {
    let bits = bitcast<u32>(select(z, 0.0, z == 0.0)); // canonicalize signed zero
    return select(bits ^ 0x80000000u, ~bits, (bits & 0x80000000u) != 0u);
}
@compute @workgroup_size(256)
fn classify(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) lane: u32) {
    let i = (group.x + group.y * 65535u) * 256u + lane;
    let count = min(indirect[params.range.z * 4u + 1u], params.range.y);
    var key = vec4<u32>(0xffffffffu);
    if i < count {
        let slot = alive[params.range.x + i];
        let p = particles[slot];
        let world = globals.world_from_effect * vec4<f32>(p.position, 1.0);
        let z = (params.view_from_world * world).z;
        // Corrupt/nonfinite positions sort to the end deterministically; raster clipping still owns visibility.
        key = vec4<u32>(select(0xffffffffu, depth_key(z), abs(z) <= 3.402823e38), p.particle_index, slot, 0u);
    }
    page[lane] = key;
    workgroupBarrier();
    for (var width = 2u; width <= 256u; width *= 2u) {
        for (var gap = width / 2u; gap > 0u; gap /= 2u) {
            let other = lane ^ gap;
            let a = page[lane];
            let b = page[other];
            let ascending = (lane & width) == 0u;
            let lower = (lane & gap) == 0u;
            let take_b = select(before(a, b), before(b, a), ascending == lower);
            workgroupBarrier();
            page[lane] = select(a, b, take_b);
            workgroupBarrier();
        }
    }
    if i < arrayLength(&destination) { destination[i] = page[lane]; }
}
@compute @workgroup_size(64)
fn merge(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x + gid.y * 65535u * 64u;
    if i >= arrayLength(&source) { return; }
    let width = params.range.w;
    let run = i / width;
    let other = run ^ 1u;
    let a = source[i];
    var low = 0u;
    var high = width;
    while low < high {
        let mid = low + (high - low) / 2u;
        let b = source[other * width + mid];
        // Exact ties have left-before-right ranks, including repeated padding sentinels.
        if before(b, a) || ((run & 1u) != 0u && !before(a, b)) { low = mid + 1u; }
        else { high = mid; }
    }
    let output = (run / 2u) * (width * 2u) + (i % width) + low;
    destination[output] = a;
}
@compute @workgroup_size(64)
fn finish(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x + gid.y * 65535u * 64u;
    if i < arrayLength(&source) { indices[i] = source[i].z; }
}
