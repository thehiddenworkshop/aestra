//! Host bindings HB1: registry-aware validation of binding declarations.

use aestra_compiler::{EffectCompiler, ExtensionRegistry};
use aestra_core::{
    AESTRA_FIELD_LINEAR_VELOCITY, AESTRA_FIELD_POSITION, AESTRA_FIELD_ROTATION, BindingFieldId,
    BindingKindId, BindingUpdateMode, DiagnosticCode, EffectAsset, EffectBinding, EffectClip,
    Emitter, ExtensionId,
};
use aestra_project::ResolvedEffectProject;
use aestra_runtime::BindingSlot;
use std::collections::BTreeMap;

fn effect(bindings: Vec<EffectBinding>) -> EffectAsset {
    let mut effect = EffectAsset::new("Bound", 1.0);
    effect.emitters.push(Emitter::basic_sprite("Emitter", 1.0));
    effect.bindings = bindings;
    effect
}

fn diagnostics(effect: &EffectAsset) -> Vec<(DiagnosticCode, String, String)> {
    match EffectCompiler::with_extensions(ExtensionRegistry::builtin()).compile(effect) {
        Ok(_) => Vec::new(),
        Err(error) => error
            .report()
            .diagnostics
            .iter()
            .map(|d| (d.code, d.path.clone(), d.message.clone()))
            .collect(),
    }
}

#[test]
fn spatial_source_and_target_bindings_compile() {
    let mut target = EffectBinding::spatial("Target", BindingUpdateMode::Live);
    target
        .optional_fields
        .insert(BindingFieldId::new(AESTRA_FIELD_LINEAR_VELOCITY));
    let effect = effect(vec![
        EffectBinding::spatial("Source", BindingUpdateMode::SnapshotOnSpawn),
        target,
    ]);
    assert_eq!(diagnostics(&effect), Vec::new());
}

#[test]
fn an_unknown_core_kind_or_unsupplied_field_is_an_invalid_reference() {
    let mut unknown = EffectBinding::spatial("Target", BindingUpdateMode::Live);
    unknown.kind = BindingKindId::new("aestra.binding.nonexistent");
    let found = diagnostics(&effect(vec![unknown]));
    assert!(found.iter().any(|(code, path, _)| {
        *code == DiagnosticCode::InvalidReference && path == "effect.bindings[0].kind"
    }));

    let mut wrong_field = EffectBinding::spatial("Target", BindingUpdateMode::Live);
    wrong_field
        .required_fields
        .insert(BindingFieldId::new("aestra.field.temperature"));
    let found = diagnostics(&effect(vec![wrong_field]));
    assert!(found.iter().any(|(code, path, message)| {
        *code == DiagnosticCode::InvalidReference
            && path == "effect.bindings[0].fields.aestra.field.temperature"
            && message.contains("does not supply")
    }));
}

#[test]
fn a_plugin_kind_whose_extension_is_missing_is_reported_and_recorded() {
    let mut source = EffectBinding::spatial("Emitter Source", BindingUpdateMode::Live);
    source.kind = BindingKindId::new("org.example.fluid::binding/source");
    let mut asset = effect(vec![source]);
    let registry = ExtensionRegistry::builtin();
    asset.extensions = registry.derive_requirements(&asset);
    assert_eq!(
        asset.extensions.len(),
        1,
        "the plugin kind is a recorded requirement"
    );
    assert_eq!(
        asset.extensions[0].plugin,
        ExtensionId::new("org.example.fluid")
    );

    let found = diagnostics(&asset);
    assert!(found.iter().any(|(code, path, message)| {
        *code == DiagnosticCode::MissingExtension
            && path == "effect.bindings[0].kind"
            && message.contains("org.example.fluid")
    }));
    // The declaration survives a save/load round trip without the plugin.
    let reopened = EffectAsset::from_ron(&asset.to_pretty_ron().unwrap()).unwrap();
    assert_eq!(reopened.bindings, asset.bindings);
}

fn compiler() -> EffectCompiler {
    EffectCompiler::with_extensions(ExtensionRegistry::builtin())
}

#[test]
fn bindings_compile_to_dense_slots_with_layouts_in_kind_field_order() {
    let source = EffectBinding::spatial("Source", BindingUpdateMode::SnapshotOnSpawn);
    let mut target = EffectBinding::spatial("Target", BindingUpdateMode::Live);
    // Declared out of kind order: velocity (optional) before rotation (required) in authoring terms.
    target
        .optional_fields
        .insert(BindingFieldId::new(AESTRA_FIELD_LINEAR_VELOCITY));
    target
        .required_fields
        .insert(BindingFieldId::new(AESTRA_FIELD_ROTATION));
    let asset = effect(vec![source.clone(), target.clone()]);
    let compiled = compiler().compile(&asset).unwrap();

    assert_eq!(compiled.binding_slots[&source.id], BindingSlot(0));
    assert_eq!(compiled.binding_slots[&target.id], BindingSlot(1));
    let layout = &compiled.bindings[1].layout;
    let packed: Vec<(&str, u32)> = layout
        .fields
        .iter()
        .map(|field| (field.field.as_str(), field.offset))
        .collect();
    // Kind order is position, rotation, scale, linear velocity; undeclared scale is left out.
    assert_eq!(
        packed,
        [
            (AESTRA_FIELD_POSITION, 0),
            (AESTRA_FIELD_ROTATION, 3),
            (AESTRA_FIELD_LINEAR_VELOCITY, 7)
        ]
    );
    assert_eq!(layout.stride, 10);

    let requirements = compiled.host_requirements();
    assert_eq!(requirements.bindings.len(), 2);
    assert_eq!(requirements.bindings[1].name, "Target");
    assert!(requirements.bindings[1].required);
    assert_eq!(compiled.binding_named("Source").unwrap().0, BindingSlot(0));
}

/// A root effect with `Target`, and a child clip whose effect requires its own `Aim` binding.
fn forwarding_project(forward: bool) -> (ResolvedEffectProject, EffectBinding, EffectBinding) {
    let child_aim = EffectBinding::spatial("Aim", BindingUpdateMode::Live);
    let mut child = effect(vec![child_aim.clone()]);
    child.name = "Child".into();
    let root_target = EffectBinding::spatial("Target", BindingUpdateMode::Live);
    let mut root = effect(vec![
        EffectBinding::spatial("Unused", BindingUpdateMode::Live),
        root_target.clone(),
    ]);
    let mut clip = EffectClip::new(child.id, 0.0, 1.0);
    if forward {
        clip.binding_forwards.insert(child_aim.id, root_target.id);
    }
    root.effect_clips.push(clip);
    let project = ResolvedEffectProject {
        root,
        dependencies: BTreeMap::from([(child.id, child)]),
        material_programs: BTreeMap::new(),
        material_functions: BTreeMap::new(),
    };
    (project, child_aim, root_target)
}

#[test]
fn child_clip_bindings_forward_to_parent_slots() {
    let (project, child_aim, _) = forwarding_project(true);
    let compiled = compiler().compile_resolved_project(&project).unwrap();
    let forwards = &compiled.root.effect_clips[0].binding_forwards;
    assert_eq!(forwards.len(), 1);
    assert_eq!(forwards[0].child, child_aim.id);
    assert_eq!(forwards[0].child_slot, BindingSlot(0));
    assert_eq!(
        forwards[0].parent_slot,
        BindingSlot(1),
        "Target is the root's second slot"
    );
    compiled.validate_binding_forwards().unwrap();
}

fn project_messages(project: &ResolvedEffectProject) -> Vec<String> {
    match compiler().compile_resolved_project(project) {
        Ok(_) => Vec::new(),
        Err(aestra_compiler::ProjectCompileError::Effect { source, .. }) => source
            .report()
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.clone())
            .collect(),
        Err(other) => vec![other.to_string()],
    }
}

#[test]
fn unforwarded_required_child_bindings_and_bad_forwards_are_diagnosed() {
    let (project, _, _) = forwarding_project(false);
    assert!(
        project_messages(&project)
            .iter()
            .any(|message| message.contains("requires binding 'Aim'"))
    );

    // Forwarding a child binding that does not exist.
    let (mut project, _, root_target) = forwarding_project(true);
    project.root.effect_clips[0]
        .binding_forwards
        .insert(aestra_core::BindingId::new(), root_target.id);
    assert!(
        project_messages(&project)
            .iter()
            .any(|message| message.contains("declares no binding"))
    );

    // The child requires a field the parent binding does not require.
    let (mut project, child_aim, _) = forwarding_project(true);
    let child = project.dependencies.values_mut().next().unwrap();
    child.bindings[0]
        .required_fields
        .insert(BindingFieldId::new(AESTRA_FIELD_ROTATION));
    assert_eq!(child.bindings[0].id, child_aim.id);
    assert!(
        project_messages(&project)
            .iter()
            .any(|message| message.contains("does not require"))
    );
}

#[test]
fn a_forward_from_an_undeclared_parent_binding_is_a_core_diagnostic() {
    let (mut project, child_aim, _) = forwarding_project(true);
    project.root.effect_clips[0]
        .binding_forwards
        .insert(child_aim.id, aestra_core::BindingId::new());
    let report = project.root.validation_report();
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::InvalidReference
            && diagnostic.path.contains("binding_forwards")
    }));
}
