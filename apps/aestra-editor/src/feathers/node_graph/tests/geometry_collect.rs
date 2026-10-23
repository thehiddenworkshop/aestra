use super::super::model::{GraphDocumentKey, GraphGeometryChange, GraphNodeKey, GraphResizeReason};
use super::*;
use crate::editor_identity::DocumentKey;
use crate::editor_view_registry::{EditorViewKind, open_document_view};
use aestra_core::MaterialProgramId;
use bevy::{ecs::system::RunSystemOnce, math::Affine2};
use std::collections::BTreeSet;

struct Scene {
    world: World,
    view: Entity,
    node: Entity,
    port: Entity,
    key: GraphViewKey,
}

fn asset(seed: u128) -> DocumentKey {
    DocumentKey::MaterialProgram(MaterialProgramId::from_u128(seed))
}
fn viewport() -> FeathersGraphViewport {
    FeathersGraphViewport {
        key: "view".into(),
        pan: Vec2::ZERO,
        zoom: 1.0,
        content_size: Vec2::splat(800.0),
        selection_bounds: None,
        frame_request: None,
        measured_frame: None,
    }
}
fn graph_node() -> FeathersGraphNode {
    FeathersGraphNode {
        graph_key: "graph".into(),
        node_key: "output".into(),
        position: Vec2::new(50.0, 40.0),
        selected: false,
        pinned: false,
        collapsed: false,
        dragging: false,
        drag_before: None,
        drag_modifier: None,
        suppress_release_click: false,
    }
}
fn computed(size: Vec2, scale: f32) -> ComputedNode {
    ComputedNode {
        size: size * scale,
        inverse_scale_factor: scale.recip(),
        ..default()
    }
}
fn scene(scale: f32, zoom: f32) -> Scene {
    let mut world = World::new();
    world.init_resource::<GraphGeometryRegistry>();
    world.init_resource::<DocumentManager>();
    world.init_resource::<EditorViewManager>();
    world.init_resource::<ActiveEditorContext>();
    // The same production open path used by the host, not a fabricated view/document mapping.
    let mut documents = DocumentManager::default();
    let mut views = EditorViewManager::default();
    let mut active = ActiveEditorContext::default();
    let id = open_document_view(
        &mut documents,
        &mut views,
        &mut active,
        asset(1),
        EditorViewKind::MaterialGraph,
    );
    world.insert_resource(documents);
    world.insert_resource(views);
    world.insert_resource(active);
    let key = GraphViewKey {
        document: GraphDocumentKey {
            project: "project".into(),
            asset: asset(1),
        },
        view: Some(id),
    };
    let view = world
        .spawn((
            viewport(),
            Node::default(),
            computed(Vec2::splat(800.0), scale),
            GraphGeometryView {
                key: key.clone(),
                nodes: BTreeSet::from([GraphNodeKey::MaterialOutputs]),
            },
        ))
        .id();
    let size = Vec2::new(224.0, 80.0);
    let center = Vec2::new(400.0, 250.0) * scale;
    let node = world
        .spawn((
            graph_node(),
            Node::default(),
            computed(size, scale),
            UiGlobalTransform::from(Affine2::from_scale_angle_translation(
                Vec2::splat(zoom),
                0.0,
                center,
            )),
            GraphGeometryNode::new(GraphNodeKey::MaterialOutputs, &"body", false),
            ChildOf(view),
        ))
        .id();
    let offset = Vec2::new(15.0, 35.0);
    let port = world
        .spawn((
            GraphGeometryPort::Output,
            Node::default(),
            computed(Vec2::splat(10.0), scale),
            UiGlobalTransform::from(Affine2::from_translation(
                center + (offset - size * 0.5) * scale * zoom,
            )),
            ChildOf(node),
        ))
        .id();
    Scene {
        world,
        view,
        node,
        port,
        key,
    }
}
impl Scene {
    fn collect(&mut self, root: Option<&str>) {
        self.world
            .run_system_once_with(collect_graph_geometry, root.map(PathBuf::from))
            .unwrap();
    }
    fn settle(&mut self) {
        self.collect(Some("project"));
        self.collect(Some("project"));
    }
    fn snapshot(&self) -> Option<&super::super::model::GraphGeometrySnapshot> {
        self.world
            .resource::<GraphGeometryRegistry>()
            .mounted_view_snapshot(&self.key, self.view)
    }
}

#[test]
fn real_ui_layout_feeds_measured_ports_and_preview_extent_into_the_collector() {
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
    for scale in [1.0, 1.25, 2.0] {
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
        .init_resource::<UiScale>()
        .init_resource::<bevy::input_focus::InputFocus>()
        .init_resource::<UiSurface>()
        .init_resource::<GraphGeometryRegistry>()
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
        )
        .add_systems(
            PostUpdate,
            (|| Some(PathBuf::from("project")))
                .pipe(collect_graph_geometry)
                .after(bevy::ui::UiSystems::PostLayout),
        );
        let mut documents = DocumentManager::default();
        let mut views = EditorViewManager::default();
        let mut active = ActiveEditorContext::default();
        let id = open_document_view(
            &mut documents,
            &mut views,
            &mut active,
            asset(1),
            EditorViewKind::MaterialGraph,
        );
        app.insert_resource(documents)
            .insert_resource(views)
            .insert_resource(active);
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
        let key = GraphViewKey {
            document: GraphDocumentKey {
                project: "project".into(),
                asset: asset(1),
            },
            view: Some(id),
        };
        let view = app
            .world_mut()
            .spawn((
                viewport(),
                GraphGeometryView {
                    key: key.clone(),
                    nodes: BTreeSet::from([GraphNodeKey::MaterialOutputs]),
                },
                Node {
                    width: Val::Px(960.0),
                    height: Val::Px(640.0),
                    ..default()
                },
                UiTargetCamera(camera),
            ))
            .id();
        let node = app
            .world_mut()
            .spawn((
                graph_node(),
                GraphGeometryNode::new(GraphNodeKey::MaterialOutputs, &"body", true),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(50.0),
                    top: Val::Px(40.0),
                    width: Val::Px(224.0),
                    height: Val::Px(220.0),
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                ChildOf(view),
            ))
            .id();
        app.world_mut().spawn((
            GraphGeometryPort::Output,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(10.0),
                top: Val::Px(30.0),
                width: Val::Px(10.0),
                height: Val::Px(10.0),
                ..default()
            },
            ChildOf(node),
        ));
        app.world_mut().spawn((
            FeathersGraphNodePreview,
            Node {
                width: Val::Px(100.0),
                height: Val::Px(120.0),
                flex_shrink: 0.0,
                margin: UiRect {
                    top: Val::Px(10.0),
                    bottom: Val::Px(10.0),
                    ..default()
                },
                ..default()
            },
            ChildOf(node),
        ));
        for _ in 0..4 {
            app.update();
        }
        let snapshot = app
            .world()
            .resource::<GraphGeometryRegistry>()
            .mounted_view_snapshot(&key, view)
            .unwrap();
        let measured = &snapshot.nodes[&GraphNodeKey::MaterialOutputs];
        assert!(measured.size.distance(Vec2::new(224.0, 220.0)) <= 0.5);
        assert!(measured.compact_size.distance(Vec2::new(224.0, 80.0)) <= 0.5);
        // Child edges and parent centers round independently. Allow one physical layout
        // pixel per axis; do not mistake fractional-DPI raster rounding for graph drift.
        assert!(
            (measured.ports[0].offset - Vec2::new(15.0, 35.0))
                .abs()
                .max_element()
                <= scale.recip(),
            "scale {scale}: {measured:?}"
        );
        assert!(snapshot.document_generation.is_some());
        assert!(
            app.world()
                .resource::<GraphGeometryRegistry>()
                .changes()
                .is_empty()
        );
    }
}

#[test]
fn actual_collector_normalizes_ports_at_all_scales_and_zoom_without_mutating_documents() {
    for scale in [1.0, 1.25, 2.0] {
        for zoom in [0.5, 1.0, 1.75] {
            let mut s = scene(scale, zoom);
            s.settle();
            let snapshot = s.snapshot().unwrap();
            let node = &snapshot.nodes[&GraphNodeKey::MaterialOutputs];
            assert_eq!(node.size, Vec2::new(224.0, 80.0));
            assert!(node.ports[0].offset.distance(Vec2::new(15.0, 35.0)) < 0.001);
            assert_eq!(node.effective_position, Vec2::new(50.0, 40.0));
            let documents = s.world.resource::<DocumentManager>();
            let document = documents.find(&asset(1)).unwrap();
            assert_eq!(snapshot.document_generation, Some(document));
            assert!(!documents.is_dirty(document));
            assert_eq!(documents.revision(document).unwrap().0, 0);
            assert_eq!(
                s.world.get::<FeathersGraphNode>(s.node).unwrap().position,
                Vec2::new(50.0, 40.0)
            );
        }
    }
}

#[test]
fn docked_views_reject_stale_wrong_asset_and_partially_removed_authorities() {
    for mode in 0..5 {
        let mut s = scene(1.0, 1.0);
        s.settle();
        assert!(s.snapshot().is_some());
        match mode {
            0 => {
                s.world
                    .resource_mut::<EditorViewManager>()
                    .close_view(s.key.view.unwrap());
            }
            1 => {
                let id = s
                    .world
                    .resource::<DocumentManager>()
                    .find(&asset(1))
                    .unwrap();
                s.world.resource_mut::<DocumentManager>().close(id);
            }
            2 => {
                s.world
                    .get_mut::<GraphGeometryView>(s.view)
                    .unwrap()
                    .key
                    .document
                    .asset = asset(2);
            }
            3 => {
                s.world.remove_resource::<DocumentManager>();
            }
            _ => {
                s.world.remove_resource::<EditorViewManager>();
            }
        }
        s.collect(Some("project"));
        assert!(s.snapshot().is_none());
        assert!(
            s.world
                .resource::<GraphGeometryRegistry>()
                .snapshot(&s.key.document)
                .is_none()
        );
    }
}

#[test]
fn current_project_input_retires_old_root_immediately_without_mirrored_state() {
    let mut s = scene(1.0, 1.0);
    s.settle();
    assert!(s.snapshot().is_some());
    s.collect(Some("other"));
    assert!(s.snapshot().is_none());
    s.settle();
    assert!(s.snapshot().is_some());
    assert!(
        s.world
            .resource::<GraphGeometryRegistry>()
            .changes()
            .is_empty()
    );
}

#[test]
fn closed_and_reopened_document_requires_a_fresh_view_and_generation_baseline() {
    let mut s = scene(1.0, 1.0);
    s.settle();
    let old = s.snapshot().unwrap().document_generation.unwrap();
    s.world.resource_mut::<DocumentManager>().close(old);
    let new = s.world.resource_mut::<DocumentManager>().open(asset(1));
    assert_ne!(new, old);
    s.collect(Some("project"));
    assert!(s.snapshot().is_none());
    s.world.resource_mut::<EditorViewManager>().restore_view(
        s.key.view.unwrap(),
        new,
        EditorViewKind::MaterialGraph,
    );
    s.settle();
    assert_eq!(s.snapshot().unwrap().document_generation, Some(new));
    assert!(
        s.world
            .resource::<GraphGeometryRegistry>()
            .changes()
            .is_empty()
    );
}

#[test]
fn duplicate_live_view_or_node_or_port_identities_never_publish_partial_geometry() {
    for mode in 0..3 {
        let mut s = scene(1.0, 1.0);
        match mode {
            0 => {
                let meta = s.world.get::<GraphGeometryView>(s.view).unwrap().clone();
                s.world.spawn((
                    viewport(),
                    meta,
                    Node::default(),
                    computed(Vec2::splat(800.0), 1.0),
                ));
            }
            1 => {
                let meta = s.world.get::<GraphGeometryNode>(s.node).unwrap().clone();
                s.world.spawn((
                    graph_node(),
                    meta,
                    Node::default(),
                    computed(Vec2::new(224.0, 80.0), 1.0),
                    UiGlobalTransform::default(),
                    ChildOf(s.view),
                ));
            }
            _ => {
                s.world.spawn((
                    GraphGeometryPort::Output,
                    Node::default(),
                    computed(Vec2::splat(10.0), 1.0),
                    UiGlobalTransform::default(),
                    ChildOf(s.node),
                ));
            }
        }
        s.settle();
        assert!(s.snapshot().is_none());
        assert!(
            s.world
                .resource::<GraphGeometryRegistry>()
                .changes()
                .is_empty()
        );
    }
}

#[test]
fn hidden_ancestors_and_invalid_transforms_make_geometry_unavailable() {
    for mode in 0..4 {
        let mut s = scene(1.0, 1.0);
        s.settle();
        assert!(s.snapshot().is_some());
        match mode {
            0 => {
                s.world.get_mut::<Node>(s.view).unwrap().display = Display::None;
            }
            1 => {
                s.world.entity_mut(s.view).insert(Visibility::Hidden);
            }
            2 => {
                s.world
                    .entity_mut(s.node)
                    .insert(UiGlobalTransform::from_scale(Vec2::ZERO));
            }
            _ => {
                s.world.get_mut::<ComputedNode>(s.node).unwrap().size = Vec2::splat(f32::NAN);
            }
        }
        s.collect(Some("project"));
        assert!(s.snapshot().is_none());
    }
}

#[test]
fn previews_use_measured_height_and_margins_without_baking_presentation_positions() {
    let mut s = scene(2.0, 1.75);
    s.settle();
    s.world.get_mut::<ComputedNode>(s.node).unwrap().size.y = 220.0 * 2.0;
    s.world
        .get_mut::<GraphGeometryNode>(s.node)
        .unwrap()
        .preview = true;
    s.world.spawn((
        FeathersGraphNodePreview,
        computed(Vec2::new(100.0, 120.0), 2.0),
        Node {
            margin: UiRect {
                top: Val::Px(10.0),
                bottom: Val::Px(10.0),
                ..default()
            },
            ..default()
        },
        ChildOf(s.node),
    ));
    s.settle();
    let snapshot = s.snapshot().unwrap();
    assert_eq!(
        snapshot.nodes[&GraphNodeKey::MaterialOutputs].compact_size,
        Vec2::new(224.0, 80.0)
    );
    let changes = s.world.resource::<GraphGeometryRegistry>().changes();
    assert_eq!(changes.len(), 1);
    assert!(matches!(
        changes[0].change,
        GraphGeometryChange::Resized {
            reason: GraphResizeReason::PreviewOpened,
            ..
        }
    ));
    assert_eq!(
        s.world.get::<FeathersGraphNode>(s.node).unwrap().position,
        Vec2::new(50.0, 40.0)
    );
}

#[test]
fn valid_focused_split_view_wins_but_mismatched_active_document_cannot_steal_owner() {
    let mut s = scene(1.0, 1.0);
    s.settle();
    let document = s
        .world
        .resource::<DocumentManager>()
        .find(&asset(1))
        .unwrap();
    let other_id = s
        .world
        .resource_mut::<EditorViewManager>()
        .create_view(document, EditorViewKind::MaterialGraph);
    let mut other_key = s.key.clone();
    other_key.view = Some(other_id);
    let other_view = s
        .world
        .spawn((
            viewport(),
            Node::default(),
            computed(Vec2::splat(800.0), 1.0),
            GraphGeometryView {
                key: other_key.clone(),
                nodes: BTreeSet::from([GraphNodeKey::MaterialOutputs]),
            },
        ))
        .id();
    s.world.spawn((
        graph_node(),
        Node::default(),
        computed(Vec2::new(400.0, 160.0), 1.0),
        UiGlobalTransform::default(),
        GraphGeometryNode::new(GraphNodeKey::MaterialOutputs, &"other-body", false),
        ChildOf(other_view),
    ));
    s.world.resource_mut::<ActiveEditorContext>().active_view = Some(other_id);
    let unrelated = s.world.resource_mut::<DocumentManager>().open(asset(2));
    s.world
        .resource_mut::<ActiveEditorContext>()
        .active_document = Some(unrelated);
    s.settle();
    assert_eq!(
        s.world
            .resource::<GraphGeometryRegistry>()
            .snapshot(&s.key.document)
            .unwrap()
            .measured_in,
        s.key
    );
    s.world
        .resource_mut::<ActiveEditorContext>()
        .active_document = Some(document);
    s.collect(Some("project"));
    let registry = s.world.resource::<GraphGeometryRegistry>();
    assert_eq!(
        registry.snapshot(&s.key.document).unwrap().measured_in,
        other_key
    );
    assert!(registry.changes().is_empty()); // View-owner handoff establishes a baseline.
    assert_eq!(
        s.snapshot().unwrap().nodes[&GraphNodeKey::MaterialOutputs].size,
        Vec2::new(224.0, 80.0)
    );
}

#[test]
fn collapsed_hidden_ports_are_omitted_and_invalid_visible_ports_defer_snapshot() {
    let mut s = scene(1.0, 1.0);
    s.settle();
    s.world.get_mut::<Node>(s.port).unwrap().display = Display::None;
    s.world
        .get_mut::<FeathersGraphNode>(s.node)
        .unwrap()
        .collapsed = true;
    s.settle();
    assert!(
        s.snapshot().unwrap().nodes[&GraphNodeKey::MaterialOutputs]
            .ports
            .is_empty()
    );
    s.world.get_mut::<Node>(s.port).unwrap().display = Display::Flex;
    s.world.get_mut::<ComputedNode>(s.port).unwrap().size = Vec2::ZERO;
    s.collect(Some("project"));
    assert!(s.snapshot().is_none());
}

#[test]
fn tool_panel_resolves_document_without_requiring_a_docked_view() {
    let mut s = scene(1.0, 1.0);
    s.key.view = None;
    s.world
        .get_mut::<GraphGeometryView>(s.view)
        .unwrap()
        .key
        .view = None;
    s.world.remove_resource::<EditorViewManager>();
    s.settle();
    assert!(s.snapshot().unwrap().document_generation.is_some());
}

#[test]
fn invalid_measurements_are_unavailable_not_zero_sized_geometry() {
    for (size, inverse_scale) in [
        (Vec2::ZERO, 1.0),
        (Vec2::new(-1.0, 10.0), 1.0),
        (Vec2::new(f32::NAN, 20.0), 1.0),
        (Vec2::splat(f32::INFINITY), 1.0),
        (Vec2::splat(40.0), 0.0),
        (Vec2::splat(40.0), f32::NAN),
    ] {
        assert!(
            logical_size(&ComputedNode {
                size,
                inverse_scale_factor: inverse_scale,
                ..default()
            })
            .is_none()
        );
    }
    let node = ComputedNode {
        size: Vec2::new(448.0, 160.0),
        inverse_scale_factor: 0.5,
        ..default()
    };
    assert_eq!(logical_size(&node), Some(Vec2::new(224.0, 80.0)));
    assert!(
        port_offset(
            &node,
            &UiGlobalTransform::from_scale(Vec2::ZERO),
            &UiGlobalTransform::default()
        )
        .is_none()
    );
}
