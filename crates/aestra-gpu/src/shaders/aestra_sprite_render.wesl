// Legacy sprite entry points. Shared geometry is in aestra_sprite_vertex.wesl.
struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) quad_position: vec2<f32>,
    @location(2) softness: f32,
    @location(3) @interpolate(flat) visible: u32,
    @location(4) uv: vec2<f32>,
    @location(5) @interpolate(flat) textured: u32,
}

@vertex
fn vertex(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    let sprite = aestra_sprite_vertex(vertex_index, instance_index);
    var output: VertexOutput;
    output.clip_position = sprite.clip_position;
    output.color = sprite.color;
    output.quad_position = sprite.quad_position;
    output.softness = sprite.softness;
    output.visible = sprite.visible;
    output.uv = sprite.uv;
    output.textured = sprite.textured;
    return output;
}

fn particle_color(input: VertexOutput) -> vec4<f32> {
    let feather = clamp(input.softness, 0.001, 1.0);
    var distance = length(input.quad_position);
    var sampled = vec4<f32>(1.0);
    if (input.textured & 1u) != 0u {
        distance = max(abs(input.quad_position.x), abs(input.quad_position.y));
        sampled = textureSample(sprite_texture, sprite_sampler, input.uv);
    }
    if (input.textured & 2u) != 0u { distance = abs(input.quad_position.x); }
    var coverage = 1.0 - smoothstep(1.0 - feather, 1.0, distance);
    if (input.textured & 4u) != 0u { coverage *= input.quad_position.y; }
    return vec4<f32>(input.color.rgb * sampled.rgb, input.color.a * sampled.a * coverage);
}

@fragment
fn fragment_alpha(input: VertexOutput) -> @location(0) vec4<f32> {
    if input.visible == 0u {
        discard;
    }
    return particle_color(input);
}

@fragment
fn fragment_additive(input: VertexOutput) -> @location(0) vec4<f32> {
    if input.visible == 0u {
        discard;
    }
    return particle_color(input);
}

@fragment
fn fragment_multiply(input: VertexOutput) -> @location(0) vec4<f32> {
    if input.visible == 0u {
        discard;
    }
    let color = particle_color(input);
    return vec4<f32>(mix(vec3<f32>(1.0), color.rgb, color.a), color.a);
}

@fragment
fn fragment_wireframe(input: VertexOutput) -> @location(0) vec4<f32> {
    if input.visible == 0u {
        discard;
    }
    var edge_distance = max(abs(input.quad_position.x), abs(input.quad_position.y));
    if (input.textured & 4u) != 0u { edge_distance = abs(input.quad_position.x); }
    let line_width = max(fwidth(edge_distance) * 1.35, 0.012);
    var coverage = smoothstep(1.0 - line_width, 1.0, edge_distance);
    if (input.textured & 4u) != 0u { coverage *= input.quad_position.y; }
    if coverage <= 0.01 {
        discard;
    }
    let wire_color = mix(input.color.rgb, vec3<f32>(0.72, 0.56, 1.0), 0.65);
    return vec4<f32>(wire_color, coverage * 0.92);
}
