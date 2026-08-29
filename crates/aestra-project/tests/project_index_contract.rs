use aestra_core::{CURRENT_FORMAT_VERSION, EffectAsset, EffectId};
use aestra_project::{
    EffectAssetRef, ProjectAssetDiagnosticCode, ProjectAssetIndex, ProjectEffectStatus,
    ResolveEffectError,
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
