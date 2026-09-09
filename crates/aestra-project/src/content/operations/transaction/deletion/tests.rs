use super::*;
use aestra_core::{AssetDefinition, EffectAsset, material::MaterialProgram};

fn fixture() -> (tempfile::TempDir, ProjectContent, ProjectSourceId) {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("pack/empty")).unwrap();
    fs::write(root.path().join("pack/image.png"), b"texture bytes").unwrap();
    let mut effect = EffectAsset::new("Pack effect", 1.0);
    effect
        .assets
        .push(AssetDefinition::texture("Image", "pack/image.png"));
    fs::write(
        root.path().join("pack/effect.aestra.ron"),
        format!("// retained\n{}", effect.to_pretty_ron().unwrap()),
    )
    .unwrap();
    MaterialProgram::additive_sprite("Material")
        .save_ron(root.path().join("pack/program.aestra.material.ron"))
        .unwrap();
    let content = ProjectContent::scan(root.path());
    let source = content.source_tree().at_relative_path("pack").unwrap().id;
    (root, content, source)
}

#[test]
fn delete_and_restore_folder_keep_ids_exact_bytes_and_empty_directories() {
    let (root, content, source) = fixture();
    let before = rename::inventory(root.path()).unwrap();
    let dirs = folders::inventory(root.path()).unwrap();
    let plan = content.plan_delete_source(source, &[], true).unwrap();
    assert_eq!(plan.file_count(), 3);
    assert_eq!(plan.folder_count(), 2);
    assert_eq!(rename::inventory(root.path()).unwrap(), before);
    let item = plan.apply().unwrap();
    assert!(item.removed);
    assert!(!root.path().join("pack").exists());
    assert!(
        ProjectContent::scan(root.path())
            .source_tree()
            .at_relative_path("pack")
            .is_none()
    );
    let fresh = ProjectContent::scan(root.path())
        .deleted_sources()
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(fresh.original, Path::new("pack"));
    fresh.restore(&[], true).unwrap();
    assert_eq!(rename::inventory(root.path()).unwrap(), before);
    assert_eq!(folders::inventory(root.path()).unwrap(), dirs);
    assert!(
        ProjectContent::scan(root.path())
            .deleted_sources()
            .unwrap()
            .is_empty()
    );
    assert!(item.journal.with_file_name("record.restored").exists());
}

#[test]
fn references_block_leaf_deletion_but_internal_folder_references_are_allowed() {
    let (root, content, source) = fixture();
    let image = content
        .source_tree()
        .at_relative_path("pack/image.png")
        .unwrap()
        .id;
    assert!(
        content
            .plan_delete_source(image, &[], true)
            .unwrap_err()
            .to_string()
            .contains("referenced by")
    );
    let mut owner = EffectAsset::new("External", 1.0);
    let target = EffectAsset::load_ron(root.path().join("pack/effect.aestra.ron")).unwrap();
    // File references also cover loader labels, without treating them as filenames.
    owner
        .assets
        .push(AssetDefinition::texture("Image", "pack/image.png#Layer0"));
    owner
        .save_ron(root.path().join("owner.aestra.ron"))
        .unwrap();
    let fresh = ProjectContent::scan(root.path());
    assert!(
        fresh
            .plan_delete_source(source, &[], true)
            .unwrap_err()
            .to_string()
            .contains("owner.aestra.ron")
    );
    assert_eq!(
        EffectAsset::load_ron(root.path().join("pack/effect.aestra.ron"))
            .unwrap()
            .id,
        target.id
    );
}

#[test]
fn semantic_usages_block_program_delete() {
    let root = tempfile::tempdir().unwrap();
    let program = MaterialProgram::additive_sprite("Used");
    program
        .save_ron(root.path().join("material.aestra.material.ron"))
        .unwrap();
    let mut effect = EffectAsset::new("Owner", 1.0);
    let instance = aestra_core::material::MaterialInstance {
        id: aestra_core::MaterialId::new(),
        program: aestra_core::material::MaterialProgramRef::Project(program.id),
        values: BTreeMap::new(),
        render_state: aestra_core::material::MaterialRenderState::additive_sprite(),
    };
    effect.material_instances.push(instance);
    effect
        .save_ron(root.path().join("owner.aestra.ron"))
        .unwrap();
    let content = ProjectContent::scan(root.path());
    let source = content
        .source_tree()
        .at_relative_path("material.aestra.material.ron")
        .unwrap()
        .id;
    assert!(
        content
            .plan_delete_source(source, &[], true)
            .unwrap_err()
            .to_string()
            .contains("referenced by")
    );
}

#[test]
fn drafts_stale_inventory_root_and_unknown_do_not_delete() {
    let (root, content, source) = fixture();
    assert!(
        content
            .plan_delete_source(content.source_tree().root(), &[], true)
            .is_err()
    );
    assert!(content.plan_delete_source(source, &[], false).is_err());
    assert!(
        content
            .plan_delete_source(
                source,
                &[(
                    source,
                    DraftDocument::Effect(Box::new(EffectAsset::new("Draft", 1.0)))
                )],
                true
            )
            .is_err()
    );
    let plan = content.plan_delete_source(source, &[], true).unwrap();
    fs::write(root.path().join("pack/new.png"), b"external").unwrap();
    assert!(plan.apply().is_err());
    fs::write(root.path().join("pack/unknown.bin"), b"opaque").unwrap();
    assert!(
        ProjectContent::scan(root.path())
            .plan_delete_source(source, &[], true)
            .is_err()
    );
    assert!(root.path().join("pack/empty").is_dir());
    assert!(!root.path().join(".aestra/deleted").exists());
}

#[test]
fn empty_folder_restore_and_readonly_payload_are_guarded() {
    let (root, content, _) = fixture();
    let source = content
        .source_tree()
        .at_relative_path("pack/empty")
        .unwrap()
        .id;
    let item = content
        .plan_delete_source(source, &[], true)
        .unwrap()
        .apply()
        .unwrap();
    item.restore(&[], true).unwrap();
    assert!(root.path().join("pack/empty").is_dir());

    let file = root.path().join("pack/program.aestra.material.ron");
    let original_permissions = fs::metadata(&file).unwrap().permissions();
    let mut readonly = original_permissions.clone();
    readonly.set_readonly(true);
    fs::set_permissions(&file, readonly.clone()).unwrap();
    let id = content
        .source_tree()
        .at_relative_path("pack/program.aestra.material.ron")
        .unwrap()
        .id;
    assert!(content.plan_delete_source(id, &[], true).is_err());
    fs::set_permissions(&file, original_permissions.clone()).unwrap();
    let item = content
        .plan_delete_source(id, &[], true)
        .unwrap()
        .apply()
        .unwrap();
    fs::set_permissions(item.payload(), readonly).unwrap();
    let listed = content.deleted_sources().unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].blocked_reason.is_some());
    assert!(item.clone().restore(&[], true).is_err());
    fs::set_permissions(item.payload(), original_permissions).unwrap();
    item.restore(&[], true).unwrap();
}

#[test]
fn publication_rechecks_external_edits_and_keeps_an_explicit_recovery_record() {
    let (root, content, source) = fixture();
    let before = rename::inventory(root.path()).unwrap();
    let plan = content.plan_delete_source(source, &[], true).unwrap();
    assert!(
        plan.apply_with(|_| {
            fs::write(root.path().join("external.png"), b"new")?;
            Ok(())
        })
        .is_err()
    );
    let item = content.deleted_sources().unwrap().pop().unwrap();
    assert!(!item.removed);
    item.restore(&[], true).unwrap();
    for (path, bytes) in before {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    assert_eq!(fs::read(root.path().join("external.png")).unwrap(), b"new");
}

#[test]
fn interrupted_delete_and_interrupted_restore_are_explicitly_recoverable() {
    for boundary in 0..2 {
        let (root, content, source) = fixture();
        let before = rename::inventory(root.path()).unwrap();
        assert!(
            content
                .plan_delete_source(source, &[], true)
                .unwrap()
                .apply_with(|at| {
                    if at == boundary {
                        Err(blocked("Injected interruption"))
                    } else {
                        Ok(())
                    }
                })
                .is_err()
        );
        let fresh = ProjectContent::scan(root.path());
        let item = fresh.deleted_sources().unwrap().pop().unwrap();
        assert_eq!(item.removed, boundary == 1);
        if item.removed {
            rename::rename_exclusive(&item.payload(), &root.path().join("pack")).unwrap();
        }
        // Same disk state as a crash before publication or after restore's rename.
        ProjectContent::scan(root.path())
            .deleted_sources()
            .unwrap()
            .pop()
            .unwrap()
            .restore(&[], true)
            .unwrap();
        assert_eq!(rename::inventory(root.path()).unwrap(), before);
    }
}

#[test]
fn restore_refuses_collision_edited_payload_changed_journal_and_duplicate_ids() {
    for bad in 0..4 {
        let (root, content, source) = fixture();
        let item = content
            .plan_delete_source(source, &[], true)
            .unwrap()
            .apply()
            .unwrap();
        match bad {
            0 => {
                fs::create_dir(root.path().join("pack")).unwrap();
            }
            1 => {
                fs::write(item.payload().join("image.png"), b"changed").unwrap();
            }
            2 => {
                fs::write(item.journal(), b"bad journal").unwrap();
            }
            _ => {
                fs::copy(
                    item.payload().join("effect.aestra.ron"),
                    root.path().join("duplicate.aestra.ron"),
                )
                .unwrap();
            }
        }
        assert!(item.clone().restore(&[], true).is_err());
        assert!(item.payload().is_dir());
        assert!(item.journal().exists());
    }
}

#[test]
fn restore_requires_clean_drafts_and_single_file_uses_same_path() {
    let (root, content, _) = fixture();
    let source = content
        .source_tree()
        .at_relative_path("pack/program.aestra.material.ron")
        .unwrap()
        .id;
    let before = fs::read(root.path().join("pack/program.aestra.material.ron")).unwrap();
    let item = content
        .plan_delete_source(source, &[], true)
        .unwrap()
        .apply()
        .unwrap();
    assert!(item.clone().restore(&[], false).is_err());
    assert!(item.payload().is_file());
    let path = item.restore(&[], true).unwrap();
    assert_eq!(fs::read(path).unwrap(), before);
}

#[test]
fn restore_requires_external_resources_deleted_later_to_be_restored_first() {
    let (root, content, _) = fixture();
    let source = content
        .source_tree()
        .at_relative_path("pack/effect.aestra.ron")
        .unwrap()
        .id;
    let effect = content
        .plan_delete_source(source, &[], true)
        .unwrap()
        .apply()
        .unwrap();
    let fresh = ProjectContent::scan(root.path());
    let image = fresh
        .source_tree()
        .at_relative_path("pack/image.png")
        .unwrap()
        .id;
    let image = fresh
        .plan_delete_source(image, &[], true)
        .unwrap()
        .apply()
        .unwrap();
    assert!(
        effect
            .clone()
            .restore(&[], true)
            .unwrap_err()
            .to_string()
            .contains("required resource")
    );
    image.restore(&[], true).unwrap();
    effect.restore(&[], true).unwrap();
    assert!(root.path().join("pack/effect.aestra.ron").exists());
}
