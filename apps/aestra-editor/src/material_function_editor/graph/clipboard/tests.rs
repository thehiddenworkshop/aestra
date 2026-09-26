use super::*;
use bevy::ecs::system::RunSystemOnce;

#[test]
fn function_multi_clipboard_shortcuts_preserve_links_select_new_nodes_and_undo() {
    let root = tempfile::tempdir().unwrap();
    let output = MaterialExpressionId::new();
    let constant = MaterialExpressionId::new();
    let multiply = MaterialExpressionId::new();
    let owner = MaterialFunctionId::new();
    let function = MaterialFunction {
        id: owner,
        name: "Clipboard function".into(),
        schema_version: aestra_core::material::MaterialSchemaVersion::CURRENT,
        inputs: vec![],
        outputs: vec![MaterialFunctionOutput {
            id: MaterialFunctionOutputId::new(),
            name: "Value".into(),
            value_type: MaterialValueType::Float,
            expression: output,
        }],
        expressions: vec![
            MaterialExpression {
                id: output,
                kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
            },
            MaterialExpression {
                id: constant,
                kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.75)),
            },
            MaterialExpression {
                id: multiply,
                kind: MaterialExpressionKind::Multiply(constant, constant),
            },
        ],
        custom_wesl: None,
    }
    .normalized();
    function
        .save_ron(root.path().join("clipboard.aestra.material-function.ron"))
        .unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let mut session = crate::test_support::session_with_timing_slack();
    session.open_material_function(&catalog, owner).unwrap();
    let mut app = App::new();
    app.insert_resource(session)
        .insert_resource(catalog)
        .init_resource::<GraphClipboard>()
        .init_resource::<crate::material_graph::MaterialGraphSelectionState>()
        .init_resource::<GraphViewportMemory>()
        .init_resource::<FunctionEditor>()
        .init_resource::<crate::history::MaterialProgramEditHistory>()
        .init_resource::<crate::history::EditorHistoryLedger>()
        .init_resource::<ButtonInput<KeyCode>>()
        .add_systems(Update, keyboard)
        .add_observer(crate::history::execute_history_action);
    let scope = Some(crate::docking::EditorViewId(42));
    let selected = BTreeSet::from([constant, multiply]);
    app.world_mut()
        .resource_mut::<crate::material_graph::MaterialGraphSelectionState>()
        .select_function_expressions(scope, owner, &selected, GraphSelectionMode::Replace);
    let graph_key = crate::material_graph::function_graph_memory_key(root.path(), owner);
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .place_node(&graph_key, constant.to_string(), Vec2::new(50.0, 80.0));
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .place_node(&graph_key, multiply.to_string(), Vec2::new(300.0, 120.0));
    app.world_mut()
        .run_system_once(move |mut commands: Commands| {
            commands.spawn(Node::default()).with_children(|parent| {
                let viewport = spawn_graph_viewport(
                    parent,
                    GraphViewportProps {
                        key: graph_key.clone(),
                        content_size: Vec2::splat(1000.0),
                        selection_bounds: None,
                        initial_view: Some((Vec2::ZERO, 1.0)),
                    },
                    (),
                    |_| {},
                    |_| {},
                );
                parent.commands().entity(viewport).insert((
                    View(owner),
                    ViewScope(scope),
                    RelativeCursorPosition {
                        cursor_over: true,
                        normalized: None,
                    },
                ));
            });
        })
        .unwrap();
    fn tap(app: &mut App, key: KeyCode) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::ControlLeft);
        keys.press(key);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
    }
    fn source(app: &App) -> MaterialFunction {
        app.world()
            .resource::<EditorSession>()
            .graph_function(app.world().resource::<ProjectEffectCatalog>())
            .unwrap()
    }
    tap(&mut app, KeyCode::KeyC);
    assert_eq!(source(&app), function);
    tap(&mut app, KeyCode::KeyX);
    assert_eq!(source(&app).expressions.len(), 1);
    tap(&mut app, KeyCode::KeyV);
    let pasted = source(&app);
    assert_eq!(pasted.expressions.len(), 3);
    let new_constant = pasted
        .expressions
        .iter()
        .find(|node| node.kind == MaterialExpressionKind::Constant(MaterialValue::Float(0.75)))
        .unwrap()
        .id;
    assert!(
        pasted
            .expressions
            .iter()
            .any(|node| node.kind == MaterialExpressionKind::Multiply(new_constant, new_constant))
    );
    let created = app
        .world()
        .resource::<crate::material_graph::MaterialGraphSelectionState>()
        .function_arrange_seeds(scope, owner);
    assert_eq!(created.len(), 2);
    assert!(
        created.is_disjoint(
            &selected
                .iter()
                .copied()
                .map(GraphNodeKey::Expression)
                .collect()
        )
    );
    app.world_mut().trigger(crate::history::HistoryAction::Undo);
    app.world_mut().flush();
    assert_eq!(source(&app).expressions.len(), 1);
    app.world_mut().trigger(crate::history::HistoryAction::Redo);
    app.world_mut().flush();
    assert_eq!(source(&app), pasted);
    tap(&mut app, KeyCode::KeyD);
    let new_selection = app
        .world()
        .resource::<crate::material_graph::MaterialGraphSelectionState>()
        .function_arrange_seeds(scope, owner);
    assert_eq!(source(&app).expressions.len(), 5);
    assert_eq!(new_selection.len(), 2);
    assert!(new_selection.is_disjoint(&created));
}
