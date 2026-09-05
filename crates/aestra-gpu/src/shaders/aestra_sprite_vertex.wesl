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
    attribute_flags: vec3<u32>,
    frames: array<vec4<f32>, 64>,
}

struct Particle {
    color: vec4<f32>,
    position: vec3<f32>,
    size: f32,
    rotation: f32,
    normalized_age: f32,
    emitter_index: u32,
    alive: u32,
    particle_index: u32,
    _padding_0: u32,
    _padding_1: u32,
    _padding_2: u32,
}

struct RenderGlobals {
    world_from_effect: mat4x4<f32>,
    time: f32,
    seed: u32,
    _padding: vec2<f32>,
}

struct RenderParams {
    renderer_index: u32,
    alive_offset: u32,
    _padding: vec2<u32>,
}

struct SpriteVertexData {
    clip_position: vec4<f32>,
    color: vec4<f32>,
    quad_position: vec2<f32>,
    softness: f32,
    visible: u32,
    uv: vec2<f32>,
    textured: u32,
    effect_time: f32,
    particle_normalized_age: f32,
    ribbon_direction: vec3<f32>,
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

fn flipbook_frame(renderer: Renderer, normalized_age: f32, particle_index: u32) -> u32 {
    let count = max(renderer.frame_count, 1u);
    if count <= 1u {
        return 0u;
    }
    let effect_time = (renderer.flipbook_flags & 1u) != 0u;
    let random_start = (renderer.flipbook_flags & 2u) != 0u;
    let looping = (renderer.flipbook_flags & 4u) != 0u;
    let seconds = select(
        clamp(normalized_age, 0.0, 1.0) * f32(count) / renderer.frame_rate,
        max(globals.time, 0.0),
        effect_time,
    );
    var frame = u32(floor(seconds * renderer.frame_rate));
    if random_start {
        frame += u32(hash01(particle_index, 9u) * f32(count));
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

fn aestra_sprite_vertex(
    vertex_index: u32,
    instance_index: u32,
) -> SpriteVertexData {
    if renderers[params.renderer_index].renderer_kind == 3u { return aestra_ribbon_vertex(vertex_index, instance_index); }
    if renderers[params.renderer_index].renderer_kind == 4u { return aestra_trail_vertex(vertex_index, instance_index); }
    // Four-vertex triangle strip (BL, BR, TL, TR): two triangles, CCW/front-facing,
    // so the GPU runs one vertex invocation per corner instead of six per quad —
    // cutting the scattered per-vertex particle gathers that bound the sprite pass.
    let corners = array<vec2<f32>, 4>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, 1.0),
    );
    let corner = corners[vertex_index];
    let particle_index = alive_indices[params.alive_offset + instance_index];
    let renderer = renderers[params.renderer_index];
    // Read members individually: loading the whole record defeats attribute pruning.
    var particle: Particle;
    particle.position = particles[particle_index].position;
    particle.size = particles[particle_index].size;
    particle.rotation = particles[particle_index].rotation;
    particle.emitter_index = particles[particle_index].emitter_index;
    if (renderer.attribute_flags.x & 32u) == 0u {
        particle.normalized_age = particles[particle_index].normalized_age;
    }
    let sine = sin(particle.rotation);
    let cosine = cos(particle.rotation);
    let rotated = vec2<f32>(
        corner.x * cosine - corner.y * sine,
        corner.x * sine + corner.y * cosine,
    ) * particle.size * 0.5;
    let world_center = globals.world_from_effect * vec4<f32>(particle.position, 1.0);
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

    var output: SpriteVertexData;
    output.clip_position = view.clip_from_world * world_position;
    output.color = renderer.tint;
    if renderer.particle_color != 0u {
        if (renderer.attribute_flags.x & 8u) == 0u {
            output.color = vec4<f32>(output.color.rgb * particles[particle_index].color.rgb, output.color.a);
        }
        if (renderer.attribute_flags.x & 16u) == 0u {
            output.color.a *= particles[particle_index].color.a;
        }
    }
    output.quad_position = corner;
    output.softness = renderer.softness;
    output.visible = select(0u, 1u, particle.emitter_index == renderer.emitter_index);
    var uv_bounds = vec4<f32>(renderer.uv_min, renderer.uv_max);
    if renderer.renderer_kind == 1u {
        var identity = 0u;
        if (renderer.flipbook_flags & 2u) != 0u && renderer.frame_count > 1u {
            identity = particles[particle_index].particle_index;
        }
        uv_bounds = renderer.frames[flipbook_frame(renderer, particle.normalized_age, identity)];
    }
    output.uv = mix(uv_bounds.xy, uv_bounds.zw, corner * 0.5 + vec2<f32>(0.5));
    output.textured = renderer.textured;
    output.effect_time = globals.time;
    output.particle_normalized_age = particle.normalized_age;
    return output;
}
