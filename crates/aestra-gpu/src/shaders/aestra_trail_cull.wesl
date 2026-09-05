struct TrailCullParams {
    clip_from_world: mat4x4<f32>,
    renderer_index: u32,
    instance_count: u32,
    epoch: u32,
    _padding: u32,
}
@group(0) @binding(0) var<storage, read> particles: array<Particle>;
@group(0) @binding(1) var<storage, read> renderers: array<Renderer>;
@group(0) @binding(2) var<uniform> cull: TrailCullParams;
@group(0) @binding(3) var<storage, read_write> draw: array<u32>;
@group(0) @binding(4) var<storage, read> globals: RenderGlobals;
@group(0) @binding(5) var<storage, read> aux: array<u32>;

fn finite4(value: vec4<f32>) -> bool {
    return all((bitcast<vec4<u32>>(value) & vec4<u32>(0x7f800000u)) != vec4<u32>(0x7f800000u));
}

fn trail_outside() -> bool {
    let r = renderers[cull.renderer_index];
    let root = r.attribute_flags.z;
    let bounds = particles[root];
    // Never trust uninitialized, stale, or asynchronous bounds. Render globals
    // carry the processed replay time, which can precede the requested seek time.
    if particle_alive(bounds) == 0u || bounds.rotation != globals.time
        || bounds.particle_index != globals.seed || aux[root * 3u] != cull.epoch {
        return false;
    }
    if bounds.size == 2.0 { return true; }
    if bounds.size != 1.0 { return false; }
    let width = abs(bitcast<f32>(r.attribute_flags.y));
    let minimum = bounds.position;
    let maximum = bounds.color.xyz;
    // Full unfaded width is conservative for both body and circular caps.
    let radius = bounds.color.w * width * 0.5;
    let center = minimum * 0.5 + maximum * 0.5;
    let extent = maximum * 0.5 - minimum * 0.5 + vec3<f32>(radius);
    if !finite4(vec4<f32>(center, radius)) || !finite4(vec4<f32>(extent, width))
        || any(extent < vec3<f32>(0.0)) { return false; }
    let rows = transpose(cull.clip_from_world);
    let planes = array<vec4<f32>, 6>(
        rows[3] + rows[0], rows[3] - rows[0], rows[3] + rows[1],
        rows[3] - rows[1], rows[2], rows[3] - rows[2]);
    var outside = false;
    for (var i = 0u; i < 6u; i += 1u) {
        let plane = planes[i];
        if !finite4(plane) { return false; }
        // Infinite reverse-Z projections have a constant positive far plane.
        // Degenerate planes cannot prove that geometry is outside.
        if all(plane.xyz == vec3<f32>(0.0)) { continue; }
        let reach = dot(plane.xyz, center) + plane.w + dot(abs(plane.xyz), extent);
        let tolerance = 1e-5 * (dot(abs(plane.xyz), abs(center) + extent) + abs(plane.w) + 1.0);
        if !finite4(vec4<f32>(reach, tolerance, 0.0, 0.0)) { return false; }
        outside = outside || reach < -tolerance;
    }
    return outside;
}

@compute @workgroup_size(1)
fn cull_trail() {
    draw[0] = 4u;
    draw[1] = select(cull.instance_count, 0u, trail_outside());
    draw[2] = 0u;
    draw[3] = 0u;
}
