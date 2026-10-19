use super::geometry::*;
use super::placement::*;
use super::*;

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
                                    pinned: false,
                                    muted: false,
                                    collapse_icon: default(),
                                    expand_icon: default(),
                                    pin_icon: default(),
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
