use super::*;
use aestra_core::{AssetDefinition, EffectAsset, material::MaterialProgram};

fn source(content: &ProjectContent, path: &str) -> ProjectSourceId {
    content
        .source_tree()
        .at_relative_path(Path::new(path))
        .unwrap()
        .id
}

fn labeled_mesh_fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("meshes")).unwrap();
    fs::create_dir_all(root.path().join("test/empty")).unwrap();
    fs::create_dir(root.path().join("destination")).unwrap();
    fs::write(
        root.path().join("meshes/lab_cube.gltf"),
        include_bytes!("../../../../../../assets/meshes/lab_cube.gltf"),
    )
    .unwrap();
    let mut effect = EffectAsset::new("Mesh effect", 1.0);
    for label in ["Mesh0/Primitive0", "Mesh0", "Scene0"] {
        effect.assets.push(AssetDefinition {
            id: aestra_core::AssetId::new(),
            name: label.into(),
            kind: aestra_core::AssetKind::Mesh,
            path: format!("meshes/lab_cube.gltf#{label}"),
        });
    }
    fs::write(
        root.path().join("effect.aestra.ron"),
        format!(
            "// Keep formatting and labels\n{}",
            effect.to_pretty_ron().unwrap()
        ),
    )
    .unwrap();
    root
}

#[test]
fn unrelated_folder_rename_and_move_accept_existing_mesh_subasset_references() {
    for rename in [false, true] {
        let root = labeled_mesh_fixture();
        let content = ProjectContent::scan(root.path());
        let original = fs::read(root.path().join("effect.aestra.ron")).unwrap();
        let request = if rename {
            OperationRequest::Rename {
                source: source(&content, "test"),
                name: "renamed".into(),
            }
        } else {
            OperationRequest::Move {
                source: source(&content, "test"),
                parent: source(&content, "destination"),
            }
        };
        let result = content
            .plan_content_relocations(vec![request], &[], true)
            .unwrap()
            .apply()
            .unwrap();
        assert!(result.rewritten_sources.is_empty());
        assert_eq!(
            fs::read(root.path().join("effect.aestra.ron")).unwrap(),
            original
        );
        assert!(!root.path().join("test").exists());
        assert!(
            root.path()
                .join(if rename {
                    "renamed/empty"
                } else {
                    "destination/test/empty"
                })
                .is_dir()
        );
    }
}

#[test]
fn mesh_file_and_folder_relocation_preserve_subasset_labels_and_recover_exact_bytes() {
    for folder in [false, true] {
        let root = labeled_mesh_fixture();
        let content = ProjectContent::scan(root.path());
        let before = rename::inventory(root.path()).unwrap();
        let request = if folder {
            OperationRequest::Move {
                source: source(&content, "meshes"),
                parent: source(&content, "destination"),
            }
        } else {
            OperationRequest::Rename {
                source: source(&content, "meshes/lab_cube.gltf"),
                name: "renamed".into(),
            }
        };
        let result = content
            .plan_content_relocations(vec![request], &[], true)
            .unwrap()
            .apply()
            .unwrap();
        let prefix = if folder {
            "destination/meshes/lab_cube.gltf"
        } else {
            "meshes/renamed.gltf"
        };
        let original =
            std::str::from_utf8(&before[&root.path().join("effect.aestra.ron")]).unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("effect.aestra.ron")).unwrap(),
            original.replace("meshes/lab_cube.gltf#", &format!("{prefix}#"))
        );
        // Simulate interruption at the final archival boundary, then restore.
        fs::rename(
            &result.journal,
            result.journal.parent().unwrap().join("active.pending"),
        )
        .unwrap();
        ProjectContent::scan(root.path())
            .pending_asset_move_batch()
            .unwrap()
            .unwrap()
            .rollback(&[], true)
            .unwrap();
        assert_eq!(rename::inventory(root.path()).unwrap(), before);
    }
}

#[test]
fn mesh_labels_do_not_bypass_missing_file_or_root_escape_checks() {
    for path in [
        "meshes/missing.gltf#Mesh0/Primitive0",
        "../outside.gltf#Mesh0",
        "C:/outside.gltf#Mesh0",
        "/outside.gltf#Mesh0",
        "meshes/lab_cube.gltf#",
        "meshes/lab_cube.gltf#source://Mesh0",
    ] {
        let root = labeled_mesh_fixture();
        let effect_path = root.path().join("effect.aestra.ron");
        let mut effect = EffectAsset::load_ron(&effect_path).unwrap();
        effect.assets[0].path = path.into();
        fs::write(&effect_path, ron::ser::to_string(&effect).unwrap()).unwrap();
        let content = ProjectContent::scan(root.path());
        assert!(
            content
                .plan_content_relocations(
                    vec![OperationRequest::Rename {
                        source: source(&content, "test"),
                        name: "renamed".into(),
                    }],
                    &[],
                    true
                )
                .is_err(),
            "{path}"
        );
        assert!(root.path().join("test/empty").is_dir());
        assert!(!root.path().join("renamed").exists());
    }
}

fn fixture() -> (tempfile::TempDir, ProjectContent, OperationRequest) {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("pack/nested/empty")).unwrap();
    fs::create_dir(root.path().join("destination")).unwrap();
    fs::write(
        root.path().join("pack/texture.png"),
        [137, 80, 78, 71, 0, 255],
    )
    .unwrap();
    for path in ["pack/moved.aestra.ron", "owner.aestra.ron"] {
        let mut effect = EffectAsset::new("pack/texture.png", 1.0);
        effect
            .assets
            .push(AssetDefinition::texture("Texture", "pack/texture.png"));
        fs::write(
            root.path().join(path),
            format!("// untouched comment\n{}", effect.to_pretty_ron().unwrap()),
        )
        .unwrap();
    }
    MaterialProgram::additive_sprite("Material")
        .save_ron(root.path().join("pack/program.aestra.material.ron"))
        .unwrap();
    fs::write(
        root.path()
            .join("pack/function.aestra.material-function.ron"),
        include_bytes!(
            "../../../../../../assets/materials/dissolve_edge.aestra.material-function.ron"
        ),
    )
    .unwrap();
    fs::write(
        root.path().join("pack/preset.aestra.material-preset.ron"),
        include_bytes!(
            "../../../../../../assets/materials/additive_flame.aestra.material-preset.ron"
        ),
    )
    .unwrap();
    let content = ProjectContent::scan(root.path());
    let request = OperationRequest::Move {
        source: source(&content, "pack"),
        parent: source(&content, "destination"),
    };
    (root, content, request)
}

fn interrupt(plan: AssetMoveBatchPlan, count: usize) -> (PathBuf, Record) {
    let (journal, record) =
        prepare(&plan.root, plan.moves, plan.replacements, plan.folders).unwrap();
    for index in 0..count {
        steps::advance(&plan.root, &record, index, false).unwrap();
    }
    (journal, record)
}

#[test]
fn folder_move_preserves_empty_descendants_all_identities_and_resource_bindings() {
    let (root, content, request) = fixture();
    let before = rename::inventory(root.path()).unwrap();
    let directories = folders::inventory(root.path()).unwrap();
    let plan = content
        .plan_content_relocations(vec![request], &[], true)
        .unwrap();
    assert_eq!(plan.moves.len(), 5);
    assert_eq!(plan.rewritten_sources().len(), 2);
    assert_eq!(rename::inventory(root.path()).unwrap(), before);
    assert_eq!(folders::inventory(root.path()).unwrap(), directories);
    let result = plan.apply().unwrap();
    assert_eq!(result.folders.len(), 1);
    assert!(!root.path().join("pack").exists());
    assert!(root.path().join("destination/pack/nested/empty").is_dir());
    let fresh = ProjectContent::scan(root.path());
    for item in &result.moves {
        assert!(item.destination.is_file());
        if let Some(asset) = item.asset {
            assert_eq!(
                fresh.unique_source_for_asset(asset).unwrap().path,
                item.destination
            );
        }
        if !result.rewritten_sources.contains(&item.destination) {
            assert_eq!(fs::read(&item.destination).unwrap(), before[&item.source]);
        }
    }
    for path in ["owner.aestra.ron", "destination/pack/moved.aestra.ron"] {
        let effect = EffectAsset::load_ron(root.path().join(path)).unwrap();
        assert_eq!(effect.assets[0].path, "destination/pack/texture.png");
        assert_eq!(effect.name, "pack/texture.png");
        assert!(root.path().join(&effect.assets[0].path).is_file());
    }
    assert!(fresh.pending_asset_move_batch().unwrap().is_none());
}

#[test]
fn folder_failures_restore_bytes_and_empty_directories_at_every_boundary() {
    for boundary in 0..=5 {
        let (root, content, request) = fixture();
        let before = rename::inventory(root.path()).unwrap();
        let directories = folders::inventory(root.path()).unwrap();
        let result = content
            .plan_content_relocations(vec![request], &[], true)
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
            "{boundary}"
        );
        assert_eq!(rename::inventory(root.path()).unwrap(), before);
        assert_eq!(folders::inventory(root.path()).unwrap(), directories);
        assert!(content.pending_asset_move_batch().unwrap().is_none());
    }
}

#[test]
fn folder_restart_recovery_is_read_only_until_explicit_and_resumes_reverse_steps() {
    for boundary in 0..=5 {
        let (root, content, request) = fixture();
        let before = rename::inventory(root.path()).unwrap();
        let directories = folders::inventory(root.path()).unwrap();
        let (journal, record) = interrupt(
            content
                .plan_content_relocations(vec![request], &[], true)
                .unwrap(),
            boundary,
        );
        let at_crash = rename::inventory(root.path()).unwrap();
        let pending = ProjectContent::scan(root.path())
            .pending_asset_move_batch()
            .unwrap()
            .unwrap();
        assert_eq!(pending.moved_folders, usize::from(boundary > 0));
        assert_eq!(pending.moved_files, if boundary > 0 { 5 } else { 0 });
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
        assert_eq!(rename::inventory(root.path()).unwrap(), before);
        assert_eq!(folders::inventory(root.path()).unwrap(), directories);
    }
}

#[test]
fn resource_rename_and_move_use_the_same_transaction_and_keep_extensions_and_ids() {
    let (root, content, _) = fixture();
    let original = EffectAsset::load_ron(root.path().join("owner.aestra.ron")).unwrap();
    content
        .plan_content_relocations(
            vec![OperationRequest::Rename {
                source: source(&content, "pack/texture.png"),
                name: "renamed".into(),
            }],
            &[],
            true,
        )
        .unwrap()
        .apply()
        .unwrap();
    assert!(root.path().join("pack/renamed.png").is_file());
    let content = ProjectContent::scan(root.path());
    let result = content
        .plan_content_relocations(
            vec![OperationRequest::Move {
                source: source(&content, "pack/renamed.png"),
                parent: source(&content, "destination"),
            }],
            &[],
            true,
        )
        .unwrap()
        .apply()
        .unwrap();
    assert_eq!(result.moves.len(), 1);
    assert!(result.moves[0].asset.is_none());
    let effect = EffectAsset::load_ron(root.path().join("owner.aestra.ron")).unwrap();
    assert_eq!(effect.assets[0].id, original.assets[0].id);
    assert_eq!(effect.assets[0].path, "destination/renamed.png");
    assert_eq!(effect.id, original.id);
}

#[test]
fn folder_rename_and_empty_folder_moves_work_without_merging_or_recreation() {
    let (root, content, _) = fixture();
    content
        .plan_content_relocations(
            vec![OperationRequest::Rename {
                source: source(&content, "pack"),
                name: "renamed".into(),
            }],
            &[],
            true,
        )
        .unwrap()
        .apply()
        .unwrap();
    assert!(root.path().join("renamed/nested/empty").is_dir());
    let content = ProjectContent::scan(root.path());
    let result = content
        .plan_content_relocations(
            vec![OperationRequest::Move {
                source: source(&content, "renamed/nested/empty"),
                parent: source(&content, "destination"),
            }],
            &[],
            true,
        )
        .unwrap()
        .apply()
        .unwrap();
    assert!(result.moves.is_empty());
    assert_eq!(result.folders.len(), 1);
    assert!(root.path().join("destination/empty").is_dir());
    assert!(!root.path().join("renamed/nested/empty").exists());
}

#[test]
fn root_self_descendants_overlapping_selections_and_case_collisions_are_rejected() {
    let (root, content, request) = fixture();
    let before = rename::inventory(root.path()).unwrap();
    for requests in [
        vec![OperationRequest::Move {
            source: content.source_tree().root(),
            parent: source(&content, "destination"),
        }],
        vec![OperationRequest::Move {
            source: source(&content, "pack"),
            parent: source(&content, "pack/nested"),
        }],
        vec![OperationRequest::Rename {
            source: source(&content, "pack"),
            name: "PACK".into(),
        }],
        vec![
            request.clone(),
            OperationRequest::Move {
                source: source(&content, "pack/texture.png"),
                parent: source(&content, "destination"),
            },
        ],
        vec![
            request.clone(),
            OperationRequest::Move {
                source: source(&content, "pack/nested"),
                parent: source(&content, "destination"),
            },
        ],
    ] {
        assert!(
            content
                .plan_content_relocations(requests, &[], true)
                .is_err()
        );
    }
    fs::create_dir(root.path().join("destination/pack")).unwrap();
    assert!(
        ProjectContent::scan(root.path())
            .plan_content_relocations(vec![request], &[], true)
            .is_err()
    );
    assert_eq!(rename::inventory(root.path()).unwrap(), before);
}

#[test]
fn hidden_or_excluded_descendants_unknown_formats_and_external_mesh_dependencies_block() {
    for path in [
        "pack/.aestra/secret.png",
        "pack/.git/config",
        "pack/.hidden",
        "pack/unknown.bin",
        "pack/mesh.gltf",
    ] {
        let (root, _, request) = fixture();
        let path = root.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            r#"{"asset":{"version":"2.0"},"buffers":[{"byteLength":4,"uri":"external.bin"}]}"#,
        )
        .unwrap();
        assert!(
            ProjectContent::scan(root.path())
                .plan_content_relocations(vec![request], &[], true)
                .is_err()
        );
        assert!(root.path().join("pack").is_dir());
        assert!(!root.path().join("destination/pack").exists());
    }
}

#[test]
fn external_additions_inside_moved_folder_block_recovery_without_moving_them() {
    for directory in [false, true] {
        let (root, content, request) = fixture();
        let (_, _) = interrupt(
            content
                .plan_content_relocations(vec![request], &[], true)
                .unwrap(),
            1,
        );
        let path = root.path().join("destination/pack/new-user-content");
        if directory {
            fs::create_dir(&path).unwrap();
        } else {
            fs::write(&path, b"external edit").unwrap();
        }
        let before = rename::inventory(root.path()).unwrap();
        assert!(
            ProjectContent::scan(root.path())
                .pending_asset_move_batch()
                .is_err()
        );
        assert_eq!(rename::inventory(root.path()).unwrap(), before);
        assert!(path.exists());
        assert!(!root.path().join("pack").exists());
    }
}

#[test]
fn stale_folder_plans_reject_new_empty_directories_and_changed_resource_bytes() {
    for directory in [false, true] {
        let (root, content, request) = fixture();
        let plan = content
            .plan_content_relocations(vec![request], &[], true)
            .unwrap();
        if directory {
            fs::create_dir(root.path().join("pack/new-empty")).unwrap();
        } else {
            fs::write(root.path().join("pack/texture.png"), b"new bytes").unwrap();
        }
        assert!(plan.apply().is_err());
        assert!(root.path().join("pack").exists());
        assert!(!root.path().join("destination/pack").exists());
    }
}

#[test]
fn tampered_folder_journals_never_restore_unlisted_or_escaping_paths() {
    for invalid in 0..4 {
        let (root, content, request) = fixture();
        let (journal, mut record) = interrupt(
            content
                .plan_content_relocations(vec![request], &[], true)
                .unwrap(),
            1,
        );
        match invalid {
            0 => record.folders[0].source = "../outside".into(),
            1 => {
                record.folders[0].directories.pop();
            }
            2 => {
                record.moves.pop();
            }
            _ => record.moves[0].destination = "destination/outside.png".into(),
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
fn mixed_folder_and_loose_resource_batch_uses_one_recoverable_sequence() {
    for fail in [false, true] {
        let (root, _, request) = fixture();
        fs::write(root.path().join("loose.png"), b"loose resource bytes").unwrap();
        let content = ProjectContent::scan(root.path());
        let before = rename::inventory(root.path()).unwrap();
        let directories = folders::inventory(root.path()).unwrap();
        let requests = vec![
            request,
            OperationRequest::Move {
                source: source(&content, "loose.png"),
                parent: source(&content, "destination"),
            },
        ];
        let plan = content
            .plan_content_relocations(requests, &[], true)
            .unwrap();
        let result = plan.apply_with(|at| {
            if fail && at == 4 {
                Err(blocked("Injected failure"))
            } else {
                Ok(())
            }
        });
        if fail {
            assert!(result.unwrap_err().to_string().contains("rolled back"));
            assert_eq!(rename::inventory(root.path()).unwrap(), before);
            assert_eq!(folders::inventory(root.path()).unwrap(), directories);
        } else {
            assert_eq!(result.unwrap().moves.len(), 6);
            assert_eq!(
                fs::read(root.path().join("destination/loose.png")).unwrap(),
                b"loose resource bytes"
            );
        }
    }
}

#[test]
fn added_excluded_descendants_and_read_only_folders_are_never_moved() {
    let (root, content, request) = fixture();
    let plan = content
        .plan_content_relocations(vec![request], &[], true)
        .unwrap();
    fs::create_dir(root.path().join("pack/.aestra")).unwrap();
    assert!(plan.apply().is_err());
    assert!(root.path().join("pack/.aestra").is_dir());
    assert!(!root.path().join("destination/pack").exists());
    let (root, content, request) = fixture();
    let path = root.path().join("pack/nested");
    let original = fs::metadata(&path).unwrap().permissions();
    let mut readonly = original.clone();
    readonly.set_readonly(true);
    fs::set_permissions(&path, readonly).unwrap();
    let result = content.plan_content_relocations(vec![request], &[], true);
    fs::set_permissions(&path, original).unwrap();
    assert!(result.is_err());
    assert!(!root.path().join("destination/pack").exists());
}

#[test]
fn legacy_rewrite_journal_without_folder_fields_remains_recoverable() {
    let (root, content, requests) = rewrite_tests::fixture();
    let before = rename::inventory(root.path()).unwrap();
    // Version 2 supported semantic moves and path edits but not folders/resources.
    let (journal, record) = interrupt(content.plan_asset_moves(requests, &[], true).unwrap(), 4);
    let mut legacy = serde_json::to_value(record).unwrap();
    legacy["version"] = 2.into();
    legacy.as_object_mut().unwrap().remove("folders");
    fs::write(journal, serde_json::to_vec(&legacy).unwrap()).unwrap();
    content
        .pending_asset_move_batch()
        .unwrap()
        .unwrap()
        .rollback(&[], true)
        .unwrap();
    assert_eq!(rename::inventory(root.path()).unwrap(), before);
}
