//! Workgroup prefix sums (fluid F7, G7): a WGSL module programs compose after their own source, as
//! they do the reductions.
//!
//! A scan over counts is what compacts a sparse set into a dense list in a canonical order — the
//! active bricks of a sparse grid in coordinate order, the free slots of a pool in slot order —
//! without atomics, whose completion order WebGPU leaves unspecified. The sums are of `u32`, whose
//! addition is exact and associative, so any evaluation order gives the same result; the module still
//! uses a fixed schedule and no subgroup operations.
//!
//! A grid-wide exclusive scan takes three passes of [`SCAN_WORKGROUP`]-invocation workgroups: each
//! workgroup scans its block and stores the block's total; one workgroup scans the totals, 64 at a
//! time with a running carry; each workgroup adds its block's offset.

/// `aestra_workgroup_scan(local_index, value)` returns, to each of the [`SCAN_WORKGROUP`] invocations
/// of a workgroup, the sum of the values of the invocations before it (x) and the workgroup's total
/// (y). Call it from uniform control flow (no early return before it) — it holds workgroup barriers.
pub const SCAN_WGSL: &str = r#"
const AESTRA_SCAN_WORKGROUP: u32 = 64u;
var<workgroup> aestra_scan_scratch: array<u32, 64>;

fn aestra_workgroup_scan(local_index: u32, value: u32) -> vec2<u32> {
    // A previous scan's readers are done before this one overwrites the scratch.
    workgroupBarrier();
    aestra_scan_scratch[local_index] = value;
    workgroupBarrier();
    // Hillis–Steele: after the step of `offset`, each entry sums the 2·offset values ending at it.
    for (var offset = 1u; offset < AESTRA_SCAN_WORKGROUP; offset = offset * 2u) {
        var before = 0u;
        if (local_index >= offset) {
            before = aestra_scan_scratch[local_index - offset];
        }
        workgroupBarrier();
        aestra_scan_scratch[local_index] = aestra_scan_scratch[local_index] + before;
        workgroupBarrier();
    }
    let inclusive = aestra_scan_scratch[local_index];
    return vec2<u32>(inclusive - value, aestra_scan_scratch[AESTRA_SCAN_WORKGROUP - 1u]);
}
"#;

/// Invocations per workgroup [`SCAN_WGSL`] scans over.
pub const SCAN_WORKGROUP: u32 = 64;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scan_module_validates_in_a_program() {
        let source = format!(
            "{SCAN_WGSL}\n\
             @group(0) @binding(0) var<storage, read_write> values: array<u32>;\n\
             @compute @workgroup_size(64)\n\
             fn scan(@builtin(local_invocation_index) index: u32, \
             @builtin(workgroup_id) group: vec3<u32>) {{\n    \
             let i = group.x * 64u + index;\n    \
             values[i] = aestra_workgroup_scan(index, values[i]).x;\n}}"
        );
        let module = naga::front::wgsl::parse_str(&source).expect("parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .expect("validates");
    }
}
