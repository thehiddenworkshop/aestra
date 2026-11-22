// A beam consumes the usual four-vertex indirect draw for each live particle.
fn aestra_beam_vertex(vertex_index: u32, instance_index: u32) -> SpriteVertexData {
    let r = renderers[params.renderer_index];
    let slot = alive_indices[params.alive_offset + instance_index];
    let particle = particles[slot];
    let start = (globals.world_from_effect * vec4<f32>(particle.position + r.frames[0].xyz, 1.0)).xyz;
    let end = (globals.world_from_effect * vec4<f32>(particle.position + r.frames[1].xyz, 1.0)).xyz;
    let delta = end - start;
    var output: SpriteVertexData;
    output.clip_position = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    if dot(delta, delta) < 1e-12 { return output; }

    let direction = normalize(delta);
    let forward = ribbon_unit(view.world_from_view[2].xyz, vec3<f32>(0.0, 0.0, 1.0));
    let side = ribbon_unit(cross(direction, forward), ribbon_unit(view.world_from_view[0].xyz, vec3<f32>(1.0, 0.0, 0.0)));
    let scale = max(length(globals.world_from_effect[0].xyz), max(length(globals.world_from_effect[1].xyz), length(globals.world_from_effect[2].xyz)));
    let corner = vec2<f32>(f32(vertex_index % 2u), f32(vertex_index / 2u));
    let across = corner.x * 2.0 - 1.0;
    let endpoint = mix(start, end, corner.y);
    let position = endpoint + side * (across * particle.size * r.frames[0].w * scale * 0.5);
    let beam_clip_position = view.clip_from_world * vec4<f32>(position, 1.0);
    output.clip_position = beam_clip_position;
    output.world_position = position;
    output.color = r.tint;
    if r.particle_color != 0u {
        if (r.attribute_flags.x & 8u) == 0u { output.color = vec4<f32>(output.color.rgb * particle.color.rgb, output.color.a); }
        if (r.attribute_flags.x & 16u) == 0u { output.color.a *= particle.color.a; }
    }
    output.quad_position = vec2<f32>(across, corner.y * 2.0 - 1.0);
    output.uv = mix(r.uv_min, r.uv_max, corner.yx);
    output.ribbon_direction = direction;
    output.visible = select(0u, 1u, particle_emitter(particle) == r.emitter_index);
    output.softness = r.softness;
    output.textured = r.textured | 2u;
    output.effect_time = globals.time;
    if (r.attribute_flags.x & 32u) == 0u { output.particle_normalized_age = particle.normalized_age; }
    output.particle_random = aestra_particle_random(particle);
    output.sampling_coverage = f32(1);
    return output;
}
