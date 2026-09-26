use super::*;
use bevy::ecs::system::RunSystemOnce;

fn app(root: &std::path::Path) -> App {
    let mut app = App::new();
    app.insert_resource(ProjectEffectCatalog::scan(root))
        .insert_resource(crate::test_support::session_with_timing_slack())
        .init_resource::<GraphViewportMemory>()
        .init_resource::<MaterialGraphPreviewState>()
        .init_resource::<GraphGeometryRegistry>()
        .init_resource::<MaterialGraphLayoutPersistence>()
        .add_systems(Update, reconcile)
        .add_systems(Last, persist_material_graph_layout);
    app
}

fn sources(root: &std::path::Path) -> (MaterialProgram, aestra_core::material::MaterialFunction) {
    let program = MaterialProgram::additive_sprite("Lifecycle");
    let function = aestra_core::material::MaterialFunction::from_ron(include_str!(
        "../../../../../assets/test/materials/dissolve_edge.aestra.material-function.ron"
    ))
    .unwrap();
    std::fs::write(
        root.join("test.aestra.material.ron"),
        program.to_pretty_ron().unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.join("test.aestra.material-function.ron"),
        function.to_pretty_ron().unwrap(),
    )
    .unwrap();
    (program, function)
}

#[test]
fn project_switch_round_trip_isolates_same_ids_previews_cameras_and_offsets() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let (program, function) = sources(first.path());
    std::fs::write(
        second.path().join("test.aestra.material.ron"),
        program.to_pretty_ron().unwrap(),
    )
    .unwrap();
    std::fs::write(
        second.path().join("test.aestra.material-function.ron"),
        function.to_pretty_ron().unwrap(),
    )
    .unwrap();
    let key = material_graph_view_key(program.id);
    let node = material_graph_expression_node_key(program.expressions[0].id);
    let mut app = app(first.path());
    app.update();
    let first_root = app
        .world()
        .resource::<ProjectEffectCatalog>()
        .root()
        .to_owned();
    let function_key = function_graph_memory_key(&first_root, function.id);
    {
        let mut memory = app.world_mut().resource_mut::<GraphViewportMemory>();
        memory.set_node(&key, &node, Vec2::new(300.0, 150.0), true);
        memory.set_temporary_offset(&key, &node, Vec2::splat(50.0));
        memory.set_view(format!("{key}#view:1"), Vec2::splat(20.0), 1.5);
        memory.set_node(&function_key, "outputs", Vec2::splat(700.0), false);
        memory.set_node("unrelated-widget", "node", Vec2::ONE, false);
    }
    app.world_mut()
        .resource_mut::<MaterialGraphPreviewState>()
        .toggle(program.id, MaterialGraphPreviewTarget::Output);
    app.update();
    // The material is not focused, yet its authored placement was captured.
    assert_eq!(
        app.world()
            .resource::<MaterialGraphLayoutPersistence>()
            .document
            .material_graphs[&program.id]
            .nodes[&program.expressions[0].id]
            .position,
        [300.0, 150.0]
    );
    let stale = app
        .world_mut()
        .spawn(GraphGeometryView {
            key: GraphViewKey {
                document: GraphDocumentKey {
                    project: first_root.clone(),
                    asset: DocumentKey::MaterialProgram(program.id),
                },
                view: None,
            },
            nodes: BTreeSet::new(),
        })
        .id();
    app.insert_resource(ProjectEffectCatalog::scan(second.path()));
    app.update();
    assert!(app.world().get_entity(stale).is_err());
    let memory = app.world().resource::<GraphViewportMemory>();
    assert!(memory.node(&key, &node).is_none());
    assert!(memory.view(&format!("{key}#view:1")).is_none());
    assert!(memory.node(&function_key, "outputs").is_none());
    assert_eq!(
        memory.node_position("unrelated-widget", "node"),
        Some(Vec2::ONE)
    );
    assert!(
        !app.world()
            .resource::<MaterialGraphPreviewState>()
            .output_visible(program.id)
    );
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node(&key, &node, Vec2::splat(900.0), false);
    app.update();
    app.insert_resource(ProjectEffectCatalog::scan(first.path()));
    app.update();
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node_position(&key, &node),
        Some(Vec2::new(300.0, 150.0))
    );
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node_position(&function_key, "outputs"),
        Some(Vec2::splat(700.0))
    );
    assert!(
        app.world()
            .resource::<MaterialGraphPreviewState>()
            .output_visible(program.id)
    );
    assert_eq!(
        ProjectEditorLayout::load(second.path())
            .unwrap()
            .material_graphs[&program.id]
            .nodes[&program.expressions[0].id]
            .position,
        [900.0, 900.0]
    );
}

#[test]
fn reload_prunes_removed_nodes_but_preserves_base_placement_and_semantics() {
    let root = tempfile::tempdir().unwrap();
    let (mut program, mut function) = sources(root.path());
    // Remove disconnected constants, keeping both source documents semantically valid.
    let mut unused = program.expressions[0].clone();
    unused.id = MaterialExpressionId::new();
    unused.kind = MaterialExpressionKind::Constant(MaterialValue::Float(0.5));
    program.node_constants.push(unused.id);
    program.expressions.push(unused.clone());
    unused.id = MaterialExpressionId::new();
    function.expressions.push(unused);
    std::fs::write(
        root.path().join("test.aestra.material.ron"),
        program.to_pretty_ron().unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.path().join("test.aestra.material-function.ron"),
        function.to_pretty_ron().unwrap(),
    )
    .unwrap();
    let mut app = app(root.path());
    app.update();
    let key = material_graph_view_key(program.id);
    let function_key = function_graph_memory_key(
        app.world().resource::<ProjectEffectCatalog>().root(),
        function.id,
    );
    let removed = program.expressions.last().unwrap().id;
    let kept = program.expressions[0].id;
    let function_removed = function.expressions.last().unwrap().id;
    {
        let mut memory = app.world_mut().resource_mut::<GraphViewportMemory>();
        for expression in &program.expressions {
            memory.set_node(
                &key,
                material_graph_expression_node_key(expression.id),
                Vec2::splat(30.0),
                false,
            );
        }
        memory.set_temporary_offset(
            &key,
            &material_graph_expression_node_key(kept),
            Vec2::splat(100.0),
        );
        memory.set_node(
            &function_key,
            function_removed.to_string(),
            Vec2::splat(70.0),
            false,
        );
    }
    app.world_mut()
        .resource_mut::<MaterialGraphPreviewState>()
        .toggle(program.id, MaterialGraphPreviewTarget::Expression(removed));
    app.update();
    program.expressions.pop();
    program.node_constants.retain(|id| *id != removed);
    function.expressions.pop();
    std::fs::write(
        root.path().join("test.aestra.material.ron"),
        program.to_pretty_ron().unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.path().join("test.aestra.material-function.ron"),
        function.to_pretty_ron().unwrap(),
    )
    .unwrap();
    // Simulate a published reload; the semantic source is left exactly as loaded.
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .refresh();
    assert_eq!(
        app.world()
            .resource::<ProjectEffectCatalog>()
            .material_program(program.id)
            .unwrap(),
        program.normalized()
    );
    let effect = app.world().resource::<EditorSession>().effect.clone();
    app.update();
    let memory = app.world().resource::<GraphViewportMemory>();
    assert!(
        memory
            .node(&key, &material_graph_expression_node_key(removed))
            .is_none()
    );
    assert!(
        memory
            .node(&function_key, &function_removed.to_string())
            .is_none()
    );
    assert_eq!(
        memory.node_position(&key, &material_graph_expression_node_key(kept)),
        Some(Vec2::splat(130.0))
    );
    assert!(
        !app.world()
            .resource::<MaterialGraphPreviewState>()
            .is_visible(program.id, MaterialGraphPreviewTarget::Expression(removed))
    );
    assert_eq!(app.world().resource::<EditorSession>().effect, effect);
}

#[test]
fn adding_a_node_keeps_surviving_display_positions_and_camera() {
    let root = tempfile::tempdir().unwrap();
    let (program, _) = sources(root.path());
    let mut app = app(root.path());
    app.update();
    let key = material_graph_view_key(program.id);
    let node = material_graph_expression_node_key(program.outputs.color);
    {
        let mut memory = app.world_mut().resource_mut::<GraphViewportMemory>();
        memory.set_node(&key, &node, Vec2::new(30.0, 60.0), false);
        memory.set_temporary_offset(&key, &node, Vec2::new(100.0, 50.0));
        memory.set_view(&key, Vec2::new(-240.0, 70.0), 0.5);
    }
    let mut edited = program.clone();
    edited
        .expressions
        .push(aestra_core::material::MaterialExpression {
            id: MaterialExpressionId::from_u128(123),
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        });
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .replace_material_program(&program, &edited)
        .unwrap();
    app.update();
    let memory = app.world().resource::<GraphViewportMemory>();
    assert_eq!(
        memory.node_position(&key, &node),
        Some(Vec2::new(130.0, 110.0))
    );
    assert_eq!(memory.node(&key, &node).unwrap().0, Vec2::new(30.0, 60.0));
    assert_eq!(memory.view(&key), Some((Vec2::new(-240.0, 70.0), 0.5)));
}

#[test]
fn corrupt_inventory_retains_layout_and_confirmed_removal_cleans_it_up() {
    let root = tempfile::tempdir().unwrap();
    let (program, _) = sources(root.path());
    let mut app = app(root.path());
    app.update();
    let key = material_graph_view_key(program.id);
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node(&key, "output", Vec2::ONE, false);
    app.update();
    let source = root.path().join("test.aestra.material.ron");
    std::fs::write(&source, "invalid RON").unwrap();
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .refresh();
    app.update();
    assert!(
        app.world()
            .resource::<MaterialGraphLayoutPersistence>()
            .document
            .material_graphs
            .contains_key(&program.id)
    );
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node_position(&key, "output"),
        Some(Vec2::ONE)
    );
    std::fs::remove_file(source).unwrap();
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .refresh();
    app.update();
    assert!(
        !app.world()
            .resource::<MaterialGraphLayoutPersistence>()
            .document
            .material_graphs
            .contains_key(&program.id)
    );
    assert!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node(&key, "output")
            .is_none()
    );
}

#[test]
fn blocked_layout_retry_preserves_file_until_repaired_then_reloads_without_semantic_edits() {
    let root = tempfile::tempdir().unwrap();
    let (program, _) = sources(root.path());
    let path = aestra_project::project_editor_layout_path(root.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let unsupported = "(format_version: 999, material_graphs: {})";
    std::fs::write(&path, unsupported).unwrap();
    let mut app = app(root.path());
    app.add_observer(retry_layout);
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        bevy::scene::ScenePlugin,
        bevy::text::TextPlugin,
    ));
    app.insert_resource(Localizer::new("en-US").unwrap());
    app.update();
    let root_key = app
        .world()
        .resource::<ProjectEffectCatalog>()
        .root()
        .to_owned();
    app.world_mut().spawn(GraphGeometryView {
        key: GraphViewKey {
            document: GraphDocumentKey {
                project: root_key,
                asset: DocumentKey::MaterialProgram(program.id),
            },
            view: None,
        },
        nodes: BTreeSet::new(),
    });
    app.world_mut().run_system_once(sync_notice).unwrap();
    let button = app
        .world_mut()
        .query_filtered::<Entity, With<RetryLayout>>()
        .single(app.world())
        .unwrap();
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<LayoutNotice>>()
            .iter(app.world())
            .count(),
        1
    );
    let graph = material_graph_view_key(program.id);
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node(&graph, "output", Vec2::splat(400.0), false);
    app.world_mut().trigger(Activate { entity: button });
    app.update();
    assert!(
        app.world()
            .resource::<MaterialGraphLayoutPersistence>()
            .write_blocked
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), unsupported);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node_position(&graph, "output"),
        Some(Vec2::splat(400.0))
    );
    let mut repaired = ProjectEditorLayout::default();
    repaired
        .material_graphs
        .entry(program.id)
        .or_default()
        .output = Some(MaterialGraphNodeLayout {
        position: [10.0, 20.0],
        collapsed: true,
        pinned: false,
    });
    repaired.save(root.path()).unwrap();
    let effect_before = app.world().resource::<EditorSession>().effect.clone();
    let source_before = std::fs::read(root.path().join("test.aestra.material.ron")).unwrap();
    app.world_mut().trigger(Activate { entity: button });
    app.update();
    assert!(
        !app.world()
            .resource::<MaterialGraphLayoutPersistence>()
            .write_blocked
    );
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node(&graph, "output"),
        Some((Vec2::new(10.0, 20.0), true))
    );
    assert_eq!(
        app.world().resource::<EditorSession>().effect,
        effect_before
    );
    assert_eq!(
        std::fs::read(root.path().join("test.aestra.material.ron")).unwrap(),
        source_before
    );
}

#[test]
fn idle_reconcile_and_camera_mirror_do_not_dirty_placement_memory() {
    let root = tempfile::tempdir().unwrap();
    sources(root.path());
    let mut app = app(root.path());
    #[derive(Resource, Default)]
    struct Dirtied(bool);
    app.init_resource::<Dirtied>().add_systems(
        Last,
        (
            function_layout::mirror_graph_camera,
            |memory: Res<GraphViewportMemory>, mut dirty: ResMut<Dirtied>| {
                dirty.0 = memory.is_changed();
            },
        )
            .chain(),
    );
    app.update();
    app.update();
    assert!(!app.world().resource::<Dirtied>().0);
    // Explicit loading is available to startup/recovery; serialization is never semantic editing.
    app.world_mut()
        .run_system_once(load_material_graph_layout)
        .unwrap();
    app.update();
    app.update();
    assert!(!app.world().resource::<Dirtied>().0);
}
