// Shared by compaction and vertex presentation: never reject extra geometry here.
fn trail_slot(base: u32, capacity: u32, index: u32) -> u32 {
    let count = aux[base * 3u + 1u];
    if index == count { return base; }
    return base + 1u + (aux[base * 3u] + capacity - count + index) % capacity;
}

fn trail_segment_delta(base: u32, capacity: u32, count: u32, start: u32, end: u32, cap: bool, head_cap: bool) -> vec3<f32> {
    var delta = particles[end].position - particles[start].position;
    if cap && dot(delta, delta) < 1e-12 {
        for (var step = 1u; step <= count; step += 1u) {
            let other = trail_slot(base, capacity, select(step, count - step, head_cap));
            delta = select(particles[other].position - particles[start].position,
                particles[end].position - particles[other].position, head_cap);
            if dot(delta, delta) >= 1e-12 { break; }
        }
    }
    return delta;
}

fn trail_primitive_visible(instance_index: u32, renderer_index: u32) -> bool {
    let r = renderers[renderer_index];
    let capacity = r.frame_count - 1u;
    let stride = capacity + select(0u, 16u, (r.flipbook_flags & 2u) != 0u);
    let base = r.attribute_flags.z + 1u + (instance_index / stride) * r.frame_count;
    let primitive = instance_index % stride;
    let count = aux[base * 3u + 1u];
    if particle_alive(particles[base]) == 0u || count == 0u { return false; }
    let cap = primitive >= capacity;
    let head_cap = primitive >= capacity + 8u;
    var segment = primitive;
    if cap { segment = select(0u, count - 1u, head_cap); }
    if segment >= count { return false; }
    let start = trail_slot(base, capacity, segment);
    let end = trail_slot(base, capacity, segment + 1u);
    let delta = trail_segment_delta(base, capacity, count, start, end, cap, head_cap);
    // Keep the rejection comparisons identical to vertex presentation, including NaN behavior.
    return !(dot(delta, delta) < 1e-12 || globals.time - particles[end].rotation >= r.frame_rate);
}
