// Aestra Fluid — the force on each collider (fluid F11), reported to the host as a stage output: the
// pressure of every fluid cell pushing, through each face it shares with a cell inside the collider,
// on that face (with a density of 1). Summed in a fixed tree — per workgroup, then one workgroup over
// the partials — so a rerun gives the same bits. The host reads the outputs after each frame's ticks
// and zeroes them; each collider's record keeps the frame's strongest push: force xyz, then its size.
// Composed with the solver into every fluid program.

@group(0) @binding(39) var<storage, read_write> collider_forces: array<vec4<f32>>;
@group(0) @binding(40) var<storage, read_write> fluid_outputs: array<f32>;

// Colliders a stage reports forces for (lib.rs's MAX_COLLIDERS), and words per record.
const FORCE_COLLIDERS: u32 = 4u;
const FORCE_RECORD: u32 = 4u;

// The step the pressure was solved for: a liquid's substep, else the tick.
fn pressure_dt() -> f32 {
    if (free_surface()) {
        return frame_dt() / f32(max(constants[liquid_base() + 1u], 1u));
    }
    return frame_dt();
}

// The force a fluid cell's pressure puts on collider `c` through its faces with the collider's cells.
fn cell_force(cell: vec3<u32>, c: u32) -> vec3<f32> {
    let here = vec3<i32>(cell);
    if (c >= collider_count() || !in_grid(cell) || is_solid(here)) {
        return vec3<f32>(0.0);
    }
    let base = collider_base() + c * COLLIDER_WORDS;
    let h = cell_size();
    let push = pressure[cell_index(cell)] / pressure_dt() * h * h;
    var force = vec3<f32>(0.0);
    for (var axis = 0u; axis < 3u; axis += 1u) {
        for (var side = -1; side <= 1; side += 2) {
            let step = axis_step(axis) * side;
            let neighbour = here + step;
            let center = grid_origin() + (vec3<f32>(neighbour) + vec3<f32>(0.5)) * h;
            if (inside(neighbour) && collider_distance(base, center) < 0.0) {
                force += push * vec3<f32>(step);
            }
        }
    }
    return force;
}

// Each workgroup's partial force on every collider.
@compute @workgroup_size(4, 4, 4)
fn measure_collider_forces(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_index) local: u32,
    @builtin(workgroup_id) group: vec3<u32>,
) {
    let cell = grid_cell(gid);
    let partial = grid_group_index(group) * FORCE_COLLIDERS;
    for (var c = 0u; c < FORCE_COLLIDERS; c += 1u) {
        let total = aestra_workgroup_sum(local, vec4<f32>(cell_force(cell, c), 0.0));
        if (local == 0u) {
            collider_forces[partial + c] = total;
        }
    }
}

// One workgroup: every collider's total, kept when it is the frame's strongest.
@compute @workgroup_size(64)
fn collider_force_total(@builtin(local_invocation_index) local: u32) {
    for (var c = 0u; c < FORCE_COLLIDERS; c += 1u) {
        var own = vec4<f32>(0.0);
        for (var g = local; g < grid_group_count(); g += 64u) {
            own += collider_forces[g * FORCE_COLLIDERS + c];
        }
        let total = aestra_workgroup_sum(local, own);
        let at = c * FORCE_RECORD;
        let size = length(total.xyz);
        if (local == 0u && c < collider_count() && size > fluid_outputs[at + 3u]) {
            fluid_outputs[at] = total.x;
            fluid_outputs[at + 1u] = total.y;
            fluid_outputs[at + 2u] = total.z;
            fluid_outputs[at + 3u] = size;
        }
    }
}
