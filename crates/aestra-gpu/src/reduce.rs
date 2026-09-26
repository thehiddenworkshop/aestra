//! Deterministic workgroup reductions (fluid F5, G7): a WGSL module programs compose after their own
//! source, as they do the host-binding accessors.
//!
//! A sum is reduced in a **fixed tree**: every invocation stores its value, then halves the active
//! range each step, adding the upper half onto the lower. The pairing never depends on the hardware —
//! no subgroup operations are used — so the same inputs give the same bits on every device that runs
//! the program, with or without subgroup support, and a rerun reproduces them. A multi-level sum
//! (per-workgroup partials, then one workgroup over the partials in a fixed strided order) keeps that
//! property.

/// Sums four values at once over the invocations of one workgroup of [`REDUCE_WORKGROUP`]
/// invocations: `aestra_workgroup_sum(local_index, value)` returns the total to every invocation.
/// Call it from uniform control flow (no early return before it) — it holds workgroup barriers.
pub const REDUCE_WGSL: &str = r#"
const AESTRA_REDUCE_WORKGROUP: u32 = 64u;
var<workgroup> aestra_reduce_scratch: array<vec4<f32>, 64>;

fn aestra_workgroup_sum(local_index: u32, value: vec4<f32>) -> vec4<f32> {
    // A previous sum's readers are done before this one overwrites the scratch.
    workgroupBarrier();
    aestra_reduce_scratch[local_index] = value;
    workgroupBarrier();
    for (var stride = AESTRA_REDUCE_WORKGROUP / 2u; stride > 0u; stride = stride / 2u) {
        if (local_index < stride) {
            aestra_reduce_scratch[local_index] =
                aestra_reduce_scratch[local_index] + aestra_reduce_scratch[local_index + stride];
        }
        workgroupBarrier();
    }
    return aestra_reduce_scratch[0];
}
"#;

/// Invocations per workgroup [`REDUCE_WGSL`] reduces over.
pub const REDUCE_WORKGROUP: u32 = 64;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reduction_module_validates_in_a_program() {
        let source = format!(
            "{REDUCE_WGSL}\n\
             @group(0) @binding(0) var<storage, read_write> values: array<vec4<f32>>;\n\
             @compute @workgroup_size(4, 4, 4)\n\
             fn reduce(@builtin(local_invocation_index) index: u32, \
             @builtin(workgroup_id) group: vec3<u32>) {{\n    \
             let total = aestra_workgroup_sum(index, values[index]);\n    \
             if (index == 0u) {{ values[group.x] = total; }}\n}}"
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
