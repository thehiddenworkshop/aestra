//! Artifact v4: compiled host bindings, clip binding forwards (host bindings HB2) and plugin
//! extension stages (extensible-stages M10) survive offline compilation and reload, and corrupted
//! data is rejected instead of trusted.

use aestra_artifact::{ArtifactError, decode_effect, encode_effect};
use aestra_compiler::{EffectCompiler, ExtensionRegistry};
use aestra_core::{
    AESTRA_FIELD_LINEAR_VELOCITY, BindingFieldId, BindingUpdateMode, EffectAsset, EffectBinding,
    EffectClip, Emitter,
};
use aestra_project::ResolvedEffectProject;
use aestra_runtime::{CompiledEffectProject, execute_reference};
use std::collections::BTreeMap;
use std::sync::Arc;

fn effect(name: &str, bindings: Vec<EffectBinding>) -> EffectAsset {
    let mut effect = EffectAsset::new(name, 1.0);
    effect.emitters.push(Emitter::basic_sprite("Emitter", 1.0));
    effect.bindings = bindings;
    effect
}

/// A root effect forwarding its `Target` to a child clip's `Aim`, compiled as a project.
fn compiled_project() -> CompiledEffectProject {
    let aim = EffectBinding::spatial("Aim", BindingUpdateMode::Live);
    let child = effect("Child", vec![aim.clone()]);
    let mut target = EffectBinding::spatial("Target", BindingUpdateMode::Live);
    target
        .optional_fields
        .insert(BindingFieldId::new(AESTRA_FIELD_LINEAR_VELOCITY));
    let mut root = effect(
        "Root",
        vec![
            EffectBinding::spatial("Source", BindingUpdateMode::SnapshotOnSpawn),
            target.clone(),
        ],
    );
    let mut clip = EffectClip::new(child.id, 0.0, 1.0);
    clip.binding_forwards.insert(aim.id, target.id);
    root.effect_clips.push(clip);
    EffectCompiler::with_extensions(ExtensionRegistry::builtin())
        .compile_resolved_project(&ResolvedEffectProject {
            root,
            dependencies: BTreeMap::from([(child.id, child)]),
            material_programs: BTreeMap::new(),
            material_functions: BTreeMap::new(),
        })
        .unwrap()
}

#[test]
fn bindings_and_forwards_round_trip_and_reassemble_into_a_valid_project() {
    let project = compiled_project();
    let root = decode_effect(&encode_effect(&project.root).unwrap()).unwrap();
    assert_eq!(root.bindings, project.root.bindings);
    assert_eq!(root.binding_slots, project.root.binding_slots);
    assert_eq!(
        root.effect_clips[0].binding_forwards,
        project.root.effect_clips[0].binding_forwards
    );
    assert_eq!(root.host_requirements(), project.root.host_requirements());

    let dependencies = project
        .dependencies
        .iter()
        .map(|(id, effect)| {
            (
                *id,
                Arc::new(decode_effect(&encode_effect(effect).unwrap()).unwrap()),
            )
        })
        .collect();
    let reloaded = CompiledEffectProject {
        root: Arc::new(root),
        dependencies,
    };
    reloaded.validate_binding_forwards().unwrap();
}

#[test]
fn reassembling_mismatched_artifacts_is_detected() {
    let mut project = compiled_project();
    // Replace the child with an effect that has no bindings: the forward now points nowhere.
    let child_id = *project.dependencies.keys().next().unwrap();
    let mut stranger = EffectCompiler::with_extensions(ExtensionRegistry::builtin())
        .compile(&effect("Stranger", Vec::new()))
        .unwrap();
    stranger.source = child_id;
    project.dependencies.insert(child_id, Arc::new(stranger));
    assert!(project.validate_binding_forwards().is_err());
}

fn tampered(from: &str, to: &str) -> Result<aestra_runtime::CompiledEffect, ArtifactError> {
    let text = String::from_utf8(encode_effect(&compiled_project().root).unwrap()).unwrap();
    assert!(
        text.contains(from),
        "the encoded artifact contains `{from}`"
    );
    decode_effect(text.replacen(from, to, 1).as_bytes())
}

#[test]
fn corrupted_binding_data_is_rejected() {
    assert!(matches!(
        tampered("parent_slot:1", "parent_slot:9"),
        Err(ArtifactError::InvalidData { path, .. }) if path.ends_with("parent_slot")
    ));
    // The Target layout packs position (3) + linear velocity (3); a wrong stride is rejected.
    assert!(matches!(
        tampered("stride:6", "stride:7"),
        Err(ArtifactError::InvalidData { path, .. }) if path.ends_with("layout")
    ));
}

#[test]
fn artifact_version_3_is_rejected_so_it_is_recompiled() {
    let text = String::from_utf8(encode_effect(&compiled_project().root).unwrap()).unwrap();
    let old = text.replacen("format_version:4", "format_version:3", 1);
    assert!(matches!(
        decode_effect(old.as_bytes()),
        Err(ArtifactError::UnsupportedVersion { found: 3 })
    ));
}

#[test]
fn plugin_extension_stages_round_trip_with_their_execution_ir() {
    let mut registry = ExtensionRegistry::builtin();
    registry
        .install(&aestra_example_extension::ExampleExtension)
        .unwrap();
    let source = include_str!("../../../sample-project/effects/plugin_lab.aestra.ron");
    let effect = EffectAsset::from_ron(source).unwrap();
    let compiled = EffectCompiler::with_extensions(registry)
        .compile(&effect)
        .unwrap();
    let original = &compiled.emitters[0].extension_stages;
    assert!(!original.is_empty());

    let reloaded = decode_effect(&encode_effect(&compiled).unwrap()).unwrap();
    let stages = &reloaded.emitters[0].extension_stages;
    assert_eq!(stages, original);
    assert_eq!(
        execute_reference(&stages[0].block).steps,
        execute_reference(&original[0].block).steps
    );
}
