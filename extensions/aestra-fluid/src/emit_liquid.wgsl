// Aestra Fluid — a liquid's secondary emission (fluid F10), composed with `emit.wgsl` into the liquid
// program: its candidates are its particles — spray, the ones flying clear of the liquid (their
// cell's liquid fraction under the threshold) at least the minimum speed. A pass covers the particles
// in workgroups of 64, the plan's word 4.

@compute @workgroup_size(64)
fn emit_particles(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_index) local: u32,
    @builtin(workgroup_id) group: vec3<u32>,
) {
    var chosen = false;
    let at = gid.x * LIQUID_STRIDE;
    if (gid.x < liquid_header[0] && liquid_particles[at].w >= 0.5) {
        let x = liquid_particles[at].xyz;
        let cell = clamp(
            vec3<i32>(floor((x - grid_origin()) / cell_size())),
            vec3<i32>(0),
            vec3<i32>(i32(grid_res()) - 1),
        );
        chosen = density[cell_index(vec3<u32>(cell))] < emission_threshold()
            && length(liquid_particles[at + 1u].xyz) >= emission_min_speed()
            && emission_chosen(gid.x);
    }
    emission_rank(local, group.x, chosen);
}

// A chosen particle's record: where it is, as fast as it goes.
@compute @workgroup_size(64)
fn emit_particles_write(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_index) local: u32,
    @builtin(workgroup_id) group: vec3<u32>,
) {
    let slot = emission_slot(local, group.x);
    if (slot >= emission_capacity()) {
        return;
    }
    let at = gid.x * LIQUID_STRIDE;
    emission_write(slot, liquid_particles[at].xyz, liquid_particles[at + 1u].xyz);
}
