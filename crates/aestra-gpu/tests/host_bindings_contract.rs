//! Host bindings HB6: the GPU host-binding ABI. Snapshots pack into one word buffer with static
//! per-slot offsets, the WGSL accessors validate, and built-in host-field reads reach the GPU emitter
//! records rebuilt from the instance each frame.

use aestra_compiler::{EffectCompiler, ExtensionRegistry};
use aestra_core::{
    AESTRA_FIELD_LINEAR_VELOCITY, BindingFieldId, BindingUpdateMode, EffectAsset, EffectBinding,
    Emitter, HostFieldRef, MODULE_INITIALIZE, PropertySource,
};
use aestra_gpu::{
    GpuEffectArtifact, GpuHostBindings, HOST_BINDING_BOUND, HOST_BINDING_HEADER_WORDS,
    HOST_BINDINGS_WGSL, host_binding_value_offsets, host_binding_value_word,
};
use aestra_runtime::{BindingSlot, EffectInstance, SpatialBindingSnapshot};
use std::sync::Arc;

/// `Source` (position) and `Target` (position + optional velocity); Initialize `direction` reads
/// `Target`'s velocity.
fn instance() -> EffectInstance {
    let mut effect = EffectAsset::new("Bound", 2.0);
    let mut emitter = Emitter::basic_sprite("Sparks", 2.0);
    let mut target = EffectBinding::spatial("Target", BindingUpdateMode::Live);
    target
        .optional_fields
        .insert(BindingFieldId::new(AESTRA_FIELD_LINEAR_VELOCITY));
    for module in &mut emitter.modules {
        if module.module_type.0 == MODULE_INITIALIZE {
            module
                .property_sources
                .insert("direction".into(), PropertySource::HostBinding);
            module.host_bindings.insert(
                "direction".into(),
                HostFieldRef::new(target.id, AESTRA_FIELD_LINEAR_VELOCITY),
            );
        }
    }
    effect.emitters.push(emitter);
    effect.bindings = vec![
        EffectBinding::spatial("Source", BindingUpdateMode::SnapshotOnSpawn),
        target,
    ];
    let compiled = EffectCompiler::with_extensions(ExtensionRegistry::builtin())
        .compile(&effect)
        .unwrap();
    EffectInstance::new(Arc::new(compiled))
}

#[test]
fn snapshots_pack_with_static_offsets_and_headers() {
    let mut instance = instance();
    let effect = instance.effect().clone();
    // Headers for two slots, then Source (3 words) and Target (6 words).
    let offsets = host_binding_value_offsets(&effect);
    assert_eq!(offsets, [9, 12]);

    let unbound = GpuHostBindings::from_instance(&instance);
    assert_eq!(
        unbound.words.len(),
        1 + 2 * HOST_BINDING_HEADER_WORDS + 3 + 6
    );
    assert_eq!(unbound.words[0], 2);
    assert_eq!(unbound.words[1], 0, "Source is unbound");
    assert_eq!(
        unbound.words[1 + 4 + 3],
        12,
        "Target's values offset is static"
    );

    let mut moving = SpatialBindingSnapshot::at([4.0, 5.0, 6.0]);
    moving.linear_velocity = Some([0.0, 0.0, -2.0]);
    instance.set_spatial_binding("Target", moving).unwrap();
    let packed = GpuHostBindings::from_instance(&instance);
    let target = 1 + HOST_BINDING_HEADER_WORDS;
    assert_eq!(packed.words[target], HOST_BINDING_BOUND);
    assert_eq!(
        packed.words[target + 1],
        0b11,
        "position and velocity present"
    );
    assert_eq!(packed.words[target + 2], 6);
    let floats: Vec<f32> = packed.words[12..18]
        .iter()
        .map(|w| f32::from_bits(*w))
        .collect();
    assert_eq!(floats, [4.0, 5.0, 6.0, 0.0, 0.0, -2.0]);
    // The same word a plugin lowerer would compute at compile time.
    let velocity = effect.bindings[1]
        .layout
        .field(&BindingFieldId::new(AESTRA_FIELD_LINEAR_VELOCITY))
        .unwrap()
        .1
        .offset;
    assert_eq!(
        host_binding_value_word(&effect, BindingSlot(1), velocity),
        Some(15)
    );
    assert_eq!(packed.byte_len(), packed.words.len() as u64 * 4);
}

#[test]
fn the_wgsl_accessors_validate_in_a_consuming_kernel() {
    let wgsl = format!(
        "@group(0) @binding(0) var<storage, read> aestra_host_bindings: array<u32>;\n\
         @group(0) @binding(1) var<storage, read_write> output: array<f32>;\n\
         {HOST_BINDINGS_WGSL}\n\
         @compute @workgroup_size(1) fn read_target() {{\n\
             let p = aestra_binding_vec3(1u, 0u);\n\
             output[0] = p.x; output[1] = p.y; output[2] = p.z;\n\
             output[3] = select(0.0, 1.0, aestra_binding_bound(1u));\n\
             output[4] = select(0.0, 1.0, aestra_binding_present(1u, 1u));\n\
             output[5] = aestra_binding_vec4(1u, 0u).w;\n\
         }}\n"
    );
    let module = naga::front::wgsl::parse_str(&wgsl).unwrap_or_else(|error| {
        panic!("{}", error.emit_to_string(&wgsl));
    });
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("the host-binding accessors validate");
}

#[test]
fn builtin_host_fields_reach_the_gpu_emitter_records() {
    let mut instance = instance();
    let direction = |instance: &EffectInstance| {
        GpuEffectArtifact::dynamics_from_instance(instance)
            .unwrap()
            .emitters[0]
            .direction
            .to_array()
    };
    assert_eq!(
        direction(&instance),
        [0.0, 1.0, 0.0],
        "the authored fallback"
    );
    let mut moving = SpatialBindingSnapshot::at([0.0; 3]);
    moving.linear_velocity = Some([3.0, 0.0, 0.0]);
    instance.set_spatial_binding("Target", moving).unwrap();
    // Packed normalized, exactly as the CPU reference normalizes the launch direction.
    assert_eq!(direction(&instance), [1.0, 0.0, 0.0], "the live velocity");
}
