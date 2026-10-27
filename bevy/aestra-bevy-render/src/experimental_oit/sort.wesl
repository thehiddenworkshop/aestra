// Overflow-only in-place bottom-up linked merge sort. O(N log N) link work,
// O(1) private state; existing global node/head storage is reused per pixel.
import bevy_render::view::View;
import bevy_pbr::render::mesh_view_types::{OitFragmentNode, OrderIndependentTransparencySettings};
@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> settings: OrderIndependentTransparencySettings;
@group(0) @binding(2) var<storage, read_write> nodes: array<OitFragmentNode>;
@group(0) @binding(3) var<storage, read_write> heads: array<u32>;
const END: u32 = 0xffffffffu;

fn before(a: u32, b: u32) -> bool {
    // Preserve native packed reverse-depth ordering. Canonical color breaks
    // packed-depth/alpha ties, not physical equal-depth transparency semantics.
    return nodes[a].depth_alpha > nodes[b].depth_alpha ||
        (nodes[a].depth_alpha == nodes[b].depth_alpha && nodes[a].color >= nodes[b].color);
}

@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(view.viewport.zw);
    if any(id.xy >= size) { return; }
    let pixel = id.x + id.y * size.x;
    if pixel >= arrayLength(&heads) { return; }
    var head = heads[pixel] - 1u;
    let capacity = arrayLength(&nodes);
    var next = head;
    var count = 0u;
    // Validate before rewriting. Bound even malformed/cyclic input by allocated
    // capacity; never let a corrupt chain enter the native unbounded resolve.
    while next != END && count < capacity {
        if next >= capacity { heads[pixel] = 0u; return; }
        count += 1u;
        next = nodes[next].next;
    }
    if next != END { heads[pixel] = 0u; return; }
    if count <= settings.sorted_fragment_max_count { return; }

    var width = 1u;
    while width < count {
        var p = head;
        var tail = END;
        var new_head = END;
        var emitted = 0u;
        while p != END && emitted < count {
            var q = p;
            var left = 0u;
            for (var i = 0u; i < width && q != END; i += 1u) {
                left += 1u;
                q = nodes[q].next;
            }
            var right = width;
            while (left > 0u || (right > 0u && q != END)) && emitted < count {
                var take = END;
                if left == 0u {
                    take = q; q = nodes[q].next; right -= 1u;
                } else if right == 0u || q == END {
                    take = p; p = nodes[p].next; left -= 1u;
                } else if before(p, q) {
                    take = p; p = nodes[p].next; left -= 1u;
                } else {
                    take = q; q = nodes[q].next; right -= 1u;
                }
                if tail == END { new_head = take; }
                else { nodes[tail].next = take; }
                tail = take;
                emitted += 1u;
            }
            p = q;
        }
        nodes[tail].next = END;
        head = new_head;
        // count is bounded by the admitted storage binding, far below 2^31.
        if width >= count - width { break; }
        width *= 2u;
    }
    heads[pixel] = head + 1u;
}
