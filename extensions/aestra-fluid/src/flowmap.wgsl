// Aestra Fluid — Leapfrog Flow Maps (fluid F6; Sun et al., "Leapfrog Flow Maps for Real-Time Fluid
// Simulation", SIGGRAPH 2025). Composed after `solver.wgsl`, whose bindings and helpers it uses.
//
// The velocity is carried in reinitialization cycles of n steps (the tick's step is `tick mod n`).
// Each step:
//
// - the midpoint velocity u_{i+1/2} is advected leapfrog-style — u_{i-3/2} (plus the forces) carried
//   over 2Δt through u_{i-1/2} with an RK2 backtrace; the cycle's first two steps start from u_0 over
//   Δt/2 and from u_{1/2} over Δt — then projected (the solver's usual projection), and stored;
// - the forward flow map Φ and the column of its Jacobian F each face needs are marched one step
//   (RK4, through u_{i+1/2}), and the forces' path integral Fᵀ·f(Φ) gathers into u_0.
//
// At the cycle's last step the backward map Ψ and its Jacobian column T are marched from every face
// back through all n stored midpoint velocities, the impulse m = Tᵀ·u_0(Ψ) is mapped, the round-trip
// error e = (Fᵀ·m(Φ) − u_0)/2 is measured and taken back off (m −= Tᵀ·e(Ψ)), and m is projected: the
// velocity that starts the next cycle.
//
// On the MAC grid, face d's component needs only column d of each Jacobian — and the columns of
// J' = ∇u·J evolve independently — so a face carries 6 floats of map, not 12. Maps and the pulled-back
// fields are sampled with quadratic B-splines, whose smooth gradients the Jacobians need; the
// leapfrog advection samples trilinearly. Positions are in cell coordinates (cell centres at
// integers), velocities in cells per second.

fn lfm_step() -> u32 {
    return frame[0] % lfm_cycle();
}

// The cycle's last step: the tick that maps the impulse and starts the next cycle.
fn lfm_reinit() -> bool {
    return lfm_step() + 1u == lfm_cycle();
}

fn lfm_cells() -> u32 {
    let n = grid_res();
    return n * n * n;
}

// Face d of cell `c`: its minimum face on axis d.
fn face_point(c: vec3<u32>, d: u32) -> vec3<f32> {
    return vec3<f32>(c) - 0.5 * axis_offset(d);
}

// Component `i` (0, 1, 2) of a vector, by selects (see `affine_row`).
fn pick(v: vec3<f32>, i: i32) -> f32 {
    return select(select(v.z, v.y, i == 1), v.x, i == 0);
}

fn history_at(slot: u32, i: u32, a: u32) -> f32 {
    return lfm_history[(slot * lfm_cells() + i) * 3u + a];
}

// ---- Trilinear sampling of component `a` of a staggered field (as `sample_component`) ----

fn corners_of(p: vec3<f32>, a: u32) -> Trilinear {
    return trilinear(p + 0.5 * axis_offset(a));
}

fn tri_initial(p: vec3<f32>, a: u32) -> f32 {
    let s = corners_of(p, a);
    return interpolate(array<f32, 8>(
        component(lfm_initial[s.corners[0]], a), component(lfm_initial[s.corners[1]], a),
        component(lfm_initial[s.corners[2]], a), component(lfm_initial[s.corners[3]], a),
        component(lfm_initial[s.corners[4]], a), component(lfm_initial[s.corners[5]], a),
        component(lfm_initial[s.corners[6]], a), component(lfm_initial[s.corners[7]], a),
    ), s.t);
}

fn tri_history(slot: u32, p: vec3<f32>, a: u32) -> f32 {
    let s = corners_of(p, a);
    return interpolate(array<f32, 8>(
        history_at(slot, s.corners[0], a), history_at(slot, s.corners[1], a),
        history_at(slot, s.corners[2], a), history_at(slot, s.corners[3], a),
        history_at(slot, s.corners[4], a), history_at(slot, s.corners[5], a),
        history_at(slot, s.corners[6], a), history_at(slot, s.corners[7], a),
    ), s.t);
}

fn tri_force(p: vec3<f32>, a: u32) -> f32 {
    let s = corners_of(p, a);
    return interpolate(array<f32, 8>(
        component(lfm_force[s.corners[0]], a), component(lfm_force[s.corners[1]], a),
        component(lfm_force[s.corners[2]], a), component(lfm_force[s.corners[3]], a),
        component(lfm_force[s.corners[4]], a), component(lfm_force[s.corners[5]], a),
        component(lfm_force[s.corners[6]], a), component(lfm_force[s.corners[7]], a),
    ), s.t);
}

// ---- Quadratic B-spline sampling ----

// The three nodes along one axis around sample coordinate `s`, their weights and the weights'
// derivatives.
struct BAxis {
    base: i32,
    w: vec3<f32>,
    dw: vec3<f32>,
}

fn bspline_axis(s: f32) -> BAxis {
    let base = floor(s - 0.5);
    let f = s - base;
    let w = vec3<f32>(0.5 * (1.5 - f) * (1.5 - f), 0.75 - (f - 1.0) * (f - 1.0), 0.5 * (f - 0.5) * (f - 0.5));
    let dw = vec3<f32>(f - 1.5, -2.0 * (f - 1.0), f - 0.5);
    return BAxis(i32(base), w, dw);
}

// The weight of node (i, j, k) of a 3³ stencil — value, then ∂/∂x, ∂/∂y, ∂/∂z.
fn bspline_weight(bx: BAxis, by: BAxis, bz: BAxis, i: i32, j: i32, k: i32) -> vec4<f32> {
    let wx = pick(bx.w, i);
    let wy = pick(by.w, j);
    let wz = pick(bz.w, k);
    return vec4<f32>(
        wx * wy * wz,
        pick(bx.dw, i) * wy * wz,
        wx * pick(by.dw, j) * wz,
        wx * wy * pick(bz.dw, k),
    );
}

struct BStencil {
    x: BAxis,
    y: BAxis,
    z: BAxis,
}

fn bspline_stencil(p: vec3<f32>, a: u32) -> BStencil {
    let s = p + 0.5 * axis_offset(a);
    return BStencil(bspline_axis(s.x), bspline_axis(s.y), bspline_axis(s.z));
}

fn bspline_node(b: BStencil, i: i32, j: i32, k: i32) -> u32 {
    return clamped_index(vec3<i32>(b.x.base + i, b.y.base + j, b.z.base + k));
}

// Component `a` of the current velocity at `p`, and its gradient in cell coordinates.
fn velocity_bspline(p: vec3<f32>, a: u32) -> vec4<f32> {
    let b = bspline_stencil(p, a);
    var sum = vec4<f32>(0.0);
    for (var k = 0; k < 3; k += 1) {
        for (var j = 0; j < 3; j += 1) {
            for (var i = 0; i < 3; i += 1) {
                let value = component(velocity[bspline_node(b, i, j, k)], a);
                sum += value * bspline_weight(b.x, b.y, b.z, i, j, k);
            }
        }
    }
    return sum;
}

fn history_bspline(slot: u32, p: vec3<f32>, a: u32) -> vec4<f32> {
    let b = bspline_stencil(p, a);
    var sum = vec4<f32>(0.0);
    for (var k = 0; k < 3; k += 1) {
        for (var j = 0; j < 3; j += 1) {
            for (var i = 0; i < 3; i += 1) {
                let value = history_at(slot, bspline_node(b, i, j, k), a);
                sum += value * bspline_weight(b.x, b.y, b.z, i, j, k);
            }
        }
    }
    return sum;
}

fn initial_bspline(p: vec3<f32>, a: u32) -> f32 {
    let b = bspline_stencil(p, a);
    var sum = 0.0;
    for (var k = 0; k < 3; k += 1) {
        for (var j = 0; j < 3; j += 1) {
            for (var i = 0; i < 3; i += 1) {
                let value = component(lfm_initial[bspline_node(b, i, j, k)], a);
                sum += value * bspline_weight(b.x, b.y, b.z, i, j, k).x;
            }
        }
    }
    return sum;
}

// Component `a` of the impulse at `p`, and the weight on faces that kept their midpoint velocity
// instead of a mapped impulse (the impulse's `w` flags them).
fn impulse_bspline(p: vec3<f32>, a: u32) -> vec2<f32> {
    let b = bspline_stencil(p, a);
    var sum = vec2<f32>(0.0);
    for (var k = 0; k < 3; k += 1) {
        for (var j = 0; j < 3; j += 1) {
            for (var i = 0; i < 3; i += 1) {
                let node = lfm_impulse[bspline_node(b, i, j, k)];
                sum += vec2<f32>(component(node, a), node.w) * bspline_weight(b.x, b.y, b.z, i, j, k).x;
            }
        }
    }
    return sum;
}

// The round-trip error, which the compensation keeps in `lfm_force`.
fn error_bspline(p: vec3<f32>, a: u32) -> f32 {
    let b = bspline_stencil(p, a);
    var sum = 0.0;
    for (var k = 0; k < 3; k += 1) {
        for (var j = 0; j < 3; j += 1) {
            for (var i = 0; i < 3; i += 1) {
                let value = component(lfm_force[bspline_node(b, i, j, k)], a);
                sum += value * bspline_weight(b.x, b.y, b.z, i, j, k).x;
            }
        }
    }
    return sum;
}

// A velocity in cells per second and its gradient ∂v/∂p (per second): M·J is J's rate of change.
struct Flow {
    v: vec3<f32>,
    m: mat3x3<f32>,
}

fn to_flow(r0: vec4<f32>, r1: vec4<f32>, r2: vec4<f32>) -> Flow {
    let inverse_h = 1.0 / cell_size();
    return Flow(
        vec3<f32>(r0.x, r1.x, r2.x) * inverse_h,
        transpose(mat3x3<f32>(r0.yzw, r1.yzw, r2.yzw)) * inverse_h,
    );
}

fn velocity_flow(p: vec3<f32>) -> Flow {
    return to_flow(velocity_bspline(p, 0u), velocity_bspline(p, 1u), velocity_bspline(p, 2u));
}

fn history_flow(slot: u32, p: vec3<f32>) -> Flow {
    return to_flow(
        history_bspline(slot, p, 0u),
        history_bspline(slot, p, 1u),
        history_bspline(slot, p, 2u),
    );
}

// A point of a flow map and the matching Jacobian column.
struct MapPoint {
    y: vec3<f32>,
    j: vec3<f32>,
}

struct Marched {
    end: MapPoint,
    // The RK4 stage at the step's middle, where the forces' path integral is sampled, and the flow
    // there.
    middle: MapPoint,
    middle_flow: Flow,
}

// One RK4 step of a map point through the current velocity, over `h` seconds.
fn march_velocity(s: MapPoint, h: f32) -> Marched {
    let f1 = velocity_flow(s.y);
    let k1 = f1.v;
    let q1 = f1.m * s.j;
    let m2 = MapPoint(s.y + 0.5 * h * k1, s.j + 0.5 * h * q1);
    let f2 = velocity_flow(m2.y);
    let k2 = f2.v;
    let q2 = f2.m * m2.j;
    let m3 = MapPoint(s.y + 0.5 * h * k2, s.j + 0.5 * h * q2);
    let f3 = velocity_flow(m3.y);
    let k3 = f3.v;
    let q3 = f3.m * m3.j;
    let m4 = MapPoint(s.y + h * k3, s.j + h * q3);
    let f4 = velocity_flow(m4.y);
    let k4 = f4.v;
    let q4 = f4.m * m4.j;
    return Marched(
        MapPoint(
            s.y + h / 6.0 * (k1 + 2.0 * k2 + 2.0 * k3 + k4),
            s.j + h / 6.0 * (q1 + 2.0 * q2 + 2.0 * q3 + q4),
        ),
        m3,
        f3,
    );
}

// One RK4 step of a map point through a stored midpoint velocity, over `h` seconds.
fn march_history(slot: u32, s: MapPoint, h: f32) -> MapPoint {
    let f1 = history_flow(slot, s.y);
    let k1 = f1.v;
    let q1 = f1.m * s.j;
    let f2 = history_flow(slot, s.y + 0.5 * h * k1);
    let k2 = f2.v;
    let q2 = f2.m * (s.j + 0.5 * h * q1);
    let f3 = history_flow(slot, s.y + 0.5 * h * k2);
    let k3 = f3.v;
    let q3 = f3.m * (s.j + 0.5 * h * q2);
    let f4 = history_flow(slot, s.y + h * k3);
    let k4 = f4.v;
    let q4 = f4.m * (s.j + h * q3);
    return MapPoint(
        s.y + h / 6.0 * (k1 + 2.0 * k2 + 2.0 * k3 + k4),
        s.j + h / 6.0 * (q1 + 2.0 * q2 + 2.0 * q3 + q4),
    );
}

// A face's map point in `lfm_forward` or `lfm_backward`: 6 floats at `cell·18 + d·6`.
fn map_offset(i: u32, d: u32) -> u32 {
    return i * 18u + d * 6u;
}

fn load_forward(i: u32, d: u32) -> MapPoint {
    let o = map_offset(i, d);
    return MapPoint(
        vec3<f32>(lfm_forward[o], lfm_forward[o + 1u], lfm_forward[o + 2u]),
        vec3<f32>(lfm_forward[o + 3u], lfm_forward[o + 4u], lfm_forward[o + 5u]),
    );
}

fn store_forward(i: u32, d: u32, s: MapPoint) {
    let o = map_offset(i, d);
    lfm_forward[o] = s.y.x;
    lfm_forward[o + 1u] = s.y.y;
    lfm_forward[o + 2u] = s.y.z;
    lfm_forward[o + 3u] = s.j.x;
    lfm_forward[o + 4u] = s.j.y;
    lfm_forward[o + 5u] = s.j.z;
}

fn load_backward(i: u32, d: u32) -> MapPoint {
    let o = map_offset(i, d);
    return MapPoint(
        vec3<f32>(lfm_backward[o], lfm_backward[o + 1u], lfm_backward[o + 2u]),
        vec3<f32>(lfm_backward[o + 3u], lfm_backward[o + 4u], lfm_backward[o + 5u]),
    );
}

fn store_backward(i: u32, d: u32, s: MapPoint) {
    let o = map_offset(i, d);
    lfm_backward[o] = s.y.x;
    lfm_backward[o + 1u] = s.y.y;
    lfm_backward[o + 2u] = s.y.z;
    lfm_backward[o + 3u] = s.j.x;
    lfm_backward[o + 4u] = s.j.y;
    lfm_backward[o + 5u] = s.j.z;
}

// ---- Every step ----

// This tick's forces, from what the force passes did to the velocity: f = (u − u_before)/Δt, with the
// velocity before them copied into `lfm_force` first. Every force module then enters the flow map's
// path integral, without each needing a force field of its own. The grid's velocity dissipation is a
// linear drag among them.
@compute @workgroup_size(4, 4, 4)
fn lfm_forces(@builtin(global_invocation_id) gid: vec3<u32>) {
    let cell = grid_cell(gid);
    if (!in_grid(cell)) { return; }
    let i = cell_index(cell);
    let u = velocity[i].xyz;
    let applied = (u - lfm_force[i].xyz) / frame_dt();
    lfm_force[i] = vec4<f32>(applied - velocity_dissipation() * u, 0.0);
}

// The velocity the step's advection is carried through (cells per second).
fn lfm_advecting(step: u32, p: vec3<f32>) -> vec3<f32> {
    let inverse_h = 1.0 / cell_size();
    if (step == 0u) {
        return vec3<f32>(tri_initial(p, 0u), tri_initial(p, 1u), tri_initial(p, 2u)) * inverse_h;
    }
    let slot = select(step - 1u, 0u, step == 1u);
    return vec3<f32>(tri_history(slot, p, 0u), tri_history(slot, p, 1u), tri_history(slot, p, 2u))
        * inverse_h;
}

// The velocity the step carries: u_0, u_{1/2}, then u_{i-3/2}.
fn lfm_carried(step: u32, p: vec3<f32>, a: u32) -> f32 {
    if (step == 0u) {
        return tri_initial(p, a);
    }
    return tri_history(select(step - 2u, 0u, step == 1u), p, a);
}

// Face `a` of the leapfrog-advected midpoint velocity: the carried velocity plus the forces over the
// carried span (Δt/2, Δt, then 2Δt), taken from an RK2 backtrace through the advecting velocity.
fn lfm_advect_face(c: vec3<u32>, a: u32) -> f32 {
    let step = lfm_step();
    let dt = frame_dt();
    var span = 2.0 * dt;
    if (step == 0u) {
        span = 0.5 * dt;
    } else if (step == 1u) {
        span = dt;
    }
    let p = face_point(c, a);
    let v1 = lfm_advecting(step, p);
    let v2 = lfm_advecting(step, p - 0.5 * span * v1);
    let back = p - span * v2;
    return lfm_carried(step, back, a) + span * tri_force(back, a);
}

@compute @workgroup_size(4, 4, 4)
fn lfm_advect(@builtin(global_invocation_id) gid: vec3<u32>) {
    let cell = grid_cell(gid);
    if (!in_grid(cell)) { return; }
    velocity_next[cell_index(cell)] = vec4<f32>(
        lfm_advect_face(cell, 0u),
        lfm_advect_face(cell, 1u),
        lfm_advect_face(cell, 2u),
        0.0,
    );
}

// Marches face d's forward map one step and returns its share of the path integral: Δt·Fᵀ·(f + ∇½|u|²)
// at the step's middle, component d.
//
// ∇½|u|² is the impulse gauge's kinetic part: ξ = ∫(p − ½|u|²) dτ along the paths, and the gradient
// of such an integral is the same pullback as the forces' (∇ₓ∫g(Ψ) dτ = Tᵀ·∫Fᵀ·∇g dτ). Gathered here,
// the gauge the cycle's projection solves for is ∫p dτ alone — which is 0 at an open side, as that
// projection assumes. Left in the gauge, an open side's ½|u|² reads as a pressure drop that
// accelerates the outflow cycle after cycle.
fn lfm_forward_face(c: vec3<u32>, d: u32) -> f32 {
    let i = cell_index(c);
    var s = MapPoint(face_point(c, d), axis_offset(d));
    if (lfm_step() != 0u) {
        s = load_forward(i, d);
    }
    let marched = march_velocity(s, frame_dt());
    store_forward(i, d, marched.end);
    let y = marched.middle.y;
    let f = vec3<f32>(tri_force(y, 0u), tri_force(y, 1u), tri_force(y, 2u));
    // ∇½|u|² = (∇u)ᵀ·u; the flow's velocity is in cells per second, its gradient per second.
    let flow = marched.middle_flow;
    let kinetic = transpose(flow.m) * (flow.v * cell_size());
    return frame_dt() * dot(marched.middle.j, f + kinetic);
}

// After the projection: the midpoint velocity is stored, the forward maps marched and the forces
// gathered into the cycle's initial velocity.
@compute @workgroup_size(4, 4, 4)
fn lfm_march_forward(@builtin(global_invocation_id) gid: vec3<u32>) {
    let cell = grid_cell(gid);
    if (!in_grid(cell)) { return; }
    let i = cell_index(cell);
    let gathered = vec3<f32>(
        lfm_forward_face(cell, 0u),
        lfm_forward_face(cell, 1u),
        lfm_forward_face(cell, 2u),
    );
    lfm_initial[i] = vec4<f32>(lfm_initial[i].xyz + gathered, 0.0);
    let v = velocity[i];
    let o = (lfm_step() * lfm_cells() + i) * 3u;
    lfm_history[o] = v.x;
    lfm_history[o + 1u] = v.y;
    lfm_history[o + 2u] = v.z;
}

// ---- The cycle's last step ----

// Marches face d's backward map through every stored midpoint velocity, newest first, and returns the
// impulse it maps: m_d = Tᵀ·u_0(Ψ), component d. Where the map has left the grid — fluid that came in
// through an open side this cycle — there is no initial velocity to map: the face keeps the step's own
// midpoint velocity, and so does everything the compensation later samples there.
fn lfm_backward_face(c: vec3<u32>, d: u32) -> vec2<f32> {
    var s = MapPoint(face_point(c, d), axis_offset(d));
    let last = lfm_cycle() - 1u;
    for (var k = 0u; k <= last; k += 1u) {
        s = march_history(last - k, s, -frame_dt());
    }
    let i = cell_index(c);
    store_backward(i, d, s);
    if (!lfm_trusted(s)) {
        return vec2<f32>(component(velocity[i], d), 1.0);
    }
    let u0 = vec3<f32>(initial_bspline(s.y, 0u), initial_bspline(s.y, 1u), initial_bspline(s.y, 2u));
    return vec2<f32>(dot(s.j, u0), 0.0);
}

@compute @workgroup_size(4, 4, 4)
fn lfm_pull_back(@builtin(global_invocation_id) gid: vec3<u32>) {
    let cell = grid_cell(gid);
    if (!in_grid(cell) || !lfm_reinit()) { return; }
    let x = lfm_backward_face(cell, 0u);
    let y = lfm_backward_face(cell, 1u);
    let z = lfm_backward_face(cell, 2u);
    // `w` flags a face that kept its midpoint velocity.
    lfm_impulse[cell_index(cell)] = vec4<f32>(x.x, y.x, z.x, max(x.y, max(y.y, z.y)));
}

// Whether a map point is still inside the grid (its faces span −½ to N − ½ in cell coordinates). A
// map that has left through an open side carries nothing it can sample: the grid holds no field
// there.
fn lfm_trusted(s: MapPoint) -> bool {
    let limit = f32(grid_res()) - 0.5;
    return all(s.y >= vec3<f32>(-0.5)) && all(s.y <= vec3<f32>(limit));
}

// Face d's round-trip error: the impulse mapped forward again, e = (Fᵀ·m(Φ) − u_0)/2. None where the
// forward map has left the grid, nor where the impulse it samples includes faces that kept their
// midpoint velocity: there the difference is how the flow changed over the cycle, not the maps'
// error, and taking half of it off would feed the flow energy each cycle.
fn lfm_error_face(i: u32, d: u32) -> f32 {
    let s = load_forward(i, d);
    if (!lfm_trusted(s)) {
        return 0.0;
    }
    let mx = impulse_bspline(s.y, 0u);
    let my = impulse_bspline(s.y, 1u);
    let mz = impulse_bspline(s.y, 2u);
    if (mx.y + my.y + mz.y > 0.0) {
        return 0.0;
    }
    return 0.5 * (dot(s.j, vec3<f32>(mx.x, my.x, mz.x)) - component(lfm_initial[i], d));
}

@compute @workgroup_size(4, 4, 4)
fn lfm_measure_error(@builtin(global_invocation_id) gid: vec3<u32>) {
    let cell = grid_cell(gid);
    if (!in_grid(cell) || !lfm_reinit()) { return; }
    let i = cell_index(cell);
    lfm_force[i] = vec4<f32>(lfm_error_face(i, 0u), lfm_error_face(i, 1u), lfm_error_face(i, 2u), 0.0);
}

// Face d's compensated impulse, m − Tᵀ·e(Ψ), limited to the range of the uncompensated impulse on the
// 3³ faces around it: unlimited, the compensation overshoots where the maps have stretched and feeds
// on itself cycle after cycle (the paper's optional clamp). Where the backward map has left the grid
// — fluid that came in through an open side this cycle — there is no initial velocity to map: the face
// keeps the step's own midpoint velocity.
fn lfm_compensated_face(c: vec3<u32>, d: u32) -> f32 {
    let i = cell_index(c);
    let s = load_backward(i, d);
    if (!lfm_trusted(s)) {
        return component(lfm_impulse[i], d);
    }
    let e = vec3<f32>(error_bspline(s.y, 0u), error_bspline(s.y, 1u), error_bspline(s.y, 2u));
    let m = component(lfm_impulse[i], d);
    var low = m;
    var high = m;
    for (var k = -1; k <= 1; k += 1) {
        for (var j = -1; j <= 1; j += 1) {
            for (var l = -1; l <= 1; l += 1) {
                let n = component(lfm_impulse[clamped_index(vec3<i32>(c) + vec3<i32>(l, j, k))], d);
                low = min(low, n);
                high = max(high, n);
            }
        }
    }
    return clamp(m - dot(s.j, e), low, high);
}

// The compensated impulse replaces the velocity, to be projected.
@compute @workgroup_size(4, 4, 4)
fn lfm_compensate(@builtin(global_invocation_id) gid: vec3<u32>) {
    let cell = grid_cell(gid);
    if (!in_grid(cell) || !lfm_reinit()) { return; }
    let i = cell_index(cell);
    velocity[i] = vec4<f32>(
        lfm_compensated_face(cell, 0u),
        lfm_compensated_face(cell, 1u),
        lfm_compensated_face(cell, 2u),
        0.0,
    );
}

// The impulse's divergence — none on the other steps, so their second solve starts converged and runs
// no iteration.
@compute @workgroup_size(4, 4, 4)
fn lfm_impulse_divergence(@builtin(global_invocation_id) gid: vec3<u32>) {
    let cell = grid_cell(gid);
    if (!in_grid(cell)) { return; }
    divergence[cell_index(cell)] = select(0.0, divergence_at(cell), lfm_reinit());
}

@compute @workgroup_size(4, 4, 4)
fn lfm_project(@builtin(global_invocation_id) gid: vec3<u32>) {
    let cell = grid_cell(gid);
    if (!in_grid(cell) || !lfm_reinit()) { return; }
    project_cell(cell);
}

// The cycle's safeguard. The mapped velocity legitimately holds more energy than the last midpoint
// velocity — what the cycle's advection dissipated, which the maps recover — but not half as much
// again: that is the maps' error, which would double cycle after cycle (strong shear distorts them
// within a cycle). Partials of |u_n|² and |u_{n−1/2}|², summed in the fixed order of the pressure
// solve's reductions.
@compute @workgroup_size(4, 4, 4)
fn lfm_energy(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_index) local: u32,
    @builtin(workgroup_id) group: vec3<u32>,
) {
    let cell = grid_cell(gid);
    if (!lfm_reinit()) { return; }
    var energies = vec4<f32>(0.0);
    if (in_grid(cell)) {
        let i = cell_index(cell);
        let mapped = velocity[i].xyz;
        let o = ((lfm_cycle() - 1u) * lfm_cells() + i) * 3u;
        let midpoint = vec3<f32>(lfm_history[o], lfm_history[o + 1u], lfm_history[o + 2u]);
        energies = vec4<f32>(dot(mapped, mapped), dot(midpoint, midpoint), 0.0, 0.0);
    }
    let total = aestra_workgroup_sum(local, energies);
    if (local == 0u) {
        pcg_reduction[PCG_PARTIALS + pcg_group_index(group)] = total;
    }
}

@compute @workgroup_size(64)
fn lfm_energy_total(@builtin(local_invocation_index) local: u32) {
    if (!lfm_reinit()) { return; }
    let total = pcg_total(local);
    if (local == 0u) {
        pcg_reduction[0] = total;
    }
}

// The next cycle starts from the mapped velocity — or, when the safeguard trips (half as much energy
// again as the last midpoint velocity), from that midpoint velocity: the cycle degrades to plain
// advection instead of feeding an error.
@compute @workgroup_size(4, 4, 4)
fn lfm_restart(@builtin(global_invocation_id) gid: vec3<u32>) {
    let cell = grid_cell(gid);
    if (!in_grid(cell) || !lfm_reinit()) { return; }
    let i = cell_index(cell);
    let energies = pcg_reduction[0];
    if (energies.x > 1.5 * energies.y + 1e-6) {
        let o = ((lfm_cycle() - 1u) * lfm_cells() + i) * 3u;
        velocity[i] = vec4<f32>(lfm_history[o], lfm_history[o + 1u], lfm_history[o + 2u], 0.0);
    }
    lfm_initial[i] = velocity[i];
}
