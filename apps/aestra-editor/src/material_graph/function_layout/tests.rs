use super::*;
use bevy::ecs::system::RunSystemOnce;

fn function() -> MaterialFunction {
    MaterialFunction::from_ron(include_str!(
        "../../../../../assets/test/materials/dissolve_edge.aestra.material-function.ron"
    ))
    .unwrap()
}

#[test]
fn function_camera_persistence_uses_focused_visible_owner_not_query_order() {
    camera_owner(DocumentKey::MaterialFunction(MaterialFunctionId::new()));
}

#[test]
fn material_camera_persistence_uses_focused_visible_owner_not_query_order() {
    camera_owner(DocumentKey::MaterialProgram(MaterialProgramId::new()));
}

fn camera_owner(asset: DocumentKey) {
    use crate::{docking::EditorViewId, editor_view::ActiveEditorContext};
    let root = tempfile::tempdir().unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let key = match asset {
        DocumentKey::MaterialFunction(id) => function_graph_memory_key(catalog.root(), id),
        DocumentKey::MaterialProgram(id) => material_graph_view_key(id),
        _ => unreachable!(),
    };
    let document = GraphDocumentKey {
        project: catalog.root().to_owned(),
        asset,
    };
    let (mut app, ui_root) = crate::feathers::node_graph::geometry::tests::layout_app(1.25);
    app.insert_resource(catalog)
        .insert_resource(ActiveEditorContext {
            active_view: Some(EditorViewId(2)),
            active_document: None,
        });
    let cameras = [
        (Vec2::new(20.0, 40.0), 0.5),
        (Vec2::new(120.0, -60.0), 1.25),
        (Vec2::new(-300.0, 250.0), 0.75),
    ];
    let mut entities = Vec::new();
    for id in [1, 2, 3] {
        let camera = cameras[(id - 1) as usize];
        let viewport_key = if id == 3 {
            format!("{key}#tool")
        } else {
            format!("{key}#view:{id}")
        };
        app.world_mut()
            .resource_mut::<GraphViewportMemory>()
            .set_view(&viewport_key, camera.0, camera.1);
        app.world_mut()
            .commands()
            .entity(ui_root)
            .with_children(|parent| {
                let entity = spawn_graph_viewport(
                    parent,
                    GraphViewportProps {
                        key: viewport_key,
                        content_size: Vec2::splat(100.0),
                        selection_bounds: None,
                        initial_view: Some(camera),
                    },
                    (),
                    |_| {},
                    |_| {},
                );
                parent.commands().entity(entity).insert(GraphGeometryView {
                    key: GraphViewKey {
                        document: document.clone(),
                        view: (id != 3).then_some(EditorViewId(id)),
                    },
                    nodes: BTreeSet::new(),
                });
                entities.push(entity);
            });
    }
    app.world_mut().flush();
    for entity in &entities {
        app.world_mut().get_mut::<Node>(*entity).unwrap().width = Val::Px(480.0);
    }
    for _ in 0..4 {
        app.update();
    }
    app.world_mut()
        .run_system_once(mirror_graph_camera)
        .unwrap();
    assert_eq!(
        app.world().resource::<GraphViewportMemory>().view(&key),
        Some(cameras[1])
    );
    app.world_mut()
        .resource_mut::<ActiveEditorContext>()
        .active_view = Some(EditorViewId(1));
    for _ in 0..4 {
        app.update();
    }
    app.world_mut()
        .run_system_once(mirror_graph_camera)
        .unwrap();
    assert_eq!(
        app.world().resource::<GraphViewportMemory>().view(&key),
        Some(cameras[0])
    );
    app.world_mut()
        .get_mut::<Node>(entities[0])
        .unwrap()
        .display = Display::None;
    for _ in 0..4 {
        app.update();
    }
    app.world_mut()
        .run_system_once(mirror_graph_camera)
        .unwrap();
    let memory = app.world().resource::<GraphViewportMemory>();
    // The retained owner was hidden: stable fallback prefers the tool panel (None).
    assert_eq!(memory.view(&key), Some(cameras[2]));
    for (index, camera) in cameras[..2].iter().copied().enumerate() {
        assert_eq!(
            memory.view(&format!("{key}#view:{}", index + 1)),
            Some(camera)
        );
    }
    assert_eq!(memory.view(&format!("{key}#tool")), Some(cameras[2]));
    app.world_mut()
        .resource_mut::<ActiveEditorContext>()
        .active_view = Some(EditorViewId(2));
    // A live tool panel and split view must not alternate writes to the document slot when idle.
    #[derive(Resource, Default)]
    struct Dirtied(bool);
    app.init_resource::<Dirtied>().add_systems(
        Last,
        (
            mirror_graph_camera,
            |memory: Res<GraphViewportMemory>, mut dirty: ResMut<Dirtied>| {
                dirty.0 = memory.is_changed();
            },
        )
            .chain(),
    );
    app.update();
    app.update();
    assert_eq!(
        app.world().resource::<GraphViewportMemory>().view(&key),
        Some(cameras[1])
    );
    assert!(!app.world().resource::<Dirtied>().0);
    app.world_mut()
        .get_mut::<Node>(entities[1])
        .unwrap()
        .display = Display::None;
    for _ in 0..4 {
        app.update();
    }
    assert_eq!(
        app.world().resource::<GraphViewportMemory>().view(&key),
        Some(cameras[2])
    );
}

#[test]
fn material_and_function_disk_round_trip_keeps_base_not_effective_positions() {
    let root = tempfile::tempdir().unwrap();
    let function = function();
    let program = MaterialProgram::additive_sprite("Base placement");
    let function_before = function.to_pretty_ron().unwrap();
    let program_before = program.to_pretty_ron().unwrap();
    let function_key = function_graph_memory_key(root.path(), function.id);
    let program_key = material_graph_view_key(program.id);
    let expression = function.expressions[0].id;
    let program_expression = program.expressions[0].id;
    let mut memory = GraphViewportMemory::default();
    let base = Vec2::new(-80.0, 96.0);
    for (graph, node) in [
        (function_key.clone(), expression.to_string()),
        (function_key.clone(), OUTPUT_NODE.into()),
        (
            program_key.clone(),
            material_graph_expression_node_key(program_expression),
        ),
        (program_key.clone(), MATERIAL_GRAPH_OUTPUT_NODE_KEY.into()),
    ] {
        memory.set_node(&graph, &node, base, true);
        assert!(memory.set_temporary_offset(&graph, &node, Vec2::new(900.0, 400.0)));
        assert_eq!(
            memory.node_position(&graph, &node),
            Some(base + Vec2::new(900.0, 400.0))
        );
    }
    memory.set_view(&function_key, Vec2::new(24.0, -12.0), 1.25);
    let mut layout = ProjectEditorLayout::default();
    update(
        root.path(),
        &mut layout,
        std::slice::from_ref(&function),
        &memory,
    );
    update_material_graph_layout_document(
        &mut layout,
        std::slice::from_ref(&program),
        &memory,
        &MaterialGraphPreviewState::default(),
    );
    layout.save(root.path()).unwrap();
    let loaded = ProjectEditorLayout::load(root.path()).unwrap();
    let mut restored = GraphViewportMemory::default();
    restore(root.path(), &loaded, &mut restored);
    restore_material_graph_layouts(
        &loaded,
        &mut restored,
        &mut MaterialGraphPreviewState::default(),
    );
    assert_eq!(
        restored.node_position(&function_key, &expression.to_string()),
        Some(base)
    );
    assert_eq!(
        restored.node(&function_key, OUTPUT_NODE),
        Some((base, true))
    );
    assert_eq!(
        restored.node_position(
            &program_key,
            &material_graph_expression_node_key(program_expression)
        ),
        Some(base)
    );
    assert_eq!(
        restored.node(&program_key, MATERIAL_GRAPH_OUTPUT_NODE_KEY),
        Some((base, true))
    );
    assert_eq!(
        restored.view(&function_key),
        Some((Vec2::new(24.0, -12.0), 1.25))
    );
    assert_eq!(function.to_pretty_ron().unwrap(), function_before);
    assert_eq!(program.to_pretty_ron().unwrap(), program_before);

    // Renaming a signature label preserves expression identity/placement; removed IDs are pruned.
    let stale = MaterialExpressionId::new();
    layout
        .function_graphs
        .get_mut(&function.id)
        .unwrap()
        .nodes
        .insert(stale, MaterialGraphNodeLayout::default());
    let mut renamed = function;
    renamed.inputs[0].name = "Renamed input".into();
    update(root.path(), &mut layout, &[renamed], &restored);
    assert!(
        !layout
            .function_graphs
            .values()
            .next()
            .unwrap()
            .nodes
            .contains_key(&stale)
    );
    assert_eq!(
        layout.function_graphs.values().next().unwrap().nodes[&expression].position,
        base.to_array()
    );
}

#[test]
fn function_layout_restores_into_the_actual_canvas_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let function = function();
    function
        .save_ron(root.path().join("body.aestra.material-function.ron"))
        .unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let mut session = crate::test_support::session_with_timing_slack();
    session
        .open_material_function(&catalog, function.id)
        .unwrap();
    let effect_before = session.effect.clone();
    let key = function_graph_memory_key(catalog.root(), function.id);
    let mut memory = GraphViewportMemory::default();
    for expression in &function.expressions {
        memory.set_node(
            &key,
            expression.id.to_string(),
            Vec2::new(500.0, 180.0),
            true,
        );
    }
    memory.set_node(&key, OUTPUT_NODE, Vec2::new(780.0, 320.0), true);
    memory.set_view(&key, Vec2::new(30.0, 15.0), 0.75);
    let mut layout = ProjectEditorLayout::default();
    update(
        catalog.root(),
        &mut layout,
        std::slice::from_ref(&function),
        &memory,
    );
    layout.save(catalog.root()).unwrap();

    let (mut app, ui_root) = crate::feathers::node_graph::geometry::tests::layout_app(1.25);
    app.insert_resource(session)
        .insert_resource(catalog)
        .init_resource::<MaterialGraphLayoutPersistence>()
        .init_resource::<MaterialGraphPreviewState>();
    app.world_mut()
        .run_system_once(load_material_graph_layout)
        .unwrap();
    let host = app
        .world_mut()
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            ChildOf(ui_root),
        ))
        .id();
    app.world_mut()
        .run_system_once(
            move |mut commands: Commands,
                  session: Res<EditorSession>,
                  catalog: Res<ProjectEffectCatalog>,
                  memory: Res<GraphViewportMemory>,
                  assets: Res<AssetServer>| {
                commands.entity(host).with_children(|parent| {
                    crate::material_function_editor::spawn_graph(
                        parent,
                        &session,
                        &catalog,
                        &assets,
                        &memory,
                        &session.material_target,
                        Some(crate::docking::EditorViewId(3)),
                    )
                });
            },
        )
        .unwrap();
    for _ in 0..4 {
        app.update();
    }
    let world = app.world_mut();
    for (marker, node) in world
        .query::<(&GraphGeometryNode, &FeathersGraphNode)>()
        .iter(world)
    {
        assert_eq!(
            node.position(),
            if marker.key == GraphNodeKey::FunctionOutputs {
                Vec2::new(780.0, 320.0)
            } else {
                Vec2::new(500.0, 180.0)
            }
        );
    }
    let view = world
        .query::<&FeathersGraphViewport>()
        .single(world)
        .unwrap();
    assert_eq!(
        view.project_graph_point(Vec2::new(100.0, 100.0)),
        Vec2::new(105.0, 90.0)
    );
    assert_eq!(world.resource::<EditorSession>().effect, effect_before);
    // The editor save path must retain layouts when the function isn't the active target.
    world
        .resource_mut::<EditorSession>()
        .return_to_effect_material();
    world
        .run_system_once(persist_material_graph_layout)
        .unwrap();
    assert_eq!(
        world
            .resource::<MaterialGraphLayoutPersistence>()
            .document
            .function_graphs,
        layout.function_graphs
    );
}

#[test]
fn failed_layout_load_and_exit_flush_never_overwrite_the_original_file() {
    for source in [
        "not valid RON",
        "(format_version: 999, material_graphs: {})",
    ] {
        let root = tempfile::tempdir().unwrap();
        let path = aestra_project::project_editor_layout_path(root.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, source).unwrap();
        let mut app = App::new();
        app.insert_resource(ProjectEffectCatalog::scan(root.path()))
            .insert_resource(crate::test_support::session_with_timing_slack())
            .init_resource::<GraphViewportMemory>()
            .init_resource::<MaterialGraphPreviewState>()
            .init_resource::<MaterialGraphLayoutPersistence>()
            .init_resource::<Messages<AppExit>>();
        app.world_mut()
            .run_system_once(load_material_graph_layout)
            .unwrap();
        assert!(
            app.world()
                .resource::<MaterialGraphLayoutPersistence>()
                .write_blocked
        );
        {
            let mut persistence = app
                .world_mut()
                .resource_mut::<MaterialGraphLayoutPersistence>();
            persistence
                .document
                .function_graphs
                .entry(MaterialFunctionId::new())
                .or_default();
            persistence.changed_at = Some(Instant::now() - MATERIAL_GRAPH_LAYOUT_SAVE_DELAY);
        }
        app.world_mut()
            .run_system_once(persist_material_graph_layout)
            .unwrap();
        app.world_mut().write_message(AppExit::Success);
        app.world_mut()
            .run_system_once(flush_material_graph_layout_on_exit)
            .unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
        assert!(
            app.world()
                .resource::<MaterialGraphLayoutPersistence>()
                .last_error
                .is_some()
        );
    }
}

#[test]
fn external_newer_layout_and_project_root_changes_block_saves() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let mut app = App::new();
    app.insert_resource(ProjectEffectCatalog::scan(first.path()))
        .insert_resource(crate::test_support::session_with_timing_slack())
        .init_resource::<GraphViewportMemory>()
        .init_resource::<MaterialGraphPreviewState>()
        .init_resource::<MaterialGraphLayoutPersistence>()
        .init_resource::<Messages<AppExit>>();
    app.world_mut()
        .run_system_once(load_material_graph_layout)
        .unwrap();
    {
        let mut persistence = app
            .world_mut()
            .resource_mut::<MaterialGraphLayoutPersistence>();
        persistence
            .document
            .function_graphs
            .entry(MaterialFunctionId::new())
            .or_default();
        persistence.changed_at = Some(Instant::now() - MATERIAL_GRAPH_LAYOUT_SAVE_DELAY);
    }
    app.insert_resource(ProjectEffectCatalog::scan(second.path()));
    app.world_mut()
        .run_system_once(persist_material_graph_layout)
        .unwrap();
    app.world_mut().write_message(AppExit::Success);
    app.world_mut()
        .run_system_once(flush_material_graph_layout_on_exit)
        .unwrap();
    assert!(!aestra_project::project_editor_layout_path(first.path()).exists());
    assert!(!aestra_project::project_editor_layout_path(second.path()).exists());

    let path = aestra_project::project_editor_layout_path(first.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let source = "(format_version: 999, material_graphs: {})";
    std::fs::write(&path, source).unwrap();
    save_material_graph_layout(
        &mut app
            .world_mut()
            .resource_mut::<MaterialGraphLayoutPersistence>(),
    );
    assert!(
        app.world()
            .resource::<MaterialGraphLayoutPersistence>()
            .write_blocked
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), source);
}
