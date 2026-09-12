use super::*;

#[test]
fn cached_panel_graph_timeline_and_compile_queries_do_not_reopen_sources() {
    use aestra_core::{
        EffectClip, MaterialId,
        material::{MaterialInstance, MaterialProgramRef, MaterialRenderState},
    };
    let directory = tempfile::tempdir().unwrap();
    let (mut world, _) = fixture(directory.path());
    let mut child = world.resource::<EditorSession>().effect.clone();
    child.id = aestra_core::EffectId::new();
    child.name = "Child".into();
    child
        .save_ron(directory.path().join("child.aestra.ron"))
        .unwrap();
    let program = MaterialProgram::additive_sprite("Program").normalized();
    program
        .save_ron(directory.path().join("program.aestra.material.ron"))
        .unwrap();
    world.resource_mut::<EditorProjectContent>().refresh();
    let mut root = world.resource::<EditorSession>().effect.clone();
    root.effect_clips
        .push(EffectClip::new(child.id, 0.0, child.duration));
    root.material_instances.push(MaterialInstance {
        id: MaterialId::new(),
        program: MaterialProgramRef::Project(program.id),
        values: default(),
        render_state: MaterialRenderState::additive_sprite(),
    });
    let catalog = world.resource::<EditorProjectContent>();
    let graph = catalog
        .content()
        .cached_effect_usage_graph(child.id.into())
        .unwrap();
    assert!(catalog.prepared.is_none());
    for name in [
        "open.aestra.ron",
        "child.aestra.ron",
        "program.aestra.material.ron",
    ] {
        fs::remove_file(directory.path().join(name)).unwrap();
    }
    assert_eq!(catalog.cached_effect(child.id.into()).unwrap(), child);
    assert_eq!(
        catalog
            .effect_for_placement(&root, child.id.into())
            .unwrap(),
        child
    );
    assert_eq!(
        catalog.material_programs_for_effect(&root).unwrap(),
        vec![program]
    );
    assert!(catalog.material_function_library().is_ok());
    assert!(catalog.material_preset_catalog().is_ok());
    assert!(catalog.dependency_validation_report(&root).is_valid());
    assert_eq!(
        catalog
            .content()
            .cached_effect_usage_graph(child.id.into())
            .unwrap(),
        graph
    );
    assert!(catalog.compile_project(&root).is_ok());
    // Commands keep the latest-source policy, especially deletion confirmation.
    assert!(catalog.load_effect(child.id.into()).is_err());
    assert!(catalog.index().effect_usage_graph(child.id.into()).is_err());
}

#[test]
fn cached_program_never_authorizes_editing_over_an_external_change() {
    use aestra_core::{
        MaterialId,
        material::{MaterialInstance, MaterialProgramRef, MaterialRenderState},
    };
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("program.aestra.material.ron");
    let original = MaterialProgram::additive_sprite("Original").normalized();
    original.save_ron(&path).unwrap();
    let mut catalog = EditorProjectContent::scan(directory.path());
    let mut root = EffectAsset::new("Root", 1.0);
    root.material_instances.push(MaterialInstance {
        id: MaterialId::new(),
        program: MaterialProgramRef::Project(original.id),
        values: default(),
        render_state: MaterialRenderState::additive_sprite(),
    });
    let mut external = original.clone();
    external.name = "External".into();
    external.save_ron(&path).unwrap();
    let mut local = original.clone();
    local.name = "Local".into();
    assert_eq!(
        catalog.material_programs_for_effect(&root).unwrap(),
        vec![original.clone()]
    );
    assert!(catalog.replace_material_program(&original, &local).is_err());
    assert!(catalog.material_drafts.is_empty());
    assert_eq!(MaterialProgram::load_ron(&path).unwrap(), external);
}

#[test]
fn published_material_refresh_updates_cached_queries_but_keeps_draft_overlay() {
    use aestra_core::{
        MaterialId,
        material::{MaterialInstance, MaterialProgramRef, MaterialRenderState},
    };
    let directory = tempfile::tempdir().unwrap();
    let (mut world, mut watch) = fixture(directory.path());
    let path = directory.path().join("program.aestra.material.ron");
    let original = MaterialProgram::additive_sprite("Original").normalized();
    original.save_ron(&path).unwrap();
    world.resource_mut::<EditorProjectContent>().refresh();
    watch.accept_current(world.resource::<EditorProjectContent>());
    let mut root = world.resource::<EditorSession>().effect.clone();
    root.material_instances.push(MaterialInstance {
        id: MaterialId::new(),
        program: MaterialProgramRef::Project(original.id),
        values: default(),
        render_state: MaterialRenderState::additive_sprite(),
    });
    let mut updated = original.clone();
    updated.name = "Updated".into();
    updated.save_ron(&path).unwrap();
    settle(&mut world, &mut watch);
    assert_eq!(
        world
            .resource::<EditorProjectContent>()
            .material_programs_for_effect(&root)
            .unwrap(),
        vec![updated.clone()]
    );
    let mut draft = updated.clone();
    draft.name = "Unsaved".into();
    world
        .resource_mut::<EditorProjectContent>()
        .replace_material_program(&updated, &draft)
        .unwrap();
    updated.name = "Second external edit".into();
    updated.save_ron(&path).unwrap();
    settle(&mut world, &mut watch);
    let catalog = world.resource::<EditorProjectContent>();
    assert_eq!(
        catalog.material_programs_for_effect(&root).unwrap(),
        vec![draft]
    );
    assert!(catalog.material_drafts.preflight().is_err());
}

fn fixture(root: &Path) -> (World, ProjectEffectWatchState) {
    let path = root.join("open.aestra.ron");
    let session = crate::test_support::session_with_source_path(&path);
    session.effect.save_ron(&path).unwrap();
    let mut world = World::new();
    world.insert_resource(EditorProjectContent::scan(root));
    world.insert_resource(session);
    let watch = ProjectEffectWatchState::from_world(&mut world);
    (world, watch)
}

fn prepare(world: &World) -> RefreshResult {
    let catalog = world.resource::<EditorProjectContent>();
    prepare_refresh(
        catalog.clone(),
        RefreshInput::capture(catalog, world.resource::<EditorSession>()),
        true,
    )
}

fn finish(world: &mut World, watch: &mut ProjectEffectWatchState, result: RefreshResult) {
    // Use actual system borrows: resource_scope removes/reinserts the resource and changes its
    // ticks in Bevy 0.19, which would itself invalidate the UI regardless of this system.
    let mut state = bevy::ecs::system::SystemState::<(
        ResMut<EditorProjectContent>,
        ResMut<EditorSession>,
    )>::new(world);
    let (mut catalog, mut session) = state.get_mut(world).unwrap();
    finish_result(
        result,
        watch,
        catalog.reborrow(),
        session.reborrow(),
        &Localizer::new("en-US").unwrap(),
    );
    state.apply(world);
}

fn settle(world: &mut World, watch: &mut ProjectEffectWatchState) {
    for _ in 0..2 {
        let result = prepare(world);
        finish(world, watch, result);
    }
}

#[test]
fn generic_refresh_publishes_tree_without_bevy_ui_or_playback_invalidation() {
    let directory = tempfile::tempdir().unwrap();
    let (mut world, mut watch) = fixture(directory.path());
    world.resource_mut::<EditorSession>().playing = true;
    let revision = world.resource::<EditorSession>().ui_revision;
    let content_revision = world.resource::<EditorProjectContent>().content_revision();
    let preview = world
        .resource::<EditorSession>()
        .preview
        .as_ref()
        .unwrap()
        .effect()
        .clone();
    let status = world.resource::<EditorSession>().status.clone();
    fs::write(directory.path().join("notes.txt"), "notes").unwrap();
    world.clear_trackers();
    settle(&mut world, &mut watch);
    let catalog = world.resource_ref::<EditorProjectContent>();
    assert!(!catalog.is_changed());
    assert_ne!(catalog.content_revision(), content_revision);
    assert!(
        catalog
            .snapshot
            .content
            .source_tree()
            .at_relative_path("notes.txt")
            .is_some()
    );
    let session = world.resource_ref::<EditorSession>();
    assert!(!session.is_changed());
    assert_eq!(session.ui_revision, revision);
    assert!(session.playing);
    assert_eq!(session.status, status);
    assert!(std::sync::Arc::ptr_eq(
        &preview,
        session.preview.as_ref().unwrap().effect()
    ));
}

#[test]
fn unchanged_refresh_does_not_publish_or_invalidate_resources() {
    let directory = tempfile::tempdir().unwrap();
    let (mut world, mut watch) = fixture(directory.path());
    let version = world.resource::<EditorProjectContent>().content_revision();
    world.clear_trackers();
    settle(&mut world, &mut watch);
    assert_eq!(
        world.resource::<EditorProjectContent>().content_revision(),
        version
    );
    assert!(!world.resource_ref::<EditorProjectContent>().is_changed());
    assert!(!world.resource_ref::<EditorSession>().is_changed());
}

#[test]
fn source_reload_is_prepared_off_thread_and_apply_uses_captured_bytes() {
    let directory = tempfile::tempdir().unwrap();
    let (mut world, mut watch) = fixture(directory.path());
    let path = directory.path().join("open.aestra.ron");
    let mut effect = world.resource::<EditorSession>().effect.clone();
    effect.name = "External effect".into();
    effect.save_ron(&path).unwrap();
    let first = prepare(&world);
    finish(&mut world, &mut watch, first);
    let second = prepare(&world);
    // Deliberately remove disk after the worker's coherent observation. Applying that result
    // remains a memory-only operation; the next poll will report the subsequent deletion.
    fs::remove_file(&path).unwrap();
    finish(&mut world, &mut watch, second);
    assert_eq!(world.resource::<EditorSession>().effect, effect);
    assert!(!world.resource::<EditorSession>().dirty);
    assert!(
        world
            .resource::<EditorProjectContent>()
            .compile_project(&effect)
            .is_ok()
    );
    assert!(
        world
            .resource::<EditorSession>()
            .status
            .contains("Reloaded externally changed")
    );
}

#[test]
fn stale_worker_cannot_overwrite_a_new_project_or_an_internal_save() {
    let first_root = tempfile::tempdir().unwrap();
    let second_root = tempfile::tempdir().unwrap();
    let (mut world, mut watch) = fixture(first_root.path());
    fs::write(first_root.path().join("notes.txt"), "old scan").unwrap();
    let stale = prepare(&world);
    world.resource_mut::<EditorProjectContent>().refresh();
    let version = world.resource::<EditorProjectContent>().content_revision();
    finish(&mut world, &mut watch, stale);
    assert_eq!(
        world.resource::<EditorProjectContent>().content_revision(),
        version
    );
    let stale = prepare(&world);
    world.insert_resource(EditorProjectContent::scan(second_root.path()));
    finish(&mut world, &mut watch, stale);
    assert_eq!(
        world.resource::<EditorProjectContent>().root(),
        second_root.path()
    );
}

#[test]
fn editing_while_worker_runs_discards_its_document_result() {
    let directory = tempfile::tempdir().unwrap();
    let (mut world, mut watch) = fixture(directory.path());
    let path = directory.path().join("open.aestra.ron");
    let mut effect = world.resource::<EditorSession>().effect.clone();
    effect.name = "External".into();
    effect.save_ron(&path).unwrap();
    let first = prepare(&world);
    finish(&mut world, &mut watch, first);
    let stale = prepare(&world);
    world.resource_mut::<EditorSession>().effect.name = "Unsaved local".into();
    world.resource_mut::<EditorSession>().dirty = true;
    finish(&mut world, &mut watch, stale);
    assert_eq!(
        world.resource::<EditorSession>().effect.name,
        "Unsaved local"
    );
    assert!(
        watch
            .tracker
            .observe(watch.version(), &ProjectTreeStamp::scan(directory.path()))
            .is_none()
    );
}

#[test]
fn external_material_edit_preserves_draft_and_its_exact_byte_save_guard() {
    let directory = tempfile::tempdir().unwrap();
    let (mut world, mut watch) = fixture(directory.path());
    let path = directory.path().join("program.aestra.material.ron");
    let original = MaterialProgram::additive_sprite("Program");
    original.save_ron(&path).unwrap();
    world.resource_mut::<EditorProjectContent>().refresh();
    watch.accept_current(world.resource::<EditorProjectContent>());
    let mut edited = original.clone();
    edited.name = "Unsaved shared program".into();
    world
        .resource_mut::<EditorProjectContent>()
        .replace_material_program(&original, &edited)
        .unwrap();
    let drafts = world
        .resource::<EditorProjectContent>()
        .material_drafts
        .clone();
    world
        .resource_mut::<EditorSession>()
        .set_material_drafts(drafts.clone());
    let external = format!("// External change\n{}", fs::read_to_string(&path).unwrap());
    fs::write(&path, &external).unwrap();
    settle(&mut world, &mut watch);
    assert_eq!(
        world.resource::<EditorProjectContent>().material_drafts,
        drafts
    );
    assert_eq!(world.resource::<EditorSession>().material_drafts, drafts);
    assert!(
        world
            .resource::<EditorSession>()
            .status
            .contains("Shared source conflict")
    );
    assert!(
        world
            .resource_mut::<EditorProjectContent>()
            .save_material_drafts()
            .is_err()
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), external);
}

#[test]
fn unsaved_function_creation_survives_external_target_collision() {
    use aestra_core::material::{
        MaterialExpression, MaterialExpressionKind, MaterialFunctionOutput, MaterialSchemaVersion,
        MaterialValue, MaterialValueType,
    };
    use aestra_core::{MaterialExpressionId, MaterialFunctionId, MaterialFunctionOutputId};
    let directory = tempfile::tempdir().unwrap();
    let (mut world, mut watch) = fixture(directory.path());
    let expression = MaterialExpressionId::new();
    let function = MaterialFunction {
        schema_version: MaterialSchemaVersion::CURRENT,
        id: MaterialFunctionId::new(),
        name: "Unsaved function".into(),
        inputs: vec![],
        outputs: vec![MaterialFunctionOutput {
            id: MaterialFunctionOutputId::new(),
            name: "Value".into(),
            value_type: MaterialValueType::Float,
            expression,
        }],
        expressions: vec![MaterialExpression {
            id: expression,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        }],
        custom_wesl: None,
    };
    world
        .resource_mut::<EditorProjectContent>()
        .create_material_function(&function)
        .unwrap();
    let drafts = world
        .resource::<EditorProjectContent>()
        .material_drafts
        .clone();
    let path = drafts.functions[&function.id].path.clone();
    world
        .resource_mut::<EditorSession>()
        .set_material_drafts(drafts.clone());
    fs::write(&path, "external bytes").unwrap();
    settle(&mut world, &mut watch);
    assert_eq!(
        world.resource::<EditorProjectContent>().material_drafts,
        drafts
    );
    assert_eq!(world.resource::<EditorSession>().material_drafts, drafts);
    assert!(
        world
            .resource_mut::<EditorProjectContent>()
            .save_material_drafts()
            .is_err()
    );
    assert_eq!(fs::read_to_string(path).unwrap(), "external bytes");
}

#[test]
fn generic_refresh_does_not_reset_or_rebase_existing_material_drafts() {
    let directory = tempfile::tempdir().unwrap();
    let (mut world, mut watch) = fixture(directory.path());
    let path = directory.path().join("program.aestra.material.ron");
    let original = MaterialProgram::additive_sprite("Program");
    original.save_ron(&path).unwrap();
    world.resource_mut::<EditorProjectContent>().refresh();
    watch.accept_current(world.resource::<EditorProjectContent>());
    let mut edited = original.clone();
    edited.name = "Draft".into();
    world
        .resource_mut::<EditorProjectContent>()
        .replace_material_program(&original, &edited)
        .unwrap();
    let drafts = world
        .resource::<EditorProjectContent>()
        .material_drafts
        .clone();
    world
        .resource_mut::<EditorSession>()
        .set_material_drafts(drafts.clone());
    fs::create_dir(directory.path().join("empty")).unwrap();
    settle(&mut world, &mut watch);
    assert_eq!(
        world.resource::<EditorProjectContent>().material_drafts,
        drafts
    );
    assert_eq!(world.resource::<EditorSession>().material_drafts, drafts);
}
