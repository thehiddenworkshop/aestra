// Persistent tail of the particle buffer: one emitter header followed by bounded
// owner chunks. A chunk is a live head plus a ring of fixed-cadence samples.
// History positions/sizes are world-space. rotation stores the observation time;
// head padding stores next ring index, count, and last sample time (f32 bits).
// Owners use stable particle_index, never compacted/physical particle slots.
fn record_trails(emitter_index: u32) {
    let e = emitters[emitter_index];
    if e.trail_points < 2u { return; }
    let root = e.trail_offset;
    let now = globals.time;
    let last = particles[root];
    let reset = last.alive == 0u || last._padding_0 != globals._padding.x || last.particle_index != globals.seed || now < last.rotation || now - last.rotation > e.trail_lifetime;
    if !reset && now == last.rotation { return; }
    let capacity = e.trail_points - 1u;
    var evictions = select(last._padding_1, 0u, reset);
    for (var i = 0u; i < e.trail_capacity; i += 1u) {
        let base = root + 1u + i * e.trail_points;
        if reset || now - particles[base].rotation >= e.trail_lifetime {
            particles[base].alive = 0u;
        }
    }
    let count = min(atomicLoad(&indirect[emitter_index * 4u + 1u]), e.max_particles);
    // Reserve every surviving owner before allocating, so a newly born particle
    // cannot evict a living parent's history. Sorted IDs make eviction deterministic.
    for (var n = 0u; n < count; n += 1u) {
        let id = particles[alive_indices[e.slot_offset + n]].particle_index;
        for (var i = 0u; i < e.trail_capacity; i += 1u) {
            let base = root + 1u + i * e.trail_points;
            if particles[base].alive != 0u && particles[base].particle_index == id { particles[base].alive = 2u; break; }
        }
    }
    let world_scale = max(length(globals.world_from_effect[0].xyz), max(length(globals.world_from_effect[1].xyz), length(globals.world_from_effect[2].xyz)));
    for (var n = 0u; n < count; n += 1u) {
        var head = particles[alive_indices[e.slot_offset + n]];
        var owner = 0xffffffffu;
        var candidate = 0xffffffffu;
        var oldest = 3.402823e38;
        for (var i = 0u; i < e.trail_capacity; i += 1u) {
            let base = root + 1u + i * e.trail_points;
            let previous = particles[base];
            if previous.alive != 0u && previous.particle_index == head.particle_index { owner = base; break; }
            if previous.alive == 0u {
                if oldest > -3.402823e38 { candidate = base; oldest = -3.402823e38; }
            } else if previous.alive != 2u && previous.rotation < oldest {
                candidate = base; oldest = previous.rotation;
            }
        }
        head.position = (globals.world_from_effect * vec4<f32>(head.position, 1.0)).xyz;
        head.size *= world_scale;
        head.rotation = now;
        head.alive = 2u;
        if owner == 0xffffffffu {
            if candidate == 0xffffffffu { continue; }
            owner = candidate;
            if particles[owner].alive != 0u { evictions += 1u; }
            head._padding_0 = 1u % capacity;
            head._padding_1 = 1u;
            head._padding_2 = bitcast<u32>(now);
            particles[owner + 1u] = head;
        } else {
            let previous = particles[owner];
            head._padding_0 = previous._padding_0;
            head._padding_1 = previous._padding_1;
            var tick = bitcast<f32>(previous._padding_2);
            // Skip samples that would immediately fall out of the bounded ring.
            let steps = u32(max(0.0, floor((now - tick) / e.trail_interval)));
            if steps > capacity { tick += f32(steps - capacity) * e.trail_interval; }
            for (var s = 0u; s < min(steps, capacity); s += 1u) {
                tick += e.trail_interval;
                let t = clamp((tick - previous.rotation) / max(now - previous.rotation, 1e-6), 0.0, 1.0);
                var sample = head;
                sample.position = mix(previous.position, head.position, t);
                sample.color = mix(previous.color, head.color, t);
                sample.size = mix(previous.size, head.size, t);
                sample.rotation = tick;
                particles[owner + 1u + head._padding_0] = sample;
                head._padding_0 = (head._padding_0 + 1u) % capacity;
                head._padding_1 = min(head._padding_1 + 1u, capacity);
            }
            head._padding_2 = bitcast<u32>(tick);
        }
        particles[owner] = head;
    }
    var occupied = 0u;
    var retired = 0u;
    for (var i = 0u; i < e.trail_capacity; i += 1u) {
        let base = root + 1u + i * e.trail_points;
        if particles[base].alive != 0u { occupied += 1u; }
        if particles[base].alive == 1u { retired += 1u; }
        if particles[base].alive == 2u { particles[base].alive = 1u; }
    }
    particles[root].alive = 1u;
    particles[root].rotation = now;
    particles[root].particle_index = globals.seed;
    particles[root]._padding_0 = globals._padding.x;
    particles[root]._padding_1 = evictions;
    let peak = max(occupied, select(last._padding_2, 0u, reset));
    particles[root]._padding_2 = peak;
    // Small asynchronous profiler readback; never copy the history buffer.
    let stats = 2u + emitter_index * 5u;
    atomicStore(&counters[stats], occupied);
    atomicStore(&counters[stats + 1u], retired);
    atomicStore(&counters[stats + 2u], evictions);
    atomicStore(&counters[stats + 3u], peak);
    atomicStore(&counters[stats + 4u], globals._padding.x);
}

@compute @workgroup_size(64)
fn update_trails(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x < arrayLength(&emitters) { record_trails(id.x); }
}
