// Persistent tail of the particle buffer: one emitter header followed by bounded
// owner chunks. A chunk is a live head plus a ring of time- or distance-spaced samples.
// History positions/sizes are world-space. rotation stores the observation time;
// head padding stores next ring index, count, and last sample time (Time) or
// residual world-space path length (Distance), encoded as f32 bits.
// Owners use stable particle_index, never compacted/physical particle slots.
fn record_trails(emitter_index: u32) {
    let e = emitters[emitter_index];
    if e.trail_points < 2u { return; }
    let root = e.trail_offset;
    let now = globals.time;
    let last = particles[root];
    let reset = particle_alive(last) == 0u || aux[root * 3u] != globals._padding.x || last.particle_index != globals.seed || now < last.rotation || now - last.rotation > e.trail_lifetime;
    if !reset && now == last.rotation { return; }
    let capacity = e.trail_points - 1u;
    var evictions = select(aux[root * 3u + 1u], 0u, reset);
    for (var i = 0u; i < e.trail_capacity; i += 1u) {
        let base = root + 1u + i * e.trail_points;
        if reset || now - particles[base].rotation >= e.trail_lifetime {
            particles[base].packed_emitter_alive = set_alive(particles[base].packed_emitter_alive, 0u);
        }
    }
    let count = min(atomicLoad(&indirect[emitter_index * 4u + 1u]), e.max_particles);
    // Reserve every surviving owner before allocating, so a newly born particle
    // cannot evict a living parent's history. Sorted IDs make eviction deterministic.
    for (var n = 0u; n < count; n += 1u) {
        let id = particles[alive_indices[e.slot_offset + n]].particle_index;
        for (var i = 0u; i < e.trail_capacity; i += 1u) {
            let base = root + 1u + i * e.trail_points;
            if particle_alive(particles[base]) != 0u && particles[base].particle_index == id { particles[base].packed_emitter_alive = set_alive(particles[base].packed_emitter_alive, 2u); break; }
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
            if particle_alive(previous) != 0u && previous.particle_index == head.particle_index { owner = base; break; }
            if particle_alive(previous) == 0u {
                if oldest > -3.402823e38 { candidate = base; oldest = -3.402823e38; }
            } else if particle_alive(previous) != 2u && previous.rotation < oldest {
                candidate = base; oldest = previous.rotation;
            }
        }
        head.position = (globals.world_from_effect * vec4<f32>(head.position, 1.0)).xyz;
        head.size *= world_scale;
        head.rotation = now;
        head.packed_emitter_alive = set_alive(head.packed_emitter_alive, 2u);
        // Ring state (next index, sample count, tick/remainder bits) now lives in the
        // shared aux buffer at aux[owner*3 + 0/1/2], off the particle ABI.
        var ring_head = 0u;
        var ring_count = 0u;
        var ring_tick = 0u;
        // Sample aux word 0 stores accumulated world path distance. Word 1 of
        // the first physical sample stores the owner's current head distance;
        // it is independent of ring rotation. No live particle fields are reused.
        var path_distance = 0.0;
        // First physical sample's spare aux word remembers the newest point
        // discarded by capacity. Warn only while that missing point is unexpired.
        var discarded_time = -3.402823e38;
        if owner != 0xffffffffu {
            discarded_time = bitcast<f32>(aux[(owner + 1u) * 3u + 2u]);
        }
        if owner == 0xffffffffu {
            if candidate == 0xffffffffu { continue; }
            owner = candidate;
            if particle_alive(particles[owner]) != 0u { evictions += 1u; }
            ring_head = 1u % capacity;
            ring_count = 1u;
            ring_tick = bitcast<u32>(select(now, 0.0, e.trail_sampling == 1u));
            particles[owner + 1u] = head;
            aux[(owner + 1u) * 3u] = bitcast<u32>(0.0);
        } else if e.trail_sampling == 1u {
            let previous = particles[owner];
            ring_head = aux[owner * 3u];
            ring_count = aux[owner * 3u + 1u];
            ring_tick = aux[owner * 3u + 2u];
            let segment = length(head.position - previous.position);
            let previous_distance = bitcast<f32>(aux[(owner + 1u) * 3u + 1u]);
            path_distance = previous_distance + segment;
            let total = bitcast<f32>(ring_tick) + segment;
            // Sample the observed world-space polyline. Carry the sub-spacing
            // remainder across frames/corners; stationary parents consume no points.
            if segment > 0.0 && total <= min(3.402823e38, 3.402823e38 * e.trail_distance) {
                let steps = floor(total / e.trail_distance);
                let retained = u32(min(steps, f32(capacity)));
                let remainder = clamp(total - steps * e.trail_distance, 0.0, e.trail_distance);
                if steps > f32(capacity) {
                    let along = segment - remainder - f32(capacity) * e.trail_distance;
                    discarded_time = max(discarded_time, mix(previous.rotation, now, clamp(along / segment, 0.0, 1.0)));
                }
                // Skip points that would immediately fall out of the ring. Work
                // stays bounded even when a fast parent travels many sample distances.
                for (var s = 0u; s < retained; s += 1u) {
                    let along = segment - remainder - f32(retained - 1u - s) * e.trail_distance;
                    let t = clamp(along / segment, 0.0, 1.0);
                    var sample = head;
                    sample.position = mix(previous.position, head.position, t);
                    sample.color = mix(previous.color, head.color, t);
                    sample.size = mix(previous.size, head.size, t);
                    sample.rotation = mix(previous.rotation, now, t);
                    if ring_count == capacity {
                        discarded_time = max(discarded_time, particles[owner + 1u + ring_head].rotation);
                    }
                    particles[owner + 1u + ring_head] = sample;
                    aux[(owner + 1u + ring_head) * 3u] = bitcast<u32>(previous_distance + along);
                    ring_head = (ring_head + 1u) % capacity;
                    ring_count = min(ring_count + 1u, capacity);
                }
                ring_tick = bitcast<u32>(remainder);
            }
        } else {
            let previous = particles[owner];
            let previous_distance = bitcast<f32>(aux[(owner + 1u) * 3u + 1u]);
            path_distance = previous_distance + length(head.position - previous.position);
            ring_head = aux[owner * 3u];
            ring_count = aux[owner * 3u + 1u];
            var tick = bitcast<f32>(aux[owner * 3u + 2u]);
            // Skip samples that would immediately fall out of the bounded ring.
            let steps = u32(max(0.0, floor((now - tick) / e.trail_interval)));
            if steps > capacity {
                tick += f32(steps - capacity) * e.trail_interval;
                discarded_time = max(discarded_time, tick);
            }
            for (var s = 0u; s < min(steps, capacity); s += 1u) {
                tick += e.trail_interval;
                let t = clamp((tick - previous.rotation) / max(now - previous.rotation, 1e-6), 0.0, 1.0);
                var sample = head;
                sample.position = mix(previous.position, head.position, t);
                sample.color = mix(previous.color, head.color, t);
                sample.size = mix(previous.size, head.size, t);
                sample.rotation = tick;
                if ring_count == capacity {
                    discarded_time = max(discarded_time, particles[owner + 1u + ring_head].rotation);
                }
                particles[owner + 1u + ring_head] = sample;
                aux[(owner + 1u + ring_head) * 3u] = bitcast<u32>(mix(previous_distance, path_distance, t));
                ring_head = (ring_head + 1u) % capacity;
                ring_count = min(ring_count + 1u, capacity);
            }
            ring_tick = bitcast<u32>(tick);
        }
        aux[owner * 3u] = ring_head;
        aux[owner * 3u + 1u] = ring_count;
        aux[owner * 3u + 2u] = ring_tick;
        aux[(owner + 1u) * 3u + 1u] = bitcast<u32>(path_distance);
        aux[(owner + 1u) * 3u + 2u] = bitcast<u32>(discarded_time);
        particles[owner] = head;
    }
    var occupied = 0u;
    var retired = 0u;
    var truncated = 0u;
    for (var i = 0u; i < e.trail_capacity; i += 1u) {
        let base = root + 1u + i * e.trail_points;
        // Distance mode may leave its ring untouched for many frames. Remove
        // expired anchors, otherwise a stationary live head could keep an old
        // partial segment visible forever. The moving head itself is not a sample.
        if e.trail_sampling == 1u && particle_alive(particles[base]) != 0u {
            for (var s = 0u; s < capacity; s += 1u) {
                let points = aux[base * 3u + 1u];
                if points == 0u { break; }
                let oldest_slot = base + 1u + (aux[base * 3u] + capacity - points) % capacity;
                if now - particles[oldest_slot].rotation < e.trail_lifetime { break; }
                aux[base * 3u + 1u] = points - 1u;
            }
        }
        if particle_alive(particles[base]) != 0u { occupied += 1u; }
        if particle_alive(particles[base]) != 0u && now - bitcast<f32>(aux[(base + 1u) * 3u + 2u]) < e.trail_lifetime {
            truncated += 1u;
        }
        if particle_alive(particles[base]) == 1u { retired += 1u; }
        if particle_alive(particles[base]) == 2u { particles[base].packed_emitter_alive = set_alive(particles[base].packed_emitter_alive, 1u); }
    }
    particles[root].packed_emitter_alive = set_alive(particles[root].packed_emitter_alive, 1u);
    particles[root].rotation = now;
    particles[root].particle_index = globals.seed;
    aux[root * 3u] = globals._padding.x;
    aux[root * 3u + 1u] = evictions;
    let peak = max(occupied, select(aux[root * 3u + 2u], 0u, reset));
    aux[root * 3u + 2u] = peak;
    // Small asynchronous profiler readback; never copy the history buffer.
    let stats = 2u + emitter_index * 6u;
    atomicStore(&counters[stats], occupied);
    atomicStore(&counters[stats + 1u], retired);
    atomicStore(&counters[stats + 2u], evictions);
    atomicStore(&counters[stats + 3u], peak);
    atomicStore(&counters[stats + 4u], globals._padding.x);
    atomicStore(&counters[stats + 5u], truncated);
    record_trail_bounds(emitter_index);
}

@compute @workgroup_size(64)
fn update_trails(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x < arrayLength(&emitters) { record_trails(id.x); }
}
