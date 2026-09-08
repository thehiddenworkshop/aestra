use super::*;
use aestra_core::{
    EffectAsset,
    material::{MaterialFunction, MaterialProgram},
};

fn fixture() -> (tempfile::TempDir, ProjectContent, Vec<OperationRequest>) {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("destination")).unwrap();
    let effect = EffectAsset::new("Effect", 1.0);
    let program = MaterialProgram::additive_sprite("Material");
    let function = MaterialFunction::from_ron(include_str!(
        "../../../../../../assets/materials/dissolve_edge.aestra.material-function.ron"
    ))
    .unwrap();
    fs::write(
        root.path().join("effect.aestra.ron"),
        format!("// preserve comments\n{}", effect.to_pretty_ron().unwrap()),
    )
    .unwrap();
    program
        .save_ron(root.path().join("material.aestra.material.ron"))
        .unwrap();
    function
        .save_ron(root.path().join("function.aestra.material-function.ron"))
        .unwrap();
    let content = ProjectContent::scan(root.path());
    let parent = content
        .source_tree()
        .at_relative_path(Path::new("destination"))
        .unwrap()
        .id;
    let requests = [
        ProjectAssetId::Effect(effect.id),
        ProjectAssetId::MaterialProgram(program.id),
        ProjectAssetId::MaterialFunction(function.id),
    ]
    .into_iter()
    .map(|asset| OperationRequest::Move {
        source: content.unique_source_for_asset(asset).unwrap().id,
        parent,
    })
    .collect();
    (root, content, requests)
}

#[test]
fn batch_preserves_every_byte_identity_and_returns_relocations() {
    let (root, content, requests) = fixture();
    let before = rename::inventory(root.path()).unwrap();
    let result = content
        .plan_asset_moves(requests, &[], true)
        .unwrap()
        .apply()
        .unwrap();
    assert_eq!(result.moves.len(), 3);
    assert!(result.journal.exists());
    assert_eq!(result.journal.extension().unwrap(), "complete");
    let fresh = ProjectContent::scan(root.path());
    for item in result.moves {
        assert!(!item.source.exists());
        assert_eq!(fs::read(&item.destination).unwrap(), before[&item.source]);
        assert_eq!(
            fresh.unique_source_for_asset(item.asset).unwrap().path,
            item.destination
        );
    }
    assert!(fresh.pending_asset_move_batch().unwrap().is_none());
}

#[test]
fn failure_at_every_publication_boundary_rolls_back_all_files() {
    for boundary in 0..=3 {
        let (root, content, requests) = fixture();
        let before = rename::inventory(root.path()).unwrap();
        let result = content
            .plan_asset_moves(requests, &[], true)
            .unwrap()
            .apply_with(|at| {
                if at == boundary {
                    Err(blocked("Injected publication failure"))
                } else {
                    Ok(())
                }
            });
        assert!(result.unwrap_err().to_string().contains("rolled back"));
        assert_eq!(rename::inventory(root.path()).unwrap(), before);
        assert!(content.pending_asset_move_batch().unwrap().is_none());
        assert_eq!(
            fs::read_dir(root.path().join(".aestra/asset-transactions"))
                .unwrap()
                .filter_map(Result::ok)
                .filter(|entry| entry
                    .path()
                    .extension()
                    .is_some_and(|ext| ext == "rolled-back"))
                .count(),
            1
        );
    }
}

// Stop with the real durable journal and exactly N exclusive moves published,
// without running apply's in-process error handler (simulated process termination).
fn interrupt(plan: AssetMoveBatchPlan, count: usize) -> (PathBuf, Record) {
    let (journal, record) = prepare(&plan.root, plan.moves).unwrap();
    for item in record.moves.iter().take(count) {
        move_one(&plan.root, item, false).unwrap();
    }
    (journal, record)
}

#[test]
fn restart_inspection_is_read_only_and_explicit_rollback_is_restartable() {
    for count in 0..=3 {
        let (root, content, requests) = fixture();
        let before = rename::inventory(root.path()).unwrap();
        let (journal, record) = interrupt(
            content.plan_asset_moves(requests, &[], true).unwrap(),
            count,
        );
        let interrupted = rename::inventory(root.path()).unwrap();
        let fresh = ProjectContent::scan(root.path());
        let pending = fresh.pending_asset_move_batch().unwrap().unwrap();
        assert_eq!(pending.moved_files, count);
        assert_eq!(
            rename::inventory(root.path()).unwrap(),
            interrupted,
            "Inspection must not restore anything"
        );
        assert!(ensure_idle(root.path()).is_err());
        if count > 0 {
            // Crash after the first reverse step. A subsequent process resumes
            // from file states, not a stale numeric progress counter.
            assert!(
                rollback(root.path(), &journal, &record, |_| Err(blocked(
                    "Interrupted rollback"
                )))
                .is_err()
            );
        }
        let restarted = ProjectContent::scan(root.path());
        let pending = restarted.pending_asset_move_batch().unwrap().unwrap();
        let archived = pending.rollback(&[], true).unwrap();
        assert_eq!(archived.extension().unwrap(), "rolled-back");
        assert_eq!(rename::inventory(root.path()).unwrap(), before);
        assert!(restarted.pending_asset_move_batch().unwrap().is_none());
        ensure_idle(root.path()).unwrap();
    }
}

#[test]
fn external_changes_block_recovery_without_overwriting_other_moved_files() {
    for collision in [false, true] {
        let (root, content, requests) = fixture();
        let (journal, record) =
            interrupt(content.plan_asset_moves(requests, &[], true).unwrap(), 3);
        let pending = ProjectContent::scan(root.path())
            .pending_asset_move_batch()
            .unwrap()
            .unwrap();
        let changed = root.path().join(if collision {
            &record.moves[0].source
        } else {
            &record.moves[0].destination
        });
        fs::write(&changed, b"external edit; do not overwrite").unwrap();
        let before = rename::inventory(root.path()).unwrap();
        assert!(pending.rollback(&[], true).is_err());
        assert_eq!(rename::inventory(root.path()).unwrap(), before);
        assert!(journal.exists());
        assert_eq!(
            read_record(root.path(), &journal).unwrap().moves,
            record.moves
        );
    }
}

#[test]
fn stale_plans_dirty_sources_collisions_and_overlapping_destinations_are_blocked() {
    let (root, content, requests) = fixture();
    let plan = content
        .plan_asset_moves(requests.clone(), &[], true)
        .unwrap();
    assert!(
        content
            .plan_asset_moves(requests.clone(), &[], false)
            .is_err()
    );
    let source = match requests[0] {
        OperationRequest::Move { source, .. } => source,
        _ => unreachable!(),
    };
    let effect = EffectAsset::load_ron(&content.source(source).unwrap().path).unwrap();
    assert!(
        content
            .plan_asset_moves(
                requests.clone(),
                &[(source, DraftDocument::Effect(Box::new(effect)))],
                true
            )
            .is_err()
    );
    assert!(
        content
            .plan_asset_moves(vec![requests[0].clone(), requests[0].clone()], &[], true)
            .is_err()
    );
    fs::write(
        root.path().join("destination/effect.aestra.ron"),
        b"collision",
    )
    .unwrap();
    assert!(plan.apply().is_err());
    assert!(content.plan_asset_moves(requests, &[], true).is_err());
    assert!(
        !root
            .path()
            .join(".aestra/asset-transactions/active.pending")
            .exists()
    );
}

#[test]
fn pending_transaction_blocks_other_mutations_and_draftful_recovery() {
    let (root, content, requests) = fixture();
    let existing = content
        .plan_asset_move(requests[0].clone(), &[], true)
        .unwrap();
    let folder = content
        .plan_operation(OperationRequest::CreateFolder {
            parent: content.source_tree().root(),
            name: "new".into(),
        })
        .unwrap();
    interrupt(
        content
            .plan_asset_moves(requests.clone(), &[], true)
            .unwrap(),
        1,
    );
    assert!(existing.apply().is_err());
    assert!(folder.apply().is_err());
    assert!(content.plan_asset_moves(requests, &[], true).is_err());
    let pending = content.pending_asset_move_batch().unwrap().unwrap();
    assert!(pending.rollback(&[], false).is_err());
    assert!(!root.path().join("new").exists());
}

#[test]
fn malformed_traversing_relocated_and_modified_journals_never_replay() {
    for invalid in 0..5 {
        let (root, content, requests) = fixture();
        let (journal, mut record) =
            interrupt(content.plan_asset_moves(requests, &[], true).unwrap(), 1);
        let inspected = content.pending_asset_move_batch().unwrap().unwrap();
        match invalid {
            0 => record.version = 999,
            1 => record.moves[0].source = "../outside.ron".into(),
            2 => record.root = root.path().join("different-root"),
            3 => record.moves[0].bytes = b"not a semantic asset".to_vec(),
            _ => record.archive = "../escape".into(),
        }
        fs::write(&journal, serde_json::to_vec(&record).unwrap()).unwrap();
        let before = rename::inventory(root.path()).unwrap();
        assert!(content.pending_asset_move_batch().is_err());
        assert!(inspected.rollback(&[], true).is_err());
        assert_eq!(rename::inventory(root.path()).unwrap(), before);
        assert!(journal.exists());
    }
}

#[test]
fn exclusive_move_failure_does_not_overwrite_destination() {
    let (root, content, requests) = fixture();
    let plan = content.plan_asset_moves(requests, &[], true).unwrap();
    let destination = root.path().join(&plan.moves[1].destination);
    let result = plan.apply_with(|at| {
        if at == 1 {
            fs::write(&destination, b"created concurrently")?;
        }
        Ok(())
    });
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Recovery required")
    );
    assert_eq!(fs::read(destination).unwrap(), b"created concurrently");
    assert!(
        root.path()
            .join(".aestra/asset-transactions/active.pending")
            .exists()
    );
}

#[test]
fn distinct_assets_cannot_plan_the_same_destination() {
    let root = tempfile::tempdir().unwrap();
    for folder in ["a", "b", "destination"] {
        fs::create_dir(root.path().join(folder)).unwrap();
    }
    EffectAsset::new("A", 1.0)
        .save_ron(root.path().join("a/same.aestra.ron"))
        .unwrap();
    EffectAsset::new("B", 1.0)
        .save_ron(root.path().join("b/same.aestra.ron"))
        .unwrap();
    let content = ProjectContent::scan(root.path());
    let parent = content
        .source_tree()
        .at_relative_path(Path::new("destination"))
        .unwrap()
        .id;
    let requests = ["a/same.aestra.ron", "b/same.aestra.ron"]
        .into_iter()
        .map(|path| OperationRequest::Move {
            source: content
                .source_tree()
                .at_relative_path(Path::new(path))
                .unwrap()
                .id,
            parent,
        })
        .collect();
    assert!(
        content
            .plan_asset_moves(requests, &[], true)
            .unwrap_err()
            .to_string()
            .contains("Overlapping")
    );
    assert!(
        fs::read_dir(root.path().join("destination"))
            .unwrap()
            .next()
            .is_none()
    );
}

#[test]
fn existing_archive_is_never_overwritten_and_commit_failure_rolls_back() {
    let (root, content, requests) = fixture();
    let before = rename::inventory(root.path()).unwrap();
    let result = content
        .plan_asset_moves(requests, &[], true)
        .unwrap()
        .apply_with(|at| {
            if at == 3 {
                let pending = directory(root.path(), false)?.join("active.pending");
                let record = read_record(root.path(), &pending)?;
                fs::write(
                    pending
                        .parent()
                        .unwrap()
                        .join(format!("{}.complete", record.archive)),
                    b"unrelated archive",
                )?;
            }
            Ok(())
        });
    assert!(result.unwrap_err().to_string().contains("rolled back"));
    assert_eq!(rename::inventory(root.path()).unwrap(), before);
    let archives: Vec<_> = fs::read_dir(directory(root.path(), false).unwrap())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|ext| ext == "complete")
        })
        .collect();
    assert_eq!(archives.len(), 1);
    assert_eq!(fs::read(archives[0].path()).unwrap(), b"unrelated archive");
}
