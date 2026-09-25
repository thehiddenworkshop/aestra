use super::*;

#[test]
fn function_socket_palette_choices_are_transactional() {
    let function = fixture();
    let document = MaterialAuthoringDocument::standalone(vec![])
        .with_material_functions(vec![function.clone()]);
    let before = document.clone();
    let options = choices(
        &document,
        &function,
        Some(SocketKind::Target(Target::Output(function.outputs[0].id))),
    );
    assert!(!options.is_empty());
    assert_eq!(document, before);
    for option in options {
        assert!(validate(&document, function.id, &option.edits));
    }
}

fn fixture() -> MaterialFunction {
    let expression = MaterialExpressionId::new();
    MaterialFunction {
        id: MaterialFunctionId::new(),
        name: "Socket creation".into(),
        schema_version: aestra_core::material::MaterialSchemaVersion::CURRENT,
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
    }
}

#[test]
fn source_menu_names_each_input_and_rejects_stale_endpoints() {
    let function = fixture();
    let document = MaterialAuthoringDocument::standalone(vec![])
        .with_material_functions(vec![function.clone()]);
    let options = choices(
        &document,
        &function,
        Some(SocketKind::Source(function.expressions[0].id)),
    );
    assert!(options.iter().any(|option| option.label == "Multiply — A"));
    assert!(options.iter().any(|option| option.label == "Multiply — B"));
    assert!(!options.iter().any(|option| option.label == "Float"));
    for option in &options {
        assert!(validate(&document, function.id, &option.edits));
    }
    assert!(
        choices(
            &document,
            &function,
            Some(SocketKind::Source(MaterialExpressionId::new()))
        )
        .is_empty()
    );
    assert!(
        choices(
            &document,
            &function,
            Some(SocketKind::Target(Target::Output(
                MaterialFunctionOutputId::new()
            )))
        )
        .is_empty()
    );
}

#[test]
fn target_menu_supports_ordinary_function_inputs() {
    let mut function = fixture();
    let derivative = MaterialExpressionId::new();
    function.expressions.push(MaterialExpression {
        id: derivative,
        kind: MaterialExpressionKind::DerivativeX {
            value: function.expressions[0].id,
        },
    });
    function.outputs[0].expression = derivative;
    let document = MaterialAuthoringDocument::standalone(vec![])
        .with_material_functions(vec![function.clone()]);
    let options = choices(
        &document,
        &function,
        Some(SocketKind::Target(Target::Input(
            derivative,
            MaterialExpressionInput::Value,
        ))),
    );
    let option = options
        .iter()
        .find(|option| option.label == "Float")
        .unwrap();
    assert!(
        matches!(option.edits.last(), Some(Edit::Rewire { expression, input: MaterialExpressionInput::Value, .. }) if *expression == derivative)
    );
    assert!(validate(&document, function.id, &option.edits));
}

#[test]
fn socket_drag_release_opens_palette_without_snapping_into_another_view() {
    use bevy::picking::{
        backend::HitData,
        pointer::{Location, PointerButton, PointerId},
    };
    let (mut app, _root, viewport, source, target) = setup(1.0, 1.0);
    let socket = *app.world().get::<Socket>(target).unwrap();
    let foreign = app.world_mut().spawn(View(socket.owner)).id();
    let foreign_node = app.world_mut().spawn(ChildOf(foreign)).id();
    let foreign_socket = app
        .world_mut()
        .spawn((
            Socket {
                node: foreign_node,
                ..socket
            },
            ChildOf(foreign_node),
        ))
        .id();
    let (screen, _) = pointer(app.world(), viewport);
    let location = Location {
        target: bevy::camera::NormalizedRenderTarget::None {
            width: 960,
            height: 640,
        },
        position: screen,
    };
    app.world_mut().trigger(Pointer::new(
        PointerId::Mouse,
        location.clone(),
        DragStart {
            button: PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        },
        source,
    ));
    let preview = app.world().resource::<ConnectionPreview>();
    assert!(preview.1.contains(&target));
    assert!(!preview.1.contains(&foreign_socket));
    app.world_mut().trigger(Pointer::new(
        PointerId::Mouse,
        location,
        DragEnd {
            button: PointerButton::Primary,
            distance: Vec2::splat(100.0),
        },
        source,
    ));
    app.world_mut().flush();
    assert!(app.world().resource::<Palette>().0.is_some());
}

#[test]
fn enter_chooses_the_filtered_input_without_mutating_on_cancel() {
    let (mut app, _root, viewport, source, _) = setup(1.0, 1.0);
    let (screen, _) = pointer(app.world(), viewport);
    open(app.world_mut(), source, Vec2::splat(-500.0));
    assert!(app.world().resource::<Palette>().0.is_none());
    open(app.world_mut(), source, screen);
    let open = app.world().resource::<Palette>().0.as_ref().unwrap();
    let input = open.input;
    let expected = open
        .choices
        .iter()
        .find(|choice| choice.label == "Multiply — B")
        .unwrap()
        .created
        .last()
        .copied()
        .unwrap();
    app.world_mut().trigger(ValueChange {
        source: input,
        value: "multiply — b".to_string(),
        is_final: false,
    });
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Enter);
    maintain(app.world_mut());
    app.world_mut().flush();
    assert!(app.world().resource::<Palette>().0.is_none());
    let current = app
        .world()
        .resource::<EditorSession>()
        .graph_function(app.world().resource::<ProjectEffectCatalog>())
        .unwrap();
    let original = match app.world().get::<Socket>(source).unwrap().kind {
        SocketKind::Source(id) => id,
        _ => unreachable!(),
    };
    assert!(
        matches!(current.expressions.iter().find(|node| node.id == expected).unwrap().kind, MaterialExpressionKind::Multiply(_, right) if right == original)
    );
}

#[test]
fn restored_function_view_opens_and_chooses_socket_node_from_its_own_target() {
    let (mut app, _root, viewport, source, _) = setup(1.0, 1.0);
    let function = app.world().get::<Socket>(source).unwrap().owner;
    app.world_mut()
        .resource_mut::<EditorSession>()
        .return_to_effect_material();
    let (screen, _) = pointer(app.world(), viewport);
    open(app.world_mut(), source, screen);
    let open = app.world().resource::<Palette>().0.as_ref().unwrap();
    assert!(!open.choices.is_empty());
    let input = open.input;
    app.world_mut().trigger(ValueChange {
        source: input,
        value: "multiply — b".to_string(),
        is_final: false,
    });
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Enter);
    maintain(app.world_mut());
    app.world_mut().flush();
    assert!(app.world().resource::<Palette>().0.is_none());
    assert_eq!(
        app.world()
            .resource::<EditorSession>()
            .standalone_function(),
        Some(function)
    );
}

fn setup(scale: f32, zoom: f32) -> (App, tempfile::TempDir, Entity, Entity, Entity) {
    use bevy::ecs::system::RunSystemOnce;
    let root = tempfile::tempdir().unwrap();
    let function = fixture();
    function
        .save_ron(root.path().join("body.aestra.material-function.ron"))
        .unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let mut session = crate::test_support::session_with_timing_slack();
    session
        .open_material_function(&catalog, function.id)
        .unwrap();
    let (mut app, host) = geometry::tests::layout_app(scale);
    app.insert_resource(session)
        .insert_resource(catalog)
        .init_resource::<InputFocus>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<crate::history::MaterialProgramEditHistory>()
        .init_resource::<crate::history::EditorHistoryLedger>()
        .init_resource::<FunctionEditor>()
        .add_observer(crate::history::execute_history_action);
    super::super::register(&mut app);
    let graph_key = crate::material_graph::function_graph_memory_key(root.path(), function.id);
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_view(format!("{graph_key}#view:9"), Vec2::new(10.0, 20.0), zoom);
    app.world_mut()
        .run_system_once(
            move |mut commands: Commands,
                  session: Res<EditorSession>,
                  catalog: Res<ProjectEffectCatalog>,
                  assets: Res<AssetServer>,
                  memory: Res<GraphViewportMemory>| {
                commands.entity(host).with_children(|parent| {
                    super::super::spawn(
                        parent,
                        &session,
                        &catalog,
                        &assets,
                        &memory,
                        &crate::material_graph::MaterialGraphSelectionState::default(),
                        &FunctionGraphMenuState::default(),
                        &session.material_target,
                        Some(crate::docking::EditorViewId(9)),
                    )
                });
            },
        )
        .unwrap();
    for _ in 0..4 {
        app.update();
    }
    let viewport = app
        .world_mut()
        .query_filtered::<Entity, With<View>>()
        .single(app.world())
        .unwrap();
    let source = app
        .world_mut()
        .query::<(Entity, &Socket)>()
        .iter(app.world())
        .find(|(_, socket)| matches!(socket.kind, SocketKind::Source(_)))
        .unwrap()
        .0;
    let target = app
        .world_mut()
        .query::<(Entity, &Socket)>()
        .iter(app.world())
        .find(|(_, socket)| matches!(socket.kind, SocketKind::Target(_)))
        .unwrap()
        .0;
    (app, root, viewport, source, target)
}

fn pointer(world: &World, viewport: Entity) -> (Vec2, Vec2) {
    let computed = world.get::<ComputedNode>(viewport).unwrap();
    let transform = world.get::<UiGlobalTransform>(viewport).unwrap();
    let local = computed.size() * computed.inverse_scale_factor * Vec2::new(0.8, 0.85);
    let screen = (transform.translation.trunc() - computed.size() * 0.5)
        * computed.inverse_scale_factor
        + local;
    (screen, local)
}

#[test]
fn socket_menu_uses_origin_view_coordinates_and_cleans_up_on_escape_or_teardown() {
    for scale in [1.0, 1.25, 2.0] {
        for zoom in [0.5, 1.0] {
            let (mut app, _root, viewport, source, _) = setup(scale, zoom);
            let (screen, local) = pointer(app.world(), viewport);
            let expected = app
                .world()
                .get::<FeathersGraphViewport>(viewport)
                .unwrap()
                .unproject_viewport_point(local);
            open(app.world_mut(), source, screen);
            let opened = app.world().resource::<Palette>().0.as_ref().unwrap();
            assert_eq!(opened.viewport, viewport);
            assert!(opened.position.distance(expected) < 0.001);
            let anchor = opened.anchor;
            assert!(
                app.world_mut()
                    .query_filtered::<Entity, With<Surface>>()
                    .iter(app.world())
                    .count()
                    == 1
            );
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape);
            maintain(app.world_mut());
            assert!(app.world().resource::<Palette>().0.is_none());
            assert!(app.world().get_entity(anchor).is_err());
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
            open(app.world_mut(), source, screen);
            app.world_mut().entity_mut(viewport).despawn();
            maintain(app.world_mut());
            assert!(app.world().resource::<Palette>().0.is_none());
        }
    }
}

#[test]
fn socket_creation_is_one_undo_and_redo_and_rejects_changes_while_menu_is_open() {
    for from_source in [false, true] {
        let (mut app, root, viewport, source, target) = setup(1.25, 0.75);
        let origin = if from_source { source } else { target };
        let before = app
            .world()
            .resource::<EditorSession>()
            .graph_function(app.world().resource::<ProjectEffectCatalog>())
            .unwrap();
        let effect = app.world().resource::<EditorSession>().effect.clone();
        let graph = crate::material_graph::function_graph_memory_key(root.path(), before.id);
        let (screen, _) = pointer(app.world(), viewport);
        open(app.world_mut(), origin, screen);
        let wanted = if from_source {
            "Multiply — A"
        } else {
            "Float"
        };
        let opened = app.world().resource::<Palette>().0.as_ref().unwrap();
        let index = opened
            .choices
            .iter()
            .position(|choice| choice.label == wanted)
            .unwrap();
        let ids = opened.choices[index].created.clone();
        let button = app
            .world_mut()
            .query::<(Entity, &Choose)>()
            .iter(app.world())
            .find(|(_, choose)| choose.0 == index)
            .unwrap()
            .0;
        app.world_mut().trigger(Activate { entity: button });
        app.world_mut().flush();
        let after = app
            .world()
            .resource::<EditorSession>()
            .graph_function(app.world().resource::<ProjectEffectCatalog>())
            .unwrap();
        assert_ne!(after, before);
        assert_eq!(app.world().resource::<EditorSession>().effect, effect);
        let positions = app
            .world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph);
        for id in ids {
            assert!(positions.contains_key(&id.to_string()));
        }
        app.world_mut().trigger(crate::history::HistoryAction::Undo);
        app.world_mut().flush();
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .graph_function(app.world().resource::<ProjectEffectCatalog>())
                .unwrap(),
            before
        );
        app.world_mut().trigger(crate::history::HistoryAction::Redo);
        app.world_mut().flush();
        assert_eq!(
            app.world()
                .resource::<GraphViewportMemory>()
                .base_nodes(&graph),
            positions
        );
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .graph_function(app.world().resource::<ProjectEffectCatalog>())
                .unwrap(),
            after
        );
        // Reopen, then mutate the body before selecting: an old menu must not append stale edits.
        open(app.world_mut(), origin, screen);
        let button = app
            .world_mut()
            .query_filtered::<Entity, With<Choose>>()
            .iter(app.world())
            .next()
            .unwrap();
        let old = app
            .world()
            .resource::<EditorSession>()
            .graph_function(app.world().resource::<ProjectEffectCatalog>())
            .unwrap();
        let mut changed = old.clone();
        changed.name.push_str(" changed");
        app.world_mut()
            .resource_mut::<ProjectEffectCatalog>()
            .replace_material_function(&old, &changed)
            .unwrap();
        app.world_mut().trigger(Activate { entity: button });
        app.world_mut().flush();
        assert!(
            app.world()
                .resource::<EditorSession>()
                .status
                .starts_with("Node creation failed:")
        );
        assert_eq!(
            app.world()
                .resource::<GraphViewportMemory>()
                .base_nodes(&graph),
            positions
        );
        assert_eq!(
            app.world()
                .resource::<ProjectEffectCatalog>()
                .material_functions()
                .unwrap()
                .into_iter()
                .find(|function| function.id == before.id)
                .unwrap()
                .name,
            changed.name
        );
    }
}
