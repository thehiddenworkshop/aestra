// Aestra Fluid — the multigrid-preconditioned conjugate-gradient pressure solve (fluid F5, MGPCG after
// McAdams et al. 2010). Composed after `solver.wgsl`, whose bindings and helpers it uses, and the
// shared reduction module (`aestra_workgroup_sum`).
//
// The system is the compact 7-point one Jacobi relaxes: (A p)_c = d_c·p_c − Σ p_n over the neighbours
// holding an unknown, where d_c counts every face that is not a Neumann wall (a solid, a closed side;
// an open side is p = 0 outside and counts). A p = −h²·∇·u, symmetric and positive (semi)definite.
//
// PCG runs in a convergent repeat: each iteration preconditions the residual with one V-cycle,
// updates the direction, applies A, steps, and writes the relative residual |r| / |b| to the first
// word of `pcg_reduction`, which the backend compares with the tolerance on the device. Every sum is
// a fixed-order reduction, so the iterations — and the bits — repeat exactly. Every pass binds at
// most 7 storage buffers (WebGPU guarantees 8): p and q share one buffer, the scalars head the
// partial sums, and the fine level's solid flags sit with the coarse ones.
//
// The V-cycle: red-black Gauss–Seidel from a zero guess, then the residual restricted to the next
// level (the sum of a cell's 8 children, halved: ½·Pᵀ), down to the coarsest, smoothed there red,
// black, red; back up, the correction is injected into the children (P) and smoothed in the reverse
// colour order. Every level sweeps a palindrome around the coarser ones, so the whole cycle is a
// symmetric operator — as PCG requires. Passes that need no grid-wide barrier between them share a
// dispatch (a restriction with the coarse level's first red sweep, a correction with the black sweep,
// the fine level's last red sweep with the sum of r·z). Levels halve while the grid stays even and
// keeps at least 3 cells a side; a coarse cell is solid when all 8 of its children are. The solve
// starts from the previous tick's pressure.

// `pcg_reduction`: the scalars — (relative residual, r·z, α, β) then (|b|², b's mean, -, -); the
// convergence test reads the first word — then the per-workgroup partial sums.
const PCG_PARTIALS: u32 = 2u;

// A neighbour that holds no unknown: a Neumann wall (a solid, a closed side) or an open side.
const MG_WALL: i32 = -1;
const MG_OPEN: i32 = -2;

fn mg_level_res(level: u32) -> u32 {
    var n = grid_res();
    for (var l = 0u; l < level; l += 1u) {
        n = n / 2u;
    }
    return n;
}

// Where level `level` starts in the per-level buffers.
fn mg_level_base(level: u32) -> u32 {
    var n = grid_res();
    var base = 0u;
    for (var l = 0u; l < level; l += 1u) {
        base += n * n * n;
        n = n / 2u;
    }
    return base;
}

fn mg_index(n: u32, cell: vec3<u32>) -> u32 {
    return (cell.z * n + cell.y) * n + cell.x;
}

fn mg_inside(n: u32, c: vec3<i32>) -> bool {
    return all(c >= vec3<i32>(0)) && all(c < vec3<i32>(i32(n)));
}

// Without colliders nothing is solid, on any level.
fn mg_solid(level: u32, n: u32, base: u32, c: vec3<i32>) -> bool {
    if (collider_count() == 0u || !mg_inside(n, c)) {
        return false;
    }
    return mg_flags[base + mg_index(n, vec3<u32>(c))] != 0u;
}

// The neighbour of `c` across one face: its index on the level when it holds an unknown, otherwise
// MG_WALL or MG_OPEN.
fn mg_neighbour(level: u32, n: u32, base: u32, c: vec3<i32>, axis: u32, positive: bool) -> i32 {
    let step = axis_step(axis);
    let neighbour = select(c - step, c + step, positive);
    if (!mg_inside(n, neighbour)) {
        return select(MG_WALL, MG_OPEN, side_open(axis, positive));
    }
    if (mg_solid(level, n, base, neighbour)) {
        return MG_WALL;
    }
    return i32(mg_index(n, vec3<u32>(neighbour)));
}

// A cell's six neighbours — named, not an array: FXC rejects dynamically indexed local writes.
struct MgStencil {
    xm: i32,
    xp: i32,
    ym: i32,
    yp: i32,
    zm: i32,
    zp: i32,
}

fn mg_stencil(level: u32, n: u32, base: u32, c: vec3<i32>) -> MgStencil {
    return MgStencil(
        mg_neighbour(level, n, base, c, 0u, false),
        mg_neighbour(level, n, base, c, 0u, true),
        mg_neighbour(level, n, base, c, 1u, false),
        mg_neighbour(level, n, base, c, 1u, true),
        mg_neighbour(level, n, base, c, 2u, false),
        mg_neighbour(level, n, base, c, 2u, true),
    );
}

fn mg_counts(neighbour: i32) -> f32 {
    return select(1.0, 0.0, neighbour == MG_WALL);
}

// A region sealed by closed sides and solids holds pressure only up to a constant, and is solvable
// only when nothing flows into it. A collider can push fluid into one; this shift keeps every level's
// operator positive definite so such a region's pressure stays bounded rather than the solve
// diverging. Scaled like the operator, fourfold per level; far below the smallest eigenvalue of any
// grid the solver allows, so it leaves the pressure of open regions all but unchanged.
const MG_SHIFT: f32 = 1e-6;

// The diagonal of A on `level`: every face but a Neumann wall, and the shift.
fn mg_diagonal(s: MgStencil, level: u32) -> f32 {
    return mg_counts(s.xm) + mg_counts(s.xp) + mg_counts(s.ym) + mg_counts(s.yp)
        + mg_counts(s.zm) + mg_counts(s.zp) + MG_SHIFT * f32(1u << (2u * level));
}

fn mg_solution_at(base: u32, neighbour: i32) -> f32 {
    if (neighbour < 0) {
        return 0.0;
    }
    return mg_solution[base + u32(neighbour)];
}

fn mg_solution_sum(base: u32, s: MgStencil) -> f32 {
    return mg_solution_at(base, s.xm) + mg_solution_at(base, s.xp) + mg_solution_at(base, s.ym)
        + mg_solution_at(base, s.yp) + mg_solution_at(base, s.zm) + mg_solution_at(base, s.zp);
}

// ---- The V-cycle, one level per dispatch (the entry points are generated per level in lib.rs) ----

// Red-black Gauss–Seidel on `level`: the cells of `parity` take (r + Σ z_n) / d from their neighbours,
// which are all of the other colour.
fn mg_smooth(level: u32, cell: vec3<u32>, parity: u32) {
    let n = mg_level_res(level);
    if (any(cell >= vec3<u32>(n)) || ((cell.x + cell.y + cell.z) & 1u) != parity) {
        return;
    }
    let base = mg_level_base(level);
    let c = vec3<i32>(cell);
    let i = base + mg_index(n, cell);
    if (mg_solid(level, n, base, c)) {
        mg_solution[i] = 0.0;
        return;
    }
    let s = mg_stencil(level, n, base, c);
    let diagonal = mg_diagonal(s, level);
    mg_solution[i] = (mg_rhs[i] + mg_solution_sum(base, s)) / diagonal;
}

// The residual r − A z of one fine cell.
fn mg_residual(level: u32, n: u32, base: u32, c: vec3<i32>) -> f32 {
    if (mg_solid(level, n, base, c)) {
        return 0.0;
    }
    let s = mg_stencil(level, n, base, c);
    let i = base + mg_index(n, vec3<u32>(c));
    return mg_rhs[i] - (mg_diagonal(s, level) * mg_solution[i] - mg_solution_sum(base, s));
}

// The right-hand side of a `coarse` cell: its 8 children's residuals, summed and halved (the
// graph Laplacian's scale grows fourfold per level; the average of 8 times 4). The finer level has
// just swept its black cells, which leaves their residuals exactly zero: only the 4 red children
// count. A red coarse cell then takes the level's first red sweep from a zero guess at once: that
// needs only its own right-hand side.
fn mg_restrict_smooth(coarse: u32, cell: vec3<u32>) {
    let nc = mg_level_res(coarse);
    if (any(cell >= vec3<u32>(nc))) {
        return;
    }
    let fine = coarse - 1u;
    let nf = mg_level_res(fine);
    let bf = mg_level_base(fine);
    let o = vec3<i32>(cell) * 2;
    let sum = mg_residual(fine, nf, bf, o) + mg_residual(fine, nf, bf, o + X + Y)
        + mg_residual(fine, nf, bf, o + X + Z) + mg_residual(fine, nf, bf, o + Y + Z);
    let bc = mg_level_base(coarse);
    let i = bc + mg_index(nc, cell);
    let rhs = 0.5 * sum;
    mg_rhs[i] = rhs;
    if (((cell.x + cell.y + cell.z) & 1u) == 0u) {
        let c = vec3<i32>(cell);
        var z = 0.0;
        if (!mg_solid(coarse, nc, bc, c)) {
            let diagonal = mg_diagonal(mg_stencil(coarse, nc, bc, c), coarse);
            z = select(0.0, rhs / diagonal, diagonal > 0.0);
        }
        mg_solution[i] = z;
    }
}

// A neighbour's value on `fine` with the coarser level's correction added (0 for no unknown).
fn mg_corrected(fine_base: u32, coarse_base: u32, n: u32, neighbour: i32) -> f32 {
    if (neighbour < 0) {
        return 0.0;
    }
    let index = u32(neighbour);
    let cell = vec3<u32>(index % n, (index / n) % n, index / (n * n));
    return mg_solution[fine_base + index] + mg_solution[coarse_base + mg_index(n / 2u, cell / 2u)];
}

// Adds the coarser level's correction and smooths the black cells in one go. A black cell's new value
// needs only its red neighbours, each corrected here on the fly, so the red cells' corrections need not
// be stored: the red sweep that follows recomputes those cells from the black ones. Exactly the
// correction, then a black sweep, then a red one.
fn mg_prolong_smooth(fine: u32, cell: vec3<u32>) {
    let nf = mg_level_res(fine);
    if (any(cell >= vec3<u32>(nf)) || ((cell.x + cell.y + cell.z) & 1u) != 1u) {
        return;
    }
    let bf = mg_level_base(fine);
    let bc = mg_level_base(fine + 1u);
    let c = vec3<i32>(cell);
    let i = bf + mg_index(nf, cell);
    if (mg_solid(fine, nf, bf, c)) {
        mg_solution[i] = 0.0;
        return;
    }
    let s = mg_stencil(fine, nf, bf, c);
    let diagonal = mg_diagonal(s, fine);
    let sum = mg_corrected(bf, bc, nf, s.xm) + mg_corrected(bf, bc, nf, s.xp)
        + mg_corrected(bf, bc, nf, s.ym) + mg_corrected(bf, bc, nf, s.yp)
        + mg_corrected(bf, bc, nf, s.zm) + mg_corrected(bf, bc, nf, s.zp);
    mg_solution[i] = select(0.0, (mg_rhs[i] + sum) / diagonal, diagonal > 0.0);
}

// The solid flags, level by level from the fine one: a fine cell is solid inside a collider, a coarse
// one when all 8 of its children are.
fn mg_coarsen(level: u32, cell: vec3<u32>) {
    let n = mg_level_res(level);
    if (any(cell >= vec3<u32>(n))) {
        return;
    }
    if (level == 0u) {
        let i = mg_index(n, cell);
        mg_flags[i] = select(0u, 1u, solid[i].w > 0.5);
        return;
    }
    let fine = level - 1u;
    let nf = mg_level_res(fine);
    let bf = mg_level_base(fine);
    let o = vec3<i32>(cell) * 2;
    let all_solid = mg_solid(fine, nf, bf, o) && mg_solid(fine, nf, bf, o + X)
        && mg_solid(fine, nf, bf, o + Y) && mg_solid(fine, nf, bf, o + X + Y)
        && mg_solid(fine, nf, bf, o + Z) && mg_solid(fine, nf, bf, o + X + Z)
        && mg_solid(fine, nf, bf, o + Y + Z) && mg_solid(fine, nf, bf, o + X + Y + Z);
    mg_flags[mg_level_base(level) + mg_index(n, cell)] = select(0u, 1u, all_solid);
}

// ---- PCG on the fine grid ----

fn pcg_groups() -> u32 {
    return grid_res() / 4u;
}

fn pcg_group_index(group: vec3<u32>) -> u32 {
    let g = pcg_groups();
    return (group.z * g + group.y) * g + group.x;
}

// The whole grid's sum of the per-workgroup partials, in a fixed order: each invocation a strided run,
// then the workgroup tree. Called by a single-workgroup pass.
fn pcg_total(local: u32) -> vec4<f32> {
    let g = pcg_groups();
    var own = vec4<f32>(0.0);
    for (var i = local; i < g * g * g; i += 64u) {
        own += pcg_reduction[PCG_PARTIALS + i];
    }
    return aestra_workgroup_sum(local, own);
}

fn pcg_fluid(cell: vec3<u32>) -> bool {
    return in_grid(cell) && !mg_solid(0u, grid_res(), 0u, vec3<i32>(cell));
}

fn pressure_at(neighbour: i32) -> f32 {
    if (neighbour < 0) {
        return 0.0;
    }
    return pressure[u32(neighbour)];
}

// The solve starts from the previous tick's pressure (it persists; a warm start): r = b − A·x with
// b = −h²·∇·u on the fluid cells. Partials of Σb, Σb², the fluid cell count.
@compute @workgroup_size(4, 4, 4)
fn pcg_setup(
    @builtin(global_invocation_id) cell: vec3<u32>,
    @builtin(local_invocation_index) local: u32,
    @builtin(workgroup_id) group: vec3<u32>,
) {
    var b = 0.0;
    var fluid = 0.0;
    var r = 0.0;
    if (pcg_fluid(cell)) {
        let h = cell_size();
        let i = cell_index(cell);
        b = -h * h * divergence[i];
        fluid = 1.0;
        let s = mg_stencil(0u, grid_res(), 0u, vec3<i32>(cell));
        let neighbours = pressure_at(s.xm) + pressure_at(s.xp) + pressure_at(s.ym)
            + pressure_at(s.yp) + pressure_at(s.zm) + pressure_at(s.zp);
        r = b - (mg_diagonal(s, 0u) * pressure[i] - neighbours);
    }
    if (in_grid(cell)) {
        mg_rhs[cell_index(cell)] = r;
    }
    let total = aestra_workgroup_sum(local, vec4<f32>(b, b * b, fluid, 0.0));
    if (local == 0u) {
        pcg_reduction[PCG_PARTIALS + pcg_group_index(group)] = total;
    }
}

// A domain closed on every side holds pressure only up to a constant: its system is consistent only
// when b sums to zero, so b loses its mean. |b|² (of what is solved) sets the relative residual.
@compute @workgroup_size(64)
fn pcg_setup_finalize(@builtin(local_invocation_index) local: u32) {
    let total = pcg_total(local);
    if (local == 0u) {
        let closed = open_sides() == 0u && total.z > 0.0;
        let mean = select(0.0, total.x / max(total.z, 1.0), closed);
        pcg_reduction[1] = vec4<f32>(max(total.y - total.z * mean * mean, 0.0), mean, 0.0, 0.0);
        // r·z: any non-zero — the first iteration's direction is z plus a multiple of the zero p.
        pcg_reduction[0] = vec4<f32>(1.0, 1.0, 0.0, 0.0);
    }
}

// The first fine-level sweep's red half, from a zero guess: needs only the cell's own residual.
fn pcg_first_red(cell: vec3<u32>, i: u32, r: f32) {
    if (((cell.x + cell.y + cell.z) & 1u) == 0u) {
        var z = 0.0;
        if (pcg_fluid(cell)) {
            let diagonal = mg_diagonal(mg_stencil(0u, grid_res(), 0u, vec3<i32>(cell)), 0u);
            z = r / diagonal;
        }
        mg_solution[i] = z;
    }
}

// r loses b's mean (in a closed domain), and the first iteration's V-cycle starts: its red sweep.
@compute @workgroup_size(4, 4, 4)
fn pcg_start(@builtin(global_invocation_id) cell: vec3<u32>) {
    if (!in_grid(cell)) {
        return;
    }
    let i = cell_index(cell);
    var r = mg_rhs[i];
    if (pcg_fluid(cell)) {
        r = r - pcg_reduction[1].y;
        mg_rhs[i] = r;
    }
    pcg_first_red(cell, i, r);
}
// The V-cycle's last pass: the fine level's final red sweep, and the partials of r·z — the black
// cells are final already, each red one once this invocation has swept it.
@compute @workgroup_size(4, 4, 4)
fn pcg_smooth_dot(
    @builtin(global_invocation_id) cell: vec3<u32>,
    @builtin(local_invocation_index) local: u32,
    @builtin(workgroup_id) group: vec3<u32>,
) {
    var rz = 0.0;
    if (in_grid(cell)) {
        let i = cell_index(cell);
        var z = mg_solution[i];
        if (((cell.x + cell.y + cell.z) & 1u) == 0u) {
            z = 0.0;
            let c = vec3<i32>(cell);
            if (!mg_solid(0u, grid_res(), 0u, c)) {
                let s = mg_stencil(0u, grid_res(), 0u, c);
                let diagonal = mg_diagonal(s, 0u);
                z = select(0.0, (mg_rhs[i] + mg_solution_sum(0u, s)) / diagonal, diagonal > 0.0);
            }
            mg_solution[i] = z;
        }
        rz = mg_rhs[i] * z;
    }
    let total = aestra_workgroup_sum(local, vec4<f32>(rz, 0.0, 0.0, 0.0));
    if (local == 0u) {
        pcg_reduction[PCG_PARTIALS + pcg_group_index(group)] = total;
    }
}

@compute @workgroup_size(64)
fn pcg_beta(@builtin(local_invocation_index) local: u32) {
    let rz = pcg_total(local).x;
    if (local == 0u) {
        let previous = pcg_reduction[0].y;
        pcg_reduction[0].w = select(0.0, rz / previous, previous != 0.0);
        pcg_reduction[0].y = rz;
    }
}

fn solution_at(neighbour: i32) -> f32 {
    if (neighbour < 0) {
        return 0.0;
    }
    return mg_solution[u32(neighbour)];
}

// p = z + β·p and q = A·p, in one pass: q = A·z + β·q by linearity, which needs only z's neighbours —
// nothing writes z here — while each invocation updates only its own p and q. Partials of p·q.
@compute @workgroup_size(4, 4, 4)
fn pcg_apply(
    @builtin(global_invocation_id) cell: vec3<u32>,
    @builtin(local_invocation_index) local: u32,
    @builtin(workgroup_id) group: vec3<u32>,
) {
    var pq = 0.0;
    if (in_grid(cell)) {
        let i = cell_index(cell);
        let beta = pcg_reduction[0].w;
        var az = 0.0;
        if (pcg_fluid(cell)) {
            let s = mg_stencil(0u, grid_res(), 0u, vec3<i32>(cell));
            let neighbours = solution_at(s.xm) + solution_at(s.xp) + solution_at(s.ym)
                + solution_at(s.yp) + solution_at(s.zm) + solution_at(s.zp);
            az = mg_diagonal(s, 0u) * mg_solution[i] - neighbours;
        }
        let pq_old = pcg_vectors[i];
        let p = mg_solution[i] + beta * pq_old.x;
        let q = az + beta * pq_old.y;
        pcg_vectors[i] = vec2<f32>(p, q);
        pq = p * q;
    }
    let total = aestra_workgroup_sum(local, vec4<f32>(pq, 0.0, 0.0, 0.0));
    if (local == 0u) {
        pcg_reduction[PCG_PARTIALS + pcg_group_index(group)] = total;
    }
}
@compute @workgroup_size(64)
fn pcg_alpha(@builtin(local_invocation_index) local: u32) {
    let pq = pcg_total(local).x;
    if (local == 0u) {
        pcg_reduction[0].z = select(0.0, pcg_reduction[0].y / pq, pq > 0.0);
    }
}

// x += α·p, r −= α·q, and the next iteration's V-cycle starts: its red sweep. Partials of r·r.
@compute @workgroup_size(4, 4, 4)
fn pcg_step(
    @builtin(global_invocation_id) cell: vec3<u32>,
    @builtin(local_invocation_index) local: u32,
    @builtin(workgroup_id) group: vec3<u32>,
) {
    var rr = 0.0;
    if (in_grid(cell)) {
        let i = cell_index(cell);
        let alpha = pcg_reduction[0].z;
        let pq = pcg_vectors[i];
        pressure[i] = pressure[i] + alpha * pq.x;
        let r = mg_rhs[i] - alpha * pq.y;
        mg_rhs[i] = r;
        rr = r * r;
        pcg_first_red(cell, i, r);
    }
    let total = aestra_workgroup_sum(local, vec4<f32>(rr, 0.0, 0.0, 0.0));
    if (local == 0u) {
        pcg_reduction[PCG_PARTIALS + pcg_group_index(group)] = total;
    }
}

// The relative residual |r| / |b| the convergence test reads (0 when there is nothing to solve).
@compute @workgroup_size(64)
fn pcg_residual(@builtin(local_invocation_index) local: u32) {
    let rr = pcg_total(local).x;
    if (local == 0u) {
        let rr0 = pcg_reduction[1].x;
        pcg_reduction[0].x = select(0.0, sqrt(rr / rr0), rr0 > 0.0);
    }
}
