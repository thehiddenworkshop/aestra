@group(0) @binding(0) var<storage, read> particles: array<Particle>;
@group(0) @binding(1) var<storage, read> renderers: array<Renderer>;
@group(0) @binding(2) var<storage, read> globals: RenderGlobals;
@group(0) @binding(3) var<storage, read> aux: array<u32>;
// Renderer, candidate count, owner count, candidates per owner.
@group(0) @binding(4) var<uniform> compact: vec4<u32>;
@group(0) @binding(5) var<storage, read_write> scratch: array<u32>;
// Four indirect words followed by the stable candidate-index list.
@group(0) @binding(6) var<storage, read_write> compacted: array<u32>;

@compute @workgroup_size(64)
fn classify_trail(@builtin(global_invocation_id) id: vec3<u32>) {
    let owner = id.x;
    if owner >= compact.z { return; }
    var count = 0u;
    for (var primitive = 0u; primitive < compact.w; primitive += 1u) {
        let candidate = owner * compact.w + primitive;
        let valid = trail_primitive_visible(candidate, compact.x);
        scratch[candidate] = select(0xffffffffu, count, valid);
        count += select(0u, 1u, valid);
    }
    scratch[compact.y + owner] = count;
}

var<workgroup> trail_owner_scan: array<u32, 1024>;

@compute @workgroup_size(64)
fn prefix_trail(@builtin(local_invocation_index) thread: u32) {
    // Parallel exclusive scan preserves the existing alpha draw order exactly.
    var size = 1u;
    while size < compact.z { size *= 2u; }
    for (var owner = thread; owner < size; owner += 64u) {
        var count = 0u;
        if owner < compact.z { count = scratch[compact.y + owner]; }
        trail_owner_scan[owner] = count;
    }
    workgroupBarrier();
    for (var width = 2u; width <= size; width *= 2u) {
        for (var n = (thread + 1u) * width - 1u; n < size; n += 64u * width) {
            trail_owner_scan[n] += trail_owner_scan[n - width / 2u];
        }
        workgroupBarrier();
    }
    if thread == 0u {
        compacted[0] = 4u;
        compacted[1] = trail_owner_scan[size - 1u];
        compacted[2] = 0u;
        compacted[3] = 0u;
        trail_owner_scan[size - 1u] = 0u;
    }
    workgroupBarrier();
    for (var width = size; width >= 2u; width /= 2u) {
        for (var n = (thread + 1u) * width - 1u; n < size; n += 64u * width) {
            let left = trail_owner_scan[n - width / 2u];
            trail_owner_scan[n - width / 2u] = trail_owner_scan[n];
            trail_owner_scan[n] += left;
        }
        workgroupBarrier();
    }
    for (var owner = thread; owner < compact.z; owner += 64u) {
        scratch[compact.y + owner] = trail_owner_scan[owner];
    }
}

@compute @workgroup_size(64)
fn scatter_trail(@builtin(global_invocation_id) id: vec3<u32>) {
    let candidate = id.x;
    if candidate >= compact.y { return; }
    let local = scratch[candidate];
    if local == 0xffffffffu { return; }
    let offset = scratch[compact.y + candidate / compact.w];
    compacted[4u + offset + local] = candidate;
}
