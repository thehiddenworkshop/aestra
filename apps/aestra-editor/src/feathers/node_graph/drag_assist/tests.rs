use super::*;
use bevy::{
    ecs::system::RunSystemOnce,
    picking::pointer::{Location, PointerId},
};

#[derive(Resource, Default)]
struct Edits(Vec<GraphPresentationEdit>);

#[derive(Resource, Default)]
struct BatchEdits(Vec<GraphPresentationBatchEdit>);

fn location() -> Location {
    Location {
        target: bevy::camera::NormalizedRenderTarget::None {
            width: 800,
            height: 600,
        },
        position: Vec2::ZERO,
    }
}

fn start(app: &mut App, node: Entity) {
    app.world_mut().trigger(Pointer::new(
        PointerId::Mouse,
        location(),
        DragStart {
            button: PointerButton::Primary,
            hit: bevy::picking::backend::HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        },
        node,
    ));
}

fn setup_scene(zoom: f32, scale: f32) -> (App, Entity, Entity) {
    let mut app = App::new();
    app.init_resource::<State>()
        .init_resource::<GraphNodeDragGesture>()
        .init_resource::<GraphViewportMemory>()
        .init_resource::<OverrideCursor>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<Edits>()
        .init_resource::<BatchEdits>()
        .add_observer(begin_graph_node_drag)
        .add_observer(drag_graph_node)
        .add_observer(end_graph_node_drag)
        .add_observer(
            |event: On<GraphPresentationEdit>, mut edits: ResMut<Edits>| {
                edits.0.push(event.event().clone());
            },
        )
        .add_observer(
            |event: On<GraphPresentationBatchEdit>, mut edits: ResMut<BatchEdits>| {
                edits.0.push(event.event().clone());
            },
        );
    app.world_mut().resource_mut::<State>().grid = true;
    let view = app
        .world_mut()
        .spawn(FeathersGraphViewport {
            key: "view".into(),
            pan: Vec2::new(45.0, -30.0),
            zoom,
            content_size: Vec2::splat(800.0),
            selection_bounds: None,
            frame_request: None,
            measured_frame: None,
            suppress_context_click: false,
        })
        .id();
    let node = app
        .world_mut()
        .spawn((
            FeathersGraphNode {
                graph_key: "function:test".into(),
                node_key: "node".into(),
                position: Vec2::new(15.0, 15.0),
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
                inverse_scale_factor: 1.0 / scale,
                ..default()
            },
            ChildOf(view),
        ))
        .id();
    (app, node, view)
}

fn setup(zoom: f32, scale: f32) -> (App, Entity, Entity) {
    let (mut app, node, view) = setup_scene(zoom, scale);
    start(&mut app, node);
    (app, node, view)
}

fn motion(app: &mut App, node: Entity, delta: Vec2) {
    app.world_mut().trigger(Pointer::new(
        PointerId::Mouse,
        location(),
        Drag {
            button: PointerButton::Primary,
            distance: delta,
            delta,
        },
        node,
    ));
}

#[test]
fn small_pointer_deltas_escape_grid_without_drift_and_emit_one_edit() {
    for scale in [1.0, 1.25, 2.0] {
        for zoom in [0.5, 1.0, 1.75] {
            let (mut app, node, _) = setup(zoom, scale);
            for _ in 0..16 {
                motion(&mut app, node, Vec2::X * zoom * scale);
            }
            assert_eq!(
                app.world()
                    .get::<FeathersGraphNode>(node)
                    .unwrap()
                    .position
                    .x,
                32.0
            );
            assert!(app.world().resource::<Edits>().0.is_empty());
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::AltLeft);
            motion(&mut app, node, Vec2::ZERO);
            assert!(
                (app.world()
                    .get::<FeathersGraphNode>(node)
                    .unwrap()
                    .position
                    .x
                    - 31.0)
                    .abs()
                    < 0.001
            );
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .release(KeyCode::AltLeft);
            for _ in 0..20 {
                motion(&mut app, node, Vec2::X * zoom * scale);
            }
            let expected = model::snap(
                Vec2::new(51.0, 15.0),
                &model::Shape {
                    rect: Rect::default(),
                },
                &[],
                zoom,
                true,
                true,
            )
            .0;
            assert!(
                (app.world().get::<FeathersGraphNode>(node).unwrap().position - expected).length()
                    < 0.001
            );
            app.world_mut().trigger(Pointer::new(
                PointerId::Mouse,
                location(),
                DragEnd {
                    button: PointerButton::Primary,
                    distance: Vec2::X * 36.0,
                },
                node,
            ));
            app.world_mut().flush();
            assert_eq!(app.world().resource::<Edits>().0.len(), 1);
            assert_eq!(
                app.world().resource::<Edits>().0[0].before.0,
                Vec2::splat(15.0)
            );
            assert!(app.world().resource::<State>().gestures.is_empty());
        }
    }
}

#[test]
fn dragging_a_selected_node_moves_the_selected_group_as_one_edit() {
    let (mut app, node, view) = setup_scene(1.0, 1.0);
    app.world_mut()
        .get_mut::<FeathersGraphNode>(node)
        .unwrap()
        .selected = true;
    let peer = app
        .world_mut()
        .spawn((
            FeathersGraphNode {
                graph_key: "function:test".into(),
                node_key: "peer".into(),
                position: Vec2::new(100.0, 50.0),
                selected: true,
                pinned: false,
                collapsed: false,
                dragging: false,
                drag_before: None,
                drag_modifier: None,
                suppress_release_click: false,
            },
            Node::default(),
            ComputedNode::default(),
            ChildOf(view),
        ))
        .id();
    app.world_mut().resource_mut::<State>().grid = false;

    start(&mut app, node);
    motion(&mut app, node, Vec2::new(12.0, -7.0));
    assert_eq!(
        app.world().get::<FeathersGraphNode>(node).unwrap().position,
        Vec2::new(27.0, 8.0)
    );
    assert_eq!(
        app.world().get::<FeathersGraphNode>(peer).unwrap().position,
        Vec2::new(112.0, 43.0)
    );
    app.world_mut().trigger(Pointer::new(
        PointerId::Mouse,
        location(),
        DragEnd {
            button: PointerButton::Primary,
            distance: Vec2::new(12.0, -7.0),
        },
        node,
    ));
    app.world_mut().flush();

    assert!(app.world().resource::<Edits>().0.is_empty());
    let batches = &app.world().resource::<BatchEdits>().0;
    assert_eq!(batches.len(), 1);
    assert_eq!(
        batches[0].before.keys().cloned().collect::<BTreeSet<_>>(),
        BTreeSet::from(["node".into(), "peer".into()])
    );
}

#[test]
fn guides_are_passive_view_local_and_removed_after_end_or_rebuild() {
    let (mut app, node, view) = setup(1.0, 1.0);
    motion(&mut app, node, Vec2::new(16.0, 0.0));
    app.world_mut().run_system_once(sync_guides).unwrap();
    let lines = app
        .world_mut()
        .query_filtered::<(&ChildOf, &Node, &Pickable), With<GuideLine>>()
        .iter(app.world())
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].0.parent(), view);
    assert_eq!(lines[0].1.left, Val::Px(77.0));
    assert!(!lines[0].2.is_hoverable);
    app.world_mut().despawn(node);
    app.world_mut().run_system_once(sync_guides).unwrap();
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<GuideLine>>()
            .iter(app.world())
            .count(),
        0
    );
    assert!(app.world().resource::<State>().gestures.is_empty());
}

#[test]
fn toolbar_toggles_are_shared_and_do_not_touch_layout_memory() {
    let (mut app, _, _) = setup(1.0, 1.0);
    app.add_observer(toggle);
    let button = app.world_mut().spawn(Toggle::Grid).id();
    app.world_mut().trigger(Activate { entity: button });
    assert!(!app.world().resource::<State>().grid);
    assert!(app.world().resource::<State>().alignment);
    assert!(app.world().resource::<Edits>().0.is_empty());
    assert!(
        app.world()
            .resource::<GraphViewportMemory>()
            .nodes
            .is_empty()
    );
}

/// Exercise the real Bevy layout collector, not fabricated snapshot values. Both
/// document kinds use the same capture and pointer path, with independent view zoom.
#[test]
fn measured_alignment_is_view_scoped_and_invalidates_changed_targets() {
    use crate::{docking::EditorViewId, document::DocumentKey};
    use aestra_core::{MaterialExpressionId, MaterialFunctionId, MaterialProgramId};
    for asset in [
        DocumentKey::MaterialProgram(MaterialProgramId::from_u128(1)),
        DocumentKey::MaterialFunction(MaterialFunctionId::from_u128(1)),
    ] {
        for scale in [1.0, 1.25, 2.0] {
            let (mut app, root) = geometry::tests::layout_app(scale);
            app.init_resource::<State>()
                .init_resource::<GraphNodeDragGesture>()
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<OverrideCursor>()
                .add_observer(begin_graph_node_drag)
                .add_observer(drag_graph_node);
            let mut surfaces = Vec::new();
            for (view_id, zoom) in [(1, 0.5), (2, 1.75)] {
                let mut entities = Vec::new();
                let keys = [
                    GraphNodeKey::Expression(MaterialExpressionId::from_u128(1)),
                    GraphNodeKey::MaterialOutputs,
                ];
                app.world_mut()
                    .commands()
                    .entity(root)
                    .with_children(|parent| {
                        let viewport = spawn_graph_viewport(
                            parent,
                            GraphViewportProps {
                                key: format!("test#view:{view_id}"),
                                content_size: Vec2::splat(800.0),
                                selection_bounds: None,
                                initial_view: Some((Vec2::ZERO, zoom)),
                            },
                            (),
                            |_| {},
                            |canvas| {
                                for (index, key) in keys.iter().enumerate() {
                                    entities.push(spawn_graph_node(
                                        canvas,
                                        GraphNodeProps {
                                            graph_key: "test".into(),
                                            node_key: index.to_string(),
                                            title: "Node".into(),
                                            position: if index == 0 {
                                                Vec2::splat(15.0)
                                            } else {
                                                Vec2::new(200.0, 180.0)
                                            },
                                            selected: false,
                                            pinned: false,
                                            muted: false,
                                            collapse_icon: default(),
                                            expand_icon: default(),
                                            pin_icon: default(),
                                            collapse_label: "Collapse".into(),
                                            expand_label: "Expand".into(),
                                        },
                                        GraphGeometryNode::new(*key, &"body", false),
                                        |_, body| {
                                            body.spawn(Node {
                                                height: Val::Px(50.0),
                                                ..default()
                                            });
                                        },
                                    ));
                                }
                            },
                        );
                        parent
                            .commands()
                            .entity(viewport)
                            .insert(GraphGeometryView {
                                key: GraphViewKey {
                                    document: GraphDocumentKey {
                                        project: "test".into(),
                                        asset,
                                    },
                                    view: Some(EditorViewId(view_id)),
                                },
                                nodes: keys.into_iter().collect(),
                            });
                    });
                app.world_mut().flush();
                surfaces.push((entities, zoom));
            }
            for _ in 0..5 {
                app.update();
            }
            let (nodes, zoom) = &surfaces[1];
            let moving = nodes[0];
            app.world_mut().trigger(Pointer::new(
                PointerId::Mouse,
                location(),
                DragStart {
                    button: PointerButton::Primary,
                    hit: bevy::picking::backend::HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                },
                moving,
            ));
            assert_eq!(
                app.world().resource::<State>().gestures[&moving]
                    .geometry
                    .len(),
                2
            );
            motion(&mut app, moving, Vec2::new(183.0, 0.0) * *zoom * scale);
            assert!(
                (app.world()
                    .get::<FeathersGraphNode>(moving)
                    .unwrap()
                    .position
                    .x
                    - 200.0)
                    .abs()
                    < 0.001
            );
            // Changing another view's zoom cannot influence capture or pointer conversion.
            let guide = &app.world().resource::<State>().gestures[&moving].guides[0];
            assert_eq!(guide.axis, 0);
            assert_eq!(guide.coordinate, 200.0);
            // A target moved through shared base memory is stale, not a new magnet mid-drag.
            app.world_mut()
                .resource_mut::<GraphViewportMemory>()
                .set_node("test", "1", Vec2::new(400.0, 180.0), false);
            app.world_mut()
                .run_system_once(sync_graph_nodes_from_memory)
                .unwrap();
            motion(&mut app, moving, Vec2::X * *zoom * scale);
            assert!(
                (app.world()
                    .get::<FeathersGraphNode>(moving)
                    .unwrap()
                    .position
                    .x
                    - 199.0)
                    .abs()
                    < 0.001
            );
            assert!(
                app.world().resource::<State>().gestures[&moving]
                    .geometry
                    .is_empty()
            );
        }
    }
}
