// Emitter header lanes not used by history: position=min, color.xyz=max,
// color.w=max absolute sample size, size=0 unknown / 1 valid / 2 empty.
// Positions and sizes were transformed when recorded, not by the current emitter.
fn trail_finite(value: f32) -> bool {
    return (bitcast<u32>(value) & 0x7f800000u) != 0x7f800000u;
}

fn record_trail_bounds(emitter_index: u32) {
    let e = emitters[emitter_index];
    let root = e.trail_offset;
    let capacity = e.trail_points - 1u;
    var minimum = vec3<f32>(3.402823e38);
    var maximum = vec3<f32>(-3.402823e38);
    var size = 0.0;
    var found = false;
    var valid = true;
    for (var owner = 0u; owner < e.trail_capacity; owner += 1u) {
        let base = root + 1u + owner * e.trail_points;
        if particle_alive(particles[base]) == 0u { continue; }
        let count = aux[base * 3u + 1u];
        if count == 0u { continue; }
        for (var i = 0u; i <= count; i += 1u) {
            var slot = base;
            if i < count {
                slot = base + 1u + (aux[base * 3u] + capacity - count + i) % capacity;
            }
            let p = particles[slot];
            valid = valid && trail_finite(p.position.x) && trail_finite(p.position.y)
                && trail_finite(p.position.z) && trail_finite(p.size);
            minimum = min(minimum, p.position);
            maximum = max(maximum, p.position);
            size = max(size, abs(p.size));
            found = true;
        }
    }
    // Retain expired anchors too: a partially visible segment may still use one.
    // The whole owner disappears only after its head lifetime expires.
    particles[root].position = minimum;
    particles[root].color = vec4<f32>(maximum, size);
    particles[root].size = select(0.0, select(2.0, 1.0, found), valid);
}
