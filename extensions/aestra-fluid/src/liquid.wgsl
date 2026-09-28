// Aestra Fluid — a free-surface liquid (fluid F8): APIC particles on the solver's MAC grid (Jiang et
// al. 2015). Composed after `solver.wgsl`, the dense grid and `pressure.wgsl`, whose bindings,
// constants and pressure solve it shares.
//
// The liquid is its particles — the stage's own resource, not an emitter's — each carrying a position,
// a velocity and the affine matrix C of APIC (its rows c_x, c_y, c_z). A step, substepped:
// - particle to grid: each particle adds w·(v_a + c_a·(x_f − x_p)) and w to the 8 faces around it,
//   per component, with trilinear weights — in fixed point, with integer atomics, so the sums do not
//   depend on the order the particles arrive in and a rerun reproduces the same bits;
// - the faces' velocities are the weighted means, gravity is added, and each cell is fluid (it holds
//   a particle), solid (a collider) or air;
// - the pressure solve (`pressure.wgsl`) makes the fluid incompressible with p = 0 in the air;
// - grid to particle: each particle takes the interpolated velocity and, as C, its gradient, then
//   moves; walls and colliders push it back inside.
// Particles are emitted, never removed: blocks fill at tick 0, sources emit at a rate, up to the
// budget. Slots past the count stay empty (their position's w is 0), so a pass may run over more.

@group(0) @binding(32) var<storage, read_write> liquid_particles: array<vec4<f32>>;
// [count, the first slot this tick emits into].
@group(0) @binding(33) var<storage, read_write> liquid_header: array<u32>;
// Workgroup counts: this tick's emission (x, y, z, -), then every particle (x, y, z, -).
@group(0) @binding(34) var<storage, read_write> liquid_dispatch: array<u32>;
// Per cell: momentum x, y, z and weight x, y, z of its minimum faces, the particles whose centre it
// holds, and the liquid splatted at its centre — fixed point, cleared every substep.
@group(0) @binding(35) var<storage, read_write> liquid_transfer: array<atomic<i32>>;

const LIQUID_STRIDE: u32 = 5u;
const LIQUID_CELL_WORDS: u32 = 8u;
// Fixed-point scale of the transfer sums: 1/1024 of a unit, with room for thousands of units per
// second summed over dozens of particles.
const LIQUID_FIXED: f32 = 1024.0;
const LIQUID_MAX_SPEED: f32 = 100000.0;
// Particles a cell holds when full: blocks seed 2³ a cell.
const LIQUID_PER_CELL: f32 = 8.0;
const LIQUID_BLOCK_WORDS: u32 = 12u;

// The liquid block of the constants: particle budget, substeps, gravity, the blocks.
fn liquid_budget() -> u32 { return constants[liquid_base()]; }
fn liquid_substeps() -> u32 { return max(constants[liquid_base() + 1u], 1u); }
fn liquid_gravity() -> vec3<f32> {
    let b = liquid_base() + 2u;
    return vec3<f32>(bitcast<f32>(constants[b]), bitcast<f32>(constants[b + 1u]), bitcast<f32>(constants[b + 2u]));
}
fn liquid_block_count() -> u32 { return constants[liquid_base() + 5u]; }
fn liquid_block(block: u32) -> u32 { return liquid_base() + 6u + block * LIQUID_BLOCK_WORDS; }
fn liquid_block_particles(block: u32) -> u32 { return constants[liquid_block(block) + 9u]; }
fn liquid_dt() -> f32 { return frame_dt() / f32(liquid_substeps()); }

fn fixed(value: f32) -> i32 {
    return i32(round(clamp(value, -LIQUID_MAX_SPEED, LIQUID_MAX_SPEED) * LIQUID_FIXED));
}

// A source emits `rate` particles a second (its density-rate word): this tick's share.
fn liquid_source_quota(source: u32) -> u32 {
    let rate = max(bitcast<f32>(constants[SOURCE_BASE + source * SOURCE_WORDS + 7u]), 0.0);
    let tick = f32(frame[0]);
    let before = floor(rate * tick * frame_dt());
    let after = floor(rate * (tick + 1.0) * frame_dt());
    return u32(max(after - before, 0.0));
}

fn liquid_hash01(ordinal: u32, salt: u32) -> f32 {
    return f32(hash_u32(ordinal * 0x9e3779b9u ^ hash_u32(salt ^ frame_seed())) >> 8u) / 16777216.0;
}

// This tick's emission: its size, capped by the budget, and the workgroups of every pass.
@compute @workgroup_size(1)
fn liquid_plan() {
    let count = liquid_header[0];
    var quota = 0u;
    if (frame[0] == 0u) {
        for (var b = 0u; b < liquid_block_count(); b += 1u) {
            quota += liquid_block_particles(b);
        }
    }
    for (var s = 0u; s < source_count(); s += 1u) {
        quota += liquid_source_quota(s);
    }
    let total = min(count + min(quota, liquid_budget()), liquid_budget());
    liquid_header[0] = total;
    liquid_header[1] = count;
    liquid_dispatch[0] = (total - count + 63u) / 64u;
    liquid_dispatch[1] = 1u;
    liquid_dispatch[2] = 1u;
    liquid_dispatch[4] = (total + 63u) / 64u;
    liquid_dispatch[5] = 1u;
    liquid_dispatch[6] = 1u;
}

fn liquid_write(slot: u32, position: vec3<f32>, velocity: vec3<f32>) {
    let at = slot * LIQUID_STRIDE;
    liquid_particles[at] = vec4<f32>(position, 1.0);
    liquid_particles[at + 1u] = vec4<f32>(velocity, 0.0);
    liquid_particles[at + 2u] = vec4<f32>(0.0);
    liquid_particles[at + 3u] = vec4<f32>(0.0);
    liquid_particles[at + 4u] = vec4<f32>(0.0);
}

// A block's `index`-th particle: 2³ a cell of its lattice, each jittered within its octant.
fn liquid_emit_block(block: u32, index: u32, slot: u32) {
    let b = liquid_block(block);
    let low = vec3<f32>(bitcast<f32>(constants[b]), bitcast<f32>(constants[b + 1u]), bitcast<f32>(constants[b + 2u]));
    let velocity = vec3<f32>(bitcast<f32>(constants[b + 3u]), bitcast<f32>(constants[b + 4u]), bitcast<f32>(constants[b + 5u]));
    let dims = vec3<u32>(constants[b + 6u], constants[b + 7u], constants[b + 8u]);
    let cell_number = index / 8u;
    let octant = index % 8u;
    let cell = vec3<u32>(cell_number % dims.x, (cell_number / dims.x) % dims.y, cell_number / (dims.x * dims.y));
    let sub = vec3<f32>(f32(octant & 1u), f32((octant >> 1u) & 1u), f32(octant >> 2u));
    let jitter = vec3<f32>(liquid_hash01(slot, 1u), liquid_hash01(slot, 2u), liquid_hash01(slot, 3u));
    let position = low + (vec3<f32>(cell) + 0.25 + 0.5 * sub + 0.4 * (jitter - 0.5)) * cell_size();
    liquid_write(slot, position, velocity);
}

// A source's particle: uniform in its sphere, at its velocity.
fn liquid_emit_source(source: u32, slot: u32) {
    let base = SOURCE_BASE + source * SOURCE_WORDS;
    let radius = bitcast<f32>(constants[base + 3u]);
    let r = radius * pow(liquid_hash01(slot, 4u), 1.0 / 3.0);
    let z = 2.0 * liquid_hash01(slot, 5u) - 1.0;
    let angle = 6.28318530718 * liquid_hash01(slot, 6u);
    let ring = sqrt(max(1.0 - z * z, 0.0));
    let direction = vec3<f32>(ring * cos(angle), z, ring * sin(angle));
    let position = source_vec3(base, 0u, 8u, 1.0) + direction * r;
    liquid_write(slot, position, source_vec3(base, 4u, 11u, 0.0));
}

@compute @workgroup_size(64)
fn liquid_emit(@builtin(global_invocation_id) gid: vec3<u32>) {
    let slot = liquid_header[1] + gid.x;
    if (slot >= liquid_header[0]) {
        return;
    }
    var k = gid.x;
    if (frame[0] == 0u) {
        for (var b = 0u; b < liquid_block_count(); b += 1u) {
            let n = liquid_block_particles(b);
            if (k < n) {
                liquid_emit_block(b, k, slot);
                return;
            }
            k -= n;
        }
    }
    for (var s = 0u; s < source_count(); s += 1u) {
        let n = liquid_source_quota(s);
        if (k < n) {
            liquid_emit_source(s, slot);
            return;
        }
        k -= n;
    }
}

@compute @workgroup_size(4, 4, 4)
fn liquid_clear(@builtin(global_invocation_id) gid: vec3<u32>) {
    let cell = grid_cell(gid);
    if (!in_grid(cell)) {
        return;
    }
    let base = cell_index(cell) * LIQUID_CELL_WORDS;
    for (var k = 0u; k < LIQUID_CELL_WORDS; k += 1u) {
        atomicStore(&liquid_transfer[base + k], 0);
    }
}

// A particle's position in cell coordinates (cell centres at integers).
fn liquid_cell_position(x: vec3<f32>) -> vec3<f32> {
    return (x - grid_origin()) / cell_size() - vec3<f32>(0.5);
}

fn liquid_corner_weight(t: vec3<f32>, corner: vec3<u32>) -> f32 {
    let w = select(vec3<f32>(1.0) - t, t, corner == vec3<u32>(1u));
    return w.x * w.y * w.z;
}

fn liquid_corner(corner: u32) -> vec3<u32> {
    return vec3<u32>(corner & 1u, (corner >> 1u) & 1u, corner >> 2u);
}

@compute @workgroup_size(64)
fn liquid_p2g(@builtin(global_invocation_id) gid: vec3<u32>) {
    let at = gid.x * LIQUID_STRIDE;
    if (at >= arrayLength(&liquid_particles) || liquid_particles[at].w < 0.5) {
        return;
    }
    let x = liquid_particles[at].xyz;
    let v = liquid_particles[at + 1u].xyz;
    let p = liquid_cell_position(x);
    let n = i32(grid_res());
    let h = cell_size();
    // The cell holding the particle's centre is fluid; the liquid splats onto the cell centres.
    let home = vec3<i32>(floor(p + vec3<f32>(0.5)));
    if (inside(home)) {
        atomicAdd(&liquid_transfer[cell_index(vec3<u32>(home)) * LIQUID_CELL_WORDS + 6u], 1);
    }
    let splat = floor(p);
    for (var corner = 0u; corner < 8u; corner += 1u) {
        let offset = liquid_corner(corner);
        let c = vec3<i32>(splat) + vec3<i32>(offset);
        if (inside(c)) {
            let w = liquid_corner_weight(p - splat, offset);
            atomicAdd(&liquid_transfer[cell_index(vec3<u32>(c)) * LIQUID_CELL_WORDS + 7u], fixed(w));
        }
    }
    // Each velocity component on its own faces: component a of cell f sits at f − ½e_a.
    for (var axis = 0u; axis < 3u; axis += 1u) {
        let q = p + 0.5 * axis_offset(axis);
        let base = floor(q);
        let t = q - base;
        let c_row = liquid_particles[at + 2u + axis].xyz;
        let speed = component(vec4<f32>(v, 0.0), axis);
        for (var corner = 0u; corner < 8u; corner += 1u) {
            let offset = liquid_corner(corner);
            let f = vec3<i32>(base) + vec3<i32>(offset);
            if (all(f >= vec3<i32>(0)) && all(f < vec3<i32>(n))) {
                let w = liquid_corner_weight(t, offset);
                let value = speed + dot(c_row, (vec3<f32>(f) - q) * h);
                let word = cell_index(vec3<u32>(f)) * LIQUID_CELL_WORDS;
                atomicAdd(&liquid_transfer[word + axis], fixed(w * value));
                atomicAdd(&liquid_transfer[word + 3u + axis], fixed(w));
            }
        }
    }
}

// The faces' velocities (weighted means, plus gravity), the cell's state for the pressure solve, and
// the liquid fraction the look draws. Air and solid cells hold no pressure.
@compute @workgroup_size(4, 4, 4)
fn liquid_mark(@builtin(global_invocation_id) gid: vec3<u32>) {
    let cell = grid_cell(gid);
    if (!in_grid(cell)) {
        return;
    }
    let i = cell_index(cell);
    let base = i * LIQUID_CELL_WORDS;
    let gravity = liquid_gravity() * liquid_dt();
    var faces = vec3<f32>(0.0);
    let mx = atomicLoad(&liquid_transfer[base]);
    let my = atomicLoad(&liquid_transfer[base + 1u]);
    let mz = atomicLoad(&liquid_transfer[base + 2u]);
    let wx = atomicLoad(&liquid_transfer[base + 3u]);
    let wy = atomicLoad(&liquid_transfer[base + 4u]);
    let wz = atomicLoad(&liquid_transfer[base + 5u]);
    if (wx > 0) { faces.x = f32(mx) / f32(wx) + gravity.x; }
    if (wy > 0) { faces.y = f32(my) / f32(wy) + gravity.y; }
    if (wz > 0) { faces.z = f32(mz) / f32(wz) + gravity.z; }
    velocity[i] = vec4<f32>(faces, 0.0);
    density[i] = liquid_smoothed_fraction(vec3<i32>(cell));
    var flag = MG_AIR;
    if (collider_count() > 0u && solid[i].w > 0.5) {
        flag = MG_SOLID;
    } else if (atomicLoad(&liquid_transfer[base + 6u]) > 0) {
        flag = MG_FLUID;
    }
    mg_flags[u32(lv_slot(0u, vec3<i32>(cell)))] = flag;
    if (flag != MG_FLUID) {
        pressure[i] = 0.0;
    }
}

// The liquid fraction the look draws at `c`: the particles' splat, smoothed by a 3³ tent (weights 8,
// 4, 2, 1 by distance, over 64) so the surface shows the liquid's shape rather than its particles.
fn liquid_smoothed_fraction(c: vec3<i32>) -> f32 {
    var sum = 0;
    var weights = 0;
    for (var dz = -1; dz <= 1; dz += 1) {
        for (var dy = -1; dy <= 1; dy += 1) {
            for (var dx = -1; dx <= 1; dx += 1) {
                let n = c + vec3<i32>(dx, dy, dz);
                if (inside(n)) {
                    let weight = 8 >> u32(abs(dx) + abs(dy) + abs(dz));
                    sum += weight * atomicLoad(&liquid_transfer[cell_index(vec3<u32>(n)) * LIQUID_CELL_WORDS + 7u]);
                    weights += weight;
                }
            }
        }
    }
    return f32(sum) / (f32(weights) * LIQUID_FIXED * LIQUID_PER_CELL);
}

// The projected face velocity of component `axis` at face `f` (0 past the grid: a closed wall).
fn liquid_face(f: vec3<i32>, axis: u32) -> f32 {
    if (any(f < vec3<i32>(0)) || any(f >= vec3<i32>(i32(grid_res())))) {
        return 0.0;
    }
    return component(velocity[cell_index(vec3<u32>(f))], axis);
}

struct LiquidMotion {
    position: vec3<f32>,
    velocity: vec3<f32>,
}

// Component `axis` of the grid velocity at `p` (xyz: its gradient in world units, w: the value) —
// what the particle takes as its velocity and as that row of C.
fn liquid_sample(p: vec3<f32>, axis: u32) -> vec4<f32> {
    let q = p + 0.5 * axis_offset(axis);
    let base = floor(q);
    let t = q - base;
    var value = 0.0;
    var gradient = vec3<f32>(0.0);
    for (var corner = 0u; corner < 8u; corner += 1u) {
        let offset = liquid_corner(corner);
        let face = liquid_face(vec3<i32>(base) + vec3<i32>(offset), axis);
        let w3 = select(vec3<f32>(1.0) - t, t, offset == vec3<u32>(1u));
        let sign = select(vec3<f32>(-1.0), vec3<f32>(1.0), offset == vec3<u32>(1u));
        value += w3.x * w3.y * w3.z * face;
        gradient += face * sign * vec3<f32>(w3.y * w3.z, w3.x * w3.z, w3.x * w3.y);
    }
    return vec4<f32>(gradient / cell_size(), value);
}

// Pushes a particle out of the colliders it entered and takes away the velocity it has into them
// (relative to the collider's own).
fn liquid_collide(position: vec3<f32>, velocity_in: vec3<f32>) -> LiquidMotion {
    var x = position;
    var v = velocity_in;
    for (var c = 0u; c < collider_count(); c += 1u) {
        let base = collider_base() + c * COLLIDER_WORDS;
        let d = collider_distance(base, x);
        if (d < 0.0) {
            let e = 0.01 * cell_size();
            let normal = normalize(vec3<f32>(
                collider_distance(base, x + vec3<f32>(e, 0.0, 0.0)) - collider_distance(base, x - vec3<f32>(e, 0.0, 0.0)),
                collider_distance(base, x + vec3<f32>(0.0, e, 0.0)) - collider_distance(base, x - vec3<f32>(0.0, e, 0.0)),
                collider_distance(base, x + vec3<f32>(0.0, 0.0, e)) - collider_distance(base, x - vec3<f32>(0.0, 0.0, e)),
            ) + vec3<f32>(0.0, 1e-9, 0.0));
            x = x - normal * d;
            let solid_velocity = source_vec3(base, 7u, 10u, 0.0);
            let into = dot(v - solid_velocity, normal);
            if (into < 0.0) {
                v = v - normal * into;
            }
        }
    }
    return LiquidMotion(x, v);
}

@compute @workgroup_size(64)
fn liquid_g2p(@builtin(global_invocation_id) gid: vec3<u32>) {
    let at = gid.x * LIQUID_STRIDE;
    if (at >= arrayLength(&liquid_particles) || liquid_particles[at].w < 0.5) {
        return;
    }
    let x = liquid_particles[at].xyz;
    let p = liquid_cell_position(x);
    let h = cell_size();
    let sx = liquid_sample(p, 0u);
    let sy = liquid_sample(p, 1u);
    let sz = liquid_sample(p, 2u);
    var v = vec3<f32>(sx.w, sy.w, sz.w);
    // Walls: the particle stays a quarter cell inside the box, without velocity into the wall.
    let moved = x + v * liquid_dt();
    let low = grid_origin() + vec3<f32>(0.25 * h);
    let high = grid_origin() + vec3<f32>(f32(grid_res()) * h - 0.25 * h);
    let into_wall = ((moved < low) & (v < vec3<f32>(0.0))) | ((moved > high) & (v > vec3<f32>(0.0)));
    v = select(v, vec3<f32>(0.0), into_wall);
    let collided = liquid_collide(clamp(moved, low, high), v);
    liquid_particles[at] = vec4<f32>(collided.position, 1.0);
    liquid_particles[at + 1u] = vec4<f32>(collided.velocity, 0.0);
    liquid_particles[at + 2u] = vec4<f32>(sx.xyz, 0.0);
    liquid_particles[at + 3u] = vec4<f32>(sy.xyz, 0.0);
    liquid_particles[at + 4u] = vec4<f32>(sz.xyz, 0.0);
}
