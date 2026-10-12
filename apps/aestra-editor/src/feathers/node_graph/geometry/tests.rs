use super::super::*;
use super::model::{GEOMETRY_EPSILON, close, model_tests::key};
use super::*;
use aestra_authoring::MaterialExpressionInput;
use std::collections::BTreeSet;

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
    .init_asset::<crate::feathers::icon::SvgFile>()
    .init_resource::<UiScale>()
    .init_resource::<bevy::input_focus::InputFocus>()
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
fn host_project_scope_reads_current_catalog_without_a_cached_root_resource() {
    use bevy::ecs::system::RunSystemOnce;
    let mut world = World::new();
    assert_eq!(world.run_system_once(project_root).unwrap(), None);
    let catalog = crate::ProjectEffectCatalog::default();
    let first = catalog.root().to_owned();
    world.insert_resource(catalog);
    assert_eq!(
        world.run_system_once(project_root).unwrap(),
        Some(first.clone())
    );
    let different = first.join("geometry-scope-unavailable-test-project");
    let catalog = crate::ProjectEffectCatalog::scan_project(&different, different.join("effects"));
    let expected = catalog.root().to_owned();
    assert_ne!(expected, first);
    world.insert_resource(catalog);
    assert_eq!(world.run_system_once(project_root).unwrap(), Some(expected));
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
