use super::*;
use crate::material_target::MaterialEditingTarget;
use aestra_core::MaterialProgramId;
use std::{fs, path::Path};

fn delete(root: &Path, folder: &str) -> DeletedSource {
    fs::create_dir_all(root.join(folder).join("empty")).unwrap();
    fs::write(root.join(folder).join("image.png"), b"exact recovery bytes").unwrap();
    let content = aestra_project::ProjectContent::scan(root);
    let source = content.source_tree().at_relative_path(folder).unwrap().id;
    content
        .plan_delete_source(source, &[], true)
        .unwrap()
        .with_recovery_data(b"opaque editor state".to_vec())
        .apply()
        .unwrap()
}

fn program(id: u128) -> Context {
    Context::Material(MaterialEditingTarget::Program {
        root: "project".into(),
        id: MaterialProgramId::from_u128(id),
    })
}

fn top_document(order: &AssetOrder, undo: bool) -> &Context {
    match order.top(undo).unwrap() {
        Entry::Document(context) => context,
        Entry::Delete(_) => panic!("expected document history"),
    }
}

#[test]
fn filesystem_order_stays_inactive_until_a_real_delete_is_recorded() {
    let root = tempfile::tempdir().unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let mut session = crate::test_support::session_with_timing_slack();
    session.set_effect_name("Edited");
    let mut order = AssetOrder::default();
    assert!(!order.sync(&session, &catalog));
    assert!(!order.active());
    assert!(order.top(true).is_none());
    assert!(session.can_undo());
    let item = delete(root.path(), "pack");
    order.record(item, &session, &catalog);
    assert!(order.active());
    assert!(matches!(
        order.undo.front(),
        Some(Entry::Document(Context::Effect))
    ));
    assert!(matches!(order.top(true), Some(Entry::Delete(_))));
}

#[test]
fn delete_restore_redo_keep_exact_files_and_recovery_data_and_advance_only_current_revision() {
    let root = tempfile::tempdir().unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let session = crate::test_support::session_with_timing_slack();
    let item = delete(root.path(), "pack");
    let first_journal = item.journal().to_owned();
    let mut order = AssetOrder::default();
    order.record(item.clone(), &session, &catalog);
    let revision = order.revision;
    assert!(!order.finish_delete(true, revision + 1, item.clone()));
    assert!(!order.finish_delete(false, revision, item.clone()));
    assert!(order.matches_delete(true, revision));
    assert!(!root.path().join("pack").exists());
    item.clone().restore(&[], true).unwrap();
    assert!(order.finish_delete(true, revision, item.clone()));
    assert_eq!(
        fs::read(root.path().join("pack/image.png")).unwrap(),
        b"exact recovery bytes"
    );
    assert!(root.path().join("pack/empty").is_dir());
    assert!(!order.finish_delete(true, revision, item.clone()));
    let revision = order.revision;
    assert!(order.matches_delete(false, revision));
    let redone = item.plan_redo(&[], true).unwrap().apply().unwrap();
    assert_ne!(redone.journal(), first_journal);
    assert_eq!(redone.recovery_data(), b"opaque editor state");
    assert!(order.finish_delete(false, revision, redone.clone()));
    assert!(!root.path().join("pack").exists());
    redone.restore(&[], true).unwrap();
}

#[test]
fn document_contexts_interleave_with_deletion_and_restore_exact_global_step_order() {
    let root = tempfile::tempdir().unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let mut session = crate::test_support::session_with_timing_slack();
    session.set_effect_name("Before deletion");
    session.operation_order.record(program(1));
    let item = delete(root.path(), "pack");
    let mut order = AssetOrder::default();
    order.record(item.clone(), &session, &catalog);
    session.operation_order.record(program(2));
    session.set_effect_name("After deletion");
    assert!(order.sync(&session, &catalog));
    assert_eq!(top_document(&order, true), &Context::Effect);
    order.finish_document(true, Context::Effect);
    assert_eq!(top_document(&order, true), &program(2));
    order.finish_document(true, program(2));
    assert!(matches!(order.top(true), Some(Entry::Delete(_))));
    let revision = order.revision;
    item.clone().restore(&[], true).unwrap();
    assert!(order.finish_delete(true, revision, item));
    assert_eq!(top_document(&order, true), &program(1));
    assert!(matches!(order.top(false), Some(Entry::Delete(_))));
}

#[test]
fn new_document_edits_fork_global_redo_without_losing_the_recovery_journal() {
    let root = tempfile::tempdir().unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let mut session = crate::test_support::session_with_timing_slack();
    let item = delete(root.path(), "pack");
    let mut order = AssetOrder::default();
    order.record(item.clone(), &session, &catalog);
    session.operation_order.record(program(1));
    assert!(order.sync(&session, &catalog));
    order.finish_document(true, program(1));
    assert!(order.top(false).is_some());
    session.operation_order.record(program(2));
    assert!(order.sync(&session, &catalog));
    assert!(order.top(false).is_none());
    assert_eq!(top_document(&order, true), &program(2));
    assert!(item.journal().exists());
    item.restore(&[], true).unwrap();
}

#[test]
fn catalog_revision_refresh_keeps_order_but_project_generation_change_retires_it() {
    let root = tempfile::tempdir().unwrap();
    let mut catalog = ProjectEffectCatalog::scan(root.path());
    let session = crate::test_support::session_with_timing_slack();
    let item = delete(root.path(), "pack");
    let mut order = AssetOrder::default();
    order.record(item.clone(), &session, &catalog);
    catalog.refresh();
    assert!(!order.sync(&session, &catalog));
    assert!(order.active());
    let replacement = ProjectEffectCatalog::scan(root.path());
    assert!(!order.sync(&session, &replacement));
    assert!(!order.active());
    assert!(item.journal().exists());
    item.restore(&[], true).unwrap();
}

#[test]
fn another_project_or_reopened_same_effect_cannot_retain_old_global_order() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let catalog = ProjectEffectCatalog::scan(first.path());
    let mut session = crate::test_support::session_with_timing_slack();
    let item = delete(first.path(), "pack");
    let mut order = AssetOrder::default();
    order.record(item.clone(), &session, &catalog);
    assert!(!order.sync(&session, &ProjectEffectCatalog::scan(second.path())));
    assert!(!order.active());
    order.record(item.clone(), &session, &catalog);
    let effect = session.effect.clone();
    let compiled = catalog.compile_project(&effect).unwrap().root;
    session.open_compiled_effect(first.path().join("open.aestra.ron"), effect, compiled);
    assert!(!order.sync(&session, &catalog));
    assert!(!order.active());
    assert!(item.journal().exists());
    item.restore(&[], true).unwrap();
}

#[test]
fn overflow_or_regressed_serial_retires_ambiguous_order_without_purging_recovery() {
    for regress in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
        session.operation_order.record(Context::Effect);
        let item = delete(root.path(), "pack");
        let mut order = AssetOrder::default();
        order.record(item.clone(), &session, &catalog);
        if regress {
            session.operation_order.serial = 0;
        } else {
            for _ in 0..=LIMIT {
                session.operation_order.record(Context::Effect);
            }
        }
        assert!(!order.sync(&session, &catalog));
        assert!(!order.active());
        assert!(item.journal().exists());
        item.restore(&[], true).unwrap();
    }
}

#[test]
fn bounded_global_history_can_evict_a_delete_without_losing_deleted_items_recovery() {
    let root = tempfile::tempdir().unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let mut session = crate::test_support::session_with_timing_slack();
    let item = delete(root.path(), "pack");
    let mut order = AssetOrder::default();
    order.record(item.clone(), &session, &catalog);
    for _ in 0..LIMIT + 10 {
        session.operation_order.record(Context::Effect);
        assert!(order.sync(&session, &catalog));
    }
    assert_eq!(order.undo.len(), LIMIT);
    assert!(
        order
            .undo
            .iter()
            .all(|entry| matches!(entry, Entry::Document(_)))
    );
    assert!(item.journal().exists());
    item.restore(&[], true).unwrap();
}

#[test]
fn clearing_one_document_or_forgetting_restored_item_preserves_other_entries() {
    let root = tempfile::tempdir().unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let mut session = crate::test_support::session_with_timing_slack();
    session.operation_order.record(program(1));
    session.operation_order.record(program(2));
    let first = delete(root.path(), "first");
    let second = delete(root.path(), "second");
    let mut order = AssetOrder::default();
    order.record(first.clone(), &session, &catalog);
    order.record(second.clone(), &session, &catalog);
    let revision = order.revision;
    order.clear_context(&program(1));
    assert!(!order.matches_delete(true, revision));
    assert!(
        !order
            .undo
            .iter()
            .any(|entry| matches!(entry, Entry::Document(context) if context == &program(1)))
    );
    assert!(
        order
            .undo
            .iter()
            .any(|entry| matches!(entry, Entry::Document(context) if context == &program(2)))
    );
    first.clone().restore(&[], true).unwrap();
    order.forget_restored(first.journal());
    assert!(
        matches!(order.top(true), Some(Entry::Delete(item)) if item.journal() == second.journal())
    );
    second.restore(&[], true).unwrap();
}

#[test]
fn failed_restore_or_changed_redo_source_keeps_order_retryable_and_never_overwrites_disk() {
    let root = tempfile::tempdir().unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let session = crate::test_support::session_with_timing_slack();
    let item = delete(root.path(), "pack");
    let mut order = AssetOrder::default();
    order.record(item.clone(), &session, &catalog);
    let revision = order.revision;
    fs::create_dir(root.path().join("pack")).unwrap();
    fs::write(root.path().join("pack/replacement.png"), b"external").unwrap();
    assert!(item.clone().restore(&[], true).is_err());
    assert!(order.matches_delete(true, revision));
    assert_eq!(
        fs::read(root.path().join("pack/replacement.png")).unwrap(),
        b"external"
    );
    // Remove only the explicitly created collision fixture, not a broad directory.
    fs::remove_file(root.path().join("pack/replacement.png")).unwrap();
    fs::remove_dir(root.path().join("pack")).unwrap();
    item.clone().restore(&[], true).unwrap();
    assert!(order.finish_delete(true, revision, item.clone()));
    let revision = order.revision;
    fs::write(root.path().join("pack/image.png"), b"newer external bytes").unwrap();
    assert!(item.plan_redo(&[], true).is_err());
    assert!(order.matches_delete(false, revision));
    assert_eq!(
        fs::read(root.path().join("pack/image.png")).unwrap(),
        b"newer external bytes"
    );
}
