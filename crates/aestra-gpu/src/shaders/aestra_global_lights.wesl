@group(0) @binding(0) var<storage, read> input_runs: array<Light>;
@group(0) @binding(1) var<storage, read_write> output_runs: array<Light>;
@group(0) @binding(2) var<uniform> dispatch: Dispatch;
@group(0) @binding(3) var<storage, read> input_counts: array<vec4<u32>>;
@group(0) @binding(4) var<storage, read_write> counters: Counters;
@compute @workgroup_size(64)
fn sum_light_counts(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= dispatch.input.y { return; }
    let counts = input_counts[id.x];
    atomicAdd(&counters.requested, counts.x);
    atomicAdd(&counters.candidates, counts.y);
}
