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

@compute @workgroup_size(1)
fn prefix_trail() {
    // At most 1024 owners; prefixing owner totals preserves alpha draw order.
    var total = 0u;
    for (var owner = 0u; owner < compact.z; owner += 1u) {
        let count = scratch[compact.y + owner];
        scratch[compact.y + owner] = total;
        total += count;
    }
    compacted[0] = 4u;
    compacted[1] = total;
    compacted[2] = 0u;
    compacted[3] = 0u;
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
