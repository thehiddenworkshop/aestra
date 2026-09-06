use aestra_core::EffectAsset;
use aestra_project::{
    ProjectAssetId, ProjectAssetIndexAvailability, ProjectContentRefresh, ProjectContentSnapshot,
    ProjectContentVersion,
};
use std::{fs, path::Path, sync::Arc};

fn version() -> ProjectContentVersion {
    ProjectContentVersion {
        generation: 1,
        revision: 0,
    }
}

#[test]
fn generic_add_delete_and_empty_folder_changes_do_not_invalidate_semantics() {
    let directory = tempfile::tempdir().unwrap();
    let before = ProjectContentSnapshot::scan(directory.path());
    assert!(Arc::ptr_eq(
        &before.content,
        &before.poll(false).unwrap().content
    ));
    fs::write(directory.path().join("notes.txt"), "notes").unwrap();
    fs::create_dir(directory.path().join("empty")).unwrap();
    let added = before.poll(false).unwrap();
    let changes = added.stamp.changes_from(&before.stamp);
    assert_eq!(changes.added.len(), 2);
    assert!(!changes.semantic_changed);
    assert!(
        added
            .content
            .source_tree()
            .at_relative_path("empty")
            .is_some()
    );
    fs::remove_file(directory.path().join("notes.txt")).unwrap();
    let removed = added.poll(false).unwrap();
    let changes = removed.stamp.changes_from(&added.stamp);
    assert_eq!(changes.removed, [directory.path().join("notes.txt")]);
    assert!(!changes.semantic_changed);
}

#[test]
fn partial_writes_must_settle_twice_and_returning_to_baseline_cancels_pending() {
    let directory = tempfile::tempdir().unwrap();
    let before = ProjectContentSnapshot::scan(directory.path());
    let mut refresh = ProjectContentRefresh::new(version(), before.stamp.clone());
    let path = directory.path().join("effect.aestra.ron");
    fs::write(&path, "(format_version:").unwrap();
    let partial = before.poll(false).unwrap();
    assert!(refresh.observe(version(), &partial.stamp).is_none());
    let effect = EffectAsset::new("Ready", 1.0);
    effect.save_ron(&path).unwrap();
    let complete = before.poll(false).unwrap();
    assert!(refresh.observe(version(), &complete.stamp).is_none());
    assert!(
        refresh
            .observe(version(), &complete.stamp)
            .unwrap()
            .semantic_changed
    );
    assert!(refresh.observe(version(), &complete.stamp).is_none());
    refresh.reset(version(), before.stamp.clone());
    assert!(refresh.observe(version(), &partial.stamp).is_none());
    assert!(refresh.observe(version(), &before.stamp).is_none());
    assert!(refresh.observe(version(), &partial.stamp).is_none());
    refresh.unsettled();
    assert!(refresh.observe(version(), &partial.stamp).is_none());
}

#[test]
fn explicit_refresh_detects_same_size_semantic_edits_with_restored_timestamps() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("effect.aestra.ron");
    let mut effect = EffectAsset::new("AAA", 1.0);
    effect.save_ron(&path).unwrap();
    let before = ProjectContentSnapshot::scan(directory.path());
    let metadata = fs::metadata(&path).unwrap();
    effect.name = "BBB".into();
    // In-place edit: do not change the parent directory's stamp via atomic file replacement.
    fs::write(&path, effect.to_pretty_ron().unwrap()).unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(metadata.modified().unwrap())
        .unwrap();
    assert_eq!(metadata.len(), fs::metadata(&path).unwrap().len());
    assert!(Arc::ptr_eq(
        &before.content,
        &before.poll(false).unwrap().content
    ));
    // Cached fingerprints are not an exact-content guarantee. Force is the explicit full check.
    let after = before.poll(true).unwrap();
    assert!(after.stamp.changes_from(&before.stamp).semantic_changed);
    assert_ne!(
        after.stamp.file(&path).unwrap().fingerprint,
        before.stamp.file(&path).unwrap().fingerprint
    );
    assert_eq!(
        after
            .content
            .asset_index()
            .load_effect(effect.id.into())
            .unwrap()
            .name,
        "BBB"
    );
}

#[test]
fn stale_scans_are_rejected_after_root_switches_and_internal_writes() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let old = ProjectContentSnapshot::scan(first.path());
    let mut refresh = ProjectContentRefresh::new(version(), old.stamp.clone());
    fs::write(first.path().join("notes.txt"), "changed").unwrap();
    let changed = old.poll(false).unwrap();
    let after_write = ProjectContentVersion {
        generation: 1,
        revision: 1,
    };
    refresh.reset(after_write, old.stamp.clone());
    assert!(refresh.observe(version(), &changed.stamp).is_none());
    assert!(refresh.observe(version(), &changed.stamp).is_none());
    let new_root = ProjectContentSnapshot::scan(second.path());
    let switched = ProjectContentVersion {
        generation: 2,
        revision: 0,
    };
    refresh.reset(switched, new_root.stamp);
    assert!(refresh.observe(after_write, &changed.stamp).is_none());
    assert!(refresh.observe(switched, &changed.stamp).is_none());
}

#[test]
fn folder_moves_publish_updated_sources_and_preserve_semantic_identity() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("old")).unwrap();
    let effect = EffectAsset::new("Effect", 1.0);
    effect
        .save_ron(directory.path().join("old/effect.aestra.ron"))
        .unwrap();
    let before = ProjectContentSnapshot::scan(directory.path());
    fs::rename(directory.path().join("old"), directory.path().join("new")).unwrap();
    let after = before.poll(false).unwrap();
    assert!(after.stamp.changes_from(&before.stamp).semantic_changed);
    let asset = ProjectAssetId::Effect(effect.id);
    let source = after.content.unique_source_for_asset(asset).unwrap();
    assert_eq!(source.relative_path, Path::new("new/effect.aestra.ron"));
    assert_ne!(
        source.id,
        before.content.unique_source_for_asset(asset).unwrap().id
    );
    assert_eq!(
        after
            .content
            .asset_index()
            .resolve(effect.id.into())
            .unwrap()
            .id,
        source.id
    );
}

#[test]
fn root_unavailability_and_recovery_are_observable_changes() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    fs::create_dir(&root).unwrap();
    let before = ProjectContentSnapshot::scan(&root);
    fs::remove_dir(&root).unwrap();
    let unavailable = before.poll(false).unwrap();
    assert!(
        unavailable
            .stamp
            .changes_from(&before.stamp)
            .availability_changed
    );
    assert!(matches!(
        unavailable.content.asset_index().availability(),
        ProjectAssetIndexAvailability::Unavailable { .. }
    ));
    fs::create_dir(&root).unwrap();
    let recovered = unavailable.poll(false).unwrap();
    assert!(
        recovered
            .stamp
            .changes_from(&unavailable.stamp)
            .availability_changed
    );
    assert_eq!(
        recovered.content.asset_index().availability(),
        &ProjectAssetIndexAvailability::Ready
    );
}

#[cfg(windows)]
#[test]
fn windows_dialog_paths_match_verbatim_root_paths_without_filesystem_io() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("effect.aestra.ron");
    EffectAsset::new("Effect", 1.0).save_ron(&path).unwrap();
    let snapshot = ProjectContentSnapshot::scan(directory.path());
    let alias = std::path::PathBuf::from(format!(r"\\?\{}", path.display()).to_ascii_uppercase());
    fs::remove_file(&path).unwrap();
    assert_eq!(snapshot.stamp.file(&alias), snapshot.stamp.file(&path));
    assert!(snapshot.stamp.file(&alias).is_some());
}
