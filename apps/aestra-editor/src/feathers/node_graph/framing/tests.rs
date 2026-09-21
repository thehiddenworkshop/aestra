use super::*;
use crate::{docking::EditorViewId, document::DocumentKey};
use aestra_core::{MaterialExpressionId, MaterialFunctionId, MaterialProgramId};

#[test]
fn wheel_navigation_uses_logical_viewport_size_and_cancels_pending_frame() {
    use bevy::ecs::system::RunSystemOnce;
    for scale in [1.0, 1.25, 2.0] {
        for zoom in [0.5, 1.0, 1.75] {
            let mut app = App::new();
            app.init_resource::<Messages<CursorMoved>>()
                .init_resource::<Messages<MouseWheel>>()
                .init_resource::<ButtonInput<MouseButton>>()
                .init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<GraphPanGesture>()
                .init_resource::<OverrideCursor>();
            let window = app
                .world_mut()
                .spawn((Window::default(), PrimaryWindow))
                .id();
            let pan = Vec2::new(60.0, -30.0);
            let size = Vec2::new(960.0, 640.0);
            let normalized = Vec2::new(0.2, -0.1);
            let cursor = (normalized + Vec2::splat(0.5)) * size;
            let anchor = (cursor - pan) / zoom;
            let entity = app
                .world_mut()
                .spawn((
                    FeathersGraphViewport {
                        key: "view".into(),
                        pan,
                        zoom,
                        content_size: size,
                        selection_bounds: None,
                        frame_request: Some(GraphFrameTarget::All),
                        measured_frame: Some((
                            GraphFrameTarget::All,
                            GraphView {
                                pan: Vec2::splat(9999.0),
                                zoom: 0.25,
                            },
                        )),
                        suppress_context_click: false,
                    },
                    ComputedNode {
                        size: size * scale,
                        inverse_scale_factor: 1.0 / scale,
                        ..default()
                    },
                    RelativeCursorPosition {
                        cursor_over: true,
                        normalized: Some(normalized),
                    },
                ))
                .id();
            app.world_mut()
                .resource_mut::<Messages<MouseWheel>>()
                .write(MouseWheel {
                    unit: MouseScrollUnit::Line,
                    x: 0.0,
                    y: 1.0,
                    window,
                    phase: bevy::input::touch::TouchPhase::Moved,
                });
            app.world_mut()
                .run_system_once(navigate_graph_viewports)
                .unwrap();
            app.world_mut()
                .run_system_once(sync_graph_viewport_transforms)
                .unwrap();
            let viewport = app.world().get::<FeathersGraphViewport>(entity).unwrap();
            assert!((viewport.project_graph_point(anchor) - cursor).length() < 0.001);
            assert!(viewport.zoom > zoom);
            assert!(viewport.frame_request.is_none());
            assert!(viewport.measured_frame.is_none());
        }
    }
}

struct Fixture {
    viewport: Entity,
    nodes: [Entity; 2],
    bodies: [Entity; 2],
    key: GraphViewKey,
}

fn spawn(app: &mut App, root: Entity, function: bool, view: u64, zoom: f32) -> Fixture {
    let key = GraphViewKey {
        document: GraphDocumentKey {
            project: "project".into(),
            asset: if function {
                DocumentKey::MaterialFunction(MaterialFunctionId::from_u128(1))
            } else {
                DocumentKey::MaterialProgram(MaterialProgramId::from_u128(1))
            },
        },
        view: Some(EditorViewId(view)),
    };
    let keys = [
        GraphNodeKey::Expression(MaterialExpressionId::from_u128(1)),
        if function {
            GraphNodeKey::FunctionOutputs
        } else {
            GraphNodeKey::MaterialOutputs
        },
    ];
    let mut fixture = Fixture {
        viewport: Entity::PLACEHOLDER,
        nodes: [Entity::PLACEHOLDER; 2],
        bodies: [Entity::PLACEHOLDER; 2],
        key: key.clone(),
    };
    app.world_mut()
        .commands()
        .entity(root)
        .with_children(|parent| {
            fixture.viewport = spawn_graph_viewport(
                parent,
                GraphViewportProps {
                    key: format!("document#view:{view}"),
                    // Deliberately wrong bootstrap/selection bounds: runtime framing must ignore them.
                    content_size: Vec2::splat(10_000.0),
                    selection_bounds: Some(Rect::from_corners(
                        Vec2::splat(9000.0),
                        Vec2::splat(9999.0),
                    )),
                    initial_view: Some((Vec2::new(60.0, -30.0), zoom)),
                },
                (),
                |_| {},
                |canvas| {
                    for (index, key) in keys.into_iter().enumerate() {
                        fixture.nodes[index] = spawn_graph_node(
                            canvas,
                            GraphNodeProps {
                                // Placement and camera keys intentionally differ, as in document views.
                                graph_key: "document".into(),
                                node_key: format!("node:{index}"),
                                title: "Measured node".into(),
                                position: if index == 0 {
                                    Vec2::new(-180.0, 90.0)
                                } else {
                                    Vec2::new(480.0, 370.0)
                                },
                                selected: index == 0,
                                muted: false,
                                collapse_icon: default(),
                                expand_icon: default(),
                                collapse_label: "Collapse".into(),
                                expand_label: "Expand".into(),
                            },
                            GraphGeometryNode::new(key, &"initial", false),
                            |_, body| {
                                fixture.bodies[index] = body.target_entity();
                                body.spawn(Node {
                                    height: Val::Px(75.0),
                                    ..default()
                                });
                            },
                        );
                    }
                },
            );
            parent
                .commands()
                .entity(fixture.viewport)
                .insert(GraphGeometryView {
                    key,
                    nodes: keys.into_iter().collect(),
                });
        });
    app.world_mut().flush();
    fixture
}

fn advance(app: &mut App) {
    for _ in 0..4 {
        app.update();
    }
}

fn bounds(app: &App, fixture: &Fixture, selection: bool) -> Rect {
    app.world()
        .resource::<GraphGeometryRegistry>()
        .view_snapshot(&fixture.key)
        .unwrap()
        .bounds(|key| !selection || matches!(key, GraphNodeKey::Expression(_)))
        .unwrap()
}

fn request(app: &mut App, fixture: &Fixture, target: GraphFrameTarget) {
    app.world_mut()
        .get_mut::<FeathersGraphViewport>(fixture.viewport)
        .unwrap()
        .frame_request = Some(target);
}

fn assert_framed(app: &App, fixture: &Fixture, bounds: Rect, target: GraphFrameTarget) {
    let viewport = app
        .world()
        .get::<FeathersGraphViewport>(fixture.viewport)
        .unwrap();
    let computed = app.world().get::<ComputedNode>(fixture.viewport).unwrap();
    let size = computed.size() * computed.inverse_scale_factor;
    let expected = framed_graph_view(
        bounds,
        size,
        if target == GraphFrameTarget::All {
            1.0
        } else {
            MAX_ZOOM
        },
    );
    assert!(
        (viewport.pan - expected.pan).length() < 0.01,
        "{:?} != {:?}",
        viewport.pan,
        expected.pan
    );
    assert!((viewport.zoom - expected.zoom).abs() < 0.0001);
    assert!(viewport.frame_request.is_none());
    let min = viewport.project_graph_point(bounds.min);
    let max = viewport.project_graph_point(bounds.max);
    assert!(min.cmpge(Vec2::splat(FRAME_PADDING - 0.1)).all());
    assert!(max.cmple(size - Vec2::splat(FRAME_PADDING - 0.1)).all());
}

#[test]
fn measured_frame_all_selection_resize_and_collapse_across_dpi_zoom_and_graph_kind() {
    for function in [false, true] {
        for scale in [1.0, 1.25, 2.0] {
            for zoom in [0.5, 1.0, 1.75] {
                let (mut app, root) = geometry::tests::layout_app(scale);
                let fixture = spawn(&mut app, root, function, 1, zoom);
                request(&mut app, &fixture, GraphFrameTarget::All);
                app.update();
                // Not-yet-measured geometry must not consume the frame request.
                assert!(
                    app.world()
                        .get::<FeathersGraphViewport>(fixture.viewport)
                        .unwrap()
                        .frame_request
                        .is_some()
                );
                advance(&mut app);
                assert_framed(
                    &app,
                    &fixture,
                    bounds(&app, &fixture, false),
                    GraphFrameTarget::All,
                );
                request(&mut app, &fixture, GraphFrameTarget::Selection);
                advance(&mut app);
                assert_framed(
                    &app,
                    &fixture,
                    bounds(&app, &fixture, true),
                    GraphFrameTarget::Selection,
                );
                let original = bounds(&app, &fixture, true);

                // Unanticipated diagnostic/preview content, not an adapter height estimate.
                let extra = app
                    .world_mut()
                    .spawn(Node {
                        height: Val::Px(280.0),
                        ..default()
                    })
                    .id();
                app.world_mut()
                    .entity_mut(fixture.bodies[0])
                    .add_child(extra);
                request(&mut app, &fixture, GraphFrameTarget::Selection);
                app.update();
                assert!(
                    app.world()
                        .get::<FeathersGraphViewport>(fixture.viewport)
                        .unwrap()
                        .frame_request
                        .is_some()
                );
                advance(&mut app);
                let expanded = bounds(&app, &fixture, true);
                assert!(expanded.height() > original.height() + 270.0);
                assert_eq!(expanded.min, original.min);
                assert_framed(&app, &fixture, expanded, GraphFrameTarget::Selection);

                app.world_mut()
                    .get_mut::<Node>(fixture.bodies[0])
                    .unwrap()
                    .display = Display::None;
                app.world_mut()
                    .get_mut::<FeathersGraphNode>(fixture.nodes[0])
                    .unwrap()
                    .collapsed = true;
                request(&mut app, &fixture, GraphFrameTarget::Selection);
                advance(&mut app);
                let collapsed = bounds(&app, &fixture, true);
                assert!(collapsed.height() < original.height());
                assert_framed(&app, &fixture, collapsed, GraphFrameTarget::Selection);
                assert_eq!(
                    app.world()
                        .get::<FeathersGraphNode>(fixture.nodes[0])
                        .unwrap()
                        .position,
                    original.min
                );
                assert!(
                    app.world()
                        .resource::<GraphViewportMemory>()
                        .nodes
                        .is_empty()
                );
            }
        }
    }
}

#[test]
fn framing_is_view_local_survives_rebuild_and_defers_hidden_views() {
    for function in [false, true] {
        let (mut app, root) = geometry::tests::layout_app(1.25);
        let first = spawn(&mut app, root, function, 1, 0.5);
        let second = spawn(&mut app, root, function, 2, 1.75);
        for viewport in [first.viewport, second.viewport] {
            let mut node = app.world_mut().get_mut::<Node>(viewport).unwrap();
            node.width = Val::Px(480.0);
            node.flex_shrink = 0.0;
        }
        // Distinct visible geometry in a second view of the same document.
        app.world_mut()
            .get_mut::<FeathersGraphNode>(second.nodes[0])
            .unwrap()
            .position = Vec2::new(300.0, 0.0);
        app.world_mut()
            .get_mut::<Node>(second.nodes[0])
            .unwrap()
            .left = Val::Px(300.0);
        app.world_mut()
            .get_mut::<Node>(second.bodies[0])
            .unwrap()
            .height = Val::Px(300.0);
        advance(&mut app);
        request(&mut app, &first, GraphFrameTarget::Selection);
        advance(&mut app);
        assert_framed(
            &app,
            &first,
            bounds(&app, &first, true),
            GraphFrameTarget::Selection,
        );
        assert_eq!(
            app.world()
                .get::<FeathersGraphViewport>(second.viewport)
                .unwrap()
                .zoom,
            1.75
        );
        request(&mut app, &second, GraphFrameTarget::Selection);
        advance(&mut app);
        assert_framed(
            &app,
            &second,
            bounds(&app, &second, true),
            GraphFrameTarget::Selection,
        );

        app.world_mut().despawn(first.viewport);
        let rebuilt = spawn(&mut app, root, function, 1, 0.5);
        // An old same-key snapshot must not be usable for the new entity before collection.
        assert!(
            app.world()
                .resource::<GraphGeometryRegistry>()
                .mounted_view_snapshot(&rebuilt.key, rebuilt.viewport)
                .is_none()
        );
        app.world_mut()
            .get_mut::<Node>(rebuilt.viewport)
            .unwrap()
            .display = Display::None;
        request(&mut app, &rebuilt, GraphFrameTarget::Selection);
        advance(&mut app);
        assert!(
            app.world()
                .get::<FeathersGraphViewport>(rebuilt.viewport)
                .unwrap()
                .frame_request
                .is_some()
        );
        app.world_mut()
            .get_mut::<Node>(rebuilt.viewport)
            .unwrap()
            .display = Display::Flex;
        app.world_mut()
            .get_mut::<FeathersGraphNode>(rebuilt.nodes[0])
            .unwrap()
            .selected = false;
        request(&mut app, &rebuilt, GraphFrameTarget::Selection);
        advance(&mut app);
        // Empty selection falls back to measured ALL, not stale selected/bootstrap bounds.
        // The two-view width can force MIN_ZOOM; compare the formula without a padding assertion.
        let viewport = app
            .world()
            .get::<FeathersGraphViewport>(rebuilt.viewport)
            .unwrap();
        let computed = app.world().get::<ComputedNode>(rebuilt.viewport).unwrap();
        let expected = framed_graph_view(
            bounds(&app, &rebuilt, false),
            computed.size() * computed.inverse_scale_factor,
            MAX_ZOOM,
        );
        assert!((viewport.pan - expected.pan).length() < 0.01);
        assert!((viewport.zoom - expected.zoom).abs() < 0.0001);
    }
}
