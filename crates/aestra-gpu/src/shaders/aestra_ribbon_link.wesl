// Post-simulation linking. Sorting the compacted list must never rely on atomic append order.
// In-place heapsort is O(n log n), uses no scratch storage, and only runs for ribbon emitters.
fn ribbon_less(a: u32, b: u32, strands: u32) -> bool {
    let ka = particles[a].particle_index;
    let kb = particles[b].particle_index;
    let ga = ka % strands;
    let gb = kb % strands;
    if ga != gb { return ga < gb; }
    return ka < kb || (ka == kb && a < b);
}

fn ribbon_sift(offset: u32, count: u32, root: u32, strands: u32) {
    var parent = root;
    loop {
        var child = parent * 2u + 1u;
        if child >= count { break; }
        if child + 1u < count && ribbon_less(alive_indices[offset + child], alive_indices[offset + child + 1u], strands) {
            child += 1u;
        }
        if !ribbon_less(alive_indices[offset + parent], alive_indices[offset + child], strands) { break; }
        let value = alive_indices[offset + parent];
        alive_indices[offset + parent] = alive_indices[offset + child];
        alive_indices[offset + child] = value;
        parent = child;
    }
}

fn link_ribbon(emitter_index: u32) {
    let emitter = emitters[emitter_index];
    if emitter._turbulence_padding == 0u { return; }
    let strands = emitter._turbulence_padding;
    let offset = emitter.slot_offset;
    let count = min(atomicLoad(&indirect[emitter_index * 4u + 1u]), emitter.max_particles);
    var parent = count / 2u;
    loop {
        if parent == 0u { break; }
        parent -= 1u;
        ribbon_sift(offset, count, parent, strands);
    }
    var end = count;
    loop {
        if end <= 1u { break; }
        end -= 1u;
        let value = alive_indices[offset];
        alive_indices[offset] = alive_indices[offset + end];
        alive_indices[offset + end] = value;
        ribbon_sift(offset, end, 0u, strands);
    }
    // The sorted list is contiguous by strand, then stable birth identity. Find each
    // run once: linear linking after sorting, even with sparse/empty strand IDs.
    var group_start = 0u;
    var group_end = 0u;
    for (var i = 0u; i < count; i += 1u) {
        let slot = alive_indices[offset + i];
        if i == group_end {
            group_start = i;
            group_end = i + 1u;
            let group = particles[slot].particle_index % strands;
            loop {
                if group_end >= count { break; }
                if particles[alive_indices[offset + group_end]].particle_index % strands != group { break; }
                group_end += 1u;
            }
        }
        var next = 0xffffffffu;
        var previous = 0xffffffffu;
        if i + 1u < group_end { next = alive_indices[offset + i + 1u]; }
        if i > group_start { previous = alive_indices[offset + i - 1u]; }
        // Ribbon link state lives in the shared aux buffer (3 words/slot), off the
        // particle ABI. Non-ribbon readers ignore it.
        aux[slot * 3u] = next;
        aux[slot * 3u + 1u] = previous;
        aux[slot * 3u + 2u] = bitcast<u32>(f32(i - group_start) / f32(max(group_end - group_start, 2u) - 1u));
    }
}

@compute @workgroup_size(64)
fn link_ribbons(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x < arrayLength(&emitters) { link_ribbon(id.x); }
}
