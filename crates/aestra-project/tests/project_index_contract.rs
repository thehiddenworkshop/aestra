use aestra_core::{CURRENT_FORMAT_VERSION, EffectAsset, EffectClip, EffectId};
use aestra_project::{
    EffectAssetRef, ProjectAssetDiagnosticCode, ProjectAssetIndex, ProjectDependencyDiagnosticCode,
    ProjectEffectStatus, ResolveEffectError,
};
use std::fs;

fn write_effect(path: &std::path::Path, id: EffectId, name: &str) {
    let mut effect = EffectAsset::new(name, 1.0);
    effect.id = id;
    effect.save_ron(path).unwrap();
}

#[test]
fn effect_identity_survives_source_rename_and_move() {
    let temporary = tempfile::tempdir().unwrap();
    let nested = temporary.path().join("nested");
    fs::create_dir_all(&nested).unwrap();
    let id = EffectId::from_u128(0xA357);
    let original = temporary.path().join("original.aestra.ron");
    let moved = nested.join("renamed.aestra.ron");
    write_effect(&original, id, "Stable Effect");

    let first = ProjectAssetIndex::scan(temporary.path());
    let reference = EffectAssetRef::new(id);
    assert_eq!(first.resolve(reference).unwrap().path, original);

    fs::rename(&original, &moved).unwrap();
    let second = ProjectAssetIndex::scan(temporary.path());

    assert_eq!(second.resolve(reference).unwrap().path, moved);
    assert_eq!(
        second.resolve(reference).unwrap().display_name,
        "Stable Effect"
    );
}

#[test]
fn duplicate_effect_ids_are_indexed_but_never_resolve_ambiguously() {
    let temporary = tempfile::tempdir().unwrap();
    let id = EffectId::from_u128(0xD00D);
    write_effect(&temporary.path().join("one.aestra.ron"), id, "One");
    write_effect(&temporary.path().join("two.aestra.ron"), id, "Two");

    let index = ProjectAssetIndex::scan(temporary.path());
    let reference = EffectAssetRef::new(id);

    assert_eq!(index.effects().len(), 2);
    assert!(
        index
            .effects()
            .iter()
            .all(|entry| matches!(entry.status, ProjectEffectStatus::DuplicateId { .. }))
    );
    assert!(matches!(
        index.resolve(reference),
        Err(ResolveEffectError::Duplicate { ref sources, .. }) if sources.len() == 2
    ));
    assert!(
        index
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == ProjectAssetDiagnosticCode::DuplicateId)
    );
}

#[test]
fn missing_effect_references_return_a_structured_error() {
    let temporary = tempfile::tempdir().unwrap();
    let index = ProjectAssetIndex::scan(temporary.path());
    let reference = EffectAssetRef::new(EffectId::from_u128(0x5155));

    assert_eq!(
        index.resolve(reference),
        Err(ResolveEffectError::Missing { reference })
    );
}

#[test]
fn source_identity_changes_after_indexing_are_rejected() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("effect.aestra.ron");
    let original = EffectId::from_u128(0x111);
    write_effect(&path, original, "Original");
    let index = ProjectAssetIndex::scan(temporary.path());

    write_effect(&path, EffectId::from_u128(0x222), "Replacement");

    assert!(matches!(
        index.load_effect(EffectAssetRef::new(original)),
        Err(ResolveEffectError::SourceChanged { .. })
    ));
}

#[test]
fn invalid_and_future_assets_remain_visible_with_diagnostics() {
    let temporary = tempfile::tempdir().unwrap();
    fs::write(temporary.path().join("broken.aestra.ron"), "not RON").unwrap();
    let future_path = temporary.path().join("future.aestra.ron");
    EffectAsset::new("Future", 1.0)
        .save_ron(&future_path)
        .unwrap();
    let future = fs::read_to_string(&future_path).unwrap().replacen(
        &format!("format_version: {CURRENT_FORMAT_VERSION}"),
        "format_version: 999",
        1,
    );
    fs::write(&future_path, future).unwrap();

    let index = ProjectAssetIndex::scan(temporary.path());

    assert_eq!(index.effects().len(), 2);
    assert!(
        index
            .effects()
            .iter()
            .any(|entry| matches!(entry.status, ProjectEffectStatus::Invalid { .. }))
    );
    assert!(index.effects().iter().any(|entry| matches!(
        entry.status,
        ProjectEffectStatus::Unsupported { found: 999, .. }
    )));
    assert!(
        index
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == ProjectAssetDiagnosticCode::InvalidAsset)
    );
    assert!(
        index
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == ProjectAssetDiagnosticCode::UnsupportedFormat)
    );
}

#[test]
fn transitive_effect_dependencies_resolve_once_by_stable_identity() {
    let temporary = tempfile::tempdir().unwrap();
    let root_id = EffectId::from_u128(0x100);
    let child_id = EffectId::from_u128(0x200);
    let grandchild_id = EffectId::from_u128(0x300);

    let mut grandchild = EffectAsset::new("Grandchild", 1.0);
    grandchild.id = grandchild_id;
    grandchild
        .save_ron(temporary.path().join("grandchild.aestra.ron"))
        .unwrap();
    let mut child = EffectAsset::new("Child", 1.0);
    child.id = child_id;
    child
        .effect_clips
        .push(EffectClip::new(grandchild_id, 0.0, 1.0));
    child
        .save_ron(temporary.path().join("child.aestra.ron"))
        .unwrap();
    let mut root = EffectAsset::new("Root", 1.0);
    root.id = root_id;
    root.effect_clips.push(EffectClip::new(child_id, 0.0, 1.0));

    let index = ProjectAssetIndex::scan(temporary.path());
    let resolved = index.resolve_effect_project(&root).unwrap();

    assert_eq!(resolved.dependencies.len(), 2);
    assert_eq!(resolved.effect(child_id).unwrap().name, "Child");
    assert_eq!(resolved.effect(grandchild_id).unwrap().name, "Grandchild");
}

#[test]
fn missing_transitive_references_identify_the_owning_clip() {
    let temporary = tempfile::tempdir().unwrap();
    let missing = EffectId::from_u128(0x5155);
    let mut root = EffectAsset::new("Root", 1.0);
    root.effect_clips.push(EffectClip::new(missing, 0.0, 1.0));
    let clip_id = root.effect_clips[0].id;

    let report = ProjectAssetIndex::scan(temporary.path())
        .resolve_effect_project(&root)
        .unwrap_err();

    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == ProjectDependencyDiagnosticCode::Missing
            && diagnostic.owner == root.id
            && diagnostic.clip == clip_id
            && diagnostic.reference.id == missing
    }));
}

#[test]
fn indirect_effect_reference_cycles_are_rejected_with_the_cycle_path() {
    let temporary = tempfile::tempdir().unwrap();
    let first_id = EffectId::from_u128(0xA);
    let second_id = EffectId::from_u128(0xB);
    let mut first = EffectAsset::new("First", 1.0);
    first.id = first_id;
    first
        .effect_clips
        .push(EffectClip::new(second_id, 0.0, 1.0));
    let mut second = EffectAsset::new("Second", 1.0);
    second.id = second_id;
    second
        .effect_clips
        .push(EffectClip::new(first_id, 0.0, 1.0));
    first
        .save_ron(temporary.path().join("first.aestra.ron"))
        .unwrap();
    second
        .save_ron(temporary.path().join("second.aestra.ron"))
        .unwrap();

    let report = ProjectAssetIndex::scan(temporary.path())
        .resolve_effect_project(&first)
        .unwrap_err();

    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == ProjectDependencyDiagnosticCode::Cycle
            && diagnostic.cycle == [first_id, second_id, first_id]
    }));
}

#[test]
fn clips_cannot_overrun_a_non_looping_source_window() {
    let temporary = tempfile::tempdir().unwrap();
    let mut child = EffectAsset::new("Finite", 1.0);
    child.looping = false;
    child
        .save_ron(temporary.path().join("finite.aestra.ron"))
        .unwrap();
    let mut root = EffectAsset::new("Root", 2.0);
    let mut clip = EffectClip::new(child.id, 0.0, 1.0);
    clip.source_offset = 0.5;
    root.effect_clips.push(clip);

    let report = ProjectAssetIndex::scan(temporary.path())
        .resolve_effect_project(&root)
        .unwrap_err();

    assert!(
        report.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == ProjectDependencyDiagnosticCode::InvalidTiming
        })
    );
}
