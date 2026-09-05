struct View {
    clip_from_world: mat4x4<f32>,
    unjittered_clip_from_world: mat4x4<f32>,
    world_from_clip: mat4x4<f32>,
    world_from_view: mat4x4<f32>
}

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
    frames: array<vec4<f32>, 64>
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
    _padding_2: u32
}

struct RenderGlobals {
    world_from_effect: mat4x4<f32>,
    time: f32,
    seed: u32,
    _padding: vec2<f32>
}

struct RenderParams {
    renderer_index: u32,
    alive_offset: u32,
    _padding: vec2<u32>
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
    ribbon_direction: vec3<f32>
}

@group(0) @binding(0)
var<uniform> view: View;

@group(1) @binding(0)
var<storage, read> renderers: array<Renderer>;

@group(1) @binding(1)
var<storage, read> particles: array<Particle>;

@group(1) @binding(2)
var<storage, read> alive_indices: array<u32>;

@group(1) @binding(3)
var<storage, read> globals: RenderGlobals;

@group(1) @binding(4)
var<storage, read> params: RenderParams;

@group(1) @binding(5)
var sprite_texture: texture_2d<f32>;

@group(1) @binding(6)
var sprite_sampler: sampler;

fn hash01(index: u32, channel: u32) -> f32 {
    var result = (index * 2654435769u) ^ (channel * 2246822507u) ^ globals.seed;
    result ^= result >> 16u;
    result *= 2146121005u;
    result ^= result >> 15u;
    result *= 2221713035u;
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
    let seconds = select(clamp(normalized_age, 0.0, 1.0) * f32(count) / renderer.frame_rate, max(globals.time, 0.0), effect_time);
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

fn aestra_sprite_vertex(vertex_index: u32, instance_index: u32) -> SpriteVertexData {
    if renderers[params.renderer_index].renderer_kind == 3u {
        return aestra_ribbon_vertex(vertex_index, instance_index);
    }
    if renderers[params.renderer_index].renderer_kind == 4u {
        return aestra_trail_vertex(vertex_index, instance_index);
    }
    let corners = array<vec2<f32>, 4>(vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, 1.0));
    let corner = corners[vertex_index];
    let particle_index = alive_indices[params.alive_offset + instance_index];
    let renderer = renderers[params.renderer_index];
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
    let rotated = vec2<f32>(corner.x * cosine - corner.y * sine, corner.x * sine + corner.y * cosine) * particle.size * 0.5;
    let world_center = globals.world_from_effect * vec4<f32>(particle.position, 1.0);
    let effect_scale_x = length(globals.world_from_effect[0].xyz);
    let effect_scale_y = length(globals.world_from_effect[1].xyz);
    let camera_right = normalize(view.world_from_view[0].xyz);
    let camera_up = normalize(view.world_from_view[1].xyz);
    let world_position = world_center + vec4<f32>(camera_right * rotated.x * effect_scale_x + camera_up * rotated.y * effect_scale_y, 0.0);
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

fn ribbon_unit(v: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
    let square = dot(v, v);
    if square < 1e-12 {
        return fallback;
    }
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
    if previous != 4294967295u {
        before = ribbon_world(previous);
    }
    if next != 4294967295u {
        after = ribbon_world(next);
    }
    return ribbon_unit(after - before, ribbon_unit(after - point, ribbon_unit(point - before, vec3<f32>(1.0, 0.0, 0.0))));
}

fn aestra_ribbon_vertex(vertex_index: u32, instance_index: u32) -> SpriteVertexData {
    var output: SpriteVertexData;
    let renderer = renderers[params.renderer_index];
    let start = alive_indices[params.alive_offset + instance_index];
    let end = particles[start]._padding_0;
    output.clip_position = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    if end == 4294967295u {
        return output;
    }
    let delta = ribbon_world(end) - ribbon_world(start);
    if dot(delta, delta) < 1e-12 {
        return output;
    }
    let corners = array<vec2<f32>, 4>(vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, 1.0));
    let corner = corners[vertex_index];
    let slot = select(start, end, corner.y > 0.0);
    let direction = ribbon_direction(slot);
    let camera_forward = ribbon_unit(view.world_from_view[2].xyz, vec3<f32>(0.0, 0.0, 1.0));
    let side = ribbon_unit(cross(direction, camera_forward), ribbon_unit(view.world_from_view[0].xyz, vec3<f32>(1.0, 0.0, 0.0)));
    let scale = max(length(globals.world_from_effect[0].xyz), max(length(globals.world_from_effect[1].xyz), length(globals.world_from_effect[2].xyz)));
    let width = bitcast<f32>(renderer.attribute_flags.y) * particles[slot].size * scale;
    let position = ribbon_world(slot) + side * corner.x * width * 0.5;
    output.clip_position = view.clip_from_world * vec4<f32>(position, 1.0);
    output.color = renderer.tint;
    if renderer.particle_color != 0u {
        if (renderer.attribute_flags.x & 8u) == 0u {
            output.color = vec4<f32>(output.color.rgb * particles[slot].color.rgb, output.color.a);
        }
        if (renderer.attribute_flags.x & 16u) == 0u {
            output.color.a *= particles[slot].color.a;
        }
    }
    output.quad_position = corner;
    output.uv = vec2<f32>(bitcast<f32>(particles[slot]._padding_2), corner.x * 0.5 + 0.5);
    output.ribbon_direction = direction;
    output.visible = select(0u, 1u, particles[start].emitter_index == renderer.emitter_index && particles[end].emitter_index == renderer.emitter_index);
    output.softness = renderer.softness;
    output.textured = renderer.textured | 2u;
    output.effect_time = globals.time;
    if (renderer.attribute_flags.x & 32u) == 0u {
        output.particle_normalized_age = particles[slot].normalized_age;
    }
    return output;
}

fn trail_slot(base: u32, capacity: u32, index: u32) -> u32 {
    let head = particles[base];
    if index == head._padding_1 {
        return base;
    }
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
    if particles[base].alive == 0u || segment >= count {
        return output;
    }
    let start = trail_slot(base, capacity, segment);
    let end = trail_slot(base, capacity, segment + 1u);
    let delta = particles[end].position - particles[start].position;
    if dot(delta, delta) < 1e-12 || globals.time - particles[end].rotation >= r.frame_rate {
        return output;
    }
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
    if r.particle_color != 0u {
        output.color *= particles[slot].color;
    }
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

struct VertexOutput {
    @builtin(position)
    clip_position: vec4<f32>,
    @location(0)
    color: vec4<f32>,
    @location(1)
    quad_position: vec2<f32>,
    @location(2)
    softness: f32,
    @location(3) @interpolate(flat)
    visible: u32,
    @location(4)
    uv: vec2<f32>,
    @location(5) @interpolate(flat)
    textured: u32
}

@vertex
fn vertex(@builtin(vertex_index) vertex_index: u32, @builtin(instance_index) instance_index: u32) -> VertexOutput {
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
    if (input.textured & 2u) != 0u {
        distance = abs(input.quad_position.x);
    }
    var coverage = 1.0 - smoothstep(1.0 - feather, 1.0, distance);
    if (input.textured & 4u) != 0u {
        coverage *= input.quad_position.y;
    }
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
    if (input.textured & 4u) != 0u {
        edge_distance = abs(input.quad_position.x);
    }
    let line_width = max(fwidth(edge_distance) * 1.35, 0.012);
    var coverage = smoothstep(1.0 - line_width, 1.0, edge_distance);
    if (input.textured & 4u) != 0u {
        coverage *= input.quad_position.y;
    }
    if coverage <= 0.01 {
        discard;
    }
    let wire_color = mix(input.color.rgb, vec3<f32>(0.72, 0.56, 1.0), 0.65);
    return vec4<f32>(wire_color, coverage * 0.92);
}
