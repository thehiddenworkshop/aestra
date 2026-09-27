// Aestra Fluid — the sparse grid (fluid F7): the N³ grid is stored as 8³-cell bricks, and only the
// bricks where there is fluid are stored and simulated. Composed with the solver in place of
// `grid_dense.wgsl`, whose interface it implements.
//
// Every grid resource is a pool of bricks: slot s holds cells s·512 .. s·512 + 511, x fastest within
// the brick. Slot 0 is never given out: it stays zero, so a cell of an inactive brick reads as still,
// empty air — and, for the pressure, as p = 0: an inactive neighbour is open air, like an open side.
// A pass covers only the active bricks: the backend reads its workgroup counts from `brick_dispatch`,
// which the allocation below writes, and invocation t of a pass works on cell t mod 512 of the
// (t div 512)-th listed brick.
//
// Allocation runs first every tick, in integer arithmetic only, so it is deterministic:
// - `brick_activity`: a listed brick is occupied while any of its cells holds more smoke, heat or
//   fuel than the threshold;
// - `brick_need`: a brick is needed when it or one of its 26 neighbours is occupied, or a source
//   reaches into it — the ring of neighbours is where the fluid can move next;
// - `brick_plan`: new bricks are ranked in brick order and free slots in slot order (prefix sums);
// - `brick_assign`: the k-th new brick takes the k-th free slot; bricks no longer needed free theirs;
// - `brick_compact`: the active slots, in slot order, become the list the passes cover;
// - `brick_zero`: a slot given to a new brick starts as still, empty air.
// A new brick beyond the budget is not stored: it stays open air until a slot frees.

// `bricks`: a header (word 0: the active count), the list of active slots, each slot's brick (packed
// coordinates + 1; 0 for a free slot), then the brick table (each brick's slot; 0 when inactive).
@group(0) @binding(25) var<storage, read_write> bricks: array<u32>;
// Allocation scratch, cleared every tick: per slot occupied, freed and fresh flags and the free list;
// per brick the new flag and its prefix within its block; the blocks' totals, then their offsets.
@group(0) @binding(26) var<storage, read_write> brick_scratch: array<u32>;
// The passes' workgroup counts: (x, y, z, -) for the fine level's cells, then multigrid levels 1–3
// (level 1 is also one workgroup per brick).
@group(0) @binding(27) var<storage, read_write> brick_dispatch: array<u32>;

const BRICK_EDGE: u32 = 8u;
const BRICK_CELLS: u32 = 512u;
const BRICK_HEADER: u32 = 16u;
const SCRATCH_HEADER: u32 = 16u;

// Stage constants of a sparse grid: the slots (the budget + the zero slot) and the occupancy
// threshold. A negative threshold keeps every brick needed: the grid is then stored whole.
fn brick_capacity() -> u32 { return constants[19]; }
fn brick_threshold() -> f32 { return bitcast<f32>(constants[20]); }
fn brick_grid() -> u32 { return grid_res() / BRICK_EDGE; }
fn brick_count() -> u32 { return bricks[0]; }
fn brick_list(listed: u32) -> u32 { return bricks[BRICK_HEADER + listed]; }
fn brick_coord_word(slot: u32) -> u32 { return BRICK_HEADER + brick_capacity() + slot; }
fn brick_table_word(linear: u32) -> u32 { return BRICK_HEADER + 2u * brick_capacity() + linear; }

fn pack_brick(brick: vec3<u32>) -> u32 {
    return (brick.x | (brick.y << 10u) | (brick.z << 20u)) + 1u;
}

fn unpack_brick(word: u32) -> vec3<u32> {
    let packed = word - 1u;
    return vec3<u32>(packed & 1023u, (packed >> 10u) & 1023u, packed >> 20u);
}

fn brick_linear(brick: vec3<u32>) -> u32 {
    let g = brick_grid();
    return (brick.z * g + brick.y) * g + brick.x;
}

fn brick_slot(brick: vec3<u32>) -> u32 {
    return bricks[brick_table_word(brick_linear(brick))];
}

// ---- The grid interface ----

fn cell_index(cell: vec3<u32>) -> u32 {
    let local = cell % BRICK_EDGE;
    return brick_slot(cell / BRICK_EDGE) * BRICK_CELLS + (local.z * BRICK_EDGE + local.y) * BRICK_EDGE
        + local.x;
}

fn grid_cell(gid: vec3<u32>) -> vec3<u32> {
    return lv_cell(0u, gid);
}

fn cell_active(cell: vec3<i32>) -> bool {
    let last = i32(grid_res()) - 1;
    return brick_slot(vec3<u32>(clamp(cell, vec3<i32>(0), vec3<i32>(last))) / BRICK_EDGE) != 0u;
}

// Level `level` of a brick is (8 >> level)³ cells; the passes' 4³ workgroups are laid end to end.
fn lv_cell(level: u32, gid: vec3<u32>) -> vec3<u32> {
    let edge = BRICK_EDGE >> level;
    let per_brick = edge * edge * edge;
    let t = (gid.x / 4u) * 64u + (gid.z * 4u + gid.y) * 4u + gid.x % 4u;
    let listed = t / per_brick;
    if (listed >= brick_count()) {
        return vec3<u32>(0xffffffffu);
    }
    let local = t % per_brick;
    let brick = unpack_brick(bricks[brick_coord_word(brick_list(listed))]);
    return brick * edge + vec3<u32>(local % edge, (local / edge) % edge, local / (edge * edge));
}

fn lv_base(level: u32) -> u32 {
    var base = 0u;
    var per_brick = BRICK_CELLS;
    for (var l = 0u; l < level; l += 1u) {
        base += brick_capacity() * per_brick;
        per_brick = per_brick / 8u;
    }
    return base;
}

fn lv_slot(level: u32, c: vec3<i32>) -> i32 {
    let edge = BRICK_EDGE >> level;
    let cell = vec3<u32>(c);
    let slot = brick_slot(cell / edge);
    if (slot == 0u) {
        return MG_OPEN;
    }
    let local = cell % edge;
    return i32(lv_base(level) + slot * edge * edge * edge + (local.z * edge + local.y) * edge + local.x);
}

fn grid_group_count() -> u32 {
    return brick_count() * 8u;
}

fn grid_group_index(group: vec3<u32>) -> u32 {
    return group.x;
}

// Closed only when every brick is stored: an inactive one is open air.
fn grid_fully_closed() -> bool {
    let g = brick_grid();
    return open_sides() == 0u && brick_count() == g * g * g;
}

// ---- Allocation ----

fn scratch_occupied(slot: u32) -> u32 { return SCRATCH_HEADER + slot; }
fn scratch_freed(slot: u32) -> u32 { return SCRATCH_HEADER + brick_capacity() + slot; }
fn scratch_fresh(slot: u32) -> u32 { return SCRATCH_HEADER + 2u * brick_capacity() + slot; }
fn scratch_free_list(rank: u32) -> u32 { return SCRATCH_HEADER + 3u * brick_capacity() + rank; }
fn scratch_new(linear: u32) -> u32 { return SCRATCH_HEADER + 4u * brick_capacity() + linear; }
fn brick_total() -> u32 {
    let g = brick_grid();
    return g * g * g;
}
fn scratch_new_prefix(linear: u32) -> u32 { return scratch_new(brick_total()) + linear; }
fn scratch_block(block: u32) -> u32 { return scratch_new(2u * brick_total()) + block; }
fn brick_blocks() -> u32 { return (brick_total() + 63u) / 64u; }

// Whether the listed brick's cells hold fluid: one workgroup a brick, 8 cells an invocation, each
// counted when it holds more than the threshold. (Two entries, so a smoke-only stage never binds the
// fire grids.)
fn brick_mark_occupied(local: u32, slot: u32, held: f32) {
    let total = aestra_workgroup_sum(local, vec4<f32>(held, 0.0, 0.0, 0.0));
    if (local == 0u) {
        brick_scratch[scratch_occupied(slot)] = select(0u, 1u, total.x > 0.0);
    }
}

@compute @workgroup_size(64)
fn brick_activity(@builtin(local_invocation_index) local: u32, @builtin(workgroup_id) group: vec3<u32>) {
    let slot = brick_list(group.x);
    var held = 0.0;
    for (var k = 0u; k < 8u; k += 1u) {
        let i = slot * BRICK_CELLS + local * 8u + k;
        held += select(0.0, 1.0, max(density[i], 0.0) > brick_threshold());
    }
    brick_mark_occupied(local, slot, held);
}

@compute @workgroup_size(64)
fn brick_activity_fire(@builtin(local_invocation_index) local: u32, @builtin(workgroup_id) group: vec3<u32>) {
    let slot = brick_list(group.x);
    var held = 0.0;
    for (var k = 0u; k < 8u; k += 1u) {
        let i = slot * BRICK_CELLS + local * 8u + k;
        let amount = max(density[i], 0.0) + max(temperature[i], 0.0) + max(fuel[i], 0.0);
        held += select(0.0, 1.0, amount > brick_threshold());
    }
    brick_mark_occupied(local, slot, held);
}

// Whether a source's sphere reaches into brick `brick`.
fn brick_touches_source(brick: vec3<u32>) -> bool {
    let h = cell_size() * f32(BRICK_EDGE);
    let low = grid_origin() + vec3<f32>(brick) * h;
    let high = low + vec3<f32>(h);
    for (var s = 0u; s < source_count(); s += 1u) {
        let base = SOURCE_BASE + s * SOURCE_WORDS;
        let center = source_vec3(base, 0u, 8u, 1.0);
        let nearest = clamp(center, low, high);
        if (length(nearest - center) < bitcast<f32>(constants[base + 3u])) {
            return true;
        }
    }
    return false;
}

@compute @workgroup_size(64)
fn brick_need(@builtin(local_invocation_index) local: u32, @builtin(workgroup_id) group: vec3<u32>) {
    let linear = group.x * 64u + local;
    var fresh = 0u;
    if (linear < brick_total()) {
        let g = brick_grid();
        let brick = vec3<u32>(linear % g, (linear / g) % g, linear / (g * g));
        var needed = brick_threshold() < 0.0 || brick_touches_source(brick);
        let b = vec3<i32>(brick);
        for (var dz = -1; dz <= 1; dz += 1) {
            for (var dy = -1; dy <= 1; dy += 1) {
                for (var dx = -1; dx <= 1; dx += 1) {
                    let n = b + vec3<i32>(dx, dy, dz);
                    if (all(n >= vec3<i32>(0)) && all(n < vec3<i32>(i32(g)))) {
                        let slot = brick_slot(vec3<u32>(n));
                        if (slot != 0u && brick_scratch[scratch_occupied(slot)] != 0u) {
                            needed = true;
                        }
                    }
                }
            }
        }
        let slot = bricks[brick_table_word(linear)];
        if (needed && slot == 0u) {
            fresh = 1u;
        }
        if (!needed && slot != 0u) {
            brick_scratch[scratch_freed(slot)] = 1u;
        }
        brick_scratch[scratch_new(linear)] = fresh;
    }
    let scanned = aestra_workgroup_scan(local, fresh);
    if (linear < brick_total()) {
        brick_scratch[scratch_new_prefix(linear)] = scanned.x;
    }
    if (local == 0u) {
        brick_scratch[scratch_block(group.x)] = scanned.y;
    }
}

// One workgroup: the new bricks' block offsets, then the free slots in slot order.
@compute @workgroup_size(64)
fn brick_plan(@builtin(local_invocation_index) local: u32) {
    var carry = 0u;
    for (var base = 0u; base < brick_blocks(); base += 64u) {
        let block = base + local;
        var total = 0u;
        if (block < brick_blocks()) {
            total = brick_scratch[scratch_block(block)];
        }
        let scanned = aestra_workgroup_scan(local, total);
        if (block < brick_blocks()) {
            brick_scratch[scratch_block(block)] = carry + scanned.x;
        }
        carry += scanned.y;
    }
    var free_total = 0u;
    for (var base = 0u; base < brick_capacity(); base += 64u) {
        let slot = base + local;
        var free = 0u;
        if (slot > 0u && slot < brick_capacity()) {
            let unused = bricks[brick_coord_word(slot)] == 0u;
            free = select(0u, 1u, unused || brick_scratch[scratch_freed(slot)] != 0u);
        }
        let scanned = aestra_workgroup_scan(local, free);
        if (free != 0u) {
            brick_scratch[scratch_free_list(free_total + scanned.x)] = slot;
        }
        free_total += scanned.y;
    }
    if (local == 0u) {
        brick_scratch[0] = carry;
        brick_scratch[1] = free_total;
    }
}

@compute @workgroup_size(64)
fn brick_assign(@builtin(local_invocation_index) local: u32, @builtin(workgroup_id) group: vec3<u32>) {
    let linear = group.x * 64u + local;
    if (linear >= brick_total()) {
        return;
    }
    let slot = bricks[brick_table_word(linear)];
    if (slot != 0u && brick_scratch[scratch_freed(slot)] != 0u) {
        bricks[brick_table_word(linear)] = 0u;
    }
    if (brick_scratch[scratch_new(linear)] != 0u) {
        let rank = brick_scratch[scratch_block(group.x)] + brick_scratch[scratch_new_prefix(linear)];
        if (rank < brick_scratch[1]) {
            let given = brick_scratch[scratch_free_list(rank)];
            let g = brick_grid();
            let brick = vec3<u32>(linear % g, (linear / g) % g, linear / (g * g));
            bricks[brick_table_word(linear)] = given;
            bricks[brick_coord_word(given)] = pack_brick(brick);
            brick_scratch[scratch_fresh(given)] = 1u;
        }
    }
}

// One workgroup: slots freed and not given again become free; the others, in slot order, the list.
@compute @workgroup_size(64)
fn brick_compact(@builtin(local_invocation_index) local: u32) {
    var count = 0u;
    for (var base = 0u; base < brick_capacity(); base += 64u) {
        let slot = base + local;
        var live = 0u;
        if (slot > 0u && slot < brick_capacity()) {
            let freed = brick_scratch[scratch_freed(slot)] != 0u;
            if (freed && brick_scratch[scratch_fresh(slot)] == 0u) {
                bricks[brick_coord_word(slot)] = 0u;
            }
            live = select(0u, 1u, bricks[brick_coord_word(slot)] != 0u);
        }
        let scanned = aestra_workgroup_scan(local, live);
        if (live != 0u) {
            bricks[BRICK_HEADER + count + scanned.x] = slot;
        }
        count += scanned.y;
    }
    if (local == 0u) {
        bricks[0] = count;
        // Workgroups of 64 cells: 8 a brick on the fine level, 1 on level 1, then 8 and 64 bricks
        // a workgroup on levels 2 and 3.
        brick_dispatch[0] = count * 8u;
        brick_dispatch[4] = count;
        brick_dispatch[8] = (count + 7u) / 8u;
        brick_dispatch[12] = (count + 63u) / 64u;
        for (var level = 0u; level < 4u; level += 1u) {
            brick_dispatch[level * 4u + 1u] = 1u;
            brick_dispatch[level * 4u + 2u] = 1u;
        }
    }
}

// A slot given to a new brick this tick starts as still, empty air: the index of such a cell, or none.
fn fresh_cell(gid: vec3<u32>) -> i32 {
    let cell = grid_cell(gid);
    if (!in_grid(cell)) {
        return -1;
    }
    let i = cell_index(cell);
    if (brick_scratch[scratch_fresh(i / BRICK_CELLS)] == 0u) {
        return -1;
    }
    return i32(i);
}

@compute @workgroup_size(4, 4, 4)
fn brick_zero(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = fresh_cell(gid);
    if (i < 0) {
        return;
    }
    velocity[i] = vec4<f32>(0.0);
    density[i] = 0.0;
    pressure[i] = 0.0;
}

@compute @workgroup_size(4, 4, 4)
fn brick_zero_fire(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = fresh_cell(gid);
    if (i < 0) {
        return;
    }
    velocity[i] = vec4<f32>(0.0);
    density[i] = 0.0;
    pressure[i] = 0.0;
    temperature[i] = 0.0;
    fuel[i] = 0.0;
}
