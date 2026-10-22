//! Production host registration, identity lifecycle and scheduling contracts.
use super::*;
use crate::{
    ProjectEffectCatalog,
    docking::EditorViewId,
    feathers::node_graph::{GraphViewportMemory, GraphWireMaterial},
    material_document::MaterialEditingTarget,
    material_function_editor::graph::FunctionGraphMenuState,
    material_graph::sockets::{MaterialGraphSocketKind, MaterialGraphWire},
};
use aestra_core::{MaterialExpressionId, MaterialFunctionId, MaterialProgramId};

fn app() -> App {
    let mut app = App::new();
    app.insert_resource(crate::test_support::session_with_timing_slack())
        .insert_resource(ProjectEffectCatalog::from_entries(Vec::new()))
        .init_resource::<Assets<GraphWireMaterial>>()
        .init_resource::<GraphViewportMemory>();
    register(&mut app);
    app
}
fn seed(world: &mut World) {
    let mut palette = MaterialGraphPaletteState::blocking_states_for_test().remove(0);
    palette.query = "live search".into();
    world.insert_resource(palette);
    world.insert_resource(FunctionGraphMenuState::blocking_states_for_test().remove(0));
    let mut selection = world.resource_mut::<MaterialGraphSelectionState>();
    selection.select_expression(
        Some(EditorViewId(1)),
        MaterialProgramId::from_u128(1),
        MaterialExpressionId::from_u128(1),
        false,
        false,
    );
    selection.select_function_expression(
        Some(EditorViewId(2)),
        MaterialFunctionId::from_u128(2),
        MaterialExpressionId::from_u128(2),
        false,
        false,
    );
    world.insert_resource(MaterialGraphGesture::Connecting {
        program: MaterialProgramId::from_u128(1),
        viewport: Entity::PLACEHOLDER,
        origin: MaterialGraphSocketKind::ExpressionOutput(MaterialExpressionId::from_u128(1)),
        cursor: Vec2::ONE,
        snap: None,
    });
}
fn assert_state(world: &World, open: bool) {
    assert_eq!(
        world.resource::<MaterialGraphPaletteState>().is_open(),
        open
    );
    assert_eq!(world.resource::<FunctionGraphMenuState>().is_open(), open);
    assert_eq!(
        !matches!(
            world.resource::<MaterialGraphGesture>(),
            MaterialGraphGesture::Idle
        ),
        open
    );
    let selection = world.resource::<MaterialGraphSelectionState>();
    assert_eq!(
        selection.is_expression_selected(
            Some(EditorViewId(1)),
            MaterialProgramId::from_u128(1),
            MaterialExpressionId::from_u128(1)
        ),
        open
    );
    assert_eq!(
        selection.is_function_expression_selected(
            Some(EditorViewId(2)),
            MaterialFunctionId::from_u128(2),
            MaterialExpressionId::from_u128(2)
        ),
        open
    );
}

#[test]
fn first_observation_clears_all_graph_transients_without_touching_authored_work() {
    let mut app = app();
    let effect = app.world().resource::<EditorSession>().effect.clone();
    seed(app.world_mut());
    app.update();
    assert_state(app.world(), false);
    assert_eq!(app.world().resource::<EditorSession>().effect, effect);
    assert_eq!(
        app.world()
            .resource::<EditorSession>()
            .operation_order
            .edit_serial(),
        0
    );
}

#[test]
fn unchanged_identity_preserves_live_graph_state_and_idle_resource_ticks() {
    let mut app = app();
    app.update();
    seed(app.world_mut());
    app.update();
    assert_state(app.world(), true);
    let palette = app
        .world()
        .get_resource_ref::<MaterialGraphPaletteState>()
        .unwrap()
        .last_changed();
    let selection = app
        .world()
        .get_resource_ref::<MaterialGraphSelectionState>()
        .unwrap()
        .last_changed();
    let menus = app
        .world()
        .get_resource_ref::<FunctionGraphMenuState>()
        .unwrap()
        .last_changed();
    app.update();
    assert_eq!(
        app.world()
            .get_resource_ref::<MaterialGraphPaletteState>()
            .unwrap()
            .last_changed(),
        palette
    );
    assert_eq!(
        app.world()
            .get_resource_ref::<MaterialGraphSelectionState>()
            .unwrap()
            .last_changed(),
        selection
    );
    assert_eq!(
        app.world()
            .get_resource_ref::<FunctionGraphMenuState>()
            .unwrap()
            .last_changed(),
        menus
    );
    assert_state(app.world(), true);
}

#[test]
fn unrelated_session_updates_do_not_close_live_graph_controls() {
    let mut app = app();
    app.update();
    seed(app.world_mut());
    {
        let mut session = app.world_mut().resource_mut::<EditorSession>();
        session.ui_revision += 1;
        session.status = "Preview updated".into();
        session.playing = !session.playing;
    }
    app.update();
    assert_state(app.world(), true);
}

#[test]
fn program_function_and_project_identity_changes_reset_every_scope() {
    let mut app = app();
    app.update();
    let program = MaterialProgramId::from_u128(1);
    let function = MaterialFunctionId::from_u128(2);
    for target in [
        MaterialEditingTarget::Program {
            root: "one".into(),
            id: program,
        },
        MaterialEditingTarget::Program {
            root: "two".into(),
            id: program,
        },
        MaterialEditingTarget::Program {
            root: "two".into(),
            id: MaterialProgramId::from_u128(3),
        },
        MaterialEditingTarget::Function {
            root: "two".into(),
            id: function,
        },
        MaterialEditingTarget::Function {
            root: "one".into(),
            id: function,
        },
        MaterialEditingTarget::EffectInstance,
    ] {
        seed(app.world_mut());
        app.world_mut()
            .resource_mut::<EditorSession>()
            .material_target = target;
        app.update();
        assert_state(app.world(), false);
    }
}

#[test]
fn replacing_effect_resets_transients_even_when_material_target_is_unchanged() {
    let mut app = app();
    app.update();
    seed(app.world_mut());
    let old = app.world().resource::<EditorSession>().history_generation();
    app.world_mut().resource_mut::<EditorSession>().new_effect();
    assert_ne!(
        app.world().resource::<EditorSession>().history_generation(),
        old
    );
    assert_eq!(
        app.world().resource::<EditorSession>().material_target,
        MaterialEditingTarget::EffectInstance
    );
    app.update();
    assert_state(app.world(), false);
}

#[test]
fn explicit_clear_is_safe_with_absent_or_partially_removed_resources() {
    let mut world = World::new();
    clear_document_transients(&mut world);
    assert!(!world.contains_resource::<MaterialGraphPaletteState>());
    world.insert_resource(MaterialGraphPaletteState::blocking_states_for_test().remove(0));
    clear_document_transients(&mut world);
    assert!(!world.resource::<MaterialGraphPaletteState>().is_open());
    assert!(!world.contains_resource::<MaterialGraphSelectionState>());
    assert!(!world.contains_resource::<FunctionGraphMenuState>());
}

#[test]
fn explicit_clear_preserves_graph_placement_cameras_and_session_history() {
    let mut app = app();
    seed(app.world_mut());
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node("graph", "n", Vec2::new(12.0, 34.0), true);
    let revision = app.world().resource::<EditorSession>().ui_revision;
    clear_document_transients(app.world_mut());
    assert_state(app.world(), false);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node("graph", "n"),
        Some((Vec2::new(12.0, 34.0), true))
    );
    assert_eq!(
        app.world().resource::<EditorSession>().ui_revision,
        revision
    );
    assert_eq!(
        app.world()
            .resource::<EditorSession>()
            .operation_order
            .edit_serial(),
        0
    );
}

#[derive(Resource, Default)]
struct Stages(Vec<&'static str>);
#[derive(Resource)]
struct PendingTarget(Option<MaterialEditingTarget>);

#[test]
fn identity_cleanup_observes_this_frames_target_before_rebuild_and_graph_sync() {
    let mut app = app();
    app.update();
    seed(app.world_mut());
    app.configure_sets(
        Update,
        (
            EditorSet::PreViewport,
            EditorSet::MainUpdate,
            EditorSet::UiRebuild,
            EditorSet::UiSync,
        )
            .chain(),
    )
    .insert_resource(Stages::default())
    .insert_resource(PendingTarget(Some(MaterialEditingTarget::Program {
        root: "new-project".into(),
        id: MaterialProgramId::from_u128(10),
    })))
    .add_systems(
        Startup,
        (|mut stages: ResMut<Stages>| stages.0.push("setup")).in_set(EditorSet::Setup),
    )
    .add_systems(
        Update,
        (|mut session: ResMut<EditorSession>,
          mut stages: ResMut<Stages>,
          mut pending: ResMut<PendingTarget>| {
            if let Some(target) = pending.0.take() {
                session.material_target = target;
            }
            stages.0.push("identity");
        })
        .in_set(EditorSet::MainUpdate),
    )
    .add_systems(
        Update,
        (|palette: Res<MaterialGraphPaletteState>,
          menus: Res<FunctionGraphMenuState>,
          mut stages: ResMut<Stages>| {
            assert!(!palette.is_open() && !menus.is_open());
            stages.0.push("rebuild");
        })
        .in_set(EditorSet::UiRebuild),
    )
    .add_systems(
        Update,
        (|mut stages: ResMut<Stages>| stages.0.push("graph")).in_set(GraphWidgetSync),
    );
    app.update();
    assert_state(app.world(), false);
    assert_eq!(
        app.world().resource::<Stages>().0,
        ["identity", "rebuild", "graph"]
    );
}

#[test]
fn queued_project_cleanup_is_flushed_before_rebuild_without_making_a_semantic_edit() {
    let mut app = app();
    app.update();
    seed(app.world_mut());
    app.add_systems(
        Update,
        (|mut commands: Commands| commands.queue(clear_document_transients))
            .before(EditorSet::UiRebuild),
    )
    .add_systems(
        Update,
        (|selection: Res<MaterialGraphSelectionState>| {
            assert!(!selection.is_expression_selected(
                Some(EditorViewId(1)),
                MaterialProgramId::from_u128(1),
                MaterialExpressionId::from_u128(1)
            ));
        })
        .in_set(EditorSet::UiRebuild),
    );
    app.update();
    assert_state(app.world(), false);
    assert_eq!(
        app.world()
            .resource::<EditorSession>()
            .operation_order
            .edit_serial(),
        0
    );
}

#[test]
fn wires_attach_after_transform_and_layout_then_flush_before_final_readback() {
    let mut app = app();
    let wire = app
        .world_mut()
        .spawn(MaterialGraphWire {
            program: MaterialProgramId::from_u128(1),
            source: MaterialExpressionId::from_u128(1),
            target: aestra_authoring::MaterialConnectionTarget::ProgramOutput(
                aestra_authoring::MaterialOutputSocket::Alpha,
            ),
            color: Vec4::ONE,
        })
        .id();
    app.insert_resource(Stages::default())
        .add_systems(
            PostUpdate,
            (|mut stages: ResMut<Stages>, mut wires: Query<&mut MaterialGraphWire>| {
                stages.0.push("transform");
                for mut wire in &mut wires {
                    wire.color.x = 0.25;
                }
            })
            .in_set(bevy::transform::TransformSystems::Propagate),
        )
        .add_systems(
            PostUpdate,
            (|mut stages: ResMut<Stages>, mut wires: Query<&mut MaterialGraphWire>| {
                stages.0.push("layout");
                for mut wire in &mut wires {
                    wire.color.y = 0.5;
                }
            })
            .in_set(bevy::ui::UiSystems::PostLayout),
        )
        .add_systems(
            Last,
            move |wires: Query<&MaterialNode<GraphWireMaterial>>, stages: Res<Stages>| {
                assert!(wires.contains(wire));
                assert!(stages.0.contains(&"transform") && stages.0.contains(&"layout"));
            },
        );
    app.update();
    assert_eq!(app.world().resource::<Assets<GraphWireMaterial>>().len(), 1);
    let handle = &app
        .world()
        .get::<MaterialNode<GraphWireMaterial>>(wire)
        .unwrap()
        .0;
    assert_eq!(
        app.world()
            .resource::<Assets<GraphWireMaterial>>()
            .get(handle)
            .unwrap()
            .color,
        Vec4::new(0.25, 0.5, 1.0, 1.0)
    );
    app.update();
    assert_eq!(app.world().resource::<Assets<GraphWireMaterial>>().len(), 1);
}
