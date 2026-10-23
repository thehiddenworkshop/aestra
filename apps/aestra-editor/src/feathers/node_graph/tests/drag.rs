use super::super::{
    geometry::*, graph_pointer::tests as pointer, navigation::FeathersGraphViewport,
};
use super::*;
use bevy::picking::pointer::PointerId;

#[derive(Component)]
struct Control;
#[derive(Resource, Default)]
struct Log {
    edits: Vec<GraphPresentationEdit>,
    batches: Vec<GraphPresentationBatchEdit>,
    modified: Vec<GraphModifiedNodeDrag>,
    probes: usize,
    drops: Vec<insertion::Drop>,
}

struct Fixture {
    app: App,
    window: Entity,
    view: Entity,
    node: Entity,
}

fn spawn_node(app: &mut App, view: Entity, key: &str, position: Vec2, scale: f32) -> Entity {
    app.world_mut()
        .spawn((
            FeathersGraphNode {
                graph_key: "material".into(),
                node_key: key.into(),
                position,
                selected: false,
                pinned: false,
                collapsed: false,
                dragging: false,
                drag_before: None,
                drag_modifier: None,
                suppress_release_click: false,
            },
            Node::default(),
            ComputedNode {
                size: Vec2::new(80.0, 40.0) * scale,
                inverse_scale_factor: scale.recip(),
                ..default()
            },
            ChildOf(view),
        ))
        .id()
}

fn fixture(scale: f32, zoom: f32) -> Fixture {
    let mut app = App::new();
    app.init_resource::<GraphNodeDragGesture>()
        .init_resource::<GraphViewportMemory>()
        .init_resource::<OverrideCursor>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<drag_assist::State>()
        .init_resource::<insertion::State>()
        .init_resource::<GraphGeometryRegistry>()
        .init_resource::<Log>()
        .add_observer(begin_graph_node_drag::<With<Control>>)
        .add_observer(drag_graph_node)
        .add_observer(end_graph_node_drag)
        .add_observer(|e: On<GraphPresentationEdit>, mut log: ResMut<Log>| {
            log.edits.push(e.event().clone())
        })
        .add_observer(|e: On<GraphPresentationBatchEdit>, mut log: ResMut<Log>| {
            log.batches.push(e.event().clone())
        })
        .add_observer(|e: On<GraphModifiedNodeDrag>, mut log: ResMut<Log>| {
            log.modified.push(e.event().clone())
        })
        .add_observer(|_: On<insertion::Probe>, mut log: ResMut<Log>| log.probes += 1)
        .add_observer(|e: On<insertion::Drop>, mut log: ResMut<Log>| {
            log.drops.push(insertion::Drop {
                candidate: e.candidate.clone(),
                edit: e.edit.clone(),
                before_offset: e.before_offset,
            })
        });
    let window = app.world_mut().spawn_empty().id();
    let view = app
        .world_mut()
        .spawn((
            FeathersGraphViewport {
                key: "view".into(),
                pan: Vec2::ZERO,
                zoom,
                content_size: Vec2::splat(800.0),
                selection_bounds: None,
                frame_request: None,
                measured_frame: None,
            },
            ComputedNode {
                size: Vec2::splat(800.0) * scale,
                inverse_scale_factor: scale.recip(),
                ..default()
            },
        ))
        .id();
    let node = spawn_node(&mut app, view, "node", Vec2::new(15.0, 15.0), scale);
    Fixture {
        app,
        window,
        view,
        node,
    }
}

impl Fixture {
    fn start_at(&mut self, entity: Entity, button: PointerButton) {
        self.app.world_mut().trigger(pointer::start(
            entity,
            self.window,
            Vec2::ZERO,
            PointerId::Mouse,
            button,
        ));
        self.app.world_mut().flush();
    }
    fn start(&mut self) {
        self.start_at(self.node, PointerButton::Primary);
    }
    fn motion(&mut self, delta: Vec2) {
        let event = pointer::motion_delta(self.node, self.window, delta);
        self.app.world_mut().trigger(event);
        self.app.world_mut().flush();
    }
    fn end_at(&mut self, target: Entity) {
        self.app.world_mut().trigger(pointer::end(
            target,
            self.window,
            Vec2::ZERO,
            PointerId::Mouse,
            PointerButton::Primary,
        ));
        self.app.world_mut().flush();
    }
    fn end(&mut self) {
        self.end_at(self.node);
    }
    fn position(&self, entity: Entity) -> Vec2 {
        self.app
            .world()
            .get::<FeathersGraphNode>(entity)
            .unwrap()
            .position
    }
    fn no_helpers(&self) {
        assert!(
            self.app
                .world()
                .resource::<GraphNodeDragGesture>()
                .active
                .is_none()
        );
        assert!(
            self.app
                .world()
                .resource::<drag_assist::State>()
                .gestures
                .is_empty()
        );
        assert!(
            self.app
                .world()
                .resource::<insertion::State>()
                .gesture
                .is_none()
        );
        assert!(self.app.world().resource::<OverrideCursor>().0.is_none());
    }
    fn measure(&mut self, nodes: &[Entity]) {
        let identity = super::super::geometry::model_tests::key("project", 1);
        let mut observations = Vec::new();
        for (index, entity) in nodes.iter().enumerate() {
            let key = GraphNodeKey::Expression(aestra_core::MaterialExpressionId::from_u128(
                index as u128 + 1,
            ));
            let meta = GraphGeometryNode::new(key, &index, false);
            let content = meta.content_stamp();
            let position = self.position(*entity);
            self.app.world_mut().entity_mut(*entity).insert(meta);
            observations.push((
                *entity,
                key,
                GraphNodeGeometry {
                    effective_position: position,
                    size: Vec2::new(80.0, 40.0),
                    compact_size: Vec2::new(80.0, 40.0),
                    preview: false,
                    collapsed: false,
                    content,
                    geometry_revision: 0,
                    ports: vec![
                        GraphPortGeometry {
                            key: GraphGeometryPort::Output,
                            offset: Vec2::new(80.0, 20.0),
                        },
                        GraphPortGeometry {
                            key: GraphGeometryPort::Input(
                                aestra_authoring::MaterialExpressionInput::Value,
                            ),
                            offset: Vec2::new(0.0, 20.0),
                        },
                    ],
                },
            ));
        }
        self.app
            .world_mut()
            .entity_mut(self.view)
            .insert(GraphGeometryView {
                key: identity.clone(),
                nodes: observations.iter().map(|(_, key, _)| *key).collect(),
            });
        super::super::geometry::model_tests::measured_view(
            &mut self.app.world_mut().resource_mut::<GraphGeometryRegistry>(),
            identity,
            self.view,
            observations,
        );
    }
}

#[test]
fn production_drag_preserves_scaled_delta_contract_and_publishes_once_on_release() {
    // Preserve the shipping delta contract in this compatibility slice. Native DPI/picking
    // qualification is separate; these injected events do not prove OS coordinate units.
    for scale in [1.0, 1.25, 2.0] {
        for zoom in [0.5, 1.0, 1.75] {
            let mut f = fixture(scale, zoom);
            f.start();
            f.motion(Vec2::new(30.0, -10.0) * scale * zoom);
            assert!(f.position(f.node).distance(Vec2::new(45.0, 5.0)) < 0.001);
            assert!(f.app.world().resource::<Log>().edits.is_empty());
            assert!(f.app.world().resource::<OverrideCursor>().0.is_some());
            f.end();
            f.end();
            let log = f.app.world().resource::<Log>();
            assert_eq!(log.edits.len(), 1);
            assert_eq!(log.edits[0].before, (Vec2::splat(15.0), false));
            assert_eq!(log.edits[0].origin, Some(f.node));
            assert_eq!(
                f.app
                    .world()
                    .resource::<GraphViewportMemory>()
                    .node("material", "node"),
                Some(log.edits[0].after)
            );
            assert!(
                f.app
                    .world_mut()
                    .get_mut::<FeathersGraphNode>(f.node)
                    .unwrap()
                    .consume_suppressed_release_click()
            );
            f.no_helpers();
        }
    }
}

#[test]
fn selected_group_is_one_transaction_and_never_captures_another_view() {
    let mut f = fixture(1.0, 1.0);
    let peer = spawn_node(&mut f.app, f.view, "peer", Vec2::new(100.0, 50.0), 1.0);
    let other_view = f
        .app
        .world_mut()
        .spawn(FeathersGraphViewport {
            key: "other".into(),
            pan: Vec2::ZERO,
            zoom: 1.0,
            content_size: Vec2::splat(800.0),
            selection_bounds: None,
            frame_request: None,
            measured_frame: None,
        })
        .id();
    let other = spawn_node(&mut f.app, other_view, "other", Vec2::splat(200.0), 1.0);
    for entity in [f.node, peer, other] {
        f.app
            .world_mut()
            .get_mut::<FeathersGraphNode>(entity)
            .unwrap()
            .selected = true;
    }
    f.start();
    f.motion(Vec2::new(20.0, 10.0));
    f.end();
    assert_eq!(f.position(peer), Vec2::new(120.0, 60.0));
    assert_eq!(f.position(other), Vec2::splat(200.0));
    let log = f.app.world().resource::<Log>();
    assert!(log.edits.is_empty());
    assert_eq!(log.batches.len(), 1);
    assert_eq!(log.batches[0].before.len(), 2);
    assert_eq!(log.batches[0].before["peer"].0, Vec2::new(100.0, 50.0));
    f.no_helpers();
}

#[test]
fn modified_transactions_capture_precedence_and_duplicate_restores_original() {
    for (keys, modifier) in [
        (
            vec![KeyCode::AltLeft, KeyCode::ControlLeft, KeyCode::ShiftLeft],
            GraphNodeDragModifier::Duplicate,
        ),
        (
            vec![KeyCode::ControlRight, KeyCode::ShiftRight],
            GraphNodeDragModifier::Upstream,
        ),
        (vec![KeyCode::ShiftRight], GraphNodeDragModifier::Downstream),
    ] {
        let mut f = fixture(1.0, 1.0);
        f.app
            .world_mut()
            .entity_mut(f.node)
            .insert(GraphModifiedDragTarget);
        for key in keys {
            f.app
                .world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
        }
        f.start();
        f.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        f.motion(Vec2::splat(30.0));
        f.end();
        let log = f.app.world().resource::<Log>();
        assert!(log.edits.is_empty());
        assert!(log.batches.is_empty());
        assert_eq!(log.modified.len(), 1);
        assert_eq!(log.modified[0].modifier, modifier);
        assert_eq!(log.modified[0].after.0, Vec2::splat(45.0));
        assert_eq!(
            f.position(f.node),
            Vec2::splat(if modifier == GraphNodeDragModifier::Duplicate {
                15.0
            } else {
                45.0
            })
        );
        f.no_helpers();
    }
}

#[test]
fn synthetic_node_retains_ordinary_drag_under_modifiers_and_noop_has_no_edit() {
    let mut f = fixture(1.0, 1.0);
    f.start();
    f.end();
    assert!(f.app.world().resource::<Log>().edits.is_empty());
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::AltLeft);
    f.start();
    f.motion(Vec2::X * 10.0);
    f.end();
    assert_eq!(f.app.world().resource::<Log>().edits.len(), 1);
    assert!(f.app.world().resource::<Log>().modified.is_empty());
    f.no_helpers();
}

#[test]
fn nested_controls_space_and_secondary_button_cannot_begin_node_drag() {
    let mut f = fixture(1.0, 1.0);
    let control = f.app.world_mut().spawn((Control, ChildOf(f.node))).id();
    let child = f.app.world_mut().spawn(ChildOf(control)).id();
    f.start_at(child, PointerButton::Primary);
    f.start_at(f.node, PointerButton::Secondary);
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    f.start();
    assert!(
        !f.app
            .world()
            .get::<FeathersGraphNode>(f.node)
            .unwrap()
            .dragging
    );
    f.no_helpers();
}

#[test]
fn retargeted_release_ends_captured_node_and_clears_cursor_and_helpers() {
    let mut f = fixture(1.0, 1.0);
    f.start();
    f.motion(Vec2::X * 10.0);
    let rebuilt = f.app.world_mut().spawn((Control, ChildOf(f.node))).id();
    f.end_at(rebuilt);
    assert_eq!(f.app.world().resource::<Log>().edits.len(), 1);
    assert!(
        !f.app
            .world()
            .get::<FeathersGraphNode>(f.node)
            .unwrap()
            .dragging
    );
    f.no_helpers();
}

#[test]
fn invalid_motion_does_not_corrupt_real_placement_memory() {
    let mut f = fixture(1.0, 1.0);
    f.start();
    for delta in [Vec2::splat(f32::NAN), Vec2::splat(f32::INFINITY)] {
        f.motion(delta);
    }
    assert_eq!(f.position(f.node), Vec2::splat(15.0));
    f.end();
    assert!(f.app.world().resource::<Log>().edits.is_empty());
    f.no_helpers();
}

#[test]
fn measured_registry_alignment_invalidates_changed_target_and_grid_uses_raw_motion() {
    let mut f = fixture(1.0, 1.0);
    let peer = spawn_node(&mut f.app, f.view, "peer", Vec2::new(100.0, 80.0), 1.0);
    f.measure(&[f.node, peer]);
    f.start();
    f.motion(Vec2::new(81.0, 0.0));
    assert_eq!(f.position(f.node).x, 100.0);
    f.app
        .world_mut()
        .entity_mut(peer)
        .insert(GraphGeometryNode::new(
            GraphNodeKey::FunctionOutputs,
            &99,
            false,
        ));
    f.motion(Vec2::X);
    assert_eq!(f.position(f.node).x, 97.0);
    f.end();
    f.no_helpers();
    let mut f = fixture(1.0, 1.0);
    f.app.world_mut().resource_mut::<drag_assist::State>().grid = true;
    f.start();
    for _ in 0..16 {
        f.motion(Vec2::X);
    }
    assert_eq!(f.position(f.node).x, 32.0);
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::AltLeft);
    f.motion(Vec2::ZERO);
    assert_eq!(f.position(f.node).x, 31.0);
    f.end();
    f.no_helpers();
}

#[test]
fn displaced_node_records_authored_before_and_bakes_only_its_dropped_position() {
    let mut f = fixture(1.0, 1.0);
    {
        let mut memory = f.app.world_mut().resource_mut::<GraphViewportMemory>();
        memory.set_node("material", "node", Vec2::new(5.0, 8.0), false);
        memory
            .offsets
            .insert(("material".into(), "node".into()), Vec2::new(10.0, 7.0));
        memory.set_node("unrelated", "node", Vec2::splat(500.0), true);
    }
    f.start();
    f.motion(Vec2::new(30.0, 10.0));
    f.end();
    let log = f.app.world().resource::<Log>();
    assert_eq!(log.edits.len(), 1);
    assert_eq!(log.edits[0].before, (Vec2::new(5.0, 8.0), false));
    assert_eq!(log.edits[0].after, (Vec2::new(45.0, 25.0), false));
    let memory = f.app.world().resource::<GraphViewportMemory>();
    assert_eq!(
        memory.node_position("material", "node"),
        Some(Vec2::new(45.0, 25.0))
    );
    assert_eq!(
        memory.node("unrelated", "node"),
        Some((Vec2::splat(500.0), true))
    );
    assert!(
        !memory
            .offsets
            .contains_key(&("material".into(), "node".into()))
    );
    f.no_helpers();
}

#[test]
fn real_registry_wire_candidate_routes_drop_instead_of_plain_edit() {
    let mut f = fixture(1.0, 1.0);
    f.app
        .world_mut()
        .resource_mut::<drag_assist::State>()
        .alignment = false;
    f.app
        .world_mut()
        .get_mut::<FeathersGraphNode>(f.node)
        .unwrap()
        .position = Vec2::new(180.0, 200.0);
    let source = spawn_node(&mut f.app, f.view, "source", Vec2::new(20.0, 80.0), 1.0);
    let target = spawn_node(&mut f.app, f.view, "target", Vec2::new(420.0, 80.0), 1.0);
    f.measure(&[f.node, source, target]);
    let wire = insertion::Wire {
        source: f.app.world().get::<GraphGeometryNode>(source).unwrap().key,
        target: f.app.world().get::<GraphGeometryNode>(target).unwrap().key,
        port: GraphGeometryPort::Input(aestra_authoring::MaterialExpressionInput::Value),
    };
    let wire_entity = f.app.world_mut().spawn((wire, ChildOf(f.view))).id();
    f.start();
    f.motion(Vec2::new(40.0, -120.0));
    assert_eq!(
        f.app
            .world()
            .resource::<insertion::State>()
            .candidate
            .as_ref()
            .unwrap()
            .entity,
        wire_entity
    );
    assert_eq!(f.app.world().resource::<Log>().probes, 1);
    f.end();
    let log = f.app.world().resource::<Log>();
    assert!(log.edits.is_empty());
    assert_eq!(log.drops.len(), 1);
    assert_eq!(log.drops[0].candidate.wire, wire);
    assert!(log.drops[0].candidate.spacing.is_ok());
    assert_eq!(log.drops[0].edit.before.0, Vec2::new(180.0, 200.0));
    assert_eq!(log.drops[0].edit.after.0, Vec2::new(220.0, 80.0));
    f.no_helpers();
}

#[test]
fn changed_view_identity_rejects_wire_drop_at_release() {
    let mut f = fixture(1.0, 1.0);
    f.app
        .world_mut()
        .resource_mut::<drag_assist::State>()
        .alignment = false;
    f.app
        .world_mut()
        .get_mut::<FeathersGraphNode>(f.node)
        .unwrap()
        .position = Vec2::new(180.0, 200.0);
    let source = spawn_node(&mut f.app, f.view, "source", Vec2::new(20.0, 80.0), 1.0);
    let target = spawn_node(&mut f.app, f.view, "target", Vec2::new(420.0, 80.0), 1.0);
    f.measure(&[f.node, source, target]);
    let wire = insertion::Wire {
        source: f.app.world().get::<GraphGeometryNode>(source).unwrap().key,
        target: f.app.world().get::<GraphGeometryNode>(target).unwrap().key,
        port: GraphGeometryPort::Input(aestra_authoring::MaterialExpressionInput::Value),
    };
    f.app.world_mut().spawn((wire, ChildOf(f.view)));
    f.start();
    f.motion(Vec2::new(40.0, -120.0));
    assert!(
        f.app
            .world()
            .resource::<insertion::State>()
            .candidate
            .is_some()
    );
    f.app
        .world_mut()
        .get_mut::<GraphGeometryView>(f.view)
        .unwrap()
        .key
        .document
        .project = "other-project".into();
    f.end();
    let log = f.app.world().resource::<Log>();
    assert!(log.drops.is_empty());
    assert_eq!(log.edits.len(), 1);
    f.no_helpers();
}
