struct View {
    clip_from_world: mat4x4<f32>,
    unjittered_clip_from_world: mat4x4<f32>,
    world_from_clip: mat4x4<f32>,
    world_from_view: mat4x4<f32>,
}

// Native particle presentation. The vertex shader expands one compacted alive
// particle into a camera-space quad without a CPU vertex or instance buffer.

struct Renderer {
    emitter_index: u32,
    blend_mode: u32,
    softness: f32,
    textured: u32,
    uv_min: vec2<f32>,
    uv_max: vec2<f32>,
    tint: vec4<f32>,
    particle_color: u32,
    renderer_kind: u32,
    frame_count: u32,
    playback_mode: u32,
    flipbook_flags: u32,
    frame_rate: f32,
    _padding: vec3<f32>,
    frames: array<vec4<f32>, 64>,
}

struct Particle {
    color: vec4<f32>,
    position: vec2<f32>,
    size: f32,
    rotation: f32,
    normalized_age: f32,
    emitter_index: u32,
    alive: u32,
    particle_index: u32,
}

struct RenderGlobals {
    world_from_effect: mat4x4<f32>,
    time: f32,
    seed: u32,
    _padding: vec2<f32>,
}

struct RenderParams {
    renderer_index: u32,
    _padding: vec3<u32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) quad_position: vec2<f32>,
    @location(2) softness: f32,
    @location(3) @interpolate(flat) visible: u32,
    @location(4) uv: vec2<f32>,
    @location(5) @interpolate(flat) textured: u32,
}

@group(0) @binding(0) var<uniform> view: View;
@group(1) @binding(0) var<storage, read> renderers: array<Renderer>;
@group(1) @binding(1) var<storage, read> particles: array<Particle>;
@group(1) @binding(2) var<storage, read> alive_indices: array<u32>;
@group(1) @binding(3) var<storage, read> globals: RenderGlobals;
@group(1) @binding(4) var<storage, read> params: RenderParams;
@group(1) @binding(5) var sprite_texture: texture_2d<f32>;
@group(1) @binding(6) var sprite_sampler: sampler;

fn hash01(index: u32, channel: u32) -> f32 {
    var result = (index * 0x9e3779b9u) ^ (channel * 0x85ebca6bu) ^ globals.seed;
    result ^= result >> 16u;
    result *= 0x7feb352du;
    result ^= result >> 15u;
    result *= 0x846ca68bu;
    result ^= result >> 16u;
    return f32(result) / 4294967295.0;
}

fn flipbook_frame(renderer: Renderer, particle: Particle) -> u32 {
    let count = max(renderer.frame_count, 1u);
    if count <= 1u {
        return 0u;
    }
    let effect_time = (renderer.flipbook_flags & 1u) != 0u;
    let random_start = (renderer.flipbook_flags & 2u) != 0u;
    let looping = (renderer.flipbook_flags & 4u) != 0u;
    let seconds = select(
        clamp(particle.normalized_age, 0.0, 1.0) * f32(count) / renderer.frame_rate,
        max(globals.time, 0.0),
        effect_time,
    );
    var frame = u32(floor(seconds * renderer.frame_rate));
    if random_start {
        frame += u32(hash01(particle.particle_index, 9u) * f32(count));
    }
    var selected = min(frame, count - 1u);
    if looping {
        selected = frame % count;
    }
    if renderer.playback_mode == 1u {
        return count - 1u - selected;
    }
    if renderer.playback_mode == 2u {
        let period = (count - 1u) * 2u;
        var value = min(frame, period);
        if looping {
            value = frame % period;
        }
        return select(value, period - value, value >= count);
    }
    return selected;
}

@vertex
fn vertex(
    @builtin(vertex_index) vertex_index: u32,
    @builtin(instance_index) instance_index: u32,
) -> VertexOutput {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(-1.0, 1.0),
    );
    let corner = corners[vertex_index];
    let particle_index = alive_indices[instance_index];
    let particle = particles[particle_index];
    let renderer = renderers[params.renderer_index];
    let sine = sin(particle.rotation);
    let cosine = cos(particle.rotation);
    let rotated = vec2<f32>(
        corner.x * cosine - corner.y * sine,
        corner.x * sine + corner.y * cosine,
    ) * particle.size * 0.5;
    let world_center = globals.world_from_effect * vec4<f32>(particle.position, 0.0, 1.0);
    let effect_scale_x = length(globals.world_from_effect[0].xyz);
    let effect_scale_y = length(globals.world_from_effect[1].xyz);
    let camera_right = normalize(view.world_from_view[0].xyz);
    let camera_up = normalize(view.world_from_view[1].xyz);
    let world_position = world_center
        + vec4<f32>(
            camera_right * rotated.x * effect_scale_x
                + camera_up * rotated.y * effect_scale_y,
            0.0,
        );

    var output: VertexOutput;
    output.clip_position = view.clip_from_world * world_position;
    output.color = renderer.tint;
    if renderer.particle_color != 0u {
        output.color *= particle.color;
    }
    output.quad_position = corner;
    output.softness = renderer.softness;
    output.visible = select(0u, 1u, particle.emitter_index == renderer.emitter_index);
    var uv_bounds = vec4<f32>(renderer.uv_min, renderer.uv_max);
    if renderer.renderer_kind == 1u {
        uv_bounds = renderer.frames[flipbook_frame(renderer, particle)];
    }
    output.uv = mix(uv_bounds.xy, uv_bounds.zw, corner * 0.5 + vec2<f32>(0.5));
    output.textured = renderer.textured;
    return output;
}

fn particle_color(input: VertexOutput) -> vec4<f32> {
    let feather = clamp(input.softness, 0.001, 1.0);
    var distance = length(input.quad_position);
    var sampled = vec4<f32>(1.0);
    if input.textured != 0u {
        distance = max(abs(input.quad_position.x), abs(input.quad_position.y));
        sampled = textureSample(sprite_texture, sprite_sampler, input.uv);
    }
    let coverage = 1.0 - smoothstep(1.0 - feather, 1.0, distance);
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
    let edge_distance = max(abs(input.quad_position.x), abs(input.quad_position.y));
    let line_width = max(fwidth(edge_distance) * 1.35, 0.012);
    let coverage = smoothstep(1.0 - line_width, 1.0, edge_distance);
    if coverage <= 0.01 {
        discard;
    }
    let wire_color = mix(input.color.rgb, vec3<f32>(0.72, 0.56, 1.0), 0.65);
    return vec4<f32>(wire_color, coverage * 0.92);
}
