// Large pools use storage-backed lists and page-local sorting. _spawn_inverse_padding
// carries exact integer-valued [scratch base, padded heads, padded owners].
// Globals._padding.y is transient [phase kind:8, merge exponent:8, source parity:1].
// Kind 0 orders heads, kind 1 matches owners, kind 2 orders replacement candidates.
// No persistent history state depends on this scratch or on atomic append order.
fn paged_trail(e: Emitter) -> bool {
    return e.trail_points >= 2u && (e.max_particles > 1024u || e.trail_capacity > 1024u);
}

fn paged_reset(e: Emitter) -> bool {
    let last = particles[e.trail_offset];
    return particle_alive(last) == 0u || aux[e.trail_offset * 3u] != globals._padding.x
        || last.particle_index != globals.seed || globals.time < last.rotation
        || globals.time - last.rotation > e.trail_lifetime;
}

// Reads the particle buffer, which D3D's FXC cannot prove uniform: a kernel with barriers must not
// return early on it (every thread must reach each barrier in uniform control flow), but skip its
// work and its writes instead. Conditions on `emitters` and `globals` may still return early.
fn paged_record(e: Emitter) -> bool {
    return paged_trail(e) && (paged_reset(e) || globals.time != particles[e.trail_offset].rotation);
}

fn paged_heads(e: Emitter) -> u32 { return u32(e._spawn_inverse_padding.y); }
fn paged_owners(e: Emitter) -> u32 { return u32(e._spawn_inverse_padding.z); }
fn paged_base(e: Emitter) -> u32 { return u32(e._spawn_inverse_padding.x); }
fn paged_kind() -> u32 { return globals._padding.y & 255u; }
fn paged_source() -> u32 { return (globals._padding.y >> 16u) & 1u; }
fn paged_list(e: Emitter, source: u32) -> u32 {
    if paged_kind() == 0u { return paged_base(e) + source * paged_heads(e); }
    return paged_base(e) + 2u * paged_heads(e) + source * paged_owners(e);
}
fn paged_mapping(e: Emitter) -> u32 { return paged_base(e) + 2u * paged_heads(e) + 2u * paged_owners(e); }
fn paged_pages(e: Emitter) -> u32 { return paged_mapping(e) + paged_heads(e); }
fn paged_bounds(e: Emitter) -> u32 { return paged_pages(e) + (paged_heads(e) + 1023u) / 1024u; }

// The birth-page scan publishes its total in the first bounds-output word.
// Candidate ordering consumes it before bound_trail_page replaces the word
// with the current occupied count. No extra scratch or persistent state is needed.
fn paged_needs_candidates(e: Emitter) -> bool { return aux[paged_bounds(e)] != 0u; }

var<workgroup> paged_values: array<u32, 1024>;
const TRAIL_SORT_PAGE: u32 = 256u;
// One owner per lane: long sample rings no longer serialize sixteen owners on
// each lane of only a handful of workgroups. Page summaries stay GPU-resident.
const TRAIL_BOUNDS_PAGE: u32 = 64u;
var<workgroup> paged_keys: array<vec4<u32>, TRAIL_SORT_PAGE>;

// Two words per physical index, reused for each ordered phase. Keep this cache
// outside all list/rank/page outputs: later scans overwrite the head sort list.
fn paged_key_cache(e: Emitter) -> u32 {
    return paged_bounds(e) + 12u * ((paged_owners(e) + TRAIL_BOUNDS_PAGE - 1u) / TRAIL_BOUNDS_PAGE);
}

fn paged_key_address(e: Emitter, value: u32) -> u32 {
    var index = value;
    if paged_kind() == 0u { index -= e.slot_offset; }
    return paged_key_cache(e) + 2u * index;
}

// Heads are already ordered by (strand, stable ID). Match occupied owners into
// that list instead of sorting the entire owner pool by ID every observation.
// Valid history has one owner per stable ID, so matches have disjoint destinations.
// Rebuild from persistent owner records each tick; no lookup survives a checkpoint.
fn match_trail_page(e: Emitter, emitter_index: u32, start: u32, thread: u32) {
    if !paged_record(e) { return; }
    let count = min(atomicLoad(&indirect[emitter_index * 4u + 1u]), e.max_particles);
    let reset = paged_reset(e);
    let strands = max(e._turbulence_padding, 1u);
    for (var n = start + thread; n < min(start + TRAIL_SORT_PAGE, e.trail_capacity); n += 64u) {
        let base = e.trail_offset + 1u + n * e.trail_points;
        if reset || globals.time - particles[base].rotation >= e.trail_lifetime {
            particles[base].packed_emitter_alive = set_alive(particles[base].packed_emitter_alive, 0u);
        }
        if particle_alive(particles[base]) == 0u { continue; }
        let identity = particles[base].particle_index;
        let strand = identity % strands;
        var low = 0u;
        var high = count;
        while low < high {
            let middle = (low + high) / 2u;
            let slot = alive_indices[e.slot_offset + middle];
            let key = paged_key_cache(e) + 2u * (slot - e.slot_offset);
            let less = aux[key] < strand || (aux[key] == strand && aux[key + 1u] < identity);
            if less { low = middle + 1u; } else { high = middle; }
        }
        if low < count {
            let slot = alive_indices[e.slot_offset + low];
            if particles[slot].particle_index == identity {
                aux[paged_mapping(e) + low] = base;
                particles[base].packed_emitter_alive = set_alive(particles[base].packed_emitter_alive, 2u);
            }
        }
    }
}

fn paged_cached_key(e: Emitter, value: u32) -> vec4<u32> {
    if value == 0xffffffffu { return vec4<u32>(0xffffffffu); }
    let key = paged_key_address(e, value);
    return vec4<u32>(aux[key], aux[key + 1u], value, 0u);
}

// Cache immutable keys once per page instead of fetching strided particle
// records at every sorting-network comparison. The index remains separate:
// 0xffffffff is a missing list entry, not a forbidden particle identity.
fn paged_key(e: Emitter, value: u32) -> vec4<u32> {
    if value == 0xffffffffu { return vec4<u32>(0xffffffffu); }
    if paged_kind() == 0u {
        let identity = particles[value].particle_index;
        return vec4<u32>(identity % max(e._turbulence_padding, 1u), identity, value, 0u);
    }
    let p = particles[e.trail_offset + 1u + value * e.trail_points];
    return vec4<u32>(u32(particle_alive(p) != 0u), bitcast<u32>(p.rotation), value, 0u);
}

fn paged_key_less(a: vec4<u32>, b: vec4<u32>) -> bool {
    if a.x != b.x { return a.x < b.x; }
    if a.x == 0xffffffffu { return false; }
    if paged_kind() == 2u {
        if a.x == 0u { return a.z < b.z; }
        let x = bitcast<f32>(a.y);
        let y = bitcast<f32>(b.y);
        return x < y || (x == y && a.z < b.z);
    }
    return a.y < b.y || (a.y == b.y && a.z < b.z);
}

@compute @workgroup_size(64)
fn sort_trail_page(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) thread: u32) {
    let e = emitters[group.y];
    if !paged_trail(e) { return; }
    let kind = paged_kind();
    // Kind comes from read-only globals, so this branch/return is uniform even
    // for D3D/FXC. Matching has no barriers and does not touch owner sort lists.
    if kind == 1u { match_trail_page(e, group.y, group.x * TRAIL_SORT_PAGE, thread); return; }
    let count = select(paged_owners(e), paged_heads(e), kind == 0u);
    let start = group.x * TRAIL_SORT_PAGE;
    if start >= count { return; }
    let size = min(TRAIL_SORT_PAGE, count - start);
    // Owner pages sort only when the history records (see `paged_record`).
    let recording = kind == 0u || (paged_record(e) && (kind != 2u || paged_needs_candidates(e)));
    let live = min(atomicLoad(&indirect[group.y * 4u + 1u]), e.max_particles);
    for (var local = thread; recording && local < size; local += 64u) {
        let n = start + local;
        var value = 0xffffffffu;
        if kind == 0u {
            if n < live { value = alive_indices[e.slot_offset + n]; }
        } else if n < e.trail_capacity {
            let base = e.trail_offset + 1u + n * e.trail_points;
            if particle_alive(particles[base]) != 2u { value = n; }
        }
        paged_values[local] = value;
    }
    workgroupBarrier();
    // Owner keys are immutable after this page's expiry stores.
    storageBarrier();
    for (var local = thread; recording && local < size; local += 64u) {
        let value = paged_values[local];
        let key = paged_key(e, value);
        paged_keys[local] = key;
        if value != 0xffffffffu {
            let address = paged_key_address(e, value);
            aux[address] = key.x;
            aux[address + 1u] = key.y;
        }
    }
    workgroupBarrier();
    for (var width = 2u; width <= size; width *= 2u) {
        for (var gap = width / 2u; gap > 0u; gap /= 2u) {
            for (var pair = thread; recording && pair < size / 2u; pair += 64u) {
                let a = (pair / gap) * (2u * gap) + pair % gap;
                let b = a + gap;
                let ascending = (a & width) == 0u;
                if paged_key_less(paged_keys[select(a, b, ascending)], paged_keys[select(b, a, ascending)]) {
                    let temporary = paged_values[a];
                    paged_values[a] = paged_values[b];
                    paged_values[b] = temporary;
                    let key = paged_keys[a];
                    paged_keys[a] = paged_keys[b];
                    paged_keys[b] = key;
                }
            }
            workgroupBarrier();
        }
    }
    for (var local = thread; recording && local < size; local += 64u) { aux[paged_list(e, 0u) + start + local] = paged_values[local]; }
}

@compute @workgroup_size(64)
fn merge_trail_pages(@builtin(global_invocation_id) id: vec3<u32>) {
    let e = emitters[id.y];
    if !paged_trail(e) || (paged_kind() != 0u && !paged_record(e)) { return; }
    // Unlike page sorting this kernel has no barriers and can return early on
    // the storage-backed flag. The owner mapping already covers every live head.
    if paged_kind() == 2u && !paged_needs_candidates(e) { return; }
    let count = select(paged_owners(e), paged_heads(e), paged_kind() == 0u);
    let n = id.x;
    if n >= count { return; }
    let width = 1u << ((globals._padding.y >> 8u) & 255u);
    let source = paged_list(e, paged_source());
    let destination = paged_list(e, 1u - paged_source());
    let value = aux[source + n];
    if width >= count { aux[destination + n] = value; return; }
    let key = paged_cached_key(e, value);
    let block = n / (2u * width) * (2u * width);
    let right = (n & width) != 0u;
    let other = select(block + width, block, right);
    var low = 0u;
    var high = width;
    while low < high {
        let middle = (low + high) / 2u;
        let candidate = aux[source + other + middle];
        let candidate_key = paged_cached_key(e, candidate);
        // One comparison per search step; equal sentinels from the left precede
        // the right. Immutable compact keys avoid strided history-record reads.
        var less = false;
        if right { less = !paged_key_less(key, candidate_key); }
        else { less = paged_key_less(candidate_key, key); }
        if less { low = middle + 1u; }
        else { high = middle; }
    }
    let rank = block + n % width + low;
    aux[destination + rank] = value;
}

@compute @workgroup_size(64)
fn present_trail_heads(@builtin(global_invocation_id) id: vec3<u32>) {
    let e = emitters[id.y];
    if !paged_trail(e) { return; }
    let count = min(atomicLoad(&indirect[id.y * 4u + 1u]), e.max_particles);
    if id.x < count { alive_indices[e.slot_offset + id.x] = aux[paged_list(e, paged_source()) + id.x]; }
    if paged_record(e) && id.x < paged_heads(e) { aux[paged_mapping(e) + id.x] = 0xffffffffu; }
}

@compute @workgroup_size(64)
fn reserve_trail_owners(@builtin(global_invocation_id) id: vec3<u32>) {
    let e = emitters[id.y];
    if !paged_record(e) || id.x >= paged_heads(e) { return; }
    let count = min(atomicLoad(&indirect[id.y * 4u + 1u]), e.max_particles);
    let owner = aux[paged_mapping(e) + id.x];
    aux[paged_base(e) + id.x] = u32(id.x < count && owner == 0xffffffffu);
}

var<workgroup> paged_total: u32;
// `size` must be uniform to D3D's FXC: computed from `emitters` and the workgroup id alone, with no
// loop (FXC loses track of a loop-computed size and rejects the barriers below).
fn paged_scan(thread: u32, size: u32) {
    workgroupBarrier();
    for (var width = 2u; width <= size; width *= 2u) {
        for (var n = (thread + 1u) * width - 1u; n < size; n += 64u * width) { paged_values[n] += paged_values[n - width / 2u]; }
        workgroupBarrier();
    }
    if thread == 0u { paged_total = paged_values[size - 1u]; paged_values[size - 1u] = 0u; }
    workgroupBarrier();
    for (var width = size; width >= 2u; width /= 2u) {
        for (var n = (thread + 1u) * width - 1u; n < size; n += 64u * width) {
            let left = paged_values[n - width / 2u];
            paged_values[n - width / 2u] = paged_values[n];
            paged_values[n] += left;
        }
        workgroupBarrier();
    }
}

@compute @workgroup_size(64)
fn scan_trail_births(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) thread: u32) {
    let e = emitters[group.y];
    if !paged_trail(e) { return; }
    let start = group.x * 1024u;
    if start >= paged_heads(e) { return; }
    let size = min(1024u, paged_heads(e) - start);
    let recording = paged_record(e);
    for (var n = thread; n < size; n += 64u) {
        paged_values[n] = select(0u, aux[paged_base(e) + start + n], recording);
    }
    paged_scan(thread, size);
    for (var n = thread; recording && n < size; n += 64u) { aux[paged_base(e) + start + n] = paged_values[n]; }
    if thread == 0u && recording { aux[paged_pages(e) + group.x] = paged_total; }
}

@compute @workgroup_size(64)
fn scan_trail_birth_pages(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) thread: u32) {
    let e = emitters[group.y];
    if !paged_trail(e) { return; }
    let recording = paged_record(e);
    let pages = (paged_heads(e) + 1023u) / 1024u;
    var carry = 0u;
    for (var start = 0u; start < pages; start += 1024u) {
        let remaining = min(1024u, pages - start);
        // The next power of two, without a loop (see `paged_scan`).
        let size = trail_pow2_ceil(remaining);
        for (var n = thread; n < size; n += 64u) {
            var value = 0u;
            if recording && n < remaining { value = aux[paged_pages(e) + start + n]; }
            paged_values[n] = value;
        }
        paged_scan(thread, size);
        for (var n = thread; recording && n < remaining; n += 64u) { aux[paged_pages(e) + start + n] = carry + paged_values[n]; }
        carry += paged_total;
        workgroupBarrier();
    }
    if thread == 0u && recording {
        aux[paged_bounds(e)] = carry;
        atomicStore(&counters[2u + group.y * 6u + 2u], select(aux[e.trail_offset * 3u + 1u], 0u, paged_reset(e)));
    }
}

@compute @workgroup_size(64)
fn update_trail_owners(@builtin(global_invocation_id) id: vec3<u32>) {
    let e = emitters[id.y];
    if !paged_record(e) { return; }
    let count = min(atomicLoad(&indirect[id.y * 4u + 1u]), e.max_particles);
    if id.x >= count { return; }
    var owner = aux[paged_mapping(e) + id.x];
    let born = owner == 0xffffffffu;
    if born {
        let rank = aux[paged_base(e) + id.x] + aux[paged_pages(e) + id.x / 1024u];
        let candidate = aux[paged_list(e, paged_source()) + rank];
        // Live count <= parent capacity <= owner capacity guarantees a free candidate.
        if candidate == 0xffffffffu { return; }
        owner = e.trail_offset + 1u + candidate * e.trail_points;
        if particle_alive(particles[owner]) != 0u { atomicAdd(&counters[2u + id.y * 6u + 2u], 1u); }
    }
    trail_update_owner(e, alive_indices[e.slot_offset + id.x], owner, born);
}

fn paged_bound_range(e: Emitter, thread: u32, start: u32, end: u32, recording: bool) {
    let root = e.trail_offset;
    let capacity = e.trail_points - 1u;
    let now = globals.time;
    var occupied = 0u;
    var retired = 0u;
    var truncated = 0u;
    var minimum = vec3<f32>(3.402823e38);
    var maximum = vec3<f32>(-3.402823e38);
    var size = 0.0;
    var found = false;
    var valid = true;
    for (var i = start + thread; recording && i < end; i += 64u) {
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

}

@compute @workgroup_size(64)
fn bound_trail_page(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) thread: u32) {
    let e = emitters[group.y];
    if !paged_trail(e) { return; }
    let start = group.x * TRAIL_BOUNDS_PAGE;
    if start >= e.trail_capacity { return; }
    let recording = paged_record(e);
    paged_bound_range(e, thread, start, min(start + TRAIL_BOUNDS_PAGE, e.trail_capacity), recording);
    if thread == 0u && recording {
        let output = paged_bounds(e) + group.x * 12u;
        for (var n = 0u; n < 3u; n += 1u) { aux[output + n] = atomicLoad(&trail_scratch[n]); }
        for (var n = 0u; n < 8u; n += 1u) { aux[output + 3u + n] = bitcast<u32>(trail_priority[n]); }
    }
}

@compute @workgroup_size(64)
fn finish_trail_pages(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) thread: u32) {
    let emitter_index = group.y;
    let e = emitters[emitter_index];
    if !paged_trail(e) { return; }
    let recording = paged_record(e);
    let root = e.trail_offset;
    let stats = 2u + emitter_index * 6u;
    let now = globals.time;
    let reset = paged_reset(e);
    var occupied = 0u;
    var retired = 0u;
    var truncated = 0u;
    var minimum = vec3<f32>(3.402823e38);
    var maximum = vec3<f32>(-3.402823e38);
    var size = 0.0;
    var found = false;
    var valid = true;

    let pages = (e.trail_capacity + TRAIL_BOUNDS_PAGE - 1u) / TRAIL_BOUNDS_PAGE;
    for (var page = thread; recording && page < pages; page += 64u) {
        let input = paged_bounds(e) + page * 12u;
        occupied += aux[input];
        retired += aux[input + 1u];
        truncated += aux[input + 2u];
        minimum = min(minimum, vec3<f32>(bitcast<f32>(aux[input + 3u]), bitcast<f32>(aux[input + 4u]), bitcast<f32>(aux[input + 5u])));
        maximum = max(maximum, vec3<f32>(bitcast<f32>(aux[input + 6u]), bitcast<f32>(aux[input + 7u]), bitcast<f32>(aux[input + 8u])));
        size = max(size, bitcast<f32>(aux[input + 9u]));
        let flag = bitcast<f32>(aux[input + 10u]);
        valid = valid && flag != 0.0;
        found = found || flag == 2.0;
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

    if thread != 0u || !recording { return; }
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
