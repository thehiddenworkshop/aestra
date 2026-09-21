//! GPU-less validation of the stateful backend's reusable WGSL primitives (hybrid roadmap M6).
//!
//! The spawn RNG, the atomic free-list allocator, and presentation extraction are shipped as
//! `pub const` WGSL fragments in `aestra_gpu`, meant to be composed into a shader by the render
//! backend (and by the on-GPU conformance tests). Those conformance tests only run where a compute
//! adapter is present, so this file additionally checks — with no GPU — that each fragment composes
//! with minimal bindings into WGSL that naga parses and validates. It catches syntax/type breakage
//! in the primitives on GPU-less CI, before the render wiring depends on them.

use aestra_gpu::{
    STATEFUL_FREE_LIST_WGSL, STATEFUL_PRESENT_WGSL, STATEFUL_SPAWN_RNG_WGSL,
    stateful_simulation_wgsl,
};
use naga::valid::{Capabilities, ValidationFlags, Validator};

fn assert_valid_wgsl(label: &str, wgsl: &str) {
    let module = naga::front::wgsl::parse_str(wgsl).unwrap_or_else(|error| {
        panic!("{label} must parse as WGSL: {}", error.emit_to_string(wgsl))
    });
    Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .unwrap_or_else(|error| panic!("{label} must validate: {error}"));
}

#[test]
fn spawn_rng_primitive_composes_into_valid_wgsl() {
    // The RNG fragment declares no bindings; an entry that writes a launch direction exercises the
    // full emulated-u64 splitmix path.
    let entry = r#"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> seed: vec2<u32>;

@compute @workgroup_size(64)
fn spawn(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= arrayLength(&out) / 3u) { return; }
    let dir = spawn_launch_direction(seed, vec2<u32>(i, 0u));
    out[i * 3u + 0u] = dir.x;
    out[i * 3u + 1u] = dir.y;
    out[i * 3u + 2u] = dir.z;
}
"#;
    assert_valid_wgsl(
        "STATEFUL_SPAWN_RNG_WGSL",
        &format!("{STATEFUL_SPAWN_RNG_WGSL}{entry}"),
    );
}

#[test]
fn free_list_primitive_composes_into_valid_wgsl() {
    // The allocator operates on module-scope bindings the includer declares (naga rejects atomic
    // pointer parameters), so the entry supplies `free_list` and `free_count`.
    let entry = r#"
@group(0) @binding(0) var<storage, read_write> free_list: array<u32>;
@group(0) @binding(1) var<storage, read_write> free_count: atomic<u32>;
@group(0) @binding(2) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(64)
fn allocate(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= arrayLength(&output)) { return; }
    let slot = aestra_free_pop();
    output[i] = slot;
    aestra_free_push(slot);
}
"#;
    assert_valid_wgsl(
        "STATEFUL_FREE_LIST_WGSL",
        &format!("{STATEFUL_FREE_LIST_WGSL}{entry}"),
    );
}

#[test]
fn presentation_primitive_composes_into_valid_wgsl() {
    // Presentation extraction reads the persistent `state` buffer and writes the `present_out`
    // GpuParticle records, both declared by the includer.
    let entry = r#"
@group(0) @binding(0) var<storage, read> state: array<f32>;
@group(0) @binding(1) var<storage, read_write> present_out: array<f32>;

@compute @workgroup_size(64)
fn present(@builtin(global_invocation_id) gid: vec3<u32>) {
    let slot = gid.x;
    if (slot >= arrayLength(&state) / AESTRA_STATE_STRIDE) { return; }
    aestra_present_stateful(slot, slot, 0u, 0.0);
}
"#;
    assert_valid_wgsl(
        "STATEFUL_PRESENT_WGSL",
        &format!("{STATEFUL_PRESENT_WGSL}{entry}"),
    );
}

#[test]
fn the_unified_stateful_module_composes_into_valid_wgsl_with_all_three_entry_points() {
    // The render backend builds its death_integrate / spawn / present pipelines from one composed
    // module over a shared six-binding layout. Validate that the assembled module parses, validates,
    // and exposes exactly those three entry points — this is the shader the pipelines will use.
    let wgsl = stateful_simulation_wgsl();
    let module = naga::front::wgsl::parse_str(&wgsl).unwrap_or_else(|error| {
        panic!(
            "stateful_simulation_wgsl must parse: {}",
            error.emit_to_string(&wgsl)
        )
    });
    Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .unwrap_or_else(|error| panic!("stateful_simulation_wgsl must validate: {error}"));
    let entry_points: Vec<&str> = module
        .entry_points
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    for expected in ["death_integrate", "spawn", "present"] {
        assert!(
            entry_points.contains(&expected),
            "the unified module exposes the {expected} entry point (found {entry_points:?})"
        );
    }
}
