// Read-only presentation ABI: never bind simulation state for write here.
struct Particle {
    color: vec4<f32>, position: vec3<f32>, size: f32,
    rotation: f32, normalized_age: f32, packed_emitter_alive: u32, particle_index: u32,
}
struct Light {
    position_range: vec4<f32>, color_intensity: vec4<f32>,
    radius: f32, priority: u32, source_token: u32, particle_index: u32,
}
struct Key { value: vec4<f32>, padding: vec3<f32>, time: f32 }
struct Plan {
    world_from_particle: mat4x4<f32>, constant_color: vec4<f32>,
    intensity: vec4<u32>, range: vec4<u32>, gradient: vec4<u32>, source: vec4<u32>,
    padding: vec3<f32>, radius: f32,
}
struct Dispatch { input: vec4<u32>, output: vec4<u32> }
struct Counters { requested: atomic<u32>, candidates: atomic<u32>, selected: atomic<u32>, dropped: atomic<u32> }
@group(0) @binding(0) var<storage, read> particles: array<Particle>;
@group(0) @binding(1) var<storage, read> plan: Plan;
@group(0) @binding(2) var<storage, read> keys: array<Key>;
@group(0) @binding(3) var<uniform> dispatch: Dispatch;
@group(0) @binding(4) var<storage, read> input_runs: array<Light>;
@group(0) @binding(5) var<storage, read_write> output_runs: array<Light>;
@group(0) @binding(6) var<storage, read_write> counters: Counters;
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
fn better(a: Light, b: Light) -> bool {
    let av = a.color_intensity.w > 0.0;
    let bv = b.color_intensity.w > 0.0;
    if av != bv { return av; }
    if !av { return false; }
    if a.priority != b.priority { return a.priority > b.priority; }
    if a.color_intensity.w != b.color_intensity.w { return a.color_intensity.w > b.color_intensity.w; }
    if a.source_token != b.source_token { return a.source_token < b.source_token; }
    return a.particle_index < b.particle_index;
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
    let position = (plan.world_from_particle * vec4<f32>(p.position, 1.0)).xyz;
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
@compute @workgroup_size(64)
fn merge_lights(@builtin(global_invocation_id) id: vec3<u32>) {
    let width = dispatch.input.z;
    let runs = dispatch.input.w;
    if id.x >= width * runs { return; }
    let run = id.x / width;
    let lane = id.x % width;
    let pair = run / 2u;
    let other = run ^ 1u;
    let a = input_runs[id.x];
    var lower = 0u;
    var upper = select(0u, width, other < runs);
    // Rank in the other sorted run, resolving exact ties left-before-right.
    // No quadratic comparisons and no ordering through atomic append offsets.
    while lower < upper {
        let mid = lower + (upper - lower) / 2u;
        let b = input_runs[other * width + mid];
        let preceding = better(b, a) || ((run & 1u) != 0u && !better(a, b));
        if preceding { lower = mid + 1u; } else { upper = mid; }
    }
    let rank = lane + lower;
    if rank < dispatch.output.x { output_runs[pair * dispatch.output.x + rank] = a; }
    // Odd final runs leave padded entries. The missing right half is initialized
    // by its left lanes; it cannot overlap ranks written by real entries.
    if other >= runs && lane + width < dispatch.output.x {
        var empty: Light;
        output_runs[pair * dispatch.output.x + lane + width] = empty;
    }
}
@compute @workgroup_size(1)
fn finish_lights() {
    var lower = 0u;
    var upper = dispatch.input.z;
    while lower < upper {
        let mid = lower + (upper - lower) / 2u;
        if input_runs[mid].color_intensity.w > 0.0 { lower = mid + 1u; } else { upper = mid; }
    }
    atomicStore(&counters.selected, lower);
    atomicStore(&counters.dropped, atomicLoad(&counters.candidates) - lower);
}
