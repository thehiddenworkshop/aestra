use super::*;

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_corners(Vec2::new(x, y), Vec2::new(x + w, y + h))
}

#[test]
fn free_cursor_position_is_preserved_without_quantization() {
    let preferred = Vec2::new(-37.25, 41.5);
    assert_eq!(
        find_space(preferred, Vec2::new(224.0, 90.0), &[], |_| true),
        Some(preferred)
    );
}

#[test]
fn occupied_cursor_uses_nearest_gap_and_never_changes_obstacles() {
    let obstacles = vec![rect(0.0, 0.0, 224.0, 200.0), rect(248.0, 0.0, 224.0, 200.0)];
    let before = obstacles.clone();
    let placed = find_space(
        Vec2::new(10.0, 20.0),
        Vec2::new(224.0, 80.0),
        &obstacles,
        |_| true,
    )
    .unwrap();
    assert_eq!(placed, Vec2::new(10.0, -104.0));
    assert_eq!(obstacles, before);
    let reversed = obstacles.into_iter().rev().collect::<Vec<_>>();
    assert_eq!(
        find_space(
            Vec2::new(10.0, 20.0),
            Vec2::new(224.0, 80.0),
            &reversed,
            |_| true
        ),
        Some(placed)
    );
}

#[test]
fn socket_creation_stays_on_its_requested_side() {
    for after in [true, false] {
        let key = GraphNodeKey::MaterialOutputs;
        let mut area = Area {
            nodes: BTreeMap::from([(key, rect(0.0, 0.0, 224.0, 300.0))]),
            measured: true,
            ..default()
        };
        let result = area.place(
            Vec2::new(0.0, 100.0),
            Vec2::new(224.0, 80.0),
            if after {
                Neighborhood::After(key)
            } else {
                Neighborhood::Before(key)
            },
        );
        assert!(result.assisted);
        assert_eq!(result.position.x, if after { 248.0 } else { -248.0 });
    }
}

#[test]
fn helper_nodes_reserve_distinct_space_in_the_same_creation_batch() {
    let mut area = Area {
        measured: true,
        ..default()
    };
    let size = Vec2::new(224.0, 90.0);
    let a = area.place(Vec2::ZERO, size, Neighborhood::Cursor);
    let b = area.place(Vec2::ZERO, size, Neighborhood::Cursor);
    assert!(a.assisted && b.assisted);
    assert_ne!(a.position, b.position);
    assert!(area.nodes.is_empty());
}

#[test]
fn invalid_oversized_and_blocked_inputs_fail_boundedly() {
    let size = Vec2::new(224.0, 90.0);
    assert_eq!(find_space(Vec2::splat(f32::NAN), size, &[], |_| true), None);
    assert_eq!(
        find_space(
            Vec2::ZERO,
            size,
            &vec![rect(0.0, 0.0, 1.0, 1.0); MAX_OBSTACLES + 1],
            |_| true
        ),
        None
    );
    assert_eq!(
        find_space(
            Vec2::ZERO,
            size,
            &[rect(-5000.0, -5000.0, 10000.0, 10000.0)],
            |_| true
        ),
        None
    );
    let mut missing = Area::default();
    let placed = missing.place(Vec2::new(10.0, 20.0), size, Neighborhood::Cursor);
    assert!(!placed.assisted);
    assert_eq!(placed.position, Vec2::new(10.0, 20.0));
}

#[test]
fn measured_capture_uses_own_view_and_rejects_stale_memory() {
    use crate::{docking::EditorViewId, document::DocumentKey};
    use aestra_core::{MaterialFunctionId, MaterialProgramId};
    use bevy::ecs::system::RunSystemOnce;
    for asset in [
        DocumentKey::MaterialProgram(MaterialProgramId::from_u128(1)),
        DocumentKey::MaterialFunction(MaterialFunctionId::from_u128(1)),
    ] {
        for scale in [1.0, 1.25, 2.0] {
            let (mut app, root) = geometry::tests::layout_app(scale);
            let key = GraphViewKey {
                document: GraphDocumentKey {
                    project: "test".into(),
                    asset,
                },
                view: Some(EditorViewId(2)),
            };
            let mut viewport = Entity::PLACEHOLDER;
            app.world_mut()
                .commands()
                .entity(root)
                .with_children(|parent| {
                    viewport = spawn_graph_viewport(
                        parent,
                        GraphViewportProps {
                            key: "view2".into(),
                            content_size: Vec2::splat(800.0),
                            selection_bounds: None,
                            initial_view: Some((Vec2::new(20.0, 30.0), 1.75)),
                        },
                        (),
                        |_| {},
                        |canvas| {
                            spawn_graph_node(
                                canvas,
                                GraphNodeProps {
                                    graph_key: "test".into(),
                                    node_key: "output".into(),
                                    title: "Measured".into(),
                                    position: Vec2::new(50.0, 60.0),
                                    selected: false,
                                    muted: false,
                                    collapse_icon: default(),
                                    expand_icon: default(),
                                    collapse_label: "Collapse".into(),
                                    expand_label: "Expand".into(),
                                },
                                GraphGeometryNode::new(
                                    GraphNodeKey::MaterialOutputs,
                                    &"body",
                                    false,
                                ),
                                |_, body| {
                                    body.spawn(Node {
                                        height: Val::Px(140.0),
                                        ..default()
                                    });
                                },
                            );
                        },
                    );
                    parent
                        .commands()
                        .entity(viewport)
                        .insert(GraphGeometryView {
                            key: key.clone(),
                            nodes: [GraphNodeKey::MaterialOutputs].into_iter().collect(),
                        });
                });
            app.world_mut().flush();
            for _ in 0..5 {
                app.update();
            }
            let input = key.clone();
            let (mut area, center) = app
                .world_mut()
                .run_system_once(
                    move |placement: Context, memory: Res<GraphViewportMemory>| {
                        (
                            placement.capture(&input, &memory),
                            placement.center(&input).unwrap(),
                        )
                    },
                )
                .unwrap();
            assert!(area.measured);
            let expected_size =
                app.world().get::<ComputedNode>(viewport).unwrap().size() * (1.0 / scale);
            assert!(center.distance((expected_size * 0.5 - Vec2::new(20.0, 30.0)) / 1.75) < 0.001);
            let placed = area.place(
                Vec2::new(50.0, 60.0),
                Vec2::new(224.0, 80.0),
                Neighborhood::Cursor,
            );
            assert!(placed.assisted);
            assert_ne!(placed.position, Vec2::new(50.0, 60.0));
            let other = GraphViewKey {
                view: Some(EditorViewId(99)),
                ..key.clone()
            };
            let missing = app
                .world_mut()
                .run_system_once(
                    move |placement: Context, memory: Res<GraphViewportMemory>| {
                        placement.capture(&other, &memory)
                    },
                )
                .unwrap();
            assert!(!missing.measured);
            app.world_mut()
                .resource_mut::<GraphViewportMemory>()
                .set_node("test", "output", Vec2::new(900.0, 900.0), false);
            let stale = app
                .world_mut()
                .run_system_once(
                    move |placement: Context, memory: Res<GraphViewportMemory>| {
                        placement.capture(&key, &memory)
                    },
                )
                .unwrap();
            assert!(!stale.measured);
        }
    }
}
