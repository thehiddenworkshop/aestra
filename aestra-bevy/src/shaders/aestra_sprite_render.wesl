struct View {
    clip_from_world: mat4x4<f32>,
}

// Native particle presentation. The vertex shader expands one compacted alive
// particle into a camera-space quad without a CPU vertex or instance buffer.

struct Renderer {
    emitter_index: u32,
    blend_mode: u32,
    softness: f32,
    _padding: f32,
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
}

@group(0) @binding(0) var<uniform> view: View;
@group(1) @binding(0) var<storage, read> renderers: array<Renderer>;
@group(1) @binding(1) var<storage, read> particles: array<Particle>;
@group(1) @binding(2) var<storage, read> alive_indices: array<u32>;
@group(1) @binding(3) var<storage, read> globals: RenderGlobals;
@group(1) @binding(4) var<storage, read> params: RenderParams;

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
    let renderer = renderers[params.renderer_index];
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
    output.softness = renderer.softness;
    output.visible = select(0u, 1u, particle.emitter_index == renderer.emitter_index);
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
