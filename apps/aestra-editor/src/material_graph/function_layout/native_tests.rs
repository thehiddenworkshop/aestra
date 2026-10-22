//! Full function-editor composition stays in the shipping suite until that panel is ported.
use super::function_layout::update;
use super::layout_persistence::*;
use super::*;
use bevy::ecs::system::RunSystemOnce;
const OUTPUT_NODE: &str = "outputs";
fn function() -> aestra_core::material::MaterialFunction {
    aestra_core::material::MaterialFunction::from_ron(include_str!(
        "../../../../../assets/test/materials/dissolve_edge.aestra.material-function.ron"
    ))
    .unwrap()
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
                        &crate::material_graph::MaterialGraphSelectionState::default(),
                        &crate::material_function_editor::graph::FunctionGraphMenuState::default(),
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
