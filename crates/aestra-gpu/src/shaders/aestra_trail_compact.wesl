@group(0) @binding(0) var<storage, read> particles: array<Particle>;
@group(0) @binding(1) var<storage, read> renderers: array<Renderer>;
@group(0) @binding(2) var<storage, read> globals: RenderGlobals;
@group(0) @binding(3) var<storage, read> aux: array<u32>;
// Renderer, candidate count, owner count, candidates per owner.
@group(0) @binding(4) var<uniform> compact: vec4<u32>;
@group(0) @binding(5) var<storage, read_write> scratch: array<u32>;
// Four indirect words followed by the stable candidate-index list.
@group(0) @binding(6) var<storage, read_write> compacted: array<u32>;

@compute @workgroup_size(64)
fn classify_trail(@builtin(global_invocation_id) id: vec3<u32>) {
    let owner = id.x;
    if owner >= compact.z { return; }
    var count = 0u;
    for (var primitive = 0u; primitive < compact.w; primitive += 1u) {
        let candidate = owner * compact.w + primitive;
        let valid = trail_primitive_visible(candidate, compact.x);
        scratch[candidate] = select(0xffffffffu, count, valid);
        count += select(0u, 1u, valid);
    }
    scratch[compact.y + owner] = count;
}

// One 4 KiB scratch page per workgroup, independent of logical pool capacity.
// Storage scratch: candidate ranks, owner-local offsets, then page totals/offsets.
const TRAIL_SCAN_PAGE: u32 = 1024u;
var<workgroup> trail_owner_scan: array<u32, 1024>;
var<workgroup> trail_page_total: u32;

fn trail_scan_width(count: u32) -> u32 {
    var size = 1u;
    while size < count { size *= 2u; }
    return size;
}

fn scan_trail_page(thread: u32, size: u32) {
    workgroupBarrier();
    for (var width = 2u; width <= size; width *= 2u) {
        for (var n = (thread + 1u) * width - 1u; n < size; n += 64u * width) {
            trail_owner_scan[n] += trail_owner_scan[n - width / 2u];
        }
        workgroupBarrier();
    }
    if thread == 0u {
        trail_page_total = trail_owner_scan[size - 1u];
        trail_owner_scan[size - 1u] = 0u;
    }
    workgroupBarrier();
    for (var width = size; width >= 2u; width /= 2u) {
        for (var n = (thread + 1u) * width - 1u; n < size; n += 64u * width) {
            let left = trail_owner_scan[n - width / 2u];
            trail_owner_scan[n - width / 2u] = trail_owner_scan[n];
            trail_owner_scan[n] += left;
        }
        workgroupBarrier();
    }
}

@compute @workgroup_size(64)
fn prefix_trail(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) thread: u32) {
    let start = group.x * TRAIL_SCAN_PAGE;
    let size = trail_scan_width(min(compact.z - start, TRAIL_SCAN_PAGE));
    for (var local = thread; local < size; local += 64u) {
        var count = 0u;
        if start + local < compact.z { count = scratch[compact.y + start + local]; }
        trail_owner_scan[local] = count;
    }
    scan_trail_page(thread, size);
    for (var local = thread; local < size; local += 64u) {
        if start + local < compact.z {
            scratch[compact.y + start + local] = trail_owner_scan[local];
        }
    }
    if thread == 0u {
        scratch[compact.y + compact.z + group.x] = trail_page_total;
        // Keep existing single-page pools on the three-dispatch path.
        if compact.z <= TRAIL_SCAN_PAGE {
            scratch[compact.y + compact.z] = 0u;
            compacted[0] = 4u;
            compacted[1] = trail_page_total;
            compacted[2] = 0u;
            compacted[3] = 0u;
        }
    }
}

@compute @workgroup_size(64)
fn prefix_trail_pages(@builtin(local_invocation_index) thread: u32) {
    let pages = (compact.z + TRAIL_SCAN_PAGE - 1u) / TRAIL_SCAN_PAGE;
    var carry = 0u;
    // Normally one iteration; tiled page-total scans avoid a second logical cap.
    for (var start = 0u; start < pages; start += TRAIL_SCAN_PAGE) {
        let size = trail_scan_width(min(pages - start, TRAIL_SCAN_PAGE));
        for (var local = thread; local < size; local += 64u) {
            var count = 0u;
            if start + local < pages { count = scratch[compact.y + compact.z + start + local]; }
            trail_owner_scan[local] = count;
        }
        scan_trail_page(thread, size);
        for (var local = thread; local < size; local += 64u) {
            if start + local < pages {
                scratch[compact.y + compact.z + start + local] = carry + trail_owner_scan[local];
            }
        }
        carry += trail_page_total;
        workgroupBarrier();
    }
    if thread == 0u {
        compacted[0] = 4u;
        compacted[1] = carry;
        compacted[2] = 0u;
        compacted[3] = 0u;
    }
}

@compute @workgroup_size(64)
fn scatter_trail(@builtin(global_invocation_id) id: vec3<u32>) {
    let candidate = id.x;
    if candidate >= compact.y { return; }
    let local = scratch[candidate];
    if local == 0xffffffffu { return; }
    let owner = candidate / compact.w;
    var offset = scratch[compact.y + owner];
    if compact.z > TRAIL_SCAN_PAGE {
        offset += scratch[compact.y + compact.z + owner / TRAIL_SCAN_PAGE];
    }
    compacted[4u + offset + local] = candidate;
}
