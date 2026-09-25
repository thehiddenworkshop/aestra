// Aestra Fluid — a small stable-fluids solver on an N³ grid (extensible-stages M13).
//
// Every pass is a gather: each invocation writes only its own cell, so there are no atomics and a
// rerun with the same inputs reproduces the same bits. Bindings follow the Execution IR convention —
// the stage block declares its resources in exactly this order (`RESOURCES` in lib.rs).

@group(0) @binding(0) var<storage, read_write> velocity: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> density: array<f32>;
@group(0) @binding(2) var<storage, read_write> velocity_next: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> density_next: array<f32>;
@group(0) @binding(4) var<storage, read_write> pressure: array<f32>;
@group(0) @binding(5) var<storage, read_write> pressure_next: array<f32>;
@group(0) @binding(6) var<storage, read_write> divergence: array<f32>;
@group(0) @binding(7) var<storage, read_write> vorticity: array<vec4<f32>>;
@group(0) @binding(8) var<storage, read> constants: array<u32>;
@group(0) @binding(9) var<storage, read> frame: array<u32>;
@group(0) @binding(10) var<storage, read> aestra_host_bindings: array<u32>;

// Stage-constant layout, packed by the stage lowerer (`pack_constants` in lib.rs).
const NO_SLOT: u32 = 0xffffffffu;
const SOURCE_BASE: u32 = 10u;
const SOURCE_WORDS: u32 = 16u;

fn grid_res() -> u32 { return constants[0]; }
fn cell_size() -> f32 { return bitcast<f32>(constants[1]); }
fn grid_origin() -> vec3<f32> {
    return vec3<f32>(bitcast<f32>(constants[2]), bitcast<f32>(constants[3]), bitcast<f32>(constants[4]));
}
fn density_dissipation() -> f32 { return bitcast<f32>(constants[5]); }
fn velocity_dissipation() -> f32 { return bitcast<f32>(constants[6]); }
fn buoyancy_strength() -> f32 { return bitcast<f32>(constants[7]); }
fn confinement_strength() -> f32 { return bitcast<f32>(constants[8]); }
fn source_count() -> u32 { return constants[9]; }
fn frame_dt() -> f32 { return bitcast<f32>(frame[1]); }

fn in_grid(cell: vec3<u32>) -> bool {
    return all(cell < vec3<u32>(grid_res()));
}

fn cell_index(cell: vec3<u32>) -> u32 {
    let n = grid_res();
    return (cell.z * n + cell.y) * n + cell.x;
}

// The index of `cell` clamped into the grid — neighbours past a wall read the wall cell.
fn clamped_index(cell: vec3<i32>) -> u32 {
    let last = i32(grid_res()) - 1;
    return cell_index(vec3<u32>(clamp(cell, vec3<i32>(0), vec3<i32>(last))));
}

fn is_wall(cell: vec3<u32>) -> bool {
    let last = grid_res() - 1u;
    return any(cell == vec3<u32>(0u)) || any(cell == vec3<u32>(last));
}

const X: vec3<i32> = vec3<i32>(1, 0, 0);
const Y: vec3<i32> = vec3<i32>(0, 1, 0);
const Z: vec3<i32> = vec3<i32>(0, 0, 1);

// A source vec3 parameter: the authored value, or the host binding field it is bound to when that
// field is present this tick.
fn source_vec3(base: u32, value_word: u32, ref_word: u32) -> vec3<f32> {
    let fallback = vec3<f32>(
        bitcast<f32>(constants[base + value_word]),
        bitcast<f32>(constants[base + value_word + 1u]),
        bitcast<f32>(constants[base + value_word + 2u]),
    );
    let slot = constants[base + ref_word];
    if (slot == NO_SLOT || !aestra_binding_present(slot, constants[base + ref_word + 1u])) {
        return fallback;
    }
    return aestra_binding_vec3(slot, constants[base + ref_word + 2u]);
}

// Injects density and velocity from every source, then applies buoyancy.
@compute @workgroup_size(4, 4, 4)
fn add_sources(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) { return; }
    let i = cell_index(cell);
    let delta = frame_dt();
    let center = grid_origin() + (vec3<f32>(cell) + vec3<f32>(0.5)) * cell_size();
    var d = density[i];
    var v = velocity[i].xyz;
    for (var s = 0u; s < source_count(); s += 1u) {
        let base = SOURCE_BASE + s * SOURCE_WORDS;
        let radius = bitcast<f32>(constants[base + 3u]);
        let distance_to_source = length(center - source_vec3(base, 0u, 8u));
        if (distance_to_source < radius) {
            let falloff = 1.0 - distance_to_source / radius;
            d += bitcast<f32>(constants[base + 7u]) * falloff * delta;
            let emitted = source_vec3(base, 4u, 11u);
            v += (emitted - v) * clamp(falloff * delta * 8.0, 0.0, 1.0);
        }
    }
    v.y += buoyancy_strength() * d * delta;
    density[i] = d;
    velocity[i] = vec4<f32>(v, 0.0);
}

// Vorticity ω = ∇ × u (central differences); `w` holds |ω|.
@compute @workgroup_size(4, 4, 4)
fn compute_vorticity(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) { return; }
    let c = vec3<i32>(cell);
    let xp = velocity[clamped_index(c + X)].xyz;
    let xm = velocity[clamped_index(c - X)].xyz;
    let yp = velocity[clamped_index(c + Y)].xyz;
    let ym = velocity[clamped_index(c - Y)].xyz;
    let zp = velocity[clamped_index(c + Z)].xyz;
    let zm = velocity[clamped_index(c - Z)].xyz;
    let w = vec3<f32>(
        (yp.z - ym.z) - (zp.y - zm.y),
        (zp.x - zm.x) - (xp.z - xm.z),
        (xp.y - xm.y) - (yp.x - ym.x),
    ) * (0.5 / cell_size());
    vorticity[cell_index(cell)] = vec4<f32>(w, length(w));
}

// Vorticity confinement: pushes velocity along N × ω, N = ∇|ω| / |∇|ω||.
@compute @workgroup_size(4, 4, 4)
fn confine_vorticity(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) { return; }
    let c = vec3<i32>(cell);
    let i = cell_index(cell);
    let eta = vec3<f32>(
        vorticity[clamped_index(c + X)].w - vorticity[clamped_index(c - X)].w,
        vorticity[clamped_index(c + Y)].w - vorticity[clamped_index(c - Y)].w,
        vorticity[clamped_index(c + Z)].w - vorticity[clamped_index(c - Z)].w,
    ) * (0.5 / cell_size());
    let magnitude = length(eta);
    if (magnitude < 1e-6) { return; }
    let force = confinement_strength() * cell_size() * cross(eta / magnitude, vorticity[i].xyz);
    velocity[i] = vec4<f32>(velocity[i].xyz + force * frame_dt(), 0.0);
}

// Trilinear sample positions, in cell coordinates clamped to the grid.
struct Trilinear {
    corners: array<u32, 8>,
    t: vec3<f32>,
}

fn trilinear(position: vec3<f32>) -> Trilinear {
    let last = f32(grid_res() - 1u);
    let p = clamp(position, vec3<f32>(0.0), vec3<f32>(last));
    let base = floor(p);
    let b = vec3<i32>(base);
    var corners: array<u32, 8>;
    corners[0] = clamped_index(b);
    corners[1] = clamped_index(b + X);
    corners[2] = clamped_index(b + Y);
    corners[3] = clamped_index(b + X + Y);
    corners[4] = clamped_index(b + Z);
    corners[5] = clamped_index(b + X + Z);
    corners[6] = clamped_index(b + Y + Z);
    corners[7] = clamped_index(b + X + Y + Z);
    return Trilinear(corners, p - base);
}

// Where the fluid at `cell` came from one step ago (semi-Lagrangian backtrace), in cell coordinates.
fn backtrace(cell: vec3<u32>) -> vec3<f32> {
    return vec3<f32>(cell) - velocity[cell_index(cell)].xyz * (frame_dt() / cell_size());
}

@compute @workgroup_size(4, 4, 4)
fn advect_velocity(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) { return; }
    let s = trilinear(backtrace(cell));
    let x0 = mix(velocity[s.corners[0]].xyz, velocity[s.corners[1]].xyz, s.t.x);
    let x1 = mix(velocity[s.corners[2]].xyz, velocity[s.corners[3]].xyz, s.t.x);
    let x2 = mix(velocity[s.corners[4]].xyz, velocity[s.corners[5]].xyz, s.t.x);
    let x3 = mix(velocity[s.corners[6]].xyz, velocity[s.corners[7]].xyz, s.t.x);
    let sampled = mix(mix(x0, x1, s.t.y), mix(x2, x3, s.t.y), s.t.z);
    let decay = 1.0 / (1.0 + velocity_dissipation() * frame_dt());
    velocity_next[cell_index(cell)] = vec4<f32>(sampled * decay, 0.0);
}

@compute @workgroup_size(4, 4, 4)
fn advect_density(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) { return; }
    let s = trilinear(backtrace(cell));
    let x0 = mix(density[s.corners[0]], density[s.corners[1]], s.t.x);
    let x1 = mix(density[s.corners[2]], density[s.corners[3]], s.t.x);
    let x2 = mix(density[s.corners[4]], density[s.corners[5]], s.t.x);
    let x3 = mix(density[s.corners[6]], density[s.corners[7]], s.t.x);
    let sampled = mix(mix(x0, x1, s.t.y), mix(x2, x3, s.t.y), s.t.z);
    let decay = 1.0 / (1.0 + density_dissipation() * frame_dt());
    density_next[cell_index(cell)] = sampled * decay;
}

@compute @workgroup_size(4, 4, 4)
fn compute_divergence(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) { return; }
    let c = vec3<i32>(cell);
    divergence[cell_index(cell)] = (
        velocity[clamped_index(c + X)].x - velocity[clamped_index(c - X)].x
        + velocity[clamped_index(c + Y)].y - velocity[clamped_index(c - Y)].y
        + velocity[clamped_index(c + Z)].z - velocity[clamped_index(c - Z)].z
    ) * (0.5 / cell_size());
}

// One Jacobi iteration of D·G p = D·u, where D and G are the central-difference divergence and
// gradient `compute_divergence` and `project` use. On a collocated grid that operator is the wide
// Laplacian (neighbours two cells away, spacing 2h) — the compact 7-point one would leave a residual
// divergence no iteration count removes. Neumann walls via clamped neighbours.
@compute @workgroup_size(4, 4, 4)
fn relax_pressure(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) { return; }
    let c = vec3<i32>(cell);
    let neighbours = pressure[clamped_index(c + 2 * X)] + pressure[clamped_index(c - 2 * X)]
        + pressure[clamped_index(c + 2 * Y)] + pressure[clamped_index(c - 2 * Y)]
        + pressure[clamped_index(c + 2 * Z)] + pressure[clamped_index(c - 2 * Z)];
    let h = cell_size();
    pressure_next[cell_index(cell)] =
        (neighbours - 4.0 * h * h * divergence[cell_index(cell)]) / 6.0;
}

// Subtracts the pressure gradient; wall cells are no-slip.
@compute @workgroup_size(4, 4, 4)
fn project(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) { return; }
    let i = cell_index(cell);
    if (is_wall(cell)) {
        velocity[i] = vec4<f32>(0.0);
        return;
    }
    let c = vec3<i32>(cell);
    let gradient = vec3<f32>(
        pressure[clamped_index(c + X)] - pressure[clamped_index(c - X)],
        pressure[clamped_index(c + Y)] - pressure[clamped_index(c - Y)],
        pressure[clamped_index(c + Z)] - pressure[clamped_index(c - Z)],
    ) * (0.5 / cell_size());
    velocity[i] = vec4<f32>(velocity[i].xyz - gradient, 0.0);
}
