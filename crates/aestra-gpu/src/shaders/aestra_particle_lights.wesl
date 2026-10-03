// Read-only presentation ABI: never bind simulation state for write here.
struct Particle {
    color: vec4<f32>, position: vec3<f32>, size: f32,
    rotation: f32, normalized_age: f32, packed_emitter_alive: u32, particle_index: u32,
}
struct Key { value: vec4<f32>, padding: vec3<f32>, time: f32 }
struct Plan {
    world_from_particle: mat4x4<f32>, constant_color: vec4<f32>,
    intensity: vec4<u32>, range: vec4<u32>, gradient: vec4<u32>, source: vec4<u32>,
    padding: vec3<f32>, radius: f32,
}
struct RenderGlobals { world_from_effect: mat4x4<f32>, time: f32, seed: u32, padding: vec2<f32> }
@group(0) @binding(0) var<storage, read> particles: array<Particle>;
@group(0) @binding(1) var<storage, read> plan: Plan;
@group(0) @binding(2) var<storage, read> keys: array<Key>;
@group(0) @binding(3) var<uniform> dispatch: Dispatch;
@group(0) @binding(4) var<storage, read> input_runs: array<Light>;
@group(0) @binding(5) var<storage, read_write> output_runs: array<Light>;
@group(0) @binding(6) var<storage, read_write> counters: Counters;
@group(0) @binding(7) var<storage, read> render_globals: RenderGlobals;
var<workgroup> block: array<Light, 64>;
var<workgroup> requested_in_block: atomic<u32>;
var<workgroup> candidates_in_block: atomic<u32>;

fn finite(v: f32) -> bool { return (bitcast<u32>(v) & 0x7f800000u) != 0x7f800000u; }
fn finite3(v: vec3<f32>) -> bool { return finite(v.x) && finite(v.y) && finite(v.z); }
fn rgb_valid(v: vec3<f32>) -> bool { return finite3(v) && all(v >= vec3<f32>(0.0)) && all(v <= vec3<f32>(1.0)); }
fn sample_keys(offset: u32, count: u32, mode: u32, age: f32) -> vec4<f32> {
    if count == 0u { return vec4<f32>(0.0); }
    var previous = keys[offset];
    if age <= previous.time { return previous.value; }
    for (var i = 1u; i < count; i += 1u) {
        let next = keys[offset + i];
        if age <= next.time {
            if mode == 2u { return select(previous.value, next.value, age >= next.time); }
            var x = clamp((age - previous.time) / max(next.time - previous.time, 1.192092896e-7), 0.0, 1.0);
            if mode == 0u { x = x * x * (3.0 - 2.0 * x); }
            return previous.value + (next.value - previous.value) * x;
        }
        previous = next;
    }
    return previous.value;
}
fn evaluate(index: u32) -> Light {
    var result: Light;
    if index >= dispatch.input.y || plan.source.y == 0u { return result; }
    let p = particles[dispatch.input.x + index];
    if (p.packed_emitter_alive & 65535u) != 1u || (p.packed_emitter_alive >> 16u) != plan.source.x { return result; }
    atomicAdd(&requested_in_block, 1u);
    let age = p.normalized_age;
    if !finite(age) || age < 0.0 || age >= 1.0 || !finite3(p.position) { return result; }
    var rgb = p.color.xyz;
    if plan.intensity.w == 1u { rgb = plan.constant_color.xyz; }
    if plan.intensity.w == 2u { rgb = sample_keys(plan.gradient.x, plan.gradient.y, 1u, age).xyz; }
    let lumens = sample_keys(plan.intensity.x, plan.intensity.y, plan.intensity.z, age).x;
    let range = sample_keys(plan.range.x, plan.range.y, plan.range.z, age).x;
    var world = plan.world_from_particle;
    if plan.source.z != 0u { world = render_globals.world_from_effect; }
    let position = (world * vec4<f32>(p.position, 1.0)).xyz;
    if !rgb_valid(rgb) || !finite(lumens) || lumens <= 0.0 || !finite(range) || range <= 0.0 || !finite3(position) { return result; }
    result.position_range = vec4<f32>(position, range);
    result.color_intensity = vec4<f32>(rgb, lumens);
    result.radius = plan.radius;
    result.priority = plan.gradient.w;
    result.source_token = plan.gradient.z;
    result.particle_index = p.particle_index;
    atomicAdd(&candidates_in_block, 1u);
    return result;
}
@compute @workgroup_size(64)
fn evaluate_lights(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) lane: u32) {
    if lane == 0u {
        atomicStore(&requested_in_block, 0u);
        atomicStore(&candidates_in_block, 0u);
    }
    workgroupBarrier();
    block[lane] = evaluate(group.x * 64u + lane);
    workgroupBarrier();
    if lane == 0u {
        atomicAdd(&counters.requested, atomicLoad(&requested_in_block));
        atomicAdd(&counters.candidates, atomicLoad(&candidates_in_block));
    }
    // Bitonic sort valid entries first, then priority/lumens/stable ordinal.
    for (var k = 2u; k <= 64u; k *= 2u) {
        for (var j = k / 2u; j > 0u; j /= 2u) {
            let a = block[lane];
            let b = block[lane ^ j];
            let descending = (lane & k) == 0u;
            let low_lane = (lane & j) == 0u;
            let swap = select(better(a, b), better(b, a), descending == low_lane);
            workgroupBarrier();
            if swap { block[lane] = b; }
            workgroupBarrier();
        }
    }
    if lane < dispatch.output.x { output_runs[group.x * dispatch.output.x + lane] = block[lane]; }
}
