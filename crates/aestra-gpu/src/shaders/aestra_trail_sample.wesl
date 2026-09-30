fn trail_update_owner(e: Emitter, head_slot: u32, owner: u32, new_owner: bool) {
    let capacity = e.trail_points - 1u;
    let now = globals.time;
    let world_scale = max(length(globals.world_from_effect[0].xyz), max(length(globals.world_from_effect[1].xyz), length(globals.world_from_effect[2].xyz)));
    var head = particles[head_slot];
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
    if !new_owner {
        discarded_time = bitcast<f32>(aux[(owner + 1u) * 3u + 2u]);
    }
    if new_owner {
        ring_head = 1u % capacity;
        ring_count = 1u;
        ring_tick = bitcast<u32>(select(now, 0.0, e.trail_sampling != 0u));
        particles[owner + 1u] = head;
        aux[(owner + 1u) * 3u] = bitcast<u32>(0.0);
    } else if e.trail_sampling != 0u {
        let previous = particles[owner];
        ring_head = aux[owner * 3u];
        ring_count = aux[owner * 3u + 1u];
        ring_tick = aux[owner * 3u + 2u];
        let segment = length(head.position - previous.position);
        let previous_distance = bitcast<f32>(aux[(owner + 1u) * 3u + 1u]);
        path_distance = previous_distance + segment;
        var pending = bitcast<f32>(ring_tick);
        if e.trail_sampling == 2u && segment > 0.0 && segment < 3.402823e38 {
            // The pending observed polyline lies in the ellipse with its
            // endpoints as foci and arc length as major diameter. Its minor
            // radius bounds EVERY omitted point's distance from the chord,
            // not only the last observation (which misses gradual bends).
            var commit_previous = ring_count == 0u;
            if ring_count > 0u {
                let anchor = owner + 1u + (ring_head + capacity - 1u) % capacity;
                let chord = length(head.position - particles[anchor].position);
                let arc = pending + segment;
                let error = 0.5 * sqrt(max(0.0, arc - chord) * (arc + chord));
                commit_previous = pending > 0.0 && (error > e.trail_tolerance || arc > e.trail_distance);
            }
            if commit_previous {
                // The previous head was accepted by the previous observation;
                // preserve that endpoint before extending along this segment.
                if ring_count == capacity {
                    discarded_time = max(discarded_time, particles[owner + 1u + ring_head].rotation);
                }
                particles[owner + 1u + ring_head] = previous;
                aux[(owner + 1u + ring_head) * 3u] = bitcast<u32>(previous_distance);
                ring_head = (ring_head + 1u) % capacity;
                ring_count = min(ring_count + 1u, capacity);
                pending = 0.0;
            }
        }
        let total = pending + segment;
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
