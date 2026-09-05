// One quad per alive point; the final point is hidden. Shared point frames keep joins closed.
fn ribbon_unit(v: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
    let square = dot(v, v);
    if square < 1e-12 { return fallback; }
    return v * inverseSqrt(square);
}

fn ribbon_world(slot: u32) -> vec3<f32> {
    return (globals.world_from_effect * vec4<f32>(particles[slot].position, 1.0)).xyz;
}

fn ribbon_direction(slot: u32) -> vec3<f32> {
    let point = ribbon_world(slot);
    var before = point;
    var after = point;
    let previous = particles[slot]._padding_1;
    let next = particles[slot]._padding_0;
    if previous != 0xffffffffu { before = ribbon_world(previous); }
    if next != 0xffffffffu { after = ribbon_world(next); }
    return ribbon_unit(after - before, ribbon_unit(after - point, ribbon_unit(point - before, vec3<f32>(1.0, 0.0, 0.0))));
}

fn aestra_ribbon_vertex(vertex_index: u32, instance_index: u32) -> SpriteVertexData {
    var output: SpriteVertexData;
    let renderer = renderers[params.renderer_index];
    let start = alive_indices[params.alive_offset + instance_index];
    let end = particles[start]._padding_0;
    output.clip_position = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    if end == 0xffffffffu { return output; }
    let delta = ribbon_world(end) - ribbon_world(start);
    if dot(delta, delta) < 1e-12 { return output; }
    let corners = array<vec2<f32>, 6>(vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0));
    let corner = corners[vertex_index];
    let slot = select(start, end, corner.y > 0.0);
    let direction = ribbon_direction(slot);
    let camera_forward = ribbon_unit(view.world_from_view[2].xyz, vec3<f32>(0.0, 0.0, 1.0));
    // Width then length must wind toward the camera, matching sprite front faces.
    let side = ribbon_unit(cross(direction, camera_forward), ribbon_unit(view.world_from_view[0].xyz, vec3<f32>(1.0, 0.0, 0.0)));
    let scale = max(length(globals.world_from_effect[0].xyz), max(length(globals.world_from_effect[1].xyz), length(globals.world_from_effect[2].xyz)));
    let width = bitcast<f32>(renderer.attribute_flags.y) * particles[slot].size * scale;
    let position = ribbon_world(slot) + side * corner.x * width * 0.5;
    output.clip_position = view.clip_from_world * vec4<f32>(position, 1.0);
    output.color = renderer.tint;
    if renderer.particle_color != 0u {
        if (renderer.attribute_flags.x & 8u) == 0u { output.color = vec4<f32>(output.color.rgb * particles[slot].color.rgb, output.color.a); }
        if (renderer.attribute_flags.x & 16u) == 0u { output.color.a *= particles[slot].color.a; }
    }
    output.quad_position = corner;
    output.uv = vec2<f32>(bitcast<f32>(particles[slot]._padding_2), corner.x * 0.5 + 0.5);
    output.ribbon_direction = direction;
    output.visible = select(0u, 1u, particles[start].emitter_index == renderer.emitter_index && particles[end].emitter_index == renderer.emitter_index);
    output.softness = renderer.softness;
    output.textured = renderer.textured | 2u;
    output.effect_time = globals.time;
    if (renderer.attribute_flags.x & 32u) == 0u { output.particle_normalized_age = particles[slot].normalized_age; }
    return output;
}
