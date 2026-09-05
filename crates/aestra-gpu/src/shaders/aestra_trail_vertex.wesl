fn trail_slot(base: u32, capacity: u32, index: u32) -> u32 {
    let head = particles[base];
    if index == head._padding_1 { return base; }
    return base + 1u + (head._padding_0 + capacity - head._padding_1 + index) % capacity;
}

fn aestra_trail_vertex(vertex_index: u32, instance_index: u32) -> SpriteVertexData {
    var output: SpriteVertexData;
    output.clip_position = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    let r = renderers[params.renderer_index];
    let capacity = r.frame_count - 1u;
    let base = r.attribute_flags.z + 1u + (instance_index / capacity) * r.frame_count;
    let segment = instance_index % capacity;
    let count = particles[base]._padding_1;
    if particles[base].alive == 0u || segment >= count { return output; }
    let start = trail_slot(base, capacity, segment);
    let end = trail_slot(base, capacity, segment + 1u);
    let delta = particles[end].position - particles[start].position;
    if dot(delta, delta) < 1e-12 || globals.time - particles[end].rotation >= r.frame_rate { return output; }
    // Four-vertex triangle strip (see aestra_sprite_vertex): same corner set/winding.
    let corners = array<vec2<f32>, 4>(vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, 1.0));
    let corner = corners[vertex_index];
    let index = segment + select(0u, 1u, corner.y > 0.0);
    let slot = trail_slot(base, capacity, index);
    let before = trail_slot(base, capacity, select(0u, index - 1u, index > 0u));
    let after = trail_slot(base, capacity, min(count, index + 1u));
    let direction = ribbon_unit(particles[after].position - particles[before].position, ribbon_unit(delta, vec3<f32>(1.0, 0.0, 0.0)));
    let forward = ribbon_unit(view.world_from_view[2].xyz, vec3<f32>(0.0, 0.0, 1.0));
    let side = ribbon_unit(cross(direction, forward), ribbon_unit(view.world_from_view[0].xyz, vec3<f32>(1.0, 0.0, 0.0)));
    let fade = clamp(1.0 - (globals.time - particles[slot].rotation) / r.frame_rate, 0.0, 1.0);
    let width = particles[slot].size * bitcast<f32>(r.attribute_flags.y) * fade;
    let position = particles[slot].position + side * corner.x * width * 0.5;
    output.clip_position = view.clip_from_world * vec4<f32>(position, 1.0);
    output.color = r.tint;
    if r.particle_color != 0u { output.color *= particles[slot].color; }
    // Strip coverage needs only X. Carry age fade in the otherwise unused Y lane
    // so even a constant-alpha semantic material receives presentation fading.
    output.quad_position = vec2<f32>(corner.x, fade);
    output.uv = vec2<f32>(fade, corner.x * 0.5 + 0.5);
    output.ribbon_direction = direction;
    output.visible = 1u;
    output.softness = r.softness;
    output.textured = r.textured | 6u;
    output.effect_time = globals.time;
    output.particle_normalized_age = particles[slot].normalized_age;
    return output;
}
