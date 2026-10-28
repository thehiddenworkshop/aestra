struct View { viewport: vec4<f32>, capacity: u32, pad0: u32, pad1: u32, pad2: u32 }
struct Node { color: u32, depth_alpha: u32, next: u32 }
struct Layer { rect: vec4<f32>, color: vec4<f32>, info: vec4<f32> }
@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read_write> nodes: array<Node>;
@group(0) @binding(2) var<storage, read_write> heads: array<atomic<u32>>;
@group(0) @binding(3) var<storage, read_write> counter: atomic<u32>;
@group(0) @binding(4) var<storage, read> layers: array<Layer>;
@group(1) @binding(0) var opaque: texture_depth_2d;
struct Out { @builtin(position) position: vec4<f32>, @location(0) @interpolate(flat) color: vec4<f32> }
@vertex
fn vertex(@builtin(vertex_index) v: u32, @builtin(instance_index) instance: u32) -> Out {
    let corners = array(vec2(0.,0.),vec2(1.,0.),vec2(0.,1.),vec2(0.,1.),vec2(1.,0.),vec2(1.,1.));
    let layer = layers[instance];
    return Out(vec4(mix(layer.rect.xy, layer.rect.zw, corners[v]), layer.info.x, 1.0), layer.color);
}
@fragment
fn fragment(in: Out) -> @location(0) vec4<f32> {
    if in.position.z < textureLoad(opaque, vec2<i32>(in.position.xy), 0) { discard; }
    let color = vec3_to_rgb9e5_(in.color.rgb);
    let depth_alpha = pack_24bit_depth_8bit_alpha(in.position.z, in.color.a);
    if (color & 0x07ffffffu) == 0u && (depth_alpha & 255u) == 0u { discard; }
    let index = atomicAdd(&counter, 1u);
    if index >= view.capacity { discard; }
    let pixel = u32(in.position.x) + u32(in.position.y) * u32(view.viewport.z);
    let next = atomicExchange(&heads[pixel], index + 1u) - 1u;
    nodes[index] = Node(color, depth_alpha, next);
    discard;
}
