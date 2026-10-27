// Based on Bevy 0.20's oit_resolve.wesl (MIT OR Apache-2.0).
// Native adapter fix: collected node RGB is ALREADY premultiplied. Do not
// multiply it by extinction alpha again, including on overflow paths.
// Full-target overflow lists are preordered by sort.wesl before this pass.
// Retain the native bounded local array and front-to-back accumulation; without
// preprocessing (cold/unsupported paths), its early blending is approximate.
import bevy_render::view::View;
import bevy_pbr::render::mesh_view_types::OitFragmentNode;
import bevy_pbr::render::rgb9e5::rgb9e5_to_vec3_;
import bevy_core_pipeline::oit::draw::{pack_24bit_depth_8bit_alpha, unpack_24bit_depth_8bit_alpha};

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> nodes: array<OitFragmentNode>;
@group(0) @binding(2) var<storage, read_write> heads: array<u32>;
@group(0) @binding(3) var<storage, read_write> atomic_counter: u32;
@if(!DEPTH_PREPASS)
@group(1) @binding(0) var depth: texture_depth_2d;

struct OitFragment { color: u32, depth_alpha: u32 }
struct FullscreenVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}
const END: u32 = 0xFFFFFFFFu;
const SORTED_FRAGMENT_MAX_COUNT: u32 = constants::SORTED_FRAGMENT_MAX_COUNT;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    atomic_counter = 0u;
    let screen_index = u32(floor(in.position.x) + floor(in.position.y) * view.viewport.z);
    let head = heads[screen_index] - 1u;
    if head == END { discard; }
    @if(!DEPTH_PREPASS)
    let opaque_depth = textureLoad(depth, vec2<i32>(in.position.xy), 0);
    @else
    let opaque_depth = 0.0;
    let color = resolve(head, opaque_depth);
    // Preserve Bevy's camera/frame buffer-reuse contract.
    heads[screen_index] = 0u;
    return color;
}

fn premultiplied(color: u32, depth_alpha: u32) -> vec4<f32> {
    return vec4<f32>(rgb9e5_to_vec3_(color), unpack_24bit_depth_8bit_alpha(depth_alpha).y);
}

fn resolve(head: u32, opaque_depth: f32) -> vec4<f32> {
    var sorted: array<OitFragment, SORTED_FRAGMENT_MAX_COUNT>;
    var output = vec4<f32>(0.0);
    let packed_opaque = pack_24bit_depth_8bit_alpha(opaque_depth, 1.0);
    var node_index = head;
    var count = 0u;
    while node_index != END {
        let node = nodes[node_index];
        node_index = node.next;
        @if(!DEPTH_PREPASS)
        if node.depth_alpha < packed_opaque { continue; }
        if count < SORTED_FRAGMENT_MAX_COUNT {
            var i = count;
            for (; i > 0u && node.depth_alpha > sorted[i - 1u].depth_alpha; i -= 1u) {
                sorted[i] = sorted[i - 1u];
            }
            sorted[i] = OitFragment(node.color, node.depth_alpha);
            count += 1u;
        } else if sorted[0].depth_alpha > node.depth_alpha {
            output = blend(output, premultiplied(sorted[0].color, sorted[0].depth_alpha));
            var i = 0u;
            for (; i < SORTED_FRAGMENT_MAX_COUNT - 1u && node.depth_alpha < sorted[i + 1u].depth_alpha; i += 1u) {
                sorted[i] = sorted[i + 1u];
            }
            sorted[i] = OitFragment(node.color, node.depth_alpha);
        } else {
            output = blend(output, premultiplied(node.color, node.depth_alpha));
        }
    }
    for (var i = 0u; i < count; i += 1u) {
        output = blend(output, premultiplied(sorted[i].color, sorted[i].depth_alpha));
        if output.a == 1.0 { break; }
    }
    return output;
}

fn blend(front: vec4<f32>, back: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(front.rgb + (1.0 - front.a) * back.rgb,
        front.a + (1.0 - front.a) * back.a);
}
