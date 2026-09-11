use super::*;
use bevy::ecs::system::RunSystemOnce;

fn fixture() -> (tempfile::TempDir, App, RendererDropTarget) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("materials")).unwrap();
    let preset = MaterialPresetDescriptor::from_ron(include_str!(
        "../../../../../../assets/test/materials/portal.aestra.material-preset.ron"
    ))
    .unwrap();
    preset
        .save_ron(
            root.path()
                .join("materials/portal.aestra.material-preset.ron"),
        )
        .unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let source = catalog
        .content()
        .source_tree()
        .at_relative_path("materials/portal.aestra.material-preset.ron")
        .unwrap()
        .id;
    let payload = AssetPayload::capture(&catalog, source);
    let session = crate::test_support::session_with_timing_slack();
    let target = RendererDropTarget {
        effect: session.effect.id,
        renderer: session.effect.emitters[0].renderers[0].id,
    };
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        bevy::scene::ScenePlugin,
    ))
    .init_asset::<Font>();
    super::super::register(&mut app);
    app.insert_resource(catalog)
        .insert_resource(session)
        .insert_resource(Localizer::new("en-US").unwrap())
        .init_resource::<InputFocus>()
        .init_resource::<ButtonInput<KeyCode>>();
    let source = app.world_mut().spawn(payload).id();
    let source_label = app.world_mut().spawn(ChildOf(source)).id();
    let card = app.world_mut().spawn(target).id();
    let card_label = app.world_mut().spawn(ChildOf(card)).id();
    app.world_mut().trigger(Pointer::new(
        bevy::picking::pointer::PointerId::Mouse,
        bevy::picking::pointer::Location {
            target: bevy::camera::NormalizedRenderTarget::None {
                width: 800,
                height: 600,
            },
            position: Vec2::ZERO,
        },
        DragDrop {
            button: PointerButton::Primary,
            dropped: source_label,
            hit: bevy::picking::backend::HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        },
        card_label,
    ));
    app.world_mut().flush();
    assert!(
        app.world().resource::<Prompt>().0.is_some(),
        "{}",
        app.world().resource::<EditorSession>().status
    );
    (root, app, target)
}

fn enter_name(app: &mut App, name: &str) {
    let inputs: Vec<_> = app
        .world_mut()
        .query::<(&Field, &Children)>()
        .iter(app.world())
        .filter(|(field, _)| matches!(field, Field::Name))
        .flat_map(|(_, children)| children.iter())
        .collect();
    for input in inputs {
        app.world_mut()
            .entity_mut(input)
            .insert(EditableText::new(name));
    }
    app.world_mut()
        .resource_mut::<Prompt>()
        .0
        .as_mut()
        .unwrap()
        .name = name.into();
}

fn submit_prompt(app: &mut App) {
    app.world_mut()
        .resource_mut::<Prompt>()
        .0
        .as_mut()
        .unwrap()
        .submit = true;
    app.world_mut().run_system_once(submit).unwrap();
    app.world_mut().flush();
}

#[test]
fn preset_creates_new_saved_material_and_one_undoable_assignment() {
    let (root, mut app, _) = fixture();
    let before = app.world().resource::<EditorSession>().effect.clone();
    let preset_path = root
        .path()
        .join("materials/portal.aestra.material-preset.ron");
    let bytes = std::fs::read(&preset_path).unwrap();
    enter_name(&mut app, "My Portal");
    // Submission must read the native field, not a stale ValueChange value.
    app.world_mut()
        .resource_mut::<Prompt>()
        .0
        .as_mut()
        .unwrap()
        .name = "Previous frame".into();
    submit_prompt(&mut app);
    io::drain(app.world_mut());
    assert!(app.world().resource::<Prompt>().0.is_none());
    assert!(!app.world().resource::<DocumentProtectionState>().is_open());
    let path = root.path().join("materials/My Portal.aestra.material.ron");
    let program = MaterialProgram::load_ron(&path).unwrap();
    assert_eq!(program.name, "My Portal");
    let session = app.world().resource::<EditorSession>();
    assert_eq!(session.effect.material_instances.len(), 1);
    assert_eq!(
        session.effect.material_instances[0].program,
        MaterialProgramRef::Project(program.id)
    );
    assert_eq!(
        session.effect.emitters[0].renderers[0].material,
        session.effect.material_instances[0].id
    );
    assert!(!session.material_history_active);
    let after = session.effect.clone();
    let mut session = app.world_mut().resource_mut::<EditorSession>();
    session.undo();
    assert_eq!(session.effect, before);
    assert!(!session.can_undo());
    session.redo();
    assert_eq!(session.effect, after);
    assert_eq!(std::fs::read(&preset_path).unwrap(), bytes);
    assert!(
        path.exists(),
        "Reusable source creation is not document Undo"
    );
}

#[test]
fn empty_or_colliding_name_disables_create_and_escape_cancels_without_writes() {
    let (root, mut app, _) = fixture();
    let before = app.world().resource::<EditorSession>().effect.clone();
    app.world_mut().run_system_once(sync).unwrap();
    app.world_mut().flush();
    assert!(
        app.world_mut()
            .query::<(&Choice, Has<InteractionDisabled>)>()
            .iter(app.world())
            .any(|(choice, disabled)| matches!(choice, Choice::Create) && disabled)
    );
    submit_prompt(&mut app);
    assert!(io::idle_world(app.world()));
    let program = MaterialProgram::additive_sprite("Existing");
    program
        .save_ron(root.path().join("materials/Existing.aestra.material.ron"))
        .unwrap();
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .refresh();
    enter_name(&mut app, "existing");
    assert!(
        destination(
            app.world().resource::<Prompt>().0.as_ref().unwrap(),
            app.world().resource::<ProjectEffectCatalog>()
        )
        .is_err()
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.world_mut().run_system_once(sync).unwrap();
    app.world_mut().flush();
    assert!(app.world().resource::<Prompt>().0.is_none());
    assert!(!app.world().resource::<DocumentProtectionState>().is_open());
    assert_eq!(app.world().resource::<EditorSession>().effect, before);
    assert_eq!(
        std::fs::read_dir(root.path().join("materials"))
            .unwrap()
            .count(),
        2
    );
}

#[test]
fn late_collision_and_changed_preset_leave_document_intact_and_allow_cancel() {
    for changed_preset in [false, true] {
        let (root, mut app, _) = fixture();
        let before = app.world().resource::<EditorSession>().effect.clone();
        enter_name(&mut app, "New");
        if changed_preset {
            let path = root
                .path()
                .join("materials/portal.aestra.material-preset.ron");
            let mut preset = MaterialPresetDescriptor::load_ron(&path).unwrap();
            preset.display_name = "Changed externally".into();
            preset.save_ron(path).unwrap();
        } else {
            std::fs::write(
                root.path().join("materials/New.aestra.material.ron"),
                "untouched",
            )
            .unwrap();
        }
        submit_prompt(&mut app);
        io::drain(app.world_mut());
        let prompt = app.world().resource::<Prompt>().0.as_ref().unwrap();
        assert!(!prompt.busy);
        assert!(prompt.failure.is_some());
        app.world_mut().run_system_once(sync).unwrap();
        app.world_mut().flush();
        assert!(
            app.world_mut()
                .query::<(&Choice, Has<InteractionDisabled>)>()
                .iter(app.world())
                .any(|(choice, disabled)| matches!(choice, Choice::Create) && disabled)
        );
        assert_eq!(app.world().resource::<EditorSession>().effect, before);
        if changed_preset {
            assert!(
                !root
                    .path()
                    .join("materials/New.aestra.material.ron")
                    .exists()
            );
        } else {
            assert_eq!(
                std::fs::read_to_string(root.path().join("materials/New.aestra.material.ron"))
                    .unwrap(),
                "untouched"
            );
        }
    }
}

#[test]
fn editing_document_during_creation_keeps_new_source_but_never_assigns_to_changed_document() {
    let (root, mut app, _) = fixture();
    enter_name(&mut app, "Unassigned");
    submit_prompt(&mut app);
    let mut completion = io::prepared_completion(app.world_mut());
    app.world_mut().resource_mut::<EditorSession>().execute(
        "Changed",
        EffectCommand::SetEffectName {
            name: "Keep this change".into(),
        },
        true,
    );
    let before = app.world().resource::<EditorSession>().effect.clone();
    completion.apply(app.world_mut());
    assert_eq!(app.world().resource::<EditorSession>().effect, before);
    assert!(
        app.world()
            .resource::<EditorSession>()
            .status
            .contains("not assigned")
    );
    assert!(
        root.path()
            .join("materials/Unassigned.aestra.material.ron")
            .exists()
    );
}

#[test]
fn missing_folder_and_new_renderer_lock_prevent_creation() {
    let (root, mut app, target) = fixture();
    let catalog = app.world().resource::<ProjectEffectCatalog>();
    for folder in [
        "../outside",
        "/absolute",
        "C:/elsewhere",
        "missing",
        "materials/../materials",
    ] {
        assert!(folder_id(catalog, folder).is_err());
    }
    enter_name(&mut app, "Locked");
    app.world_mut()
        .resource_mut::<EditorSession>()
        .locks
        .lock(SemanticTarget::Renderer(target.renderer));
    submit_prompt(&mut app);
    assert!(io::idle_world(app.world()));
    assert!(
        app.world()
            .resource::<Prompt>()
            .0
            .as_ref()
            .unwrap()
            .failure
            .is_some()
    );
    assert!(
        !root
            .path()
            .join("materials/Locked.aestra.material.ron")
            .exists()
    );
    assert!(!app.world().resource::<EditorSession>().can_undo());
}
