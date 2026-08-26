struct View {
    clip_from_world: mat4x4<f32>,
}

// Native particle presentation. The vertex shader expands one compacted alive
// particle into a camera-space quad without a CPU vertex or instance buffer.

struct Curve {
    keys: array<vec2<f32>, 8>,
    count: u32,
    _padding: vec3<f32>,
}

struct GradientKey {
    color: vec4<f32>,
    time: f32,
    _padding: vec3<f32>,
}

struct Gradient {
    keys: array<GradientKey, 8>,
    count: u32,
    _padding: vec3<f32>,
}

struct Emitter {
    slot_offset: u32,
    max_particles: u32,
    burst_count: u32,
    shape_kind: u32,
    start_time: f32,
    duration: f32,
    spawn_rate: f32,
    shape_radius: f32,
    shape_depth: f32,
    direction_radians: f32,
    spread_radians: f32,
    drag: f32,
    lifetime: vec2<f32>,
    speed: vec2<f32>,
    angular_velocity: vec2<f32>,
    gravity: vec2<f32>,
    turbulence: f32,
    blend_mode: u32,
    softness: f32,
    _padding: f32,
    size: Curve,
    opacity: Curve,
    color: Gradient,
}

struct Particle {
    color: vec4<f32>,
    position: vec2<f32>,
    size: f32,
    rotation: f32,
    normalized_age: f32,
    emitter_index: u32,
    alive: u32,
    _padding: u32,
}

struct RenderGlobals {
    world_from_effect: mat4x4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) quad_position: vec2<f32>,
    @location(2) softness: f32,
    @location(3) @interpolate(flat) blend_mode: u32,
}

@group(0) @binding(0) var<uniform> view: View;
@group(1) @binding(0) var<storage, read> emitters: array<Emitter>;
@group(1) @binding(1) var<storage, read> particles: array<Particle>;
@group(1) @binding(2) var<storage, read> alive_indices: array<u32>;
@group(1) @binding(3) var<storage, read> globals: RenderGlobals;

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
    let particle = particles[alive_indices[instance_index]];
    let emitter = emitters[particle.emitter_index];
    let sine = sin(particle.rotation);
    let cosine = cos(particle.rotation);
    let rotated = vec2<f32>(
        corner.x * cosine - corner.y * sine,
        corner.x * sine + corner.y * cosine,
    ) * particle.size * 0.5;
    let world_position = globals.world_from_effect
        * vec4<f32>(particle.position + rotated, 0.0, 1.0);

    var output: VertexOutput;
    output.clip_position = view.clip_from_world * world_position;
    output.color = particle.color;
    output.quad_position = corner;
    output.softness = emitter.softness;
    output.blend_mode = emitter.blend_mode;
    return output;
}

fn particle_color(input: VertexOutput) -> vec4<f32> {
    let radius = length(input.quad_position);
    let feather = clamp(input.softness, 0.001, 1.0);
    let coverage = 1.0 - smoothstep(1.0 - feather, 1.0, radius);
    return vec4<f32>(input.color.rgb, input.color.a * coverage);
}

@fragment
fn fragment_alpha(input: VertexOutput) -> @location(0) vec4<f32> {
    if input.blend_mode != 0u {
        discard;
    }
    return particle_color(input);
}

@fragment
fn fragment_additive(input: VertexOutput) -> @location(0) vec4<f32> {
    if input.blend_mode != 1u {
        discard;
    }
    return particle_color(input);
}
