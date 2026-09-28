// Aestra Fluid — the world collider (fluid F11): the host's scene geometry, a signed distance volume in
// world space (`aestra_gpu::WORLD_SDF_WGSL`), marks the cells inside it solid — still, with the
// collider's boundary condition — and clears the smoke there. Composed with the solver into every
// fluid program; the liquid also pushes its particles out (`world_liquid.wgsl`).

@group(0) @binding(38) var<storage, read> aestra_world_sdf: array<u32>;

// The World Collider's words in the header: present, surface offset, sticky.
fn world_collider() -> bool { return constants[24] != 0u; }
fn world_offset() -> f32 { return bitcast<f32>(constants[25]); }
fn world_no_slip() -> bool { return constants[26] != 0u; }

// The effect's rows of world_to_effect, and their inverse applied to a point: effect space back to
// world space (the rows' adjugate over their determinant).
fn world_row(row: u32) -> vec3<f32> {
    let base = 4u + row * 4u;
    return vec3<f32>(bitcast<f32>(frame[base]), bitcast<f32>(frame[base + 1u]), bitcast<f32>(frame[base + 2u]));
}

fn effect_to_world_vector(q: vec3<f32>) -> vec3<f32> {
    let r0 = world_row(0u);
    let r1 = world_row(1u);
    let r2 = world_row(2u);
    let c0 = cross(r1, r2);
    let c1 = cross(r2, r0);
    let c2 = cross(r0, r1);
    return (c0 * q.x + c1 * q.y + c2 * q.z) / dot(r0, c0);
}

fn effect_to_world(p: vec3<f32>) -> vec3<f32> {
    return effect_to_world_vector(
        p - vec3<f32>(bitcast<f32>(frame[7]), bitcast<f32>(frame[11]), bitcast<f32>(frame[15])),
    );
}

// The cells whose centre lies inside the world (within the offset) become still solids.
@compute @workgroup_size(4, 4, 4)
fn mark_world_solids(@builtin(global_invocation_id) gid: vec3<u32>) {
    let cell = grid_cell(gid);
    if (!in_grid(cell)) { return; }
    let i = cell_index(cell);
    let center = grid_origin() + (vec3<f32>(cell) + vec3<f32>(0.5)) * cell_size();
    if (aestra_world_sdf_distance(effect_to_world(center)) < world_offset()) {
        solid[i] = vec4<f32>(0.0, 0.0, 0.0, select(1.0, 2.0, world_no_slip()));
        density[i] = 0.0;
    }
}
