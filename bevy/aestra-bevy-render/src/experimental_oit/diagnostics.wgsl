// Diagnostic only: bounded traversal, never copies fragments or heads to the CPU.
struct Node { color: u32, depth_alpha: u32, next: u32 }
struct Params { pixels: u32, capacity: u32, sorted_max: u32, width: u32 }
struct Result {
    attempted: atomic<u32>,
    over_sort_pixels: atomic<u32>,
    invalid_links: atomic<u32>,
    capped_max_depth: atomic<u32>,
}
@group(0) @binding(0) var<storage, read> nodes: array<Node>;
@group(0) @binding(1) var<storage, read> heads: array<u32>;
@group(0) @binding(2) var<storage, read> counter: u32;
@group(0) @binding(3) var<uniform> params: Params;
@group(0) @binding(4) var<storage, read_write> result: Result;
// Unused by main, so the ordinary diagnostic retains its original binding ABI.
@group(0) @binding(5) var<uniform> region: vec4<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= params.width { return; }
    let pixel = id.x + id.y * params.width;
    if pixel == 0u { atomicStore(&result.attempted, counter); }
    inspect(pixel);
}
@compute @workgroup_size(64)
fn region_main(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= region.zw) { return; }
    // The first *local* invocation records attempts even for a nonzero origin.
    if all(id.xy == vec2<u32>(0u)) { atomicStore(&result.attempted, counter); }
    let xy = region.xy + id.xy;
    inspect(xy.x + xy.y * params.width);
}
fn inspect(pixel: u32) {
    if pixel >= params.pixels { return; }
    var next = heads[pixel] - 1u;
    var depth = 0u;
    // Only threshold+1 nodes are needed. Even a corrupt cycle cannot hang this pass.
    loop {
        if next == 0xffffffffu || depth > params.sorted_max { break; }
        if next >= params.capacity {
            atomicAdd(&result.invalid_links, 1u);
            break;
        }
        depth += 1u;
        next = nodes[next].next;
    }
    atomicMax(&result.capped_max_depth, depth);
    if depth > params.sorted_max { atomicAdd(&result.over_sort_pixels, 1u); }
}
