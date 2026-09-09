use super::*;

fn fixture() -> (tempfile::TempDir, App, ProjectSourceId) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("folder/empty")).unwrap();
    std::fs::write(root.path().join("folder/image.png"), b"texture").unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let source = catalog
        .content()
        .source_tree()
        .at_relative_path("folder")
        .unwrap()
        .id;
    let mut app = App::new();
    app.insert_resource(catalog)
        .insert_resource(crate::test_support::session_with_timing_slack())
        .insert_resource(Localizer::new("en-US").unwrap())
        .init_resource::<super::super::AssetBrowserState>();
    register(&mut app);
    (root, app, source)
}
fn choose(app: &mut App, choice: Choice) {
    let entity = app.world_mut().spawn(choice).id();
    app.world_mut().trigger(Activate { entity });
    app.world_mut().flush();
}

#[test]
fn preview_cancel_delete_and_restore_are_explicit_and_preserve_document() {
    let (root, mut app, source) = fixture();
    let effect = app.world().resource::<EditorSession>().effect.clone();
    app.world_mut().trigger(Open(Some(source)));
    io::drain(app.world_mut());
    assert!(app.world().resource::<State>().plan.is_some());
    assert!(app.world().resource::<DocumentProtectionState>().is_open());
    assert!(root.path().join("folder").is_dir());
    choose(&mut app, Choice::Cancel);
    assert!(!app.world().resource::<DocumentProtectionState>().is_open());
    assert!(!root.path().join(".aestra/deleted").exists());
    app.world_mut().trigger(Open(Some(source)));
    io::drain(app.world_mut());
    choose(&mut app, Choice::Confirm);
    io::drain(app.world_mut());
    assert!(!root.path().join("folder").exists());
    assert_eq!(app.world().resource::<EditorSession>().effect, effect);
    app.world_mut().trigger(Open(None));
    io::drain(app.world_mut());
    assert_eq!(app.world().resource::<State>().entries.len(), 1);
    choose(&mut app, Choice::Restore(0));
    io::drain(app.world_mut());
    assert!(root.path().join("folder/empty").is_dir());
    assert_eq!(
        std::fs::read(root.path().join("folder/image.png")).unwrap(),
        b"texture"
    );
    assert_eq!(app.world().resource::<EditorSession>().effect, effect);
    assert!(
        app.world()
            .resource::<super::super::AssetBrowserState>()
            .selected
            .is_some()
    );
}

#[test]
fn edits_before_or_during_confirm_cancel_without_touching_files() {
    for during in [false, true] {
        let (root, mut app, source) = fixture();
        app.world_mut().trigger(Open(Some(source)));
        io::drain(app.world_mut());
        let mut completion = if during {
            choose(&mut app, Choice::Confirm);
            Some(io::prepared_completion(app.world_mut()))
        } else {
            None
        };
        app.world_mut().resource_mut::<EditorSession>().execute(
            "Edit",
            aestra_authoring::EffectCommand::SetEffectName {
                name: "Unsaved".into(),
            },
            false,
        );
        if let Some(completion) = completion.as_mut() {
            completion.apply(app.world_mut());
        } else {
            choose(&mut app, Choice::Confirm);
        }
        assert!(root.path().join("folder/image.png").exists());
        assert!(!root.path().join(".aestra/deleted").exists());
        assert!(app.world().resource::<State>().message.contains("changed"));
    }
}

#[test]
fn active_source_blocks_and_escape_closes_confirmation() {
    let (root, mut app, source) = fixture();
    let effect = crate::test_support::effect_with_timing_slack();
    let path = root.path().join("folder/effect.aestra.ron");
    effect.save_ron(&path).unwrap();
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .refresh();
    app.world_mut()
        .resource_mut::<EditorSession>()
        .open(&path)
        .unwrap();
    app.world_mut().trigger(Open(Some(source)));
    io::drain(app.world_mut());
    assert!(app.world().resource::<State>().plan.is_none());
    assert!(
        app.world()
            .resource::<State>()
            .message
            .contains("active document")
    );
    choose(&mut app, Choice::Confirm);
    assert!(path.exists());
    app.init_resource::<ButtonInput<KeyCode>>();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.update();
    assert!(!app.world().resource::<State>().open);
    assert!(!app.world().resource::<DocumentProtectionState>().is_open());
}

#[test]
fn active_resource_in_an_embedded_effect_blocks_folder_deletion() {
    let (_, mut app, source) = fixture();
    app.world_mut()
        .resource_mut::<EditorSession>()
        .effect
        .assets
        .push(aestra_core::AssetDefinition::texture(
            "Image",
            "folder/image.png#Layer0",
        ));
    let world = app.world();
    assert!(active_block(
        world.resource::<ProjectEffectCatalog>(),
        world.resource::<EditorSession>(),
        source
    ));
}

#[test]
fn native_confirmation_scrolls_long_content_and_defaults_to_cancel() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("folder")).unwrap();
    let mut app = super::super::tests::browser_layout_app(root.path(), UVec2::new(800, 500), 1.0);
    let host = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, (With<Node>, Without<ChildOf>)>()
            .single(world)
            .unwrap()
    };
    app.world_mut().get_mut::<Node>(host).unwrap().height = Val::Px(500.0);
    app.world_mut().commands().entity(host).with_children(spawn);
    app.world_mut().flush();
    let source = app
        .world()
        .resource::<ProjectEffectCatalog>()
        .content()
        .source_tree()
        .at_relative_path("folder")
        .unwrap()
        .id;
    app.world_mut().trigger(Open(Some(source)));
    io::drain(app.world_mut());
    for _ in 0..4 {
        app.update();
    }
    let world = app.world_mut();
    let confirm = world
        .query::<(Entity, &Choice)>()
        .iter(world)
        .find(|(_, choice)| matches!(choice, Choice::Confirm))
        .unwrap()
        .0;
    assert!(!world.entity(confirm).contains::<InteractionDisabled>());
    let focused = world.resource::<InputFocus>().get().unwrap();
    assert!(matches!(world.get::<Choice>(focused), Some(Choice::Cancel)));
    world.resource_mut::<State>().plan = None;
    world.resource_mut::<State>().message = "A long dependency blocker\n".repeat(90);
    for _ in 0..4 {
        app.update();
    }
    let world = app.world_mut();
    let (_, node, scroll) = world
        .query_filtered::<(Entity, &ComputedNode, &Node), With<Body>>()
        .single(world)
        .unwrap();
    assert!(
        node.size().y > 40.0 && node.size().y <= 400.0,
        "dialog height {:?}",
        node.size()
    );
    assert_eq!(scroll.overflow, Overflow::scroll_y());
    let confirm = world
        .query::<(Entity, &Choice)>()
        .iter(world)
        .find(|(_, choice)| matches!(choice, Choice::Confirm))
        .unwrap()
        .0;
    assert!(world.entity(confirm).contains::<InteractionDisabled>());
    assert!(root.path().join("folder").is_dir());
    assert!(!root.path().join(".aestra/deleted").exists());
}
