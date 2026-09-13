use super::super::*;
use super::*;
use aestra_core::MaterialProgramId;

fn key(project: &str, view: u64) -> GraphViewKey {
    GraphViewKey {
        document: GraphDocumentKey {
            project: project.into(),
            asset: DocumentKey::MaterialProgram(MaterialProgramId::from_u128(1)),
        },
        view: Some(EditorViewId(view)),
    }
}

fn raw() -> ViewObservation {
    ViewObservation {
        entity: Entity::from_bits(1),
        generation: Some(DocumentId(1)),
        expected: BTreeSet::from([GraphNodeKey::MaterialOutputs]),
        complete: true,
        nodes: BTreeMap::from([(
            GraphNodeKey::MaterialOutputs,
            NodeObservation {
                entity: Entity::from_bits(2),
                content: 1,
                preview: false,
                collapsed: false,
                inverse_scale: 1.0,
                geometry: GraphNodeGeometry {
                    effective_position: Vec2::new(50.0, 30.0),
                    size: Vec2::new(224.0, 80.0),
                    compact_size: Vec2::new(224.0, 80.0),
                    preview: false,
                    collapsed: false,
                    content: 1,
                    ports: vec![],
                    geometry_revision: 0,
                },
            },
        )]),
    }
}

fn settle(
    registry: &mut GraphGeometryRegistry,
    views: &[(GraphViewKey, ViewObservation)],
    focused: Option<EditorViewId>,
) {
    for _ in 0..STABLE_FRAMES {
        registry.observe(views.iter().cloned().collect(), focused);
    }
}

#[test]
fn stable_preview_resize_emits_once_and_never_moves_a_node() {
    let key = key("project", 1);
    let mut raw = raw();
    let mut registry = GraphGeometryRegistry::default();
    settle(&mut registry, &[(key.clone(), raw.clone())], None);
    assert!(registry.changes().is_empty());
    let before = registry.snapshot(&key.document).unwrap().nodes[&GraphNodeKey::MaterialOutputs]
        .effective_position;
    for (preview, height, reason) in [
        (true, 300.0, GraphResizeReason::PreviewOpened),
        (false, 80.0, GraphResizeReason::PreviewClosed),
    ] {
        let node = raw.nodes.get_mut(&GraphNodeKey::MaterialOutputs).unwrap();
        node.preview = preview;
        node.geometry.size.y = height;
        registry.observe(BTreeMap::from([(key.clone(), raw.clone())]), None);
        assert!(registry.snapshot(&key.document).is_none());
        assert!(registry.changes().is_empty());
        registry.observe(BTreeMap::from([(key.clone(), raw.clone())]), None);
        assert!(
            matches!(registry.changes(), [GraphGeometryEvent { change: GraphGeometryChange::Resized { reason: actual, .. }, .. }] if *actual == reason)
        );
        let snapshot = registry.snapshot(&key.document).unwrap();
        assert_eq!(
            snapshot.nodes[&GraphNodeKey::MaterialOutputs].effective_position,
            before
        );
        assert_eq!(
            snapshot.geometry_revision,
            registry.changes()[0].geometry_revision
        );
        registry.observe(BTreeMap::from([(key.clone(), raw.clone())]), None);
        assert!(registry.changes().is_empty());
    }
}

#[test]
fn move_insert_remove_and_collapse_are_coalesced_after_stability() {
    let key = key("project", 1);
    let mut raw = raw();
    let mut registry = GraphGeometryRegistry::default();
    settle(&mut registry, &[(key.clone(), raw.clone())], None);
    let node = raw.nodes.get_mut(&GraphNodeKey::MaterialOutputs).unwrap();
    node.geometry.effective_position.x += 40.0;
    node.geometry.size.y = 30.0;
    node.collapsed = true;
    settle(&mut registry, &[(key.clone(), raw.clone())], None);
    assert_eq!(registry.changes().len(), 2);
    assert!(matches!(
        registry.changes()[0].change,
        GraphGeometryChange::Moved { .. }
    ));
    assert!(matches!(
        registry.changes()[1].change,
        GraphGeometryChange::Resized {
            reason: GraphResizeReason::Collapsed,
            ..
        }
    ));
    let added = GraphNodeKey::Expression(MaterialExpressionId::from_u128(4));
    raw.expected.insert(added);
    raw.nodes
        .insert(added, raw.nodes[&GraphNodeKey::MaterialOutputs].clone());
    settle(&mut registry, &[(key.clone(), raw.clone())], None);
    assert!(
        matches!(registry.changes()[0].change, GraphGeometryChange::Inserted { node } if node == added)
    );
    raw.nodes.remove(&added); // Not measured yet is NOT semantic removal.
    settle(&mut registry, &[(key.clone(), raw.clone())], None);
    assert!(registry.snapshot(&key.document).is_none());
    assert!(registry.changes().is_empty());
    raw.expected.remove(&added);
    settle(&mut registry, &[(key, raw)], None);
    assert!(
        matches!(registry.changes()[0].change, GraphGeometryChange::Removed { node } if node == added)
    );
}

#[test]
fn owner_handoff_rebuild_dpi_and_document_generation_are_baselines() {
    let first = key("project", 1);
    let second = key("project", 2);
    let mut registry = GraphGeometryRegistry::default();
    let a = raw();
    let mut b = raw();
    b.nodes
        .get_mut(&GraphNodeKey::MaterialOutputs)
        .unwrap()
        .geometry
        .size
        .y = 120.0;
    settle(
        &mut registry,
        &[(first.clone(), a.clone()), (second.clone(), b.clone())],
        Some(EditorViewId(2)),
    );
    assert_eq!(
        registry.snapshot(&first.document).unwrap().measured_in,
        second
    );
    registry.observe(
        BTreeMap::from([(first.clone(), a.clone()), (second.clone(), b)]),
        None,
    );
    assert_eq!(
        registry.snapshot(&first.document).unwrap().measured_in,
        second
    );
    assert!(registry.changes().is_empty());
    // Closing/hiding the owner falls back to the first valid view, without requesting movement.
    registry.observe(BTreeMap::from([(first.clone(), a.clone())]), None);
    assert_eq!(
        registry.snapshot(&first.document).unwrap().measured_in,
        first
    );
    assert!(registry.view_snapshot(&second).is_none());
    assert!(registry.changes().is_empty());
    let mut rebuilt = a;
    rebuilt.entity = Entity::from_bits(3);
    rebuilt
        .nodes
        .get_mut(&GraphNodeKey::MaterialOutputs)
        .unwrap()
        .geometry
        .size
        .y += 10.0;
    settle(&mut registry, &[(first.clone(), rebuilt.clone())], None);
    assert!(registry.changes().is_empty());
    let node = rebuilt
        .nodes
        .get_mut(&GraphNodeKey::MaterialOutputs)
        .unwrap();
    node.inverse_scale = 0.5;
    node.geometry.size.y += 1.0;
    settle(&mut registry, &[(first.clone(), rebuilt.clone())], None);
    assert!(registry.changes().is_empty());
    rebuilt.generation = Some(DocumentId(2));
    rebuilt
        .nodes
        .get_mut(&GraphNodeKey::MaterialOutputs)
        .unwrap()
        .preview = true;
    rebuilt
        .nodes
        .get_mut(&GraphNodeKey::MaterialOutputs)
        .unwrap()
        .geometry
        .size
        .y = 300.0;
    settle(&mut registry, &[(first.clone(), rebuilt)], None);
    assert!(registry.changes().is_empty());
    registry.observe(BTreeMap::new(), None);
    assert!(registry.snapshot(&first.document).is_none());
    assert!(registry.views.is_empty());
    assert!(registry.documents.is_empty());
}

#[test]
fn same_asset_ids_are_scoped_to_project_and_stale_results_have_different_revisions() {
    let first = key("first", 1);
    let second = key("second", 1);
    let mut registry = GraphGeometryRegistry::default();
    settle(&mut registry, &[(first.clone(), raw())], None);
    let revision = registry
        .snapshot(&first.document)
        .unwrap()
        .geometry_revision;
    settle(&mut registry, &[(second.clone(), raw())], None);
    assert!(registry.snapshot(&first.document).is_none());
    assert!(
        registry
            .snapshot(&second.document)
            .unwrap()
            .geometry_revision
            > revision
    );
    assert!(registry.changes().is_empty());
}

#[test]
fn transient_sizes_and_rounding_noise_do_not_emit_resizes() {
    let key = key("project", 1);
    let mut raw = raw();
    let mut registry = GraphGeometryRegistry::default();
    settle(&mut registry, &[(key.clone(), raw.clone())], None);
    let revision = registry.snapshot(&key.document).unwrap().geometry_revision;
    for noise in [0.25, -0.25, 0.4, 0.0] {
        raw.nodes
            .get_mut(&GraphNodeKey::MaterialOutputs)
            .unwrap()
            .geometry
            .size
            .y = 80.0 + noise;
        registry.observe(BTreeMap::from([(key.clone(), raw.clone())]), None);
        assert_eq!(
            registry.snapshot(&key.document).unwrap().geometry_revision,
            revision
        );
        assert!(registry.changes().is_empty());
    }
    raw.nodes
        .get_mut(&GraphNodeKey::MaterialOutputs)
        .unwrap()
        .geometry
        .size
        .y = 150.0;
    registry.observe(BTreeMap::from([(key.clone(), raw.clone())]), None);
    raw.nodes
        .get_mut(&GraphNodeKey::MaterialOutputs)
        .unwrap()
        .geometry
        .size
        .y = 190.0;
    registry.observe(BTreeMap::from([(key.clone(), raw.clone())]), None);
    assert!(registry.changes().is_empty());
    registry.observe(BTreeMap::from([(key, raw)]), None);
    assert!(
        matches!(registry.changes()[0].change, GraphGeometryChange::Resized { old_size, new_size, .. } if old_size.y == 80.0 && new_size.y == 190.0)
    );
}

/// Uses Bevy's real UI layout and text systems, without a window event loop or GPU.
pub(crate) fn enable_overlays(app: &mut App) {
    app.init_resource::<overlay::GraphOverlays>().add_systems(
        Update,
        overlay::reconcile
            .after(restore_graph_nodes)
            .before(sync_graph_nodes_from_memory),
    );
}

pub(crate) fn layout_app(scale: f32) -> (App, Entity) {
    use bevy::{
        app::{HierarchyPropagatePlugin, PropagateSet},
        camera::{ComputedCameraValues, RenderTargetInfo, Viewport},
        ui::{
            ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system,
            ui_surface::UiSurface,
            update::propagate_ui_target_cameras,
            widget::{measure_text_system, text_system},
        },
    };
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        bevy::scene::ScenePlugin,
        bevy::text::TextPlugin,
        HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate),
        HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate),
    ))
    .init_asset::<Image>()
    .init_asset::<bevy_resvg::prelude::SvgFile>()
    .init_resource::<UiScale>()
    .init_resource::<UiSurface>()
    .init_resource::<GraphViewportMemory>()
    .add_systems(
        Update,
        (
            restore_graph_nodes,
            sync_graph_nodes_from_memory,
            sync_graph_viewport_transforms,
        )
            .chain(),
    )
    .add_systems(
        PostUpdate,
        (
            propagate_ui_target_cameras,
            measure_text_system,
            ui_layout_system,
            text_system,
        )
            .chain()
            .after(bevy::text::load_font_assets_into_font_collection)
            .before(bevy::ui::UiSystems::PostLayout),
    )
    .configure_sets(
        PostUpdate,
        PropagateSet::<ComputedUiTargetCamera>::default()
            .after(propagate_ui_target_cameras)
            .before(measure_text_system),
    )
    .configure_sets(
        PostUpdate,
        PropagateSet::<ComputedUiRenderTargetInfo>::default()
            .after(propagate_ui_target_cameras)
            .before(measure_text_system),
    );
    register(&mut app);
    let size = UVec2::new((960.0 * scale) as u32, (640.0 * scale) as u32);
    let window = app.world_mut().spawn(Window::default()).id();
    let camera = app
        .world_mut()
        .spawn((
            Camera2d,
            bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window)),
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: size,
                        scale_factor: scale,
                    }),
                    ..default()
                },
                viewport: Some(Viewport {
                    physical_size: size,
                    ..default()
                }),
                ..default()
            },
        ))
        .id();
    let root = app
        .world_mut()
        .spawn((
            Node {
                width: Val::Px(960.0),
                height: Val::Px(640.0),
                ..default()
            },
            UiTargetCamera(camera),
        ))
        .id();
    (app, root)
}

fn advance(app: &mut App) -> Vec<GraphGeometryEvent> {
    let mut events = Vec::new();
    for _ in 0..4 {
        app.update();
        events.extend_from_slice(app.world().resource::<GraphGeometryRegistry>().changes());
    }
    events
}

/// Verify the actual adapters' initial frame uses measured content, including restored positions.
pub(crate) fn assert_frame_all(world: &mut World) {
    let toolbar_keys = world
        .query::<&GraphFrameAction>()
        .iter(world)
        .map(|action| action.key.clone())
        .collect::<BTreeSet<_>>();
    let mut query = world.query::<(
        Entity,
        &GraphGeometryView,
        &FeathersGraphViewport,
        &ComputedNode,
    )>();
    let registry = world.resource::<GraphGeometryRegistry>();
    let mut count = 0;
    for (entity, marker, viewport, computed) in query.iter(world) {
        assert!(toolbar_keys.contains(&viewport.key));
        if let Some(view) = marker.key.view {
            assert!(viewport.key.ends_with(&format!("#view:{}", view.0)));
        }
        let snapshot = registry.mounted_view_snapshot(&marker.key, entity).unwrap();
        let bounds = snapshot.bounds(|_| true).unwrap();
        let expected =
            framed_graph_view(bounds, computed.size() * computed.inverse_scale_factor, 1.0);
        assert!(viewport.frame_request.is_none());
        assert!((viewport.pan - expected.pan).length() < 0.01);
        assert!((viewport.zoom - expected.zoom).abs() < 0.0001);
        count += 1;
    }
    assert!(count > 0);
}

#[test]
fn real_layout_normalizes_ports_and_preview_resize_across_zoom_and_dpi() {
    for scale in [1.0, 1.25, 2.0] {
        for zoom in [0.5, 1.0, 1.75] {
            let (mut app, root) = layout_app(scale);
            let key = key("project", 1);
            let mut node_entity = Entity::PLACEHOLDER;
            let mut body_entity = Entity::PLACEHOLDER;
            let mut viewport_entity = Entity::PLACEHOLDER;
            app.world_mut()
                .commands()
                .entity(root)
                .with_children(|parent| {
                    viewport_entity = spawn_graph_viewport(
                        parent,
                        GraphViewportProps {
                            key: "material:p#view:1".into(),
                            content_size: Vec2::new(720.0, 420.0),
                            selection_bounds: None,
                            initial_view: Some((Vec2::new(60.0, -30.0), zoom)),
                        },
                        (),
                        |_| {},
                        |canvas| {
                            node_entity = spawn_graph_node(
                                canvas,
                                GraphNodeProps {
                                    graph_key: "material:p".into(),
                                    node_key: "output".into(),
                                    title: "Measured node".into(),
                                    position: Vec2::new(50.0, 40.0),
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
                                    body_entity = body.target_entity();
                                    spawn_graph_port(
                                        body,
                                        GraphPortProps {
                                            label: Some("Value".into()),
                                            tooltip_title: "Value".into(),
                                            tooltip_description: "".into(),
                                            side: GraphSocketSide::Input,
                                            color: Color::WHITE,
                                        },
                                        GraphGeometryPort::Input(MaterialExpressionInput::Value),
                                    );
                                },
                            );
                        },
                    );
                    parent
                        .commands()
                        .entity(viewport_entity)
                        .insert(GraphGeometryView {
                            key: key.clone(),
                            nodes: BTreeSet::from([GraphNodeKey::MaterialOutputs]),
                        });
                });
            app.world_mut().flush();
            // Apply the same shared canvas transform as the runtime, without auto-framing.
            let world = app.world_mut();
            for (canvas, mut transform) in world
                .query::<(&FeathersGraphCanvas, &mut UiTransform)>()
                .iter_mut(world)
            {
                if canvas.viewport == viewport_entity {
                    *transform = graph_canvas_transform(
                        Vec2::new(60.0, -30.0),
                        zoom,
                        Vec2::new(720.0, 420.0),
                    );
                }
            }
            assert!(advance(&mut app).is_empty());
            let before = app
                .world()
                .resource::<GraphGeometryRegistry>()
                .snapshot(&key.document)
                .unwrap()
                .nodes[&GraphNodeKey::MaterialOutputs]
                .clone();
            assert!((before.size.x - NODE_WIDTH).abs() <= GEOMETRY_EPSILON);
            assert_eq!(before.ports.len(), 1);
            assert!(
                (before.ports[0].offset.x - 15.0).abs() <= 1.0,
                "{:?}",
                before.ports
            );
            let mut preview = Entity::PLACEHOLDER;
            app.world_mut()
                .commands()
                .entity(body_entity)
                .with_children(|body| {
                    preview = spawn_graph_node_preview(body, ());
                });
            app.world_mut().flush();
            app.world_mut()
                .get_mut::<GraphGeometryNode>(node_entity)
                .unwrap()
                .preview = true;
            let events = advance(&mut app);
            assert_eq!(events.len(), 1, "{scale} {zoom}: {events:?}");
            assert!(matches!(
                events[0].change,
                GraphGeometryChange::Resized {
                    reason: GraphResizeReason::PreviewOpened,
                    ..
                }
            ));
            let after = app
                .world()
                .resource::<GraphGeometryRegistry>()
                .snapshot(&key.document)
                .unwrap()
                .nodes[&GraphNodeKey::MaterialOutputs]
                .clone();
            assert!(after.size.y - before.size.y > NODE_PREVIEW_SIZE);
            assert_eq!(after.effective_position, before.effective_position);
            assert!(close(before.ports[0].offset, after.ports[0].offset));
            assert!(
                app.world()
                    .resource::<GraphViewportMemory>()
                    .nodes
                    .is_empty()
            );
            app.world_mut().despawn(preview);
            app.world_mut()
                .get_mut::<GraphGeometryNode>(node_entity)
                .unwrap()
                .preview = false;
            let events = advance(&mut app);
            assert_eq!(events.len(), 1);
            assert!(matches!(
                events[0].change,
                GraphGeometryChange::Resized {
                    reason: GraphResizeReason::PreviewClosed,
                    ..
                }
            ));
            app.world_mut().get_mut::<Node>(root).unwrap().display = Display::None;
            assert!(advance(&mut app).is_empty());
            assert!(
                app.world()
                    .resource::<GraphGeometryRegistry>()
                    .snapshot(&key.document)
                    .is_none()
            );
        }
    }
}
