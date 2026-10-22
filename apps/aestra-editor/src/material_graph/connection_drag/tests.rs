//! Actual connection drag lifecycle, planner snapping, material assets and semantic history.
use super::*;
use crate::{
    docking::EditorViewId,
    feathers::node_graph::GraphViewportMemory,
    history::{HistoryAction, routing::Routing},
    material_document::MaterialEditingTarget,
    material_function_editor::FunctionEditor,
    material_graph::{
        MaterialGraphPreviewState, MaterialSelectionScope, click_input,
        drag_input::tests as pointer, keys::material_graph_expression_node_key,
    },
};
use aestra_authoring::{MaterialExpressionInput, MaterialOutputSocket};
use aestra_core::material::{
    MaterialExpression, MaterialExpressionKind, MaterialProgram, MaterialValue,
};
use bevy::math::Affine2;

#[derive(Resource, Default)]
struct Bubbled(usize);
struct Fixture {
    root: tempfile::TempDir,
    app: App,
    original: MaterialProgram,
    source: MaterialExpressionId,
    multiply: MaterialExpressionId,
    window: Entity,
    parent: Entity,
    viewport: Entity,
    layer: Entity,
    output: Entity,
    input: Entity,
    scope: MaterialSelectionScope,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut original = MaterialProgram::additive_sprite("Connection drag").normalized();
        let source = MaterialExpressionId::new();
        let multiply = MaterialExpressionId::new();
        original.expressions.extend([
            MaterialExpression {
                id: source,
                kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.75)),
            },
            MaterialExpression {
                id: multiply,
                kind: MaterialExpressionKind::Multiply(source, source),
            },
        ]);
        original.node_constants.push(source);
        original = original.normalized();
        original
            .save_ron(root.path().join("connection.aestra.material.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
        session
            .open_material_program(&catalog, original.id)
            .unwrap();
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(catalog)
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<FunctionEditor>()
            .init_resource::<GraphViewportMemory>()
            .init_resource::<MaterialGraphSelectionState>()
            .init_resource::<MaterialGraphPreviewState>()
            .init_resource::<MaterialGraphPaletteState>()
            .init_resource::<MaterialGraphGesture>()
            .init_resource::<Assets<GraphWireMaterial>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Bubbled>()
            .add_observer(
                |event: On<HistoryAction>, routing: Routing, mut commands: Commands| {
                    routing.execute(*event, &mut commands, |_, _, _, _| {
                        panic!("No filesystem history")
                    });
                },
            );
        crate::material_graph::host_integration::register(&mut app);
        // Run the initial document-identity observation before synthesizing user input.
        app.update();
        let window = app.world_mut().spawn(Window::default()).id();
        let parent = app
            .world_mut()
            .spawn_empty()
            .observe(|_: On<PointerDragStart>, mut b: ResMut<Bubbled>| b.0 += 1)
            .observe(|_: On<PointerDrag>, mut b: ResMut<Bubbled>| b.0 += 1)
            .observe(|_: On<PointerDragEnd>, mut b: ResMut<Bubbled>| b.0 += 1)
            .observe(|_: On<PointerClick>, mut b: ResMut<Bubbled>| b.0 += 1)
            .id();
        let scope = Some(EditorViewId(7));
        let viewport = app
            .world_mut()
            .spawn((
                ChildOf(parent),
                MaterialGraphViewport {
                    program: original.id,
                    scope,
                    editing_target: MaterialEditingTarget::Program {
                        root: root.path().into(),
                        id: original.id,
                    },
                },
                FeathersGraphViewport::clipboard_fixture(
                    material_graph_view_key(original.id),
                    Vec2::new(40.0, 20.0),
                    2.0,
                ),
                ComputedNode {
                    size: Vec2::new(800.0, 600.0),
                    ..default()
                },
                UiGlobalTransform::from(Affine2::from_translation(Vec2::new(400.0, 300.0))),
            ))
            .id();
        let layer = app
            .world_mut()
            .spawn((FeathersGraphWireLayer { viewport }, ChildOf(viewport)))
            .id();
        let node_a = app
            .world_mut()
            .spawn((
                ChildOf(viewport),
                FeathersGraphNode::clipboard_fixture(
                    material_graph_view_key(original.id),
                    material_graph_expression_node_key(source),
                    Vec2::ZERO,
                ),
                ComputedNode {
                    size: Vec2::new(224.0, 80.0),
                    ..default()
                },
                UiGlobalTransform::default(),
            ))
            .id();
        let node_b = app
            .world_mut()
            .spawn((
                ChildOf(viewport),
                FeathersGraphNode::clipboard_fixture(
                    material_graph_view_key(original.id),
                    "output".into(),
                    Vec2::new(100.0, 0.0),
                ),
                ComputedNode {
                    size: Vec2::new(224.0, 80.0),
                    ..default()
                },
                UiGlobalTransform::default(),
            ))
            .id();
        let output = app
            .world_mut()
            .spawn((
                ChildOf(node_a),
                MaterialGraphSocket {
                    program: original.id,
                    kind: MaterialGraphSocketKind::ExpressionOutput(source),
                },
                MaterialGraphSocketAnchor {
                    node: node_a,
                    offset: Some(Vec2::ZERO),
                },
                ComputedNode {
                    size: Vec2::splat(20.0),
                    ..default()
                },
                UiGlobalTransform::from(Affine2::from_translation(Vec2::new(40.0, 20.0))),
            ))
            .id();
        let input = app
            .world_mut()
            .spawn((
                ChildOf(node_b),
                MaterialGraphSocket {
                    program: original.id,
                    kind: Self::target_kind(),
                },
                MaterialGraphSocketAnchor {
                    node: node_b,
                    offset: Some(Vec2::ZERO),
                },
                ComputedNode {
                    size: Vec2::splat(20.0),
                    ..default()
                },
                UiGlobalTransform::from(Affine2::from_translation(Vec2::new(240.0, 20.0))),
            ))
            .id();
        Self {
            root,
            app,
            original,
            source,
            multiply,
            window,
            parent,
            viewport,
            layer,
            output,
            input,
            scope,
        }
    }
    fn second_view(&mut self) -> (Entity, Entity, Entity) {
        let mut marker = self
            .app
            .world()
            .get::<MaterialGraphViewport>(self.viewport)
            .unwrap()
            .clone();
        marker.scope = Some(EditorViewId(88));
        let view = self
            .app
            .world_mut()
            .spawn((
                ChildOf(self.parent),
                marker,
                FeathersGraphViewport::clipboard_fixture(
                    material_graph_view_key(self.original.id),
                    Vec2::new(100.0, 50.0),
                    0.5,
                ),
                ComputedNode {
                    size: Vec2::new(800.0, 600.0),
                    ..default()
                },
                UiGlobalTransform::from(Affine2::from_translation(Vec2::new(1200.0, 300.0))),
            ))
            .id();
        let layer = self
            .app
            .world_mut()
            .spawn((ChildOf(view), FeathersGraphWireLayer { viewport: view }))
            .id();
        let node = self
            .app
            .world_mut()
            .spawn((
                ChildOf(view),
                FeathersGraphNode::clipboard_fixture(
                    material_graph_view_key(self.original.id),
                    material_graph_expression_node_key(self.source),
                    Vec2::ZERO,
                ),
                ComputedNode {
                    size: Vec2::new(224.0, 80.0),
                    ..default()
                },
                UiGlobalTransform::default(),
            ))
            .id();
        let socket = self
            .app
            .world_mut()
            .spawn((
                ChildOf(node),
                MaterialGraphSocket {
                    program: self.original.id,
                    kind: MaterialGraphSocketKind::ExpressionOutput(self.source),
                },
                MaterialGraphSocketAnchor {
                    node,
                    offset: Some(Vec2::ZERO),
                },
                ComputedNode {
                    size: Vec2::splat(20.0),
                    ..default()
                },
                UiGlobalTransform::from(Affine2::from_translation(Vec2::new(900.0, 50.0))),
            ))
            .id();
        (view, layer, socket)
    }
    fn target_kind() -> MaterialGraphSocketKind {
        MaterialGraphSocketKind::ConnectionInput(MaterialConnectionTarget::ProgramOutput(
            MaterialOutputSocket::Alpha,
        ))
    }
    fn start(&mut self, socket: Entity, pos: Vec2) {
        self.app.world_mut().trigger(pointer::start(
            socket,
            self.window,
            pos,
            PointerButton::Primary,
        ));
        self.app.world_mut().flush();
    }
    fn motion(&mut self, socket: Entity, pos: Vec2) {
        self.app.world_mut().trigger(pointer::motion(
            socket,
            self.window,
            pos,
            PointerButton::Primary,
        ));
        self.app.world_mut().flush();
    }
    fn end(&mut self, socket: Entity, pos: Vec2) {
        self.app.world_mut().trigger(pointer::end(
            socket,
            self.window,
            pos,
            PointerButton::Primary,
        ));
        self.app.world_mut().flush();
    }
    fn snap(&self) -> Option<MaterialGraphSocketKind> {
        match self.app.world().resource::<MaterialGraphGesture>() {
            MaterialGraphGesture::Connecting { snap, .. } => *snap,
            _ => None,
        }
    }
    fn ghosts(&mut self) -> Vec<(Entity, Handle<GraphWireMaterial>)> {
        let world = self.app.world_mut();
        let mut q=world.query_filtered::<(Entity,&MaterialNode<GraphWireMaterial>),With<MaterialGraphGhostWire>>();
        q.iter(world).map(|(e, m)| (e, m.0.clone())).collect()
    }
    fn current(&self) -> MaterialProgram {
        self.app
            .world()
            .resource::<EditorSession>()
            .graph_material_programs(self.app.world().resource::<ProjectEffectCatalog>())
            .unwrap()
            .into_iter()
            .find(|p| p.id == self.original.id)
            .unwrap()
    }
    fn unchanged(&self) {
        assert_eq!(self.current(), self.original);
        assert_eq!(
            self.app
                .world()
                .resource::<EditorSession>()
                .operation_order
                .edit_serial(),
            0
        );
    }
    fn disk_unchanged(&self) {
        assert_eq!(
            MaterialProgram::load_ron(self.root.path().join("connection.aestra.material.ron"))
                .unwrap(),
            self.original
        );
    }
    fn step(&mut self, event: HistoryAction) {
        self.app.world_mut().trigger(event);
        self.app.world_mut().flush();
    }
}
#[test]
fn start_owns_containing_view_and_spawns_real_nonpickable_ghost() {
    let mut f = Fixture::new();
    f.start(f.output, Vec2::new(40.0, 20.0));
    match f.app.world().resource::<MaterialGraphGesture>() {
        MaterialGraphGesture::Connecting {
            program,
            viewport,
            origin,
            cursor,
            snap,
        } => {
            assert_eq!(*program, f.original.id);
            assert_eq!(*viewport, f.viewport);
            assert_eq!(*origin, MaterialGraphSocketKind::ExpressionOutput(f.source));
            assert_eq!(*cursor, Vec2::new(40.0, 20.0));
            assert!(snap.is_none());
        }
        _ => panic!("No gesture"),
    }
    let ghosts = f.ghosts();
    assert_eq!(ghosts.len(), 1);
    let (id, handle) = &ghosts[0];
    assert_eq!(f.app.world().get::<ChildOf>(*id).unwrap().parent(), f.layer);
    let node = f.app.world().get::<Node>(*id).unwrap();
    assert_eq!(node.position_type, PositionType::Absolute);
    assert_eq!(node.width, Val::Percent(100.0));
    assert!(f.app.world().get::<Pickable>(*id).is_some());
    assert!(
        f.app
            .world()
            .resource::<Assets<GraphWireMaterial>>()
            .contains(handle.id())
    );
    assert_eq!(f.app.world().resource::<Bubbled>().0, 0);
    f.unchanged();
}
#[test]
fn non_primary_and_space_start_are_unhandled() {
    for space in [false, true] {
        let mut f = Fixture::new();
        if space {
            f.app
                .world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Space);
        }
        let button = if space {
            PointerButton::Primary
        } else {
            PointerButton::Secondary
        };
        f.app
            .world_mut()
            .trigger(pointer::start(f.output, f.window, Vec2::ONE, button));
        f.app.world_mut().flush();
        assert!(matches!(
            f.app.world().resource::<MaterialGraphGesture>(),
            MaterialGraphGesture::Idle
        ));
        assert!(f.ghosts().is_empty());
        assert_eq!(f.app.world().resource::<Bubbled>().0, 1);
        f.unchanged();
    }
}
#[test]
fn start_without_containing_viewport_does_not_create_gesture() {
    let mut f = Fixture::new();
    f.app.world_mut().entity_mut(f.output).remove::<ChildOf>();
    f.start(f.output, Vec2::ONE);
    assert!(matches!(
        f.app.world().resource::<MaterialGraphGesture>(),
        MaterialGraphGesture::Idle
    ));
    assert!(f.ghosts().is_empty());
    f.unchanged();
}
#[test]
fn missing_wire_layer_still_preserves_original_gesture_contract() {
    let mut f = Fixture::new();
    f.app.world_mut().despawn(f.layer);
    f.start(f.output, Vec2::ONE);
    assert!(matches!(
        f.app.world().resource::<MaterialGraphGesture>(),
        MaterialGraphGesture::Connecting { .. }
    ));
    assert!(f.ghosts().is_empty());
    f.unchanged();
}
#[test]
fn motion_uses_socket_inverse_scale_and_ignores_non_socket_targets() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .get_mut::<ComputedNode>(f.output)
        .unwrap()
        .inverse_scale_factor = 0.5;
    f.start(f.output, Vec2::new(10.0, 20.0));
    f.motion(f.output, Vec2::new(20.0, 30.0));
    match f.app.world().resource::<MaterialGraphGesture>() {
        MaterialGraphGesture::Connecting { cursor, .. } => {
            assert_eq!(*cursor, Vec2::new(40.0, 60.0))
        }
        _ => panic!(),
    }
    f.motion(f.parent, Vec2::ZERO);
    match f.app.world().resource::<MaterialGraphGesture>() {
        MaterialGraphGesture::Connecting { cursor, .. } => {
            assert_eq!(*cursor, Vec2::new(40.0, 60.0))
        }
        _ => panic!(),
    }
    f.unchanged();
}
#[test]
fn real_snap_updates_ghost_and_finish_publishes_one_undoable_connection() {
    let mut f = Fixture::new();
    f.start(f.output, Vec2::new(40.0, 20.0));
    f.motion(f.output, Vec2::new(240.0, 20.0));
    f.app.update();
    assert_eq!(f.snap(), Some(Fixture::target_kind()));
    f.unchanged();
    let (_, handle) = f.ghosts().pop().unwrap();
    let material = f
        .app
        .world()
        .resource::<Assets<GraphWireMaterial>>()
        .get(&handle)
        .unwrap();
    assert_eq!(material.start, Vec2::new(40.0, 20.0));
    assert_eq!(material.end, Vec2::new(240.0, 20.0));
    assert_eq!(material.color, Vec4::new(0.45, 1.0, 0.72, 1.0));
    assert_eq!(material.width, 2.5);
    f.end(f.output, Vec2::new(240.0, 20.0));
    assert_eq!(f.current().outputs.alpha, f.source);
    assert_eq!(
        f.app
            .world()
            .resource::<EditorSession>()
            .operation_order
            .edit_serial(),
        1
    );
    assert!(matches!(
        f.app.world().resource::<MaterialGraphGesture>(),
        MaterialGraphGesture::Idle
    ));
    assert!(f.ghosts().is_empty());
    let connected = f.current();
    f.step(HistoryAction::Undo);
    assert_eq!(f.current(), f.original);
    f.step(HistoryAction::Redo);
    assert_eq!(f.current(), connected);
    f.disk_unchanged();
}
#[test]
fn input_origin_can_snap_backwards_and_ghost_direction_remains_output_to_input() {
    let mut f = Fixture::new();
    f.start(f.input, Vec2::new(240.0, 20.0));
    f.motion(f.input, Vec2::new(40.0, 20.0));
    f.app.update();
    assert_eq!(
        f.snap(),
        Some(MaterialGraphSocketKind::ExpressionOutput(f.source))
    );
    let (_, h) = f.ghosts().pop().unwrap();
    let m = f
        .app
        .world()
        .resource::<Assets<GraphWireMaterial>>()
        .get(&h)
        .unwrap();
    assert_eq!(m.start, Vec2::new(40.0, 20.0));
    assert_eq!(m.end, Vec2::new(240.0, 20.0));
    f.end(f.input, Vec2::new(40.0, 20.0));
    assert_eq!(f.current().outputs.alpha, f.source);
    f.disk_unchanged();
}
#[test]
fn snapping_radius_and_planner_reject_missing_or_cyclic_endpoints() {
    for cycle in [false, true] {
        let mut f = Fixture::new();
        f.app
            .world_mut()
            .get_mut::<MaterialGraphSocket>(f.input)
            .unwrap()
            .kind =
            MaterialGraphSocketKind::ConnectionInput(MaterialConnectionTarget::ExpressionInput {
                expression: if cycle {
                    f.multiply
                } else {
                    MaterialExpressionId::new()
                },
                input: MaterialExpressionInput::Left,
            });
        if cycle {
            f.app
                .world_mut()
                .get_mut::<MaterialGraphSocket>(f.output)
                .unwrap()
                .kind = MaterialGraphSocketKind::ExpressionOutput(f.multiply);
        }
        f.start(f.output, Vec2::new(40.0, 20.0));
        f.motion(f.output, Vec2::new(240.0, 20.0));
        f.app.update();
        assert!(f.snap().is_none());
        f.unchanged();
    }
    let mut f = Fixture::new();
    f.start(f.output, Vec2::ZERO);
    f.motion(f.output, Vec2::new(279.0, 20.0));
    f.app.update();
    assert!(f.snap().is_none());
    f.motion(f.output, Vec2::new(278.0, 20.0));
    f.app.update();
    assert_eq!(f.snap(), Some(Fixture::target_kind()));
    f.unchanged();
}
#[test]
fn foreign_program_socket_never_snaps() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .get_mut::<MaterialGraphSocket>(f.input)
        .unwrap()
        .program = MaterialProgramId::new();
    f.start(f.output, Vec2::ZERO);
    f.motion(f.output, Vec2::new(240.0, 20.0));
    f.app.update();
    assert!(f.snap().is_none());
    f.unchanged();
}
#[test]
fn unsnapped_finish_opens_typed_palette_in_origin_view_and_cleans_ghost() {
    for input in [false, true] {
        let mut f = Fixture::new();
        let socket = if input { f.input } else { f.output };
        f.start(socket, Vec2::ZERO);
        f.end(socket, Vec2::new(300.0, 200.0));
        let palette = f.app.world().resource::<MaterialGraphPaletteState>();
        let open = palette.open.as_ref().unwrap();
        assert_eq!(open.program, f.original.id);
        assert_eq!(open.scope, f.scope);
        assert_eq!(open.menu_position, Vec2::new(300.0, 200.0));
        assert_eq!(open.graph_position, Vec2::new(130.0, 90.0));
        assert_eq!(
            open.editing_target,
            MaterialEditingTarget::Program {
                root: f.root.path().into(),
                id: f.original.id
            }
        );
        assert!(matches!(
            (input, open.connection),
            (true, Some(MaterialGraphPaletteConnection::FromInput(_)))
                | (false, Some(MaterialGraphPaletteConnection::FromOutput(_)))
        ));
        assert!(f.ghosts().is_empty());
        f.unchanged();
    }
}
#[test]
fn deleted_origin_viewport_prevents_finish_publication_and_returns_idle() {
    let mut f = Fixture::new();
    f.start(f.output, Vec2::ZERO);
    // Keep the socket so the original finish gate runs after viewport removal.
    f.app.world_mut().entity_mut(f.output).remove::<ChildOf>();
    f.app.world_mut().despawn(f.viewport);
    f.end(f.output, Vec2::new(240.0, 20.0));
    f.unchanged();
    assert!(matches!(
        f.app.world().resource::<MaterialGraphGesture>(),
        MaterialGraphGesture::Idle
    ));
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphPaletteState>()
            .open
            .is_none()
    );
}
#[test]
fn unchanged_connection_reports_noop_without_history() {
    let mut f = Fixture::new();
    let old = f.original.outputs.alpha;
    f.app
        .world_mut()
        .get_mut::<MaterialGraphSocket>(f.output)
        .unwrap()
        .kind = MaterialGraphSocketKind::ExpressionOutput(old);
    f.start(f.output, Vec2::ZERO);
    f.motion(f.output, Vec2::new(240.0, 20.0));
    f.app.update();
    assert_eq!(f.snap(), Some(Fixture::target_kind()));
    f.end(f.output, Vec2::new(240.0, 20.0));
    f.unchanged();
    assert_eq!(
        f.app.world().resource::<EditorSession>().status,
        "Material connection unchanged"
    );
}
#[test]
fn non_primary_finish_does_not_terminate_active_gesture() {
    let mut f = Fixture::new();
    f.start(f.output, Vec2::ZERO);
    f.app.world_mut().trigger(pointer::end(
        f.output,
        f.window,
        Vec2::ONE,
        PointerButton::Secondary,
    ));
    f.app.world_mut().flush();
    assert!(matches!(
        f.app.world().resource::<MaterialGraphGesture>(),
        MaterialGraphGesture::Connecting { .. }
    ));
    assert_eq!(f.ghosts().len(), 1);
    f.unchanged();
}
#[test]
fn endpoint_mapping_supports_both_directions_and_rejects_same_side() {
    let source = MaterialExpressionId::new();
    let output = MaterialGraphSocketKind::ExpressionOutput(source);
    let input = Fixture::target_kind();
    assert_eq!(
        connection_endpoints(output, input),
        connection_endpoints(input, output)
    );
    assert!(connection_endpoints(output, output).is_none());
    assert!(connection_endpoints(input, input).is_none());
}
#[test]
fn primary_socket_click_is_consumed_but_other_buttons_bubble() {
    let mut f = Fixture::new();
    f.app.world_mut().trigger(click_input::click(
        f.output,
        f.window,
        Vec2::ZERO,
        PointerButton::Primary,
    ));
    f.app.world_mut().flush();
    assert_eq!(f.app.world().resource::<Bubbled>().0, 0);
    f.app.world_mut().trigger(click_input::click(
        f.output,
        f.window,
        Vec2::ZERO,
        PointerButton::Secondary,
    ));
    f.app.world_mut().flush();
    assert_eq!(f.app.world().resource::<Bubbled>().0, 1);
    f.unchanged();
}
#[test]
fn real_wire_assets_attach_once_and_follow_selection_and_input_detach() {
    let mut f = Fixture::new();
    let target = MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Alpha);
    let wire = f
        .app
        .world_mut()
        .spawn((
            ChildOf(f.layer),
            MaterialGraphWire {
                program: f.original.id,
                source: f.source,
                target,
                color: Vec4::new(0.1, 0.2, 0.3, 0.4),
            },
        ))
        .id();
    f.app.update();
    let h = f
        .app
        .world()
        .get::<MaterialNode<GraphWireMaterial>>(wire)
        .unwrap()
        .0
        .clone();
    let m = f
        .app
        .world()
        .resource::<Assets<GraphWireMaterial>>()
        .get(&h)
        .unwrap();
    assert_eq!(m.color, Vec4::new(0.1, 0.2, 0.3, 0.4));
    assert_eq!(m.width, 2.0);
    f.app
        .world_mut()
        .resource_mut::<MaterialGraphSelectionState>()
        .entry(f.scope)
        .connection = Some(MaterialGraphConnection {
        program: f.original.id,
        source: f.source,
        target,
    });
    f.app.update();
    let m = f
        .app
        .world()
        .resource::<Assets<GraphWireMaterial>>()
        .get(&h)
        .unwrap();
    assert_eq!(m.width, 4.0);
    assert_eq!(m.color, Vec4::new(0.70, 0.50, 1.0, 1.0));
    f.start(f.input, Vec2::new(240.0, 20.0));
    f.app.update();
    let m = f
        .app
        .world()
        .resource::<Assets<GraphWireMaterial>>()
        .get(&h)
        .unwrap();
    assert_eq!(m.width, 0.0);
    assert_eq!(m.color, Vec4::ZERO);
    assert_eq!(
        f.app
            .world()
            .get::<MaterialNode<GraphWireMaterial>>(wire)
            .unwrap()
            .0,
        h
    );
    f.unchanged();
}
#[test]
fn material_update_retains_root_api_and_idle_emits_no_asset_modification() {
    use bevy::asset::{AssetApp, AssetEvent, AssetPlugin};
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<GraphWireMaterial>();
    let h = app
        .world_mut()
        .resource_mut::<Assets<GraphWireMaterial>>()
        .add(GraphWireMaterial::default());
    app.update();
    app.world_mut()
        .resource_mut::<Messages<AssetEvent<GraphWireMaterial>>>()
        .clear();
    crate::material_graph::update_wire_material(
        &mut app.world_mut().resource_mut::<Assets<GraphWireMaterial>>(),
        &h,
        Vec2::new(10.0, 20.0),
        Vec2::new(300.0, 40.0),
        Vec4::ONE,
        2.5,
        0.5,
    );
    app.update();
    assert!(
        app.world_mut()
            .resource_mut::<Messages<AssetEvent<GraphWireMaterial>>>()
            .drain()
            .any(|e| matches!(e,AssetEvent::Modified{id} if id==h.id()))
    );
    let first = app
        .world()
        .resource::<Assets<GraphWireMaterial>>()
        .get(&h)
        .unwrap()
        .clone();
    crate::material_graph::update_wire_material(
        &mut app.world_mut().resource_mut::<Assets<GraphWireMaterial>>(),
        &h,
        first.start,
        first.end,
        first.color,
        first.width,
        first.inverse_scale,
    );
    app.update();
    assert!(
        !app.world_mut()
            .resource_mut::<Messages<AssetEvent<GraphWireMaterial>>>()
            .drain()
            .any(|e| matches!(e,AssetEvent::Modified{id} if id==h.id()))
    );
}

#[test]
fn nearest_valid_endpoint_wins_even_with_a_closer_invalid_socket() {
    let mut f = Fixture::new();
    let anchor = *f
        .app
        .world()
        .get::<MaterialGraphSocketAnchor>(f.input)
        .unwrap();
    for (x, expression) in [(250.0, f.multiply), (251.0, MaterialExpressionId::new())] {
        f.app.world_mut().spawn((
            ChildOf(f.viewport),
            MaterialGraphSocket {
                program: f.original.id,
                kind: MaterialGraphSocketKind::ConnectionInput(
                    MaterialConnectionTarget::ExpressionInput {
                        expression,
                        input: MaterialExpressionInput::Left,
                    },
                ),
            },
            anchor,
            ComputedNode::default(),
            UiGlobalTransform::from(Affine2::from_translation(Vec2::new(x, 20.0))),
        ));
    }
    f.start(f.output, Vec2::ZERO);
    f.motion(f.output, Vec2::new(251.0, 20.0));
    f.app.update();
    assert_eq!(
        f.snap(),
        Some(MaterialGraphSocketKind::ConnectionInput(
            MaterialConnectionTarget::ExpressionInput {
                expression: f.multiply,
                input: MaterialExpressionInput::Left
            }
        ))
    );
    f.unchanged();
}

#[test]
fn changed_editing_target_between_snap_and_finish_rejects_publication() {
    let mut f = Fixture::new();
    f.start(f.output, Vec2::ZERO);
    f.motion(f.output, Vec2::new(240.0, 20.0));
    f.app.update();
    assert!(f.snap().is_some());
    f.app
        .world_mut()
        .get_mut::<MaterialGraphViewport>(f.viewport)
        .unwrap()
        .editing_target = MaterialEditingTarget::Program {
        root: f.root.path().into(),
        id: MaterialProgramId::new(),
    };
    f.end(f.output, Vec2::new(240.0, 20.0));
    f.unchanged();
    f.disk_unchanged();
    assert!(f.ghosts().is_empty());
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .status
            .starts_with("Material connection failed:")
    );
}

#[test]
fn socket_offset_cache_does_not_mix_later_ui_transforms_into_wire_endpoints() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .get_mut::<MaterialGraphSocketAnchor>(f.input)
        .unwrap()
        .offset = None;
    let wire = f
        .app
        .world_mut()
        .spawn((
            ChildOf(f.layer),
            MaterialGraphWire {
                program: f.original.id,
                source: f.source,
                target: MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Alpha),
                color: Vec4::ONE,
            },
        ))
        .id();
    f.app.update();
    let offset = f
        .app
        .world()
        .get::<MaterialGraphSocketAnchor>(f.input)
        .unwrap()
        .offset
        .unwrap();
    let h = f
        .app
        .world()
        .get::<MaterialNode<GraphWireMaterial>>(wire)
        .unwrap()
        .0
        .clone();
    let end = f
        .app
        .world()
        .resource::<Assets<GraphWireMaterial>>()
        .get(&h)
        .unwrap()
        .end;
    f.app
        .world_mut()
        .entity_mut(f.input)
        .insert(UiGlobalTransform::from(Affine2::from_translation(
            Vec2::new(900.0, 700.0),
        )));
    f.app.update();
    assert_eq!(
        f.app
            .world()
            .get::<MaterialGraphSocketAnchor>(f.input)
            .unwrap()
            .offset,
        Some(offset)
    );
    assert_eq!(
        f.app
            .world()
            .resource::<Assets<GraphWireMaterial>>()
            .get(&h)
            .unwrap()
            .end,
        end
    );
    f.unchanged();
}

#[test]
fn ghost_and_unsnapped_palette_stay_in_second_view_of_same_program() {
    let mut f = Fixture::new();
    let (view, layer, socket) = f.second_view();
    f.start(socket, Vec2::new(900.0, 50.0));
    f.motion(socket, Vec2::new(1000.0, 200.0));
    f.app.update();
    let (ghost, handle) = f.ghosts().pop().unwrap();
    assert_eq!(f.app.world().get::<ChildOf>(ghost).unwrap().parent(), layer);
    match f.app.world().resource::<MaterialGraphGesture>() {
        MaterialGraphGesture::Connecting { viewport, .. } => assert_eq!(*viewport, view),
        _ => panic!(),
    }
    let m = f
        .app
        .world()
        .resource::<Assets<GraphWireMaterial>>()
        .get(&handle)
        .unwrap();
    assert_eq!(m.start, Vec2::new(100.0, 50.0));
    assert_eq!(m.end, Vec2::new(200.0, 200.0));
    f.end(socket, Vec2::new(1000.0, 200.0));
    let open = f
        .app
        .world()
        .resource::<MaterialGraphPaletteState>()
        .open
        .as_ref()
        .unwrap();
    assert_eq!(open.scope, Some(EditorViewId(88)));
    assert_eq!(open.graph_position, Vec2::new(200.0, 300.0));
    f.unchanged();
}
#[test]
fn passive_wires_project_through_their_own_layers_without_duplicate_materials() {
    let mut f = Fixture::new();
    let (_, second_layer, _) = f.second_view();
    let mut wires = vec![];
    for layer in [f.layer, second_layer] {
        wires.push(
            f.app
                .world_mut()
                .spawn((
                    ChildOf(layer),
                    MaterialGraphWire {
                        program: f.original.id,
                        source: f.source,
                        target: MaterialConnectionTarget::ProgramOutput(
                            MaterialOutputSocket::Alpha,
                        ),
                        color: Vec4::ONE,
                    },
                ))
                .id(),
        );
    }
    f.app.update();
    let handles = wires
        .iter()
        .map(|e| {
            f.app
                .world()
                .get::<MaterialNode<GraphWireMaterial>>(*e)
                .unwrap()
                .0
                .clone()
        })
        .collect::<Vec<_>>();
    let materials = f.app.world().resource::<Assets<GraphWireMaterial>>();
    assert_eq!(
        materials.get(&handles[0]).unwrap().start,
        Vec2::new(40.0, 20.0)
    );
    assert_eq!(
        materials.get(&handles[0]).unwrap().end,
        Vec2::new(240.0, 20.0)
    );
    assert_eq!(
        materials.get(&handles[1]).unwrap().start,
        Vec2::new(100.0, 50.0)
    );
    assert_eq!(
        materials.get(&handles[1]).unwrap().end,
        Vec2::new(150.0, 50.0)
    );
    f.app.update();
    for (wire, handle) in wires.iter().zip(handles) {
        assert_eq!(
            f.app
                .world()
                .get::<MaterialNode<GraphWireMaterial>>(*wire)
                .unwrap()
                .0,
            handle
        );
    }
    f.unchanged();
}
