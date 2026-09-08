use super::*;
use aestra_core::{AssetDefinition, EffectAsset};

fn fixture() -> (tempfile::TempDir, ProjectContent, Vec<OperationRequest>) {
    let (root, _, requests) = tests::fixture();
    let path = root.path().join("effect.aestra.ron");
    let mut moved = EffectAsset::load_ron(&path).unwrap();
    moved.assets.push(AssetDefinition::texture(
        "material.aestra.material.ron",
        "./material.aestra.material.ron",
    ));
    moved.assets.push(AssetDefinition::texture(
        "Second binding",
        "function.aestra.material-function.ron",
    ));
    fs::write(
        path,
        format!(
            "// exact original formatting and comments\n{}",
            moved.to_pretty_ron().unwrap()
        ),
    )
    .unwrap();
    let mut stationary = EffectAsset::new("material.aestra.material.ron", 2.0);
    stationary.assets = moved.assets.clone();
    // Separate declarations keep their resource IDs, regardless of reference spelling.
    stationary.assets.push(AssetDefinition::texture(
        "Third binding",
        ".\\material.aestra.material.ron",
    ));
    stationary
        .save_ron(root.path().join("owner.aestra.ron"))
        .unwrap();
    let content = ProjectContent::scan(root.path());
    (root, content, requests)
}

fn interrupted(plan: AssetMoveBatchPlan, count: usize) -> (PathBuf, Record) {
    let (journal, record) = prepare(&plan.root, plan.moves, plan.replacements).unwrap();
    for index in 0..count {
        steps::advance(&plan.root, &record, index, false).unwrap();
    }
    (journal, record)
}

#[test]
fn typed_rewrites_cover_moved_and_stationary_owners_without_changing_ids_or_names() {
    let (root, content, requests) = fixture();
    let before = rename::inventory(root.path()).unwrap();
    // The legacy single-file path still refuses a rewrite. Only the transaction
    // planner can authorize this; no unsafe RenamePlan escapes the public API.
    assert!(
        content
            .plan_asset_move(requests[1].clone(), &[], true)
            .is_err()
    );
    let plan = content.plan_asset_moves(requests, &[], true).unwrap();
    assert_eq!(plan.rewritten_sources().len(), 2);
    assert_eq!(rename::inventory(root.path()).unwrap(), before);
    assert!(
        !root
            .path()
            .join(".aestra/asset-transactions/active.pending")
            .exists()
    );
    let result = plan.apply().unwrap();
    assert_eq!(result.rewritten_sources.len(), 2);
    for source in ["effect.aestra.ron", "owner.aestra.ron"] {
        let path = if source == "effect.aestra.ron" {
            root.path().join("destination").join(source)
        } else {
            root.path().join(source)
        };
        assert!(result.rewritten_sources.contains(&path));
        let mut expected =
            EffectAsset::from_ron(std::str::from_utf8(&before[&root.path().join(source)]).unwrap())
                .unwrap();
        for resource in &mut expected.assets {
            resource.path = if resource.path.contains("function") {
                "destination/function.aestra.material-function.ron".into()
            } else {
                "destination/material.aestra.material.ron".into()
            };
        }
        assert_eq!(EffectAsset::load_ron(&path).unwrap(), expected);
        assert!(
            ProjectContent::scan(root.path())
                .unique_source_for_asset(ProjectAssetId::Effect(expected.id))
                .is_ok()
        );
    }
    for item in &result.moves[1..] {
        assert_eq!(fs::read(&item.destination).unwrap(), before[&item.source]);
    }
    let record: Record = serde_json::from_slice(&fs::read(result.journal).unwrap()).unwrap();
    for item in record.replacements {
        assert_eq!(item.before, before[&root.path().join(item.source)]);
    }
}

#[test]
fn every_move_detach_and_replacement_failure_restores_exact_original_bytes() {
    for boundary in 0..=7 {
        let (root, content, requests) = fixture();
        let before = rename::inventory(root.path()).unwrap();
        let result = content
            .plan_asset_moves(requests, &[], true)
            .unwrap()
            .apply_with(|at| {
                if at == boundary {
                    Err(blocked("Injected failure"))
                } else {
                    Ok(())
                }
            });
        assert!(
            result.unwrap_err().to_string().contains("rolled back"),
            "boundary {boundary}"
        );
        assert_eq!(
            rename::inventory(root.path()).unwrap(),
            before,
            "boundary {boundary}"
        );
        assert!(content.pending_asset_move_batch().unwrap().is_none());
    }
}

#[test]
fn restart_and_interrupted_reverse_recovery_work_at_every_replacement_boundary() {
    for boundary in 0..=7 {
        let (root, content, requests) = fixture();
        let before = rename::inventory(root.path()).unwrap();
        let (journal, record) = interrupted(
            content.plan_asset_moves(requests, &[], true).unwrap(),
            boundary,
        );
        let at_crash = rename::inventory(root.path()).unwrap();
        let fresh = ProjectContent::scan(root.path());
        let pending = fresh.pending_asset_move_batch().unwrap().unwrap();
        assert_eq!(pending.moved_files, boundary.min(3));
        assert_eq!(rename::inventory(root.path()).unwrap(), at_crash);
        if boundary > 0 {
            assert!(
                rollback(root.path(), &journal, &record, |_| Err(blocked(
                    "Interrupted restore"
                )))
                .is_err()
            );
        }
        ProjectContent::scan(root.path())
            .pending_asset_move_batch()
            .unwrap()
            .unwrap()
            .rollback(&[], true)
            .unwrap();
        assert_eq!(
            rename::inventory(root.path()).unwrap(),
            before,
            "boundary {boundary}"
        );
        assert!(fresh.pending_asset_move_batch().unwrap().is_none());
    }
}

#[test]
fn external_edits_to_owner_backup_or_staging_block_the_entire_recovery() {
    for boundary in 0..=7 {
        let (root, content, requests) = fixture();
        let (_, record) = interrupted(
            content.plan_asset_moves(requests, &[], true).unwrap(),
            boundary,
        );
        // Includes published owners, detached original backups and unpublished stages.
        let mut paths = BTreeSet::new();
        for step in steps::list(&record) {
            for relative in [step.source, step.destination] {
                let path = root.path().join(relative);
                if path.is_file() {
                    paths.insert(path);
                }
            }
        }
        for path in paths {
            let original = fs::read(&path).unwrap();
            fs::write(&path, b"external user edit").unwrap();
            let before = rename::inventory(root.path()).unwrap();
            assert!(
                ProjectContent::scan(root.path())
                    .pending_asset_move_batch()
                    .is_err()
            );
            assert!(
                rollback(
                    root.path(),
                    &root
                        .path()
                        .join(".aestra/asset-transactions/active.pending"),
                    &record,
                    |_| Ok(())
                )
                .is_err()
            );
            assert_eq!(rename::inventory(root.path()).unwrap(), before);
            assert_eq!(fs::read(&path).unwrap(), b"external user edit");
            fs::write(&path, original).unwrap();
        }
    }
}

#[test]
fn owner_drafts_stale_bytes_unknown_sources_and_unresolved_paths_block_preflight() {
    let (root, content, requests) = fixture();
    let owner_path = root.path().join("owner.aestra.ron");
    let owner = EffectAsset::load_ron(&owner_path).unwrap();
    let owner_id = content
        .unique_source_for_asset(ProjectAssetId::Effect(owner.id))
        .unwrap()
        .id;
    let before = rename::inventory(root.path()).unwrap();
    assert!(
        content
            .plan_asset_moves(
                requests.clone(),
                &[(owner_id, DraftDocument::Effect(Box::new(owner.clone())))],
                true
            )
            .is_err()
    );
    assert!(
        content
            .plan_asset_moves(requests.clone(), &[], false)
            .is_err()
    );
    let plan = content
        .plan_asset_moves(requests.clone(), &[], true)
        .unwrap();
    let mut changed = owner.clone();
    changed.name = "External saved edit".into();
    changed.save_ron(&owner_path).unwrap();
    assert!(plan.apply().is_err());
    assert_eq!(EffectAsset::load_ron(&owner_path).unwrap(), changed);
    for bad_path in [
        "missing.png",
        "../outside.png",
        "C:/outside.png",
        "/outside.png",
        ".aestra/private.png",
    ] {
        changed.assets[0].path = bad_path.into();
        // Deliberately invalid authored input bypasses the validated save helper.
        fs::write(
            &owner_path,
            ron::ser::to_string_pretty(&changed, ron::ser::PrettyConfig::default()).unwrap(),
        )
        .unwrap();
        let content = ProjectContent::scan(root.path());
        assert!(
            content
                .plan_asset_moves(requests.clone(), &[], true)
                .is_err(),
            "{bad_path}"
        );
    }
    fs::write(&owner_path, &before[&owner_path]).unwrap();
    fs::write(root.path().join("unknown.wesl"), "import package::unknown;").unwrap();
    assert!(
        ProjectContent::scan(root.path())
            .plan_asset_moves(requests, &[], true)
            .is_err()
    );
    assert!(root.path().join("effect.aestra.ron").exists());
}

#[test]
fn read_only_reference_owner_is_rejected_without_modification() {
    let (root, content, requests) = fixture();
    let path = root.path().join("owner.aestra.ron");
    let original = fs::metadata(&path).unwrap().permissions();
    let mut readonly = original.clone();
    readonly.set_readonly(true);
    fs::set_permissions(&path, readonly).unwrap();
    let result = content.plan_asset_moves(requests, &[], true);
    fs::set_permissions(path, original).unwrap();
    assert!(result.is_err());
}

#[test]
fn replacement_journals_cannot_change_unrelated_fields_or_escape_root() {
    for invalid in 0..3 {
        let (root, content, requests) = fixture();
        let (journal, mut record) =
            interrupted(content.plan_asset_moves(requests, &[], true).unwrap(), 4);
        match invalid {
            0 => record.replacements[0].source = "../outside.ron".into(),
            1 => {
                let mut after = EffectAsset::from_ron(
                    std::str::from_utf8(&record.replacements[0].after).unwrap(),
                )
                .unwrap();
                after.name = "Unplanned change".into();
                record.replacements[0].after = after.to_pretty_ron().unwrap().into_bytes();
            }
            _ => record.version = 1,
        }
        fs::write(&journal, serde_json::to_vec(&record).unwrap()).unwrap();
        let before = rename::inventory(root.path()).unwrap();
        assert!(
            ProjectContent::scan(root.path())
                .pending_asset_move_batch()
                .is_err()
        );
        assert_eq!(rename::inventory(root.path()).unwrap(), before);
    }
}

#[test]
fn version_one_move_only_journals_remain_recoverable() {
    let (root, content, requests) = tests::fixture();
    let before = rename::inventory(root.path()).unwrap();
    let (journal, record) = interrupted(content.plan_asset_moves(requests, &[], true).unwrap(), 2);
    let mut legacy = serde_json::to_value(record).unwrap();
    legacy["version"] = 1.into();
    legacy.as_object_mut().unwrap().remove("replacements");
    fs::write(journal, serde_json::to_vec(&legacy).unwrap()).unwrap();
    content
        .pending_asset_move_batch()
        .unwrap()
        .unwrap()
        .rollback(&[], true)
        .unwrap();
    assert_eq!(rename::inventory(root.path()).unwrap(), before);
}

#[test]
fn path_token_edits_preserve_comments_unknown_fields_raw_strings_and_formatting() {
    let (root, _, requests) = fixture();
    let path = root.path().join("owner.aestra.ron");
    let mut original = fs::read_to_string(&path).unwrap();
    original = original.replace(
        "path: \"./material.aestra.material.ron\"",
        "path: /* keep this */ r#\"./material.aestra.material.ron\"# /* and this */",
    );
    original.insert_str(
        original.rfind(')').unwrap(),
        "\n    extra_metadata: (path: \"./material.aestra.material.ron\", note: \"préservé\"),\n",
    );
    original = format!("// material.aestra.material.ron should stay here\r\n{original}");
    fs::write(&path, &original).unwrap();
    ProjectContent::scan(root.path())
        .plan_asset_moves(requests, &[], true)
        .unwrap()
        .apply()
        .unwrap();
    let expected = original
        .replace(
            "r#\"./material.aestra.material.ron\"#",
            "\"destination/material.aestra.material.ron\"",
        )
        .replace(
            "path: \".\\\\material.aestra.material.ron\"",
            "path: \"destination/material.aestra.material.ron\"",
        )
        .replace(
            "path: \"function.aestra.material-function.ron\"",
            "path: \"destination/function.aestra.material-function.ron\"",
        );
    assert_eq!(fs::read_to_string(path).unwrap(), expected);
}
