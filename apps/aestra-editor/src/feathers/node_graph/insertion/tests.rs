use super::*;
use aestra_core::MaterialExpressionId;

#[derive(Resource, Default)]
struct Events {
    probes: Vec<Option<Candidate>>,
    drops: usize,
    moves: usize,
}

#[test]
fn wire_insertion_hit_testing_is_view_local_dpi_independent_and_alt_bypasses() {
    for scale in [1.0, 1.25, 2.0] {
        for zoom in [0.5, 1.0] {
            let (mut app, root) = geometry::tests::layout_app(scale);
            app.init_resource::<State>()
                .init_resource::<Events>()
                .init_resource::<ButtonInput<KeyCode>>()
                .add_observer(|event: On<Probe>, mut events: ResMut<Events>| {
                    events.probes.push(event.0.clone());
                })
                .add_observer(|_: On<Drop>, mut events: ResMut<Events>| {
                    events.drops += 1;
                })
                .add_observer(|_: On<GraphPresentationEdit>, mut events: ResMut<Events>| {
                    events.moves += 1;
                });
            let keys =
                [1, 2, 3].map(|id| GraphNodeKey::Expression(MaterialExpressionId::from_u128(id)));
            let identity = GraphViewKey {
                document: GraphDocumentKey {
                    project: "test".into(),
                    asset: crate::document::DocumentKey::MaterialProgram(
                        aestra_core::MaterialProgramId::from_u128(1),
                    ),
                },
                view: Some(crate::docking::EditorViewId(4)),
            };
            let mut entities = Vec::new();
            let mut viewport = Entity::PLACEHOLDER;
            let mut wire_entity = Entity::PLACEHOLDER;
            let wire = Wire {
                source: keys[0],
                target: keys[1],
                port: GraphGeometryPort::Input(aestra_authoring::MaterialExpressionInput::Value),
            };
            app.world_mut()
                .commands()
                .entity(root)
                .with_children(|parent| {
                    viewport = spawn_graph_viewport(
                        parent,
                        GraphViewportProps {
                            key: "view".into(),
                            content_size: Vec2::splat(800.0),
                            selection_bounds: None,
                            initial_view: Some((Vec2::new(10.0, 10.0), zoom)),
                        },
                        (),
                        |layer| {
                            wire_entity = layer.spawn(wire).id();
                        },
                        |canvas| {
                            for (index, key) in keys.iter().enumerate() {
                                let entity = spawn_graph_node(
                                    canvas,
                                    GraphNodeProps {
                                        graph_key: "graph".into(),
                                        node_key: format!("{index}"),
                                        title: "Node".into(),
                                        position: [
                                            Vec2::new(20.0, 50.0),
                                            Vec2::new(550.0, 50.0),
                                            Vec2::new(260.0, 320.0),
                                        ][index],
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
                                        body.spawn((
                                            Node {
                                                width: Val::Px(20.0),
                                                height: Val::Px(20.0),
                                                ..default()
                                            },
                                            if index == 0 {
                                                GraphGeometryPort::Output
                                            } else {
                                                wire.port
                                            },
                                        ));
                                    },
                                );
                                entities.push(entity);
                            }
                        },
                    );
                    parent
                        .commands()
                        .entity(viewport)
                        .insert(GraphGeometryView {
                            key: identity.clone(),
                            nodes: keys.into_iter().collect(),
                        });
                });
            app.world_mut().flush();
            // An identical wire outside this viewport must never win the hit test.
            app.world_mut().spawn((wire, ChildOf(root)));
            for _ in 0..5 {
                app.update();
            }
            let moving = entities[2];
            let snapshot = app
                .world()
                .resource::<GraphGeometryRegistry>()
                .mounted_view_snapshot(&identity, viewport)
                .unwrap()
                .clone();
            for (index, node_entity) in entities.iter().enumerate() {
                let port_entity = app
                    .world_mut()
                    .query::<(Entity, &GraphGeometryPort)>()
                    .iter(app.world())
                    .find(|(entity, _)| belongs(app.world(), *entity, *node_entity))
                    .unwrap()
                    .0;
                let computed = app.world().get::<ComputedNode>(*node_entity).unwrap();
                let node_transform = app.world().get::<UiGlobalTransform>(*node_entity).unwrap();
                let port_transform = app.world().get::<UiGlobalTransform>(port_entity).unwrap();
                let rendered_offset = crate::material_graph::viewport_local_position(
                    computed,
                    node_transform,
                    port_transform.to_scale_angle_translation().2,
                );
                assert!(
                    rendered_offset.distance(snapshot.nodes[&keys[index]].ports[0].offset) < 0.001
                );
            }
            let start = snapshot.nodes[&keys[0]].effective_position
                + snapshot.nodes[&keys[0]].ports[0].offset;
            let end = snapshot.nodes[&keys[1]].effective_position
                + snapshot.nodes[&keys[1]].ports[0].offset;
            let position = (start + end) * 0.5 - snapshot.nodes[&keys[2]].size * 0.5;
            let original = app
                .world()
                .get::<FeathersGraphNode>(moving)
                .unwrap()
                .position;
            begin(app.world_mut(), moving);
            {
                let mut node = app
                    .world_mut()
                    .get_mut::<FeathersGraphNode>(moving)
                    .unwrap();
                node.position = position;
                node.dragging = true;
            }
            motion(app.world_mut(), moving);
            let found = app.world().resource::<State>().candidate.as_ref().unwrap();
            assert_eq!(found.entity, wire_entity);
            assert_eq!(found.view, identity);
            // Drop closer to the consumer: the measured inserted rectangle needs room.
            // The same logical cascade must be planned at every DPI and zoom.
            app.world_mut()
                .get_mut::<FeathersGraphNode>(moving)
                .unwrap()
                .position =
                Vec2::new(450.0, (start.y + end.y) * 0.5) - snapshot.nodes[&keys[2]].size * 0.5;
            motion(app.world_mut(), moving);
            assert_eq!(
                app.world()
                    .resource::<State>()
                    .candidate
                    .as_ref()
                    .unwrap()
                    .spacing
                    .as_ref()
                    .unwrap()
                    .count(),
                1
            );
            let target_position = snapshot.nodes[&keys[1]].effective_position;
            {
                let mut memory = app.world_mut().resource_mut::<GraphViewportMemory>();
                memory.set_node("graph", "1", target_position - Vec2::Y * 40.0, false);
                memory.set_temporary_offset("graph", "1", Vec2::Y * 40.0);
            }
            motion(app.world_mut(), moving);
            assert!(
                app.world()
                    .resource::<State>()
                    .candidate
                    .as_ref()
                    .unwrap()
                    .spacing
                    .as_ref()
                    .unwrap_err()
                    .contains("protected node")
            );
            app.world_mut()
                .resource_mut::<GraphViewportMemory>()
                .remove_node("graph", "1");
            app.world_mut()
                .get_mut::<FeathersGraphNode>(moving)
                .unwrap()
                .position = position;
            motion(app.world_mut(), moving);
            app.world_mut().resource_mut::<State>().allowed = true;
            assert_eq!(
                app.world()
                    .resource::<State>()
                    .color(wire_entity)
                    .unwrap()
                    .y,
                1.0
            );
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::AltLeft);
            refresh(app.world_mut());
            assert!(app.world().resource::<State>().candidate.is_none());
            let edit = GraphPresentationEdit {
                graph: "graph".into(),
                node: "2".into(),
                before: (original, false),
                after: (position, false),
            };
            finish(app.world_mut(), moving, Some(edit.clone()));
            assert_eq!(app.world().resource::<Events>().moves, 1);
            assert_eq!(app.world().resource::<Events>().drops, 0);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .release(KeyCode::AltLeft);
            begin(app.world_mut(), moving);
            finish(app.world_mut(), moving, Some(edit.clone()));
            assert_eq!(app.world().resource::<Events>().drops, 1);
            // Rebuilt/resized content invalidates the captured geometry, even at the same position.
            begin(app.world_mut(), moving);
            app.world_mut()
                .entity_mut(entities[0])
                .insert(GraphGeometryNode::new(keys[0], &"changed", false));
            assert!(candidate(app.world_mut(), moving).is_none());
            app.world_mut().entity_mut(moving).despawn();
            refresh(app.world_mut());
            assert!(app.world().resource::<State>().gesture.is_none());
        }
    }
}

#[test]
fn cubic_hit_test_handles_degenerate_and_distant_points() {
    assert_eq!(distance_to_wire(Vec2::ZERO, Vec2::ZERO, Vec2::ZERO), 0.0);
    assert!(distance_to_wire(Vec2::new(200.0, 500.0), Vec2::ZERO, Vec2::new(400.0, 0.0)) > 400.0);
    assert!(distance_to_wire(Vec2::new(200.0, 0.0), Vec2::ZERO, Vec2::new(400.0, 0.0)) < 0.01);
}

#[test]
fn wire_uniforms_keep_logical_coordinates_and_update_dpi_without_moving_endpoints() {
    let mut materials = Assets::<GraphWireMaterial>::default();
    let handle = materials.add(GraphWireMaterial::default());
    let start = Vec2::new(45.0, 90.0);
    let end = Vec2::new(410.0, 270.0);
    for scale in [1.0, 1.25, 2.0] {
        crate::material_graph::update_wire_material(
            &mut materials,
            &handle,
            start,
            end,
            Vec4::ONE,
            2.0,
            1.0 / scale,
        );
        let material = materials.get(&handle).unwrap();
        assert_eq!(material.start, start);
        assert_eq!(material.end, end);
        // Fragment coordinates arrive in physical pixels and use this same normalization.
        assert_eq!(start * scale * material.inverse_scale, start);
        assert_eq!(material.inverse_scale, 1.0 / scale);
    }
}
