fn trail_slot(base: u32, capacity: u32, index: u32) -> u32 {
    // Ring state (head, count) lives in the shared aux buffer, off the particle ABI.
    let count = aux[base * 3u + 1u];
    if index == count { return base; }
    return base + 1u + (aux[base * 3u] + capacity - count + index) % capacity;
}

fn trail_path_distance(base: u32, slot: u32) -> f32 {
    if slot == base { return bitcast<f32>(aux[(base + 1u) * 3u + 1u]); }
    return bitcast<f32>(aux[slot * 3u]);
}

fn aestra_trail_vertex(vertex_index: u32, instance_index: u32) -> SpriteVertexData {
    var output: SpriteVertexData;
    output.clip_position = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    let r = renderers[params.renderer_index];
    let capacity = r.frame_count - 1u;
    let base = r.attribute_flags.z + 1u + (instance_index / capacity) * r.frame_count;
    let segment = instance_index % capacity;
    let count = aux[base * 3u + 1u];
    if particle_alive(particles[base]) == 0u || segment >= count { return output; }
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
    let distance = trail_path_distance(base, slot);
    let oldest_distance = trail_path_distance(base, trail_slot(base, capacity, 0u));
    let head_distance = trail_path_distance(base, base);
    var u = clamp((distance - oldest_distance) / max(head_distance - oldest_distance, 1e-6), 0.0, 1.0);
    if r.flipbook_flags == 1u {
        // Leave U unwrapped: repeat samplers and shader derivatives must see
        // continuous coordinates across segment joins and tile boundaries.
        u = distance / max(r.frames[0].x, 0.001);
    }
    output.uv = vec2<f32>(u, corner.x * 0.5 + 0.5);
    output.ribbon_direction = direction;
    output.visible = 1u;
    output.softness = r.softness;
    output.textured = r.textured | 6u;
    output.effect_time = globals.time;
    output.particle_normalized_age = particles[slot].normalized_age;
    return output;
}
