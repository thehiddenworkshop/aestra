// Aestra Fluid — secondary emission (fluid F10): where the fluid asks particles to be born, as the
// emission list an emitter's Spawn From Domain reads (`aestra_runtime::EmissionLayout`). Composed with
// the solver (and the liquid, whose candidates are its particles: `emit_liquid.wgsl`).
//
// Every tick ends with three passes. The mask pass decides, per candidate, whether it emits — it
// qualifies, and a hash of the tick and the candidate falls under `rate × dt` — and ranks the chosen
// ones within their workgroup with a scan; one workgroup then turns the workgroups' totals into
// offsets; the write pass puts each chosen candidate at its rank. The list's order is the
// candidates', so it depends only on the fluid's state: no atomic append, nothing to sort.

@group(0) @binding(36) var<storage, read_write> emission: array<u32>;
// Per workgroup of candidates, 64 ranks (a chosen candidate's rank within its workgroup, plus one;
// 0 when not chosen) from word 16, then one total per workgroup — their offsets once ranked.
@group(0) @binding(37) var<storage, read_write> emission_scratch: array<u32>;

const EMISSION_HEADER: u32 = 4u;
const EMISSION_RECORD: u32 = 8u;
const EMISSION_SCRATCH_HEADER: u32 = 16u;

// The Secondary Emission block, at the constant word the header's word 23 holds.
fn emission_base() -> u32 { return constants[23]; }
fn emission_rate() -> f32 { return bitcast<f32>(constants[emission_base()]); }
fn emission_min_speed() -> f32 { return bitcast<f32>(constants[emission_base() + 1u]); }
fn emission_threshold() -> f32 { return bitcast<f32>(constants[emission_base() + 2u]); }
fn emission_capacity() -> u32 { return constants[emission_base() + 3u]; }
// The workgroups of candidates the mask pass runs at most.
fn emission_groups() -> u32 { return constants[emission_base() + 4u]; }

// A uniform in [0, 1) for candidate `index` this tick, on channel `salt`.
fn emission_hash01(index: u32, salt: u32) -> f32 {
    let tick = hash_u32(frame[0] * 0x85ebca6bu ^ frame_seed() ^ salt * 0x27d4eb2fu);
    return f32(hash_u32(index * 0x9e3779b9u ^ tick) >> 8u) / 16777216.0;
}

// Whether a qualifying candidate emits this tick: `rate` per second, on average.
fn emission_chosen(index: u32) -> bool {
    return emission_hash01(index, 0u) < emission_rate() * frame_dt();
}

fn emission_totals() -> u32 {
    return EMISSION_SCRATCH_HEADER + emission_groups() * 64u;
}

// Ranks workgroup `group`'s chosen candidates. Every invocation of the workgroup calls it.
fn emission_rank(local: u32, group: u32, chosen: bool) {
    let scanned = aestra_workgroup_scan(local, select(0u, 1u, chosen));
    emission_scratch[EMISSION_SCRATCH_HEADER + group * 64u + local] = select(0u, scanned.x + 1u, chosen);
    if (local == 0u) {
        emission_scratch[emission_totals() + group] = scanned.y;
    }
}

// Where invocation `local` of workgroup `group` writes its record, or the capacity when it has none
// (not chosen, or past the list's capacity).
fn emission_slot(local: u32, group: u32) -> u32 {
    let rank = emission_scratch[EMISSION_SCRATCH_HEADER + group * 64u + local];
    if (rank == 0u) {
        return emission_capacity();
    }
    return min(emission_scratch[emission_totals() + group] + rank - 1u, emission_capacity());
}

fn emission_write(slot: u32, position: vec3<f32>, velocity: vec3<f32>) {
    let at = EMISSION_HEADER + slot * EMISSION_RECORD;
    emission[at] = bitcast<u32>(position.x);
    emission[at + 1u] = bitcast<u32>(position.y);
    emission[at + 2u] = bitcast<u32>(position.z);
    emission[at + 3u] = 0u;
    emission[at + 4u] = bitcast<u32>(velocity.x);
    emission[at + 5u] = bitcast<u32>(velocity.y);
    emission[at + 6u] = bitcast<u32>(velocity.z);
    emission[at + 7u] = 0u;
}

// One workgroup: the workgroups' totals become their offsets, and the list its count.
@compute @workgroup_size(64)
fn emit_offsets(@builtin(local_invocation_index) local: u32) {
    let groups = emission_groups();
    var carry = 0u;
    for (var base = 0u; base < groups; base += 64u) {
        let group = base + local;
        var total = 0u;
        if (group < groups) {
            total = emission_scratch[emission_totals() + group];
        }
        let scanned = aestra_workgroup_scan(local, total);
        if (group < groups) {
            emission_scratch[emission_totals() + group] = carry + scanned.x;
        }
        carry += scanned.y;
    }
    if (local == 0u) {
        emission[0] = min(carry, emission_capacity());
    }
}

// A gas's cells: those whose density (or, burning, temperature) reaches the threshold, moving at
// least the minimum speed. `emit_cells_fire` reads the temperature.
fn emission_cell_linear(cell: vec3<u32>) -> u32 {
    let n = grid_res();
    return (cell.z * n + cell.y) * n + cell.x;
}

fn emission_cell_chosen(cell: vec3<u32>, value: f32) -> bool {
    return value >= emission_threshold()
        && length(centred_velocity(vec3<i32>(cell))) >= emission_min_speed()
        && emission_chosen(emission_cell_linear(cell));
}

@compute @workgroup_size(4, 4, 4)
fn emit_cells(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_index) local: u32,
    @builtin(workgroup_id) group: vec3<u32>,
) {
    let cell = grid_cell(gid);
    let chosen = emission_cell_chosen(cell, density[cell_index(cell)]);
    emission_rank(local, grid_group_index(group), chosen);
}

@compute @workgroup_size(4, 4, 4)
fn emit_cells_fire(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_index) local: u32,
    @builtin(workgroup_id) group: vec3<u32>,
) {
    let cell = grid_cell(gid);
    let chosen = emission_cell_chosen(cell, temperature[cell_index(cell)]);
    emission_rank(local, grid_group_index(group), chosen);
}

// A chosen cell's record: a point jittered within the cell, with the velocity at its centre.
@compute @workgroup_size(4, 4, 4)
fn emit_cells_write(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_index) local: u32,
    @builtin(workgroup_id) group: vec3<u32>,
) {
    let slot = emission_slot(local, grid_group_index(group));
    if (slot >= emission_capacity()) {
        return;
    }
    let cell = grid_cell(gid);
    let linear = emission_cell_linear(cell);
    let jitter = vec3<f32>(
        emission_hash01(linear, 1u),
        emission_hash01(linear, 2u),
        emission_hash01(linear, 3u),
    );
    let position = grid_origin() + (vec3<f32>(cell) + jitter) * cell_size();
    emission_write(slot, position, centred_velocity(vec3<i32>(cell)));
}
