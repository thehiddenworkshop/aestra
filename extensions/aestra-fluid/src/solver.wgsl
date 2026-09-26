// Aestra Fluid — a stable-fluids solver on an N³ MAC grid (extensible-stages M13, fluid F4).
//
// Velocity is staggered: `velocity[cell].x` is the x-velocity on the cell's minimum x face, `.y` on its
// minimum y face, `.z` on its minimum z face. The faces on the domain's maximum boundary are implicit:
// zero on a closed side, extrapolated on an open one. Scalars (density, temperature, fuel, pressure)
// sit at cell centres. The pressure solve is the compact 7-point Laplacian — on a MAC grid it has no
// checkerboard modes — with closed sides as Neumann walls and open sides as p = 0 outside.
//
// Every pass is a gather: each invocation writes only its own cell, so there are no atomics and a
// rerun with the same inputs reproduces the same bits. Bindings follow the Execution IR convention —
// the stage block declares its resources in exactly this order (`resources` in lib.rs).

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
// Fire (fluid F3): declared only by a stage with a Combustion module, after every other resource, so
// a smoke-only block binds none of them and none of its passes reads them.
@group(0) @binding(11) var<storage, read_write> temperature: array<f32>;
@group(0) @binding(12) var<storage, read_write> temperature_next: array<f32>;
@group(0) @binding(13) var<storage, read_write> fuel: array<f32>;
@group(0) @binding(14) var<storage, read_write> fuel_next: array<f32>;

// Stage-constant layout, packed by the stage lowerer (`pack_constants` in lib.rs).
const NO_SLOT: u32 = 0xffffffffu;
const SOURCE_BASE: u32 = 12u;
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
// Open sides: bit 2·axis for the minimum side, 2·axis + 1 for the maximum side.
fn open_sides() -> u32 { return constants[10]; }
fn frame_dt() -> f32 { return bitcast<f32>(frame[1]); }
// The Combustion block follows the sources: ignition temperature, burn rate, heat release, smoke
// yield, cooling, thermal lift.
fn combustion(word: u32) -> f32 {
    return bitcast<f32>(constants[SOURCE_BASE + source_count() * SOURCE_WORDS + word]);
}

// World space into the effect's space (frame words 4..15, rows): the grid lives in the effect's space
// and moves with it; host inputs arrive in world space. `w` is 1 for a point, 0 for a vector.
// Rows are spelled out: a dynamically indexed vector write makes FXC (D3D12) unroll the enclosing
// loops, which fails for the per-source loop.
fn affine_row(base: u32, h: vec4<f32>) -> f32 {
    return bitcast<f32>(frame[base]) * h.x
        + bitcast<f32>(frame[base + 1u]) * h.y
        + bitcast<f32>(frame[base + 2u]) * h.z
        + bitcast<f32>(frame[base + 3u]) * h.w;
}

fn world_to_effect(v: vec3<f32>, w: f32) -> vec3<f32> {
    let h = vec4<f32>(v, w);
    return vec3<f32>(affine_row(4u, h), affine_row(8u, h), affine_row(12u, h));
}

fn in_grid(cell: vec3<u32>) -> bool {
    return all(cell < vec3<u32>(grid_res()));
}

fn inside(cell: vec3<i32>) -> bool {
    return all(cell >= vec3<i32>(0)) && all(cell < vec3<i32>(i32(grid_res())));
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

const X: vec3<i32> = vec3<i32>(1, 0, 0);
const Y: vec3<i32> = vec3<i32>(0, 1, 0);
const Z: vec3<i32> = vec3<i32>(0, 0, 1);

// The unit step along `axis` (0, 1, 2), and component `axis` of a vector — both spelled with selects:
// no dynamically indexed vectors (see `affine_row`).
fn axis_step(axis: u32) -> vec3<i32> {
    return select(select(Z, Y, axis == 1u), X, axis == 0u);
}

fn axis_offset(axis: u32) -> vec3<f32> {
    return vec3<f32>(axis_step(axis));
}

fn component(v: vec4<f32>, axis: u32) -> f32 {
    return select(select(v.z, v.y, axis == 1u), v.x, axis == 0u);
}

fn axis_coordinate(cell: vec3<i32>, axis: u32) -> i32 {
    return select(select(cell.z, cell.y, axis == 1u), cell.x, axis == 0u);
}

fn side_open(axis: u32, positive: bool) -> bool {
    let bit = axis * 2u + select(0u, 1u, positive);
    return ((open_sides() >> bit) & 1u) != 0u;
}

// The `axis` velocity on the minimum `axis` face of `cell`; `cell` may sit one past the grid on that
// axis — the implicit maximum boundary face: zero when that side is closed, extrapolated when open.
fn face(cell: vec3<i32>, axis: u32) -> f32 {
    if (axis_coordinate(cell, axis) >= i32(grid_res())) {
        if (side_open(axis, true)) {
            return component(velocity[clamped_index(cell - axis_step(axis))], axis);
        }
        return 0.0;
    }
    return component(velocity[clamped_index(cell)], axis);
}

// The velocity at a cell's centre: each component the mean of the cell's two faces on that axis.
fn centred_velocity(cell: vec3<i32>) -> vec3<f32> {
    return vec3<f32>(
        0.5 * (face(cell, 0u) + face(cell + X, 0u)),
        0.5 * (face(cell, 1u) + face(cell + Y, 1u)),
        0.5 * (face(cell, 2u) + face(cell + Z, 2u)),
    );
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

// Velocity component `axis` at `position` (cell coordinates: cell centres at integers). The
// component's samples sit half a cell lower on its axis, so its grid is shifted by half a cell.
fn sample_component(position: vec3<f32>, axis: u32) -> f32 {
    let s = trilinear(position + 0.5 * axis_offset(axis));
    let x0 = mix(component(velocity[s.corners[0]], axis), component(velocity[s.corners[1]], axis), s.t.x);
    let x1 = mix(component(velocity[s.corners[2]], axis), component(velocity[s.corners[3]], axis), s.t.x);
    let x2 = mix(component(velocity[s.corners[4]], axis), component(velocity[s.corners[5]], axis), s.t.x);
    let x3 = mix(component(velocity[s.corners[6]], axis), component(velocity[s.corners[7]], axis), s.t.x);
    return mix(mix(x0, x1, s.t.y), mix(x2, x3, s.t.y), s.t.z);
}

fn sample_velocity(position: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        sample_component(position, 0u),
        sample_component(position, 1u),
        sample_component(position, 2u),
    );
}

// A source vec3 parameter: the authored value (effect space), or the host binding field it is bound
// to when that field is present this tick, brought from world into effect space (`w`: 1 for a point,
// 0 for a vector).
fn source_vec3(base: u32, value_word: u32, ref_word: u32, w: f32) -> vec3<f32> {
    let fallback = vec3<f32>(
        bitcast<f32>(constants[base + value_word]),
        bitcast<f32>(constants[base + value_word + 1u]),
        bitcast<f32>(constants[base + value_word + 2u]),
    );
    let slot = constants[base + ref_word];
    if (slot == NO_SLOT || !aestra_binding_present(slot, constants[base + ref_word + 1u])) {
        return fallback;
    }
    return world_to_effect(aestra_binding_vec3(slot, constants[base + ref_word + 2u]), w);
}

// How strongly source `base` acts at `point` (effect space): 1 at its centre, 0 at its radius.
fn source_falloff(base: u32, point: vec3<f32>) -> f32 {
    let radius = bitcast<f32>(constants[base + 3u]);
    let distance_to_source = length(point - source_vec3(base, 0u, 8u, 1.0));
    return max(1.0 - distance_to_source / radius, 0.0);
}

// Injects density at the cell's centre, and pulls each of its faces toward the sources' velocity.
@compute @workgroup_size(4, 4, 4)
fn add_sources(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) { return; }
    let i = cell_index(cell);
    let delta = frame_dt();
    let h = cell_size();
    let center = grid_origin() + (vec3<f32>(cell) + vec3<f32>(0.5)) * h;
    var d = density[i];
    var v = velocity[i].xyz;
    for (var s = 0u; s < source_count(); s += 1u) {
        let base = SOURCE_BASE + s * SOURCE_WORDS;
        d += bitcast<f32>(constants[base + 7u]) * source_falloff(base, center) * delta;
        let emitted = source_vec3(base, 4u, 11u, 0.0);
        let pull = vec3<f32>(
            source_falloff(base, center - 0.5 * h * vec3<f32>(1.0, 0.0, 0.0)),
            source_falloff(base, center - 0.5 * h * vec3<f32>(0.0, 1.0, 0.0)),
            source_falloff(base, center - 0.5 * h * vec3<f32>(0.0, 0.0, 1.0)),
        );
        v += (emitted - v) * clamp(pull * delta * 8.0, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    density[i] = d;
    velocity[i] = vec4<f32>(v, 0.0);
}

// Buoyancy: dense (and, with fire, hot) fluid lifts each y face by the mean of its two cells.
@compute @workgroup_size(4, 4, 4)
fn apply_buoyancy(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) { return; }
    let c = vec3<i32>(cell);
    let i = cell_index(cell);
    let lifted = 0.5 * (density[i] + density[clamped_index(c - Y)]);
    let v = velocity[i];
    velocity[i] = vec4<f32>(v.x, v.y + buoyancy_strength() * lifted * frame_dt(), v.z, v.w);
}

@compute @workgroup_size(4, 4, 4)
fn apply_buoyancy_fire(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) { return; }
    let c = vec3<i32>(cell);
    let i = cell_index(cell);
    let below = clamped_index(c - Y);
    let lift = buoyancy_strength() * 0.5 * (density[i] + density[below])
        + combustion(5u) * 0.5 * (temperature[i] + temperature[below]);
    let v = velocity[i];
    velocity[i] = vec4<f32>(v.x, v.y + lift * frame_dt(), v.z, v.w);
}

// Vorticity ω = ∇ × u of the cell-centred velocity (central differences); `w` holds |ω|.
@compute @workgroup_size(4, 4, 4)
fn compute_vorticity(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) { return; }
    let c = vec3<i32>(cell);
    let last = i32(grid_res()) - 1;
    let xp = centred_velocity(clamp(c + X, vec3<i32>(0), vec3<i32>(last)));
    let xm = centred_velocity(clamp(c - X, vec3<i32>(0), vec3<i32>(last)));
    let yp = centred_velocity(clamp(c + Y, vec3<i32>(0), vec3<i32>(last)));
    let ym = centred_velocity(clamp(c - Y, vec3<i32>(0), vec3<i32>(last)));
    let zp = centred_velocity(clamp(c + Z, vec3<i32>(0), vec3<i32>(last)));
    let zm = centred_velocity(clamp(c - Z, vec3<i32>(0), vec3<i32>(last)));
    let w = vec3<f32>(
        (yp.z - ym.z) - (zp.y - zm.y),
        (zp.x - zm.x) - (xp.z - xm.z),
        (xp.y - xm.y) - (yp.x - ym.x),
    ) * (0.5 / cell_size());
    vorticity[cell_index(cell)] = vec4<f32>(w, length(w));
}

// Vorticity confinement force at the cell's centre, along N × ω with N = ∇|ω| / |∇|ω||; written to
// `velocity_next`, scratch until the advection overwrites it.
@compute @workgroup_size(4, 4, 4)
fn vorticity_force(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) { return; }
    let c = vec3<i32>(cell);
    let i = cell_index(cell);
    let eta = vec3<f32>(
        vorticity[clamped_index(c + X)].w - vorticity[clamped_index(c - X)].w,
        vorticity[clamped_index(c + Y)].w - vorticity[clamped_index(c - Y)].w,
        vorticity[clamped_index(c + Z)].w - vorticity[clamped_index(c - Z)].w,
    ) * (0.5 / cell_size());
    let magnitude = length(eta);
    var force = vec3<f32>(0.0);
    if (magnitude >= 1e-6) {
        force = confinement_strength() * cell_size() * cross(eta / magnitude, vorticity[i].xyz);
    }
    velocity_next[i] = vec4<f32>(force, 0.0);
}

// Applies the confinement force to each face: the mean of the forces of the face's two cells.
@compute @workgroup_size(4, 4, 4)
fn apply_vorticity(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) { return; }
    let c = vec3<i32>(cell);
    let i = cell_index(cell);
    let own = velocity_next[i].xyz;
    let force = 0.5 * vec3<f32>(
        own.x + velocity_next[clamped_index(c - X)].x,
        own.y + velocity_next[clamped_index(c - Y)].y,
        own.z + velocity_next[clamped_index(c - Z)].z,
    );
    velocity[i] = vec4<f32>(velocity[i].xyz + force * frame_dt(), 0.0);
}

// Where the fluid at `position` (cell coordinates) came from one step ago (semi-Lagrangian backtrace).
fn backtrace(position: vec3<f32>) -> vec3<f32> {
    return position - sample_velocity(position) * (frame_dt() / cell_size());
}

// Each face's component is carried along the flow from where it was one step ago.
@compute @workgroup_size(4, 4, 4)
fn advect_velocity(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) { return; }
    let p = vec3<f32>(cell);
    let advected = vec3<f32>(
        sample_component(backtrace(p - 0.5 * axis_offset(0u)), 0u),
        sample_component(backtrace(p - 0.5 * axis_offset(1u)), 1u),
        sample_component(backtrace(p - 0.5 * axis_offset(2u)), 2u),
    );
    let decay = 1.0 / (1.0 + velocity_dissipation() * frame_dt());
    velocity_next[cell_index(cell)] = vec4<f32>(advected * decay, 0.0);
}

@compute @workgroup_size(4, 4, 4)
fn advect_density(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) { return; }
    let s = trilinear(backtrace(vec3<f32>(cell)));
    let x0 = mix(density[s.corners[0]], density[s.corners[1]], s.t.x);
    let x1 = mix(density[s.corners[2]], density[s.corners[3]], s.t.x);
    let x2 = mix(density[s.corners[4]], density[s.corners[5]], s.t.x);
    let x3 = mix(density[s.corners[6]], density[s.corners[7]], s.t.x);
    let sampled = mix(mix(x0, x1, s.t.y), mix(x2, x3, s.t.y), s.t.z);
    let decay = 1.0 / (1.0 + density_dissipation() * frame_dt());
    density_next[cell_index(cell)] = sampled * decay;
}

// The divergence of each cell: the net flow out through its six faces.
@compute @workgroup_size(4, 4, 4)
fn compute_divergence(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) { return; }
    let c = vec3<i32>(cell);
    divergence[cell_index(cell)] = (
        face(c + X, 0u) - face(c, 0u)
        + face(c + Y, 1u) - face(c, 1u)
        + face(c + Z, 2u) - face(c, 2u)
    ) / cell_size();
}

// One Jacobi iteration of the compact 7-point Poisson equation ∇²p = ∇·u. A closed side is a Neumann
// wall (the neighbour is left out); an open side holds p = 0 outside (the neighbour counts as zero).
@compute @workgroup_size(4, 4, 4)
fn relax_pressure(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) { return; }
    let c = vec3<i32>(cell);
    var sum = 0.0;
    var count = 0.0;
    for (var axis = 0u; axis < 3u; axis += 1u) {
        let step = axis_step(axis);
        for (var side = 0u; side < 2u; side += 1u) {
            let positive = side == 1u;
            let neighbour = select(c - step, c + step, positive);
            if (inside(neighbour)) {
                sum += pressure[clamped_index(neighbour)];
                count += 1.0;
            } else if (side_open(axis, positive)) {
                count += 1.0;
            }
        }
    }
    let h = cell_size();
    var relaxed = 0.0;
    if (count > 0.0) {
        relaxed = (sum - h * h * divergence[cell_index(cell)]) / count;
    }
    pressure_next[cell_index(cell)] = relaxed;
}

// Subtracts the pressure gradient across each face. A face on a closed side carries no flow; on an
// open side, the pressure outside is zero.
@compute @workgroup_size(4, 4, 4)
fn project(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) { return; }
    let c = vec3<i32>(cell);
    let i = cell_index(cell);
    let h = cell_size();
    let own = pressure[i];
    var v = velocity[i].xyz;
    let below = vec3<f32>(
        pressure[clamped_index(c - X)],
        pressure[clamped_index(c - Y)],
        pressure[clamped_index(c - Z)],
    );
    let at_wall = vec3<bool>(cell.x == 0u, cell.y == 0u, cell.z == 0u);
    let open = vec3<bool>(side_open(0u, false), side_open(1u, false), side_open(2u, false));
    let outside = select(below, vec3<f32>(0.0), at_wall);
    v = v - (vec3<f32>(own) - outside) / h;
    v = select(v, vec3<f32>(0.0), at_wall & !open);
    velocity[i] = vec4<f32>(v, 0.0);
}

// ---- Fire (fluid F3): temperature and fuel, with the Combustion module ----

// Injects heat and fuel from every source (source words 14 and 15: rates per second at the centre).
@compute @workgroup_size(4, 4, 4)
fn add_heat(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) { return; }
    let i = cell_index(cell);
    let delta = frame_dt();
    let center = grid_origin() + (vec3<f32>(cell) + vec3<f32>(0.5)) * cell_size();
    var heat = temperature[i];
    var burnable = fuel[i];
    for (var s = 0u; s < source_count(); s += 1u) {
        let base = SOURCE_BASE + s * SOURCE_WORDS;
        let falloff = source_falloff(base, center);
        heat += bitcast<f32>(constants[base + 14u]) * falloff * delta;
        burnable += bitcast<f32>(constants[base + 15u]) * falloff * delta;
    }
    temperature[i] = heat;
    fuel[i] = burnable;
}

// Burns fuel where it is hot enough, releasing heat and smoke; hot gas cools. (It rises through
// `apply_buoyancy_fire`.)
@compute @workgroup_size(4, 4, 4)
fn combust(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) { return; }
    let i = cell_index(cell);
    let delta = frame_dt();
    var heat = temperature[i];
    var burnable = fuel[i];
    if (heat > combustion(0u)) {
        let burned = min(burnable, combustion(1u) * delta);
        burnable -= burned;
        heat += combustion(2u) * burned;
        density[i] = density[i] + combustion(3u) * burned;
    }
    heat = heat / (1.0 + combustion(4u) * delta);
    temperature[i] = heat;
    fuel[i] = burnable;
}

@compute @workgroup_size(4, 4, 4)
fn advect_temperature(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) { return; }
    let s = trilinear(backtrace(vec3<f32>(cell)));
    let x0 = mix(temperature[s.corners[0]], temperature[s.corners[1]], s.t.x);
    let x1 = mix(temperature[s.corners[2]], temperature[s.corners[3]], s.t.x);
    let x2 = mix(temperature[s.corners[4]], temperature[s.corners[5]], s.t.x);
    let x3 = mix(temperature[s.corners[6]], temperature[s.corners[7]], s.t.x);
    temperature_next[cell_index(cell)] = mix(mix(x0, x1, s.t.y), mix(x2, x3, s.t.y), s.t.z);
}

@compute @workgroup_size(4, 4, 4)
fn advect_fuel(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) { return; }
    let s = trilinear(backtrace(vec3<f32>(cell)));
    let x0 = mix(fuel[s.corners[0]], fuel[s.corners[1]], s.t.x);
    let x1 = mix(fuel[s.corners[2]], fuel[s.corners[3]], s.t.x);
    let x2 = mix(fuel[s.corners[4]], fuel[s.corners[5]], s.t.x);
    let x3 = mix(fuel[s.corners[6]], fuel[s.corners[7]], s.t.x);
    fuel_next[cell_index(cell)] = mix(mix(x0, x1, s.t.y), mix(x2, x3, s.t.y), s.t.z);
}
