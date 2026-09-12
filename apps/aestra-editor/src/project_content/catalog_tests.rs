//! Project catalog/watch regressions retained from the retired Library panel.
use super::*;
use crate::test_support;
use aestra_core::material::MaterialProgramRef;
use aestra_core::{EffectAsset, EffectId, MaterialId};
use aestra_project::ProjectTreeStamp as ProjectEffectTreeSnapshot;

fn write_effect(path: &Path, name: &str) {
    let mut effect = test_support::effect_with_timing_slack();
    effect.name = name.into();
    effect.save_ron(path).expect("effect fixture should save");
}

#[test]
fn project_catalog_is_sorted_and_source_rows_are_stable_across_scans() {
    let temporary = tempfile::tempdir().unwrap();
    write_effect(&temporary.path().join("zeta.aestra.ron"), "Zeta");
    write_effect(&temporary.path().join("alpha.aestra.ron"), "Alpha");

    let first = ProjectEffectCatalog::scan(temporary.path());
    let second = ProjectEffectCatalog::scan(temporary.path());

    assert_eq!(
        first
            .entries()
            .iter()
            .map(|entry| entry.display_name.as_str())
            .collect::<Vec<_>>(),
        ["Alpha", "Zeta"]
    );
    assert_eq!(
        first
            .entries()
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        second
            .entries()
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>()
    );
}

#[test]
fn project_catalog_resolves_bundled_effect_materials_outside_the_effect_folder() {
    let asset_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/test");
    let effect_root = asset_root.join("effects");
    let catalog = ProjectEffectCatalog::scan_project(&asset_root, &effect_root);
    let reference =
        EffectAssetRef::new(EffectId::from_u128(0xa3574a00_0000_4000_8000_000000009001));

    let effect = catalog
        .load_effect(reference)
        .expect("the Material Graph Lab row should be openable");

    assert_eq!(effect.name, "Material Graph Lab");
    assert!(catalog.openable_path(reference).is_some());
    catalog
        .compile_project(&effect)
        .expect("its sibling material program should resolve from the project asset root");
}

#[test]
fn project_catalog_merges_bundled_material_presets_with_builtins() {
    let asset_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/test");
    let catalog = ProjectEffectCatalog::scan_project(&asset_root, asset_root.join("effects"));

    let presets = catalog.material_preset_catalog().unwrap();
    let hologram = presets
        .iter()
        .find(|preset| preset.display_name == "Hologram")
        .expect("the bundled project preset should be registered");

    assert_eq!(hologram.category.display_name(), "Shaping");
    assert!(hologram.tags.iter().any(|tag| tag == "hologram"));
    assert!(
        presets
            .get(aestra_compiler::MATERIAL_PRESET_DISSOLVE)
            .is_some()
    );
    for name in [
        "Additive Flame",
        "Soft Smoke",
        "Energy Beam",
        "Magic Shield",
        "Ghost",
        "Portal",
        "Impact Flash",
    ] {
        assert!(
            presets.iter().any(|preset| preset.display_name == name),
            "bundled preset {name} should be registered"
        );
    }
    let functions = catalog.material_function_library().unwrap();
    let pulse = functions
        .iter()
        .find(|function| function.name == "Pulse Wave (Custom WESL)")
        .expect("the bundled custom WESL function should be registered");
    assert!(pulse.custom_wesl.is_some());
}

#[test]
fn project_catalog_creates_effects_in_the_effect_authoring_folder() {
    let temporary = tempfile::tempdir().unwrap();
    let effect_root = temporary.path().join("effects");
    fs::create_dir(&effect_root).unwrap();
    fs::create_dir(temporary.path().join("materials")).unwrap();
    let mut catalog = ProjectEffectCatalog::scan_project(temporary.path(), &effect_root);
    let effect = EffectAsset::new("Created Effect", 1.0);

    let created = catalog.create_effect_source(&effect).unwrap();

    assert_eq!(created.path.parent(), Some(effect_root.as_path()));
    assert_eq!(catalog.effect_root(), effect_root);
}

#[test]
fn project_catalog_watch_tracks_material_program_sources() {
    let temporary = tempfile::tempdir().unwrap();
    let material = temporary.path().join("test.aestra.material.ron");
    fs::write(&material, "material").unwrap();

    let snapshot = ProjectEffectTreeSnapshot::scan(temporary.path());

    assert!(snapshot.file(&material).is_some());
    for name in [
        "function.aestra.material-function.ron",
        "preset.aestra.material-preset.ron",
        "UPPER.AESTRA.MATERIAL.RON",
    ] {
        let path = temporary.path().join(name);
        fs::write(&path, "source").unwrap();
        assert!(
            ProjectEffectTreeSnapshot::scan(temporary.path())
                .file(&path)
                .is_some()
        );
    }
}

#[test]
fn missing_material_dependencies_have_actionable_diagnostics() {
    let temporary = tempfile::tempdir().unwrap();
    let catalog = ProjectEffectCatalog::scan(temporary.path());
    let mut effect = test_support::effect_with_timing_slack();
    effect
        .material_instances
        .push(aestra_core::material::MaterialInstance {
            id: MaterialId::new(),
            program: MaterialProgramRef::Project(aestra_core::MaterialProgramId::new()),
            values: default(),
            render_state: aestra_core::material::MaterialRenderState::additive_sprite(),
        });
    let error = catalog.compile_project(&effect).unwrap_err();
    assert!(!error.trim().is_empty());
    assert!(!catalog.dependency_validation_report(&effect).is_valid());
}

#[test]
fn clean_external_reload_resolves_project_materials() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("root.aestra.ron");
    let program = MaterialProgram::additive_sprite("Project material");
    program
        .save_ron(temporary.path().join("material.aestra.material.ron"))
        .unwrap();
    let mut effect = test_support::effect_with_timing_slack();
    effect
        .material_instances
        .push(aestra_core::material::MaterialInstance {
            id: MaterialId::new(),
            program: MaterialProgramRef::Project(program.id),
            values: default(),
            render_state: aestra_core::material::MaterialRenderState::additive_sprite(),
        });
    effect.save_ron(&path).unwrap();
    let mut catalog = ProjectEffectCatalog::scan(temporary.path());
    let mut session = test_support::session_with_timing_slack();
    session.open_compiled_effect(
        &path,
        effect.clone(),
        catalog.compile_project(&effect).unwrap().root,
    );
    let previous = ProjectEffectTreeSnapshot::scan(temporary.path());
    effect.name = "Externally changed material effect".into();
    effect.save_ron(&path).unwrap();
    let current = ProjectEffectTreeSnapshot::scan(temporary.path());
    apply_project_effect_catalog_refresh(
        &mut catalog,
        &mut session,
        &previous,
        &current,
        &Localizer::new("en-US").unwrap(),
    );
    assert_eq!(session.effect.name, effect.name);
    assert!(
        session
            .preview
            .as_ref()
            .unwrap()
            .effect()
            .material_program(program.id)
            .is_some()
    );
    assert!(!session.dirty);
}

#[test]
fn empty_project_catalog_is_a_valid_state() {
    let temporary = tempfile::tempdir().unwrap();

    let catalog = ProjectEffectCatalog::scan(temporary.path());

    assert!(catalog.entries().is_empty());
    assert_eq!(
        catalog.availability(),
        &ProjectAssetIndexAvailability::Ready
    );
}

#[test]
fn missing_project_catalog_is_reported_as_unavailable() {
    let temporary = tempfile::tempdir().unwrap();
    let missing = temporary.path().join("missing-effects");

    let catalog = ProjectEffectCatalog::scan(&missing);

    assert!(catalog.entries().is_empty());
    assert!(matches!(
        catalog.availability(),
        ProjectAssetIndexAvailability::Unavailable { root, message }
            if root == &missing && !message.is_empty()
    ));
}

#[test]
fn project_catalog_preserves_invalid_and_unsupported_files() {
    let temporary = tempfile::tempdir().unwrap();
    let valid_path = temporary.path().join("valid.aestra.ron");
    let invalid_path = temporary.path().join("broken.aestra.ron");
    let unsupported_path = temporary.path().join("future.aestra.ron");
    write_effect(&valid_path, "Valid");
    fs::write(&invalid_path, "this is not RON").unwrap();
    let future_source = test_support::effect_with_timing_slack()
        .to_pretty_ron()
        .unwrap()
        .replacen("format_version: 3", "format_version: 99", 1);
    fs::write(&unsupported_path, future_source).unwrap();

    let catalog = ProjectEffectCatalog::scan(temporary.path());

    assert_eq!(catalog.entries().len(), 3);
    let valid = catalog
        .entries()
        .iter()
        .find(|entry| entry.path == valid_path)
        .unwrap();
    let invalid = catalog
        .entries()
        .iter()
        .find(|entry| entry.path == invalid_path)
        .unwrap();
    let unsupported = catalog
        .entries()
        .iter()
        .find(|entry| entry.path == unsupported_path)
        .unwrap();
    assert_eq!(valid.status, ProjectEffectStatus::Valid);
    assert!(matches!(
        invalid.status,
        ProjectEffectStatus::Invalid { ref message } if !message.is_empty()
    ));
    assert_eq!(
        unsupported.status,
        ProjectEffectStatus::Unsupported {
            found: 99,
            current: aestra_core::CURRENT_FORMAT_VERSION,
        }
    );
    assert_eq!(
        catalog.openable_path(valid.reference.unwrap()),
        Some(valid_path.as_path())
    );
    assert!(invalid.reference.is_none());
    assert!(unsupported.reference.is_none());
}

#[test]
fn duplicate_effect_ids_are_visible_but_not_openable() {
    let temporary = tempfile::tempdir().unwrap();
    write_effect(&temporary.path().join("one.aestra.ron"), "One");
    write_effect(&temporary.path().join("two.aestra.ron"), "Two");

    let catalog = ProjectEffectCatalog::scan(temporary.path());
    let reference = catalog.entries()[0].reference.unwrap();

    assert!(
        catalog
            .entries()
            .iter()
            .all(|entry| matches!(entry.status, ProjectEffectStatus::DuplicateId { .. }))
    );
    assert_eq!(catalog.openable_path(reference), None);
}

#[test]
fn project_catalog_rejects_self_and_transitive_effect_cycles() {
    let temporary = tempfile::tempdir().unwrap();
    let mut owner = test_support::effect_with_timing_slack();
    owner.id = aestra_core::EffectId::from_u128(0xa11ce);
    owner.name = "Owner".into();
    owner.effect_clips.clear();
    let mut child = test_support::effect_with_timing_slack();
    child.id = aestra_core::EffectId::from_u128(0xc41d);
    child.name = "Child".into();
    child.effect_clips = vec![aestra_core::EffectClip::new(
        EffectAssetRef::new(owner.id),
        0.0,
        0.5,
    )];
    owner
        .save_ron(temporary.path().join("owner.aestra.ron"))
        .unwrap();
    child
        .save_ron(temporary.path().join("child.aestra.ron"))
        .unwrap();
    let catalog = ProjectEffectCatalog::scan(temporary.path());

    let self_error = catalog
        .effect_for_placement(&owner, EffectAssetRef::new(owner.id))
        .unwrap_err();
    assert!(self_error.contains("cannot reference itself"));
    let cycle_error = catalog
        .effect_for_placement(&owner, EffectAssetRef::new(child.id))
        .unwrap_err();
    assert!(cycle_error.contains("reference cycle"));
}

#[test]
fn missing_project_references_are_projected_into_editor_diagnostics() {
    let temporary = tempfile::tempdir().unwrap();
    let mut owner = test_support::effect_with_timing_slack();
    owner.id = aestra_core::EffectId::from_u128(0xa11ce);
    owner.effect_clips = vec![aestra_core::EffectClip::new(
        EffectAssetRef::new(aestra_core::EffectId::from_u128(0xdead)),
        0.25,
        0.75,
    )];
    let catalog = ProjectEffectCatalog::scan(temporary.path());

    let report = catalog.dependency_validation_report(&owner);

    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(report.diagnostics[0].code, DiagnosticCode::InvalidReference);
    assert_eq!(report.diagnostics[0].path, "effect.effect_clips[0].source");
    assert!(!report.diagnostics[0].message.is_empty());
}

#[test]
fn project_effect_watch_requires_two_stable_observations() {
    let temporary = tempfile::tempdir().unwrap();
    let initial = ProjectEffectTreeSnapshot::scan(temporary.path());
    let version = aestra_project::ProjectContentVersion {
        generation: 1,
        revision: 0,
    };
    let mut watch = aestra_project::ProjectContentRefresh::new(version, initial);
    write_effect(&temporary.path().join("new.aestra.ron"), "New");
    let changed = ProjectEffectTreeSnapshot::scan(temporary.path());

    assert!(watch.observe(version, &changed).is_none());
    assert!(watch.observe(version, &changed).is_some());
    assert!(watch.observe(version, &changed).is_none());
}

#[test]
fn project_switch_replaces_the_watch_baseline_without_reloading_the_document() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    write_effect(&second.path().join("new.aestra.ron"), "New project");
    let session = test_support::session_with_timing_slack();
    let original = session.effect.clone();
    let mut app = App::new();
    app.insert_resource(ProjectEffectCatalog::scan(first.path()))
        .init_resource::<ProjectEffectWatchState>()
        .insert_resource(session)
        .insert_resource(Localizer::new("en-US").unwrap())
        .add_systems(Update, poll_project_effect_catalog);
    app.insert_resource(ProjectEffectCatalog::scan(second.path()));
    app.update();
    let watch = app.world().resource::<ProjectEffectWatchState>();
    assert_eq!(
        watch.version(),
        app.world()
            .resource::<ProjectEffectCatalog>()
            .content_revision()
    );
    assert_eq!(app.world().resource::<EditorSession>().effect, original);
}

#[test]
fn catalog_refresh_reloads_a_clean_open_source() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("open.aestra.ron");
    let session = test_support::session_with_source_path(&path);
    session.effect.save_ron(&path).unwrap();
    let mut catalog = ProjectEffectCatalog::scan(temporary.path());
    let previous = ProjectEffectTreeSnapshot::scan(temporary.path());
    let mut changed = EffectAsset::load_ron(&path).unwrap();
    changed.name = "Externally Renamed".into();
    changed.save_ron(&path).unwrap();
    let current = ProjectEffectTreeSnapshot::scan(temporary.path());
    let mut session = session;

    apply_project_effect_catalog_refresh(
        &mut catalog,
        &mut session,
        &previous,
        &current,
        &Localizer::new("en-US").unwrap(),
    );

    assert_eq!(session.effect.name, "Externally Renamed");
    assert!(!session.dirty);
    assert!(session.status.contains("Reloaded externally changed"));
    assert_eq!(
        catalog
            .load_effect(EffectAssetRef::new(changed.id))
            .unwrap()
            .name,
        "Externally Renamed"
    );
}

#[test]
fn catalog_refresh_preserves_dirty_edits_when_the_source_changes() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("open.aestra.ron");
    let mut session = test_support::session_with_source_path(&path);
    session.effect.save_ron(&path).unwrap();
    session.effect.name = "Unsaved Editor Name".into();
    session.dirty = true;
    let mut catalog = ProjectEffectCatalog::scan(temporary.path());
    let previous = ProjectEffectTreeSnapshot::scan(temporary.path());
    let mut changed = EffectAsset::load_ron(&path).unwrap();
    changed.name = "External Name".into();
    changed.save_ron(&path).unwrap();
    let current = ProjectEffectTreeSnapshot::scan(temporary.path());

    apply_project_effect_catalog_refresh(
        &mut catalog,
        &mut session,
        &previous,
        &current,
        &Localizer::new("en-US").unwrap(),
    );

    assert_eq!(session.effect.name, "Unsaved Editor Name");
    assert!(session.dirty);
    assert!(
        session
            .status
            .contains("unsaved editor changes were preserved")
    );
    assert_eq!(
        catalog
            .load_effect(EffectAssetRef::new(changed.id))
            .unwrap()
            .name,
        "External Name"
    );
}

#[test]
fn catalog_refresh_does_not_treat_an_editor_save_as_an_external_reload() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("open.aestra.ron");
    let mut session = test_support::session_with_source_path(&path);
    session.effect.save_ron(&path).unwrap();
    let mut catalog = ProjectEffectCatalog::scan(temporary.path());
    let previous = ProjectEffectTreeSnapshot::scan(temporary.path());
    session.effect.name = "Saved In Editor".into();
    session.dirty = true;
    session.save().unwrap();
    session.status = "Saved by editor".into();
    let current = ProjectEffectTreeSnapshot::scan(temporary.path());

    apply_project_effect_catalog_refresh(
        &mut catalog,
        &mut session,
        &previous,
        &current,
        &Localizer::new("en-US").unwrap(),
    );

    assert_eq!(session.effect.name, "Saved In Editor");
    assert!(!session.dirty);
    assert_eq!(session.status, "Saved by editor");
}

#[test]
fn catalog_refresh_tracks_an_externally_moved_dirty_source_by_effect_id() {
    let temporary = tempfile::tempdir().unwrap();
    let nested = temporary.path().join("nested");
    fs::create_dir(&nested).unwrap();
    let original = temporary.path().join("open.aestra.ron");
    let moved = nested.join("open.aestra.ron");
    let mut session = test_support::session_with_source_path(&original);
    session.effect.save_ron(&original).unwrap();
    session.dirty = true;
    let mut catalog = ProjectEffectCatalog::scan(temporary.path());
    let previous = ProjectEffectTreeSnapshot::scan(temporary.path());
    fs::rename(&original, &moved).unwrap();
    let current = ProjectEffectTreeSnapshot::scan(temporary.path());

    apply_project_effect_catalog_refresh(
        &mut catalog,
        &mut session,
        &previous,
        &current,
        &Localizer::new("en-US").unwrap(),
    );

    assert_eq!(session.source_path.as_deref(), Some(moved.as_path()));
    assert!(session.dirty);
    assert!(session.status.contains("moved to"));
}
