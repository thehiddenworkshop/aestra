struct TrailCullParams {
    clip_from_world: mat4x4<f32>,
    renderer_index: u32,
    instance_count: u32,
    epoch: u32,
    pixel_radius_per_clip_w: f32,
}
@group(0) @binding(0) var<storage, read> particles: array<Particle>;
@group(0) @binding(1) var<storage, read> renderers: array<Renderer>;
@group(0) @binding(2) var<uniform> cull: TrailCullParams;
@group(0) @binding(3) var<storage, read_write> draw: array<u32>;
@group(0) @binding(4) var<storage, read> globals: RenderGlobals;
@group(0) @binding(5) var<storage, read> aux: array<u32>;
@group(0) @binding(6) var<storage, read> source_draw: array<u32>;

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
    var radius = bounds.color.w * width * 0.5;
    let center = minimum * 0.5 + maximum * 0.5;
    let position_extent = maximum * 0.5 - minimum * 0.5;
    var extent = position_extent + vec3<f32>(radius);
    if !finite4(vec4<f32>(center, radius)) || !finite4(vec4<f32>(extent, width))
        || radius < 0.0 || any(position_extent < vec3<f32>(0.0)) { return false; }
    let rows = transpose(cull.clip_from_world);
    if r.blend_mode == 1u {
        let floor = r.frames[63].w;
        if !finite4(vec4<f32>(floor, 0.0, 0.0, 0.0)) { return false; }
        if floor > 0.0 {
            let scale = cull.pixel_radius_per_clip_w;
            if !finite4(vec4<f32>(scale, 0.0, 0.0, 0.0)) || scale <= 0.0 { return false; }
            // The billboard plane has constant view depth for supported cameras.
            // The smallest physical-pixel lens axis bounds every possible side
            // direction; the same sphere encloses body offsets and round caps.
            // Bound W over the whole history, not just the current head. Histories
            // crossing the eye plane fail open rather than divide by a tiny W.
            let center_w = dot(rows[3].xyz, center) + rows[3].w;
            let extent_w = dot(abs(rows[3].xyz), extent);
            let max_w = center_w + extent_w;
            if !finite4(vec4<f32>(center_w, extent_w, max_w, 0.0))
                || center_w - extent_w <= 0.00001 { return false; }
            radius = max(radius, floor * max_w * scale);
            extent = position_extent + vec3<f32>(radius);
            if !finite4(vec4<f32>(extent, radius)) { return false; }
        }
    }
    let planes = array<vec4<f32>, 6>(
        rows[3] + rows[0], rows[3] - rows[0], rows[3] + rows[1],
        rows[3] - rows[1], rows[2], rows[3] - rows[2]);
    var outside = false;
    for (var i = 0u; i < 6u; i += 1u) {
        let plane = planes[i];
        if !finite4(plane) { return false; }
        // Infinite reverse-Z projections have a constant positive far plane.
        // Degenerate planes cannot prove that geometry is outside.
        let scale = max(max(abs(plane.x), abs(plane.y)), abs(plane.z));
        if scale == 0.0 { continue; }
        // Minkowski sum of the point AABB and a width/cap sphere. Padding a
        // world-axis cube instead is needlessly loose for rotated cameras.
        // Normalize before length to avoid overflow/underflow of squared planes.
        let plane_length = scale * length(plane.xyz / scale);
        let reach = dot(plane.xyz, center) + plane.w
            + dot(abs(plane.xyz), position_extent) + radius * plane_length;
        let tolerance = 1e-5 * (dot(abs(plane.xyz), abs(center) + extent) + abs(plane.w) + 1.0);
        if !finite4(vec4<f32>(reach, tolerance, 0.0, 0.0)) { return false; }
        outside = outside || reach < -tolerance;
    }
    return outside;
}

@compute @workgroup_size(1)
fn cull_trail() {
    draw[0] = 4u;
    draw[1] = select(min(cull.instance_count, source_draw[1]), 0u, trail_outside());
    draw[2] = 0u;
    draw[3] = 0u;
}
