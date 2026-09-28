// Aestra Fluid — a liquid against the world collider (fluid F11), composed with `world.wgsl` into the
// liquid program: after each grid-to-particle step, a particle inside the world (within the offset)
// moves back out the way it came — along its velocity, backwards, so a thin wall is never crossed —
// else along the surface normal, and loses the velocity it had into the surface.

@compute @workgroup_size(64)
fn liquid_world_collide(@builtin(global_invocation_id) gid: vec3<u32>) {
    let at = gid.x * LIQUID_STRIDE;
    if (gid.x >= liquid_header[0] || liquid_particles[at].w < 0.5) {
        return;
    }
    let x = liquid_particles[at].xyz;
    let world = effect_to_world(x);
    let distance = aestra_world_sdf_distance(world);
    if (distance >= world_offset()) {
        return;
    }
    var pushed = world + aestra_world_sdf_normal(world) * (world_offset() - distance);
    let v_world = effect_to_world_vector(liquid_particles[at + 1u].xyz);
    let speed = length(v_world);
    if (speed > 1e-6) {
        // Back along its path, in half-voxel steps, over at most two steps' travel and a voxel.
        let step = 0.5 * aestra_world_sdf_voxel_size();
        let reach = speed * 2.0 * liquid_dt() + 2.0 * step;
        let steps = min(u32(ceil(reach / step)), 64u);
        let back = -v_world / speed;
        for (var k = 1u; k <= steps; k += 1u) {
            let q = world + back * (step * f32(k));
            if (aestra_world_sdf_distance(q) >= world_offset()) {
                pushed = q;
                break;
            }
        }
    }
    liquid_particles[at] = vec4<f32>(world_to_effect(pushed, 1.0), liquid_particles[at].w);
    let n = world_to_effect(aestra_world_sdf_normal(pushed), 0.0);
    let length_squared = dot(n, n);
    if (length_squared > 1e-12) {
        let unit = n / sqrt(length_squared);
        let v = liquid_particles[at + 1u].xyz;
        let into = min(dot(v, unit), 0.0);
        liquid_particles[at + 1u] = vec4<f32>(v - into * unit, liquid_particles[at + 1u].w);
    }
}
