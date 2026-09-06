fn trail_path_distance(base: u32, slot: u32) -> f32 {
    if slot == base { return bitcast<f32>(aux[(base + 1u) * 3u + 1u]); }
    return bitcast<f32>(aux[slot * 3u]);
}

fn aestra_trail_vertex(vertex_index: u32, draw_index: u32) -> SpriteVertexData {
    var instance_index = draw_index;
    if params._padding.x != 0u { instance_index = alive_indices[4u + draw_index]; }
    var output: SpriteVertexData;
    output.clip_position = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    let r = renderers[params.renderer_index];
    let capacity = r.frame_count - 1u;
    // Caps use bounded triangle fans in the same four-vertex strip pipeline.
    // Keep eight wedges in sync with TRAIL_CAP_SEGMENTS on the host.
    let stride = capacity + select(0u, 16u, (r.flipbook_flags & 2u) != 0u);
    let base = r.attribute_flags.z + 1u + (instance_index / stride) * r.frame_count;
    let primitive = instance_index % stride;
    let count = aux[base * 3u + 1u];
    if particle_alive(particles[base]) == 0u || count == 0u { return output; }
    let cap = primitive >= capacity;
    let head_cap = primitive >= capacity + 8u;
    var segment = primitive;
    if cap { segment = select(0u, count - 1u, head_cap); }
    if segment >= count { return output; }
    let start = trail_slot(base, capacity, segment);
    let end = trail_slot(base, capacity, segment + 1u);
    let delta = trail_segment_delta(base, capacity, count, start, end, cap, head_cap);
    if dot(delta, delta) < 1e-12 || globals.time - particles[end].rotation >= r.frame_rate { return output; }
    // Four-vertex triangle strip (see aestra_sprite_vertex): same corner set/winding.
    let corners = array<vec2<f32>, 4>(vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, 1.0));
    var corner = corners[vertex_index];
    var index = segment + select(0u, 1u, corner.y > 0.0);
    var extension = 0.0;
    if cap {
        index = select(0u, count, head_cap);
        // The first vertex is the center; the fourth repeats the third to
        // degenerate the second strip triangle. No overlapping cap/body area.
        let wedge = (primitive - capacity) % 8u;
        let angle = f32(wedge + select(0u, 1u, vertex_index >= 2u)) * 3.14159265359 / 8.0;
        let sign = select(-1.0, 1.0, head_cap);
        corner.x = select(cos(angle) * sign, 0.0, vertex_index == 0u);
        extension = select(sin(angle) * sign, 0.0, vertex_index == 0u);
    }
    let slot = trail_slot(base, capacity, index);
    let before = trail_slot(base, capacity, select(0u, index - 1u, index > 0u));
    let after = trail_slot(base, capacity, min(count, index + 1u));
    let direction = ribbon_unit(particles[after].position - particles[before].position, ribbon_unit(delta, vec3<f32>(1.0, 0.0, 0.0)));
    let forward = ribbon_unit(view.world_from_view[2].xyz, vec3<f32>(0.0, 0.0, 1.0));
    let side = ribbon_unit(cross(direction, forward), ribbon_unit(view.world_from_view[0].xyz, vec3<f32>(1.0, 0.0, 0.0)));
    let fade = clamp(1.0 - (globals.time - particles[slot].rotation) / r.frame_rate, 0.0, 1.0);
    let width = particles[slot].size * bitcast<f32>(r.attribute_flags.y) * fade;
    // Project the tangent into the billboard plane, keeping caps circular
    // when the trajectory points partly toward the camera.
    let tangent = ribbon_unit(cross(forward, side), direction);
    let position = particles[slot].position + (side * corner.x + tangent * extension) * width * 0.5;
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
    if (r.flipbook_flags & 1u) != 0u {
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
