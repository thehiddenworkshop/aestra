// Persistent tail of the particle buffer: one emitter header followed by bounded
// owner chunks. A chunk is a live head plus a ring of time- or distance-spaced samples.
// History positions/sizes are world-space. rotation stores the observation time;
// head padding stores next ring index, count, and last sample time (Time) or
// residual world-space path length (Distance), encoded as f32 bits.
// Owners use stable particle_index, never compacted/physical particle slots.
// One cooperative workgroup per emitter. Scratch is reused for the owner hash,
// allocation order and reduction; total shared storage is 16 KiB. Nothing here
// persists across dispatches, so checkpoint/restored history remains self-contained.
var<workgroup> trail_scratch: array<atomic<u32>, 2048>;
var<workgroup> trail_priority: array<f32, 1024>;
var<workgroup> trail_head_owner: array<u32, 1024>;

fn trail_hash(id: u32) -> u32 {
    return ((id ^ (id >> 16u)) * 0x45d9f3bu) & 2047u;
}

fn trail_candidate_before(a: u32, b: u32) -> bool {
    if a == 0xffffffffu { return false; }
    if b == 0xffffffffu { return true; }
    return trail_priority[a] < trail_priority[b]
        || (trail_priority[a] == trail_priority[b] && a < b);
}

fn trail_parent_before(a: u32, b: u32, strands: u32) -> bool {
    if a == 0xffffffffu { return false; }
    if b == 0xffffffffu { return true; }
    return ribbon_less(a, b, strands);
}

fn record_trails(emitter_index: u32, thread: u32) {
    let e = emitters[emitter_index];
    if e.trail_points < 2u { return; }
    let root = e.trail_offset;
    let now = globals.time;
    let last = particles[root];
    let reset = particle_alive(last) == 0u || aux[root * 3u] != globals._padding.x || last.particle_index != globals.seed || now < last.rotation || now - last.rotation > e.trail_lifetime;
    let capacity = e.trail_points - 1u;
    let stats = 2u + emitter_index * 6u;
    let count = min(atomicLoad(&indirect[emitter_index * 4u + 1u]), e.max_particles);
    var head_count = 1u;
    while head_count < count { head_count *= 2u; }
    // Atomic append order is not a stable allocation order. Sort parents in
    // parallel, replacing the serial ribbon heapsort previously paid by trails.
    for (var n = thread; n < head_count; n += 64u) {
        var slot = 0xffffffffu;
        if n < count { slot = alive_indices[e.slot_offset + n]; }
        trail_head_owner[n] = slot;
    }
    workgroupBarrier();
    for (var width = 2u; width <= head_count; width *= 2u) {
        for (var gap = width / 2u; gap > 0u; gap /= 2u) {
            for (var pair = thread; pair < head_count / 2u; pair += 64u) {
                let a_index = (pair / gap) * (2u * gap) + pair % gap;
                let b_index = a_index + gap;
                let a = trail_head_owner[a_index];
                let b = trail_head_owner[b_index];
                let ascending = (a_index & width) == 0u;
                let strands = max(e._turbulence_padding, 1u);
                if select(trail_parent_before(a, b, strands), trail_parent_before(b, a, strands), ascending) {
                    trail_head_owner[a_index] = b;
                    trail_head_owner[b_index] = a;
                }
            }
            workgroupBarrier();
        }
    }
    for (var n = thread; n < count; n += 64u) {
        alive_indices[e.slot_offset + n] = trail_head_owner[n];
    }
    workgroupBarrier();
    storageBarrier();
    if !reset && now == last.rotation { return; }
    if thread == 0u {
        atomicStore(&counters[stats + 2u], select(aux[root * 3u + 1u], 0u, reset));
    }
    for (var i = thread; i < 2048u; i += 64u) {
        atomicStore(&trail_scratch[i], 0u);
    }
    for (var i = thread; i < e.trail_capacity; i += 64u) {
        let base = root + 1u + i * e.trail_points;
        if reset || now - particles[base].rotation >= e.trail_lifetime {
            particles[base].packed_emitter_alive = set_alive(particles[base].packed_emitter_alive, 0u);
        }
    }
    workgroupBarrier();
    storageBarrier();
    // Hash stable particle IDs, not slots. A retired owner stays in the lookup
    // until expiry or eviction; restoring a checkpoint rebuilds the same mapping.
    for (var i = thread; i < e.trail_capacity; i += 64u) {
        let base = root + 1u + i * e.trail_points;
        if particle_alive(particles[base]) == 0u { continue; }
        var bucket = trail_hash(particles[base].particle_index);
        for (var probe = 0u; probe < 2048u;) {
            let claim = atomicCompareExchangeWeak(&trail_scratch[bucket], 0u, i + 1u);
            if claim.exchanged { break; }
            // Weak CAS can fail spuriously: retry an empty bucket.
            if claim.old_value != 0u {
                bucket = (bucket + 1u) & 2047u;
                probe += 1u;
            }
        }
    }
    workgroupBarrier();
    // Reserve every surviving owner before allocating, so a newly born particle
    // cannot evict a living parent's history. Sorted IDs make eviction deterministic.
    for (var n = thread; n < count; n += 64u) {
        let id = particles[alive_indices[e.slot_offset + n]].particle_index;
        trail_head_owner[n] = 0xffffffffu;
        var bucket = trail_hash(id);
        for (var probe = 0u; probe < 2048u; probe += 1u) {
            let entry = atomicLoad(&trail_scratch[bucket]);
            if entry == 0u { break; }
            let base = root + 1u + (entry - 1u) * e.trail_points;
            if particles[base].particle_index == id {
                trail_head_owner[n] = base;
                particles[base].packed_emitter_alive = set_alive(particles[base].packed_emitter_alive, 2u);
                break;
            }
            bucket = (bucket + 1u) & 2047u;
        }
    }
    workgroupBarrier();
    storageBarrier();
    // Sort available owners once: empty chunks first, then oldest retired tails,
    // ties by chunk index. New heads consume that order in alive-index order.
    // Never evict a surviving parent, even when all owner chunks are occupied.
    var sort_count = 1u;
    while sort_count < e.trail_capacity { sort_count *= 2u; }
    for (var i = thread; i < sort_count; i += 64u) {
        var candidate = 0xffffffffu;
        if i < e.trail_capacity {
            let previous = particles[root + 1u + i * e.trail_points];
            let alive = particle_alive(previous);
            trail_priority[i] = select(previous.rotation, -3.402823e38, alive == 0u);
            if alive != 2u { candidate = i; }
        }
        atomicStore(&trail_scratch[i], candidate);
    }
    workgroupBarrier();
    for (var width = 2u; width <= sort_count; width *= 2u) {
        for (var gap = width / 2u; gap > 0u; gap /= 2u) {
            for (var pair = thread; pair < sort_count / 2u; pair += 64u) {
                let a_index = (pair / gap) * (2u * gap) + pair % gap;
                let b_index = a_index + gap;
                let a = atomicLoad(&trail_scratch[a_index]);
                let b = atomicLoad(&trail_scratch[b_index]);
                let ascending = (a_index & width) == 0u;
                if select(trail_candidate_before(a, b), trail_candidate_before(b, a), ascending) {
                    atomicStore(&trail_scratch[a_index], b);
                    atomicStore(&trail_scratch[b_index], a);
                }
            }
            workgroupBarrier();
        }
    }
    // Exclusive scan assigns distinct candidates without a serial allocator or
    // a per-head search over all earlier births. Reuse the hash's second half.
    for (var n = thread; n < head_count; n += 64u) {
        var born = 0u;
        if n < count { born = u32(trail_head_owner[n] == 0xffffffffu); }
        atomicStore(&trail_scratch[1024u + n], born);
    }
    workgroupBarrier();
    for (var width = 2u; width <= head_count; width *= 2u) {
        for (var n = (thread + 1u) * width - 1u; n < head_count; n += 64u * width) {
            atomicAdd(&trail_scratch[1024u + n], atomicLoad(&trail_scratch[1024u + n - width / 2u]));
        }
        workgroupBarrier();
    }
    if thread == 0u { atomicStore(&trail_scratch[1024u + head_count - 1u], 0u); }
    workgroupBarrier();
    for (var width = head_count; width >= 2u; width /= 2u) {
        for (var n = (thread + 1u) * width - 1u; n < head_count; n += 64u * width) {
            let left = atomicLoad(&trail_scratch[1024u + n - width / 2u]);
            let parent = atomicLoad(&trail_scratch[1024u + n]);
            atomicStore(&trail_scratch[1024u + n - width / 2u], parent);
            atomicStore(&trail_scratch[1024u + n], parent + left);
        }
        workgroupBarrier();
    }
    let world_scale = max(length(globals.world_from_effect[0].xyz), max(length(globals.world_from_effect[1].xyz), length(globals.world_from_effect[2].xyz)));
    for (var n = thread; n < count; n += 64u) {
        var head = particles[alive_indices[e.slot_offset + n]];
        var owner = trail_head_owner[n];
        let new_owner = owner == 0xffffffffu;
        if new_owner {
            let rank = atomicLoad(&trail_scratch[1024u + n]);
            let candidate = atomicLoad(&trail_scratch[rank]);
            if candidate == 0xffffffffu { continue; }
            owner = root + 1u + candidate * e.trail_points;
            if particle_alive(particles[owner]) != 0u { atomicAdd(&counters[stats + 2u], 1u); }
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
    workgroupBarrier();
    storageBarrier();
    var occupied = 0u;
    var retired = 0u;
    var truncated = 0u;
    var minimum = vec3<f32>(3.402823e38);
    var maximum = vec3<f32>(-3.402823e38);
    var size = 0.0;
    var found = false;
    var valid = true;
    for (var i = thread; i < e.trail_capacity; i += 64u) {
        let base = root + 1u + i * e.trail_points;
        // Spatial sampling may leave its ring untouched for many frames. Remove
        // expired anchors, otherwise a stationary live head could keep an old
        // partial segment visible forever. The moving head itself is not a sample.
        if e.trail_sampling != 0u && particle_alive(particles[base]) != 0u {
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
        let points = aux[base * 3u + 1u];
        if particle_alive(particles[base]) == 0u || points == 0u { continue; }
        for (var s = 0u; s <= points; s += 1u) {
            var slot = base;
            if s < points { slot = base + 1u + (aux[base * 3u] + capacity - points + s) % capacity; }
            let p = particles[slot];
            valid = valid && trail_finite(p.position.x) && trail_finite(p.position.y)
                && trail_finite(p.position.z) && trail_finite(p.size);
            minimum = min(minimum, p.position);
            maximum = max(maximum, p.position);
            size = max(size, abs(p.size));
            found = true;
        }
    }
    atomicStore(&trail_scratch[thread * 3u], occupied);
    atomicStore(&trail_scratch[thread * 3u + 1u], retired);
    atomicStore(&trail_scratch[thread * 3u + 2u], truncated);
    for (var axis = 0u; axis < 3u; axis += 1u) {
        trail_priority[thread * 8u + axis] = minimum[axis];
        trail_priority[thread * 8u + 3u + axis] = maximum[axis];
    }
    trail_priority[thread * 8u + 6u] = size;
    trail_priority[thread * 8u + 7u] = select(0.0, select(1.0, 2.0, found), valid);
    workgroupBarrier();
    for (var gap = 32u; gap > 0u; gap /= 2u) {
        if thread < gap {
            for (var stat = 0u; stat < 3u; stat += 1u) {
                atomicAdd(&trail_scratch[thread * 3u + stat], atomicLoad(&trail_scratch[(thread + gap) * 3u + stat]));
            }
            for (var axis = 0u; axis < 3u; axis += 1u) {
                trail_priority[thread * 8u + axis] = min(trail_priority[thread * 8u + axis], trail_priority[(thread + gap) * 8u + axis]);
                trail_priority[thread * 8u + 3u + axis] = max(trail_priority[thread * 8u + 3u + axis], trail_priority[(thread + gap) * 8u + 3u + axis]);
            }
            trail_priority[thread * 8u + 6u] = max(trail_priority[thread * 8u + 6u], trail_priority[(thread + gap) * 8u + 6u]);
            let a = trail_priority[thread * 8u + 7u];
            let b = trail_priority[(thread + gap) * 8u + 7u];
            trail_priority[thread * 8u + 7u] = select(max(a, b), 0.0, a == 0.0 || b == 0.0);
        }
        workgroupBarrier();
    }
    if thread != 0u { return; }
    occupied = atomicLoad(&trail_scratch[0]);
    retired = atomicLoad(&trail_scratch[1]);
    truncated = atomicLoad(&trail_scratch[2]);
    let evictions = atomicLoad(&counters[stats + 2u]);
    particles[root].packed_emitter_alive = set_alive(particles[root].packed_emitter_alive, 1u);
    particles[root].rotation = now;
    particles[root].particle_index = globals.seed;
    aux[root * 3u] = globals._padding.x;
    aux[root * 3u + 1u] = evictions;
    let peak = max(occupied, select(aux[root * 3u + 2u], 0u, reset));
    aux[root * 3u + 2u] = peak;
    // Small asynchronous profiler readback; never copy the history buffer.
    atomicStore(&counters[stats], occupied);
    atomicStore(&counters[stats + 1u], retired);
    atomicStore(&counters[stats + 2u], evictions);
    atomicStore(&counters[stats + 3u], peak);
    atomicStore(&counters[stats + 4u], globals._padding.x);
    atomicStore(&counters[stats + 5u], truncated);
    particles[root].position = vec3<f32>(trail_priority[0], trail_priority[1], trail_priority[2]);
    particles[root].color = vec4<f32>(trail_priority[3], trail_priority[4], trail_priority[5], trail_priority[6]);
    particles[root].size = select(0.0, select(2.0, 1.0, trail_priority[7] == 2.0), trail_priority[7] != 0.0);
}

@compute @workgroup_size(64)
fn update_trails(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) thread: u32) {
    if group.x < arrayLength(&emitters) { record_trails(group.x, thread); }
}
