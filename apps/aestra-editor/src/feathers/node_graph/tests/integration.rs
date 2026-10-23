//! Complete production registration and schedule, not a manually selected consumer chain.
use super::super::{
    composition::*,
    graph_cursor::OverrideCursor,
    graph_pointer,
    materials::GraphGridMaterial,
    navigation::{FeathersGraphCanvas, GraphView, graph_canvas_transform},
    node::*,
    node_control_input,
    toolbar::GraphFrameAction,
    toolbar_input,
};
use super::*;
use crate::feathers::{icon::SvgPlugin, text_input::tests::app_with};
use bevy::{
    picking::pointer::{PointerButton, PointerId},
    ui_widgets::Activate,
    window::PrimaryWindow,
};

#[derive(Resource, Default)]
struct Edits {
    placement: usize,
    pin: usize,
}

pub(super) fn app() -> App {
    app_with(|app| {
        app.add_plugins(SvgPlugin)
            .init_resource::<OverrideCursor>()
            .init_resource::<Edits>()
            .insert_resource(crate::Localizer::new("en-US").unwrap())
            .add_observer(|_: On<GraphPresentationEdit>, mut edits: ResMut<Edits>| {
                edits.placement += 1
            })
            .add_observer(|_: On<GraphPinEdit>, mut edits: ResMut<Edits>| edits.pin += 1);
        register(app);
    })
}

#[derive(Clone, Copy)]
pub(super) struct Mounted {
    pub(super) view: Entity,
    pub(super) grid: Entity,
    pub(super) canvas: Entity,
    pub(super) node: Entity,
    pub(super) socket: Entity,
}

pub(super) fn mount(app: &mut App, key: &str) -> Mounted {
    let mut view = Entity::PLACEHOLDER;
    let mut node = Entity::PLACEHOLDER;
    let mut socket = Entity::PLACEHOLDER;
    app.world_mut()
        .commands()
        .spawn(Node::default())
        .with_children(|parent| {
            view = spawn_graph_viewport(
                parent,
                GraphViewportProps {
                    key: key.into(),
                    content_size: Vec2::new(1000.0, 800.0),
                    selection_bounds: None,
                    initial_view: Some((Vec2::new(15.0, 25.0), 0.75)),
                },
                (),
                |_| {},
                |canvas| {
                    node = spawn_graph_node(
                        canvas,
                        GraphNodeProps {
                            graph_key: "graph".into(),
                            node_key: "n".into(),
                            title: "Node".into(),
                            position: Vec2::new(10.0, 20.0),
                            selected: true,
                            pinned: false,
                            muted: false,
                            collapse_icon: default(),
                            expand_icon: default(),
                            pin_icon: default(),
                            collapse_label: "Collapse".into(),
                            expand_label: "Expand".into(),
                        },
                        (),
                        |_, body| {
                            socket = spawn_graph_port(
                                body,
                                GraphPortProps {
                                    label: Some("Input".into()),
                                    tooltip_title: "Input".into(),
                                    tooltip_description: "Input value".into(),
                                    side: super::super::socket::GraphSocketSide::Input,
                                    color: Color::WHITE,
                                },
                                (),
                            );
                        },
                    );
                },
            );
        });
    app.world_mut().flush();
    let children = app.world().get::<Children>(view).unwrap();
    Mounted {
        view,
        grid: children[0],
        canvas: children[2],
        node,
        socket,
    }
}

fn advance(app: &mut App) {
    for _ in 0..3 {
        app.update();
    }
}
fn window(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .unwrap()
}
fn action<A: Component>(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<A>>()
        .single(world)
        .unwrap()
}
fn activate(app: &mut App, entity: Entity) {
    node_control_input::set_phase(
        app.world_mut(),
        entity,
        super::super::node_controls::SocketPhase::Pressed,
    );
    app.world_mut().trigger(Activate { entity });
}

#[test]
fn actual_registration_initializes_resources_and_applies_grid_and_canvas_together() {
    let mut app = app();
    assert!(
        app.world()
            .contains_resource::<super::super::geometry::GraphGeometryRegistry>()
    );
    assert!(
        app.world()
            .contains_resource::<super::super::overlay::GraphOverlays>()
    );
    assert!(
        app.world()
            .contains_resource::<super::super::drag_assist::State>()
    );
    let m = mount(&mut app, "view");
    app.update();
    let handle = app
        .world()
        .get::<MaterialNode<GraphGridMaterial>>(m.grid)
        .unwrap();
    let material = app
        .world()
        .resource::<Assets<GraphGridMaterial>>()
        .get(&handle.0)
        .unwrap();
    assert_eq!((material.pan, material.zoom), (Vec2::new(15.0, 25.0), 0.75));
    assert_eq!(
        app.world()
            .get::<FeathersGraphCanvas>(m.canvas)
            .unwrap()
            .viewport,
        m.view
    );
    assert_eq!(
        app.world().get::<UiTransform>(m.canvas).unwrap(),
        &graph_canvas_transform(Vec2::new(15.0, 25.0), 0.75, Vec2::new(1000.0, 800.0))
    );
    advance(&mut app);
    assert_eq!(app.world().resource::<Assets<GraphGridMaterial>>().len(), 1);
    assert_eq!(app.world().resource::<Edits>().placement, 0);
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
enum Host {
    Rebuild,
    Readback,
}
#[derive(Resource)]
struct Rebuild(Option<Entity>);

#[test]
fn host_deferred_rebuild_restores_memory_before_same_frame_readback() {
    let mut app = app();
    let m = mount(&mut app, "view");
    advance(&mut app);
    app.insert_resource(Rebuild(Some(m.node)))
        .configure_sets(
            Update,
            (Host::Rebuild, GraphWidgetSync, Host::Readback).chain(),
        )
        .add_systems(
            Update,
            (|mut rebuild: ResMut<Rebuild>,
              mut commands: Commands,
              mut memory: ResMut<GraphViewportMemory>| {
                if let Some(entity) = rebuild.0.take() {
                    memory.set_node("graph", "n", Vec2::new(77.0, 88.0), true);
                    commands
                        .entity(entity)
                        .insert(FeathersGraphNode::clipboard_fixture(
                            "graph".into(),
                            "n".into(),
                            Vec2::ZERO,
                        ));
                }
            })
            .in_set(Host::Rebuild),
        )
        .add_systems(
            Update,
            (|nodes: Query<(&FeathersGraphNode, &Node)>| {
                for (node, style) in &nodes {
                    assert_eq!(node.position, Vec2::new(77.0, 88.0));
                    assert!(node.collapsed);
                    assert_eq!((style.left, style.top), (px(77.0), px(88.0)));
                }
            })
            .in_set(Host::Readback),
        );
    app.update();
    assert_eq!(app.world().resource::<Edits>().placement, 0);
}

#[test]
fn pin_and_collapse_observers_dispatch_once_without_competing_consumers() {
    let mut app = app();
    let m = mount(&mut app, "view");
    advance(&mut app);
    let collapse = action::<GraphCollapseAction>(&mut app);
    let pin = action::<GraphPinAction>(&mut app);
    activate(&mut app, collapse);
    activate(&mut app, pin);
    app.update();
    let node = app.world().get::<FeathersGraphNode>(m.node).unwrap();
    assert!(node.collapsed && node.pinned);
    assert_eq!(app.world().resource::<Edits>().placement, 1);
    assert_eq!(app.world().resource::<Edits>().pin, 1);
    advance(&mut app);
    assert_eq!(
        (
            app.world().resource::<Edits>().placement,
            app.world().resource::<Edits>().pin
        ),
        (1, 1)
    );
}

#[test]
fn viewport_restore_does_not_overwrite_saved_camera_and_views_keep_independent_cameras() {
    let mut app = app();
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .views
        .insert(
            "left".into(),
            GraphView {
                pan: Vec2::new(70.0, 80.0),
                zoom: 0.4,
            },
        );
    let left = mount(&mut app, "left");
    let right = mount(&mut app, "right");
    app.update();
    let view = app.world().get::<FeathersGraphViewport>(left.view).unwrap();
    assert_eq!((view.pan, view.zoom), (Vec2::new(70.0, 80.0), 0.4));
    let view = app
        .world()
        .get::<FeathersGraphViewport>(right.view)
        .unwrap();
    assert_eq!((view.pan, view.zoom), (Vec2::new(15.0, 25.0), 0.75));
    assert_eq!(app.world().resource::<Edits>().placement, 0);
}

#[test]
fn drag_observers_commit_once_and_embedded_socket_cannot_start_node_drag() {
    let mut app = app();
    let m = mount(&mut app, "view");
    advance(&mut app);
    let window = window(&mut app);
    app.world_mut().trigger(graph_pointer::tests::start(
        m.socket,
        window,
        Vec2::ZERO,
        PointerId::Mouse,
        PointerButton::Primary,
    ));
    assert!(
        !app.world()
            .get::<FeathersGraphNode>(m.node)
            .unwrap()
            .dragging
    );
    app.world_mut().trigger(graph_pointer::tests::start(
        m.node,
        window,
        Vec2::ZERO,
        PointerId::Mouse,
        PointerButton::Primary,
    ));
    app.world_mut().trigger(graph_pointer::tests::motion_delta(
        m.node,
        window,
        Vec2::new(24.0, 0.0),
    ));
    assert!(
        app.world()
            .get::<FeathersGraphNode>(m.node)
            .unwrap()
            .dragging
    );
    assert_eq!(app.world().resource::<Edits>().placement, 0);
    app.world_mut().trigger(graph_pointer::tests::end_distance(
        m.node,
        window,
        Vec2::new(24.0, 0.0),
    ));
    app.world_mut().flush();
    advance(&mut app);
    assert!(
        !app.world()
            .get::<FeathersGraphNode>(m.node)
            .unwrap()
            .dragging
    );
    assert_eq!(app.world().resource::<Edits>().placement, 1);
}

#[test]
fn complete_schedule_survives_active_drag_teardown_and_rebuild_without_stale_guides() {
    let mut app = app();
    let m = mount(&mut app, "view");
    advance(&mut app);
    let window = window(&mut app);
    app.world_mut().trigger(graph_pointer::tests::start(
        m.node,
        window,
        Vec2::ZERO,
        PointerId::Mouse,
        PointerButton::Primary,
    ));
    app.world_mut().despawn(m.view);
    advance(&mut app);
    let next = mount(&mut app, "view");
    advance(&mut app);
    assert!(
        !app.world()
            .get::<FeathersGraphNode>(next.node)
            .unwrap()
            .dragging
    );
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<Entity, With<super::super::drag_assist::GuideLine>>()
            .iter(world)
            .count(),
        0
    );
    assert_eq!(world.resource::<Edits>().placement, 0);
}

#[test]
fn frame_activation_is_scoped_and_deferred_to_post_layout_without_early_camera_writes() {
    let mut app = app();
    let left = mount(&mut app, "left");
    let right = mount(&mut app, "right");
    advance(&mut app);
    let button = app
        .world_mut()
        .spawn((
            crate::feathers::action_activation::FeathersActionButton,
            GraphFrameAction::new("left", GraphFrameTarget::All),
        ))
        .id();
    toolbar_input::set_pressed(app.world_mut(), button);
    app.world_mut().trigger(Activate { entity: button });
    app.update();
    assert_eq!(
        app.world()
            .get::<FeathersGraphViewport>(left.view)
            .unwrap()
            .frame_request,
        Some(GraphFrameTarget::All)
    );
    assert_eq!(
        app.world()
            .get::<FeathersGraphViewport>(right.view)
            .unwrap()
            .frame_request,
        None
    );
    // No measured viewport exists in this headless fixture, so the request remains pending.
    assert_eq!(
        app.world().get::<UiTransform>(left.canvas).unwrap(),
        &graph_canvas_transform(Vec2::new(15.0, 25.0), 0.75, Vec2::new(1000.0, 800.0))
    );
    advance(&mut app);
    assert_eq!(app.world().resource::<Edits>().placement, 0);
}

#[test]
fn native_release_does_not_drop_a_queued_graph_frame_activation() {
    use bevy::picking::{
        backend::HitData,
        events::{PointerClick, PointerRelease},
    };
    let mut app = app();
    let mounted = mount(&mut app, "released");
    let button = app
        .world_mut()
        .spawn((
            bevy::ui_widgets::Button,
            FeathersActionButton,
            GraphFrameAction::new("released", GraphFrameTarget::All),
        ))
        .id();
    advance(&mut app);
    let window = window(&mut app);
    let press = graph_pointer::tests::press(
        button,
        window,
        Vec2::ZERO,
        PointerId::Mouse,
        PointerButton::Primary,
    );
    let pointer = press.pointer.clone();
    app.world_mut().trigger(press);
    app.update();
    assert!(app.world().get::<bevy::ui::Pressed>(button).is_some());
    // Picking emits Click before Release. Activate is queued on Click, but the
    // native Button removes Pressed on Release before application Update.
    app.world_mut().trigger(PointerClick {
        entity: button,
        pointer: pointer.clone(),
        button: PointerButton::Primary,
        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        duration: std::time::Duration::ZERO,
        count: 1,
    });
    app.world_mut().flush();
    app.world_mut().trigger(PointerRelease {
        entity: button,
        pointer,
        button: PointerButton::Primary,
        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
    });
    app.world_mut().flush();
    assert!(app.world().get::<bevy::ui::Pressed>(button).is_none());
    app.update();
    assert_eq!(
        app.world()
            .get::<FeathersGraphViewport>(mounted.view)
            .unwrap()
            .frame_request,
        Some(GraphFrameTarget::All)
    );
    assert!(
        app.world()
            .get::<crate::feathers::action_activation::PendingFeathersActivation>(button)
            .is_none()
    );
    // A consumed click must not issue another request on a later idle frame.
    app.world_mut()
        .get_mut::<FeathersGraphViewport>(mounted.view)
        .unwrap()
        .frame_request = None;
    app.update();
    assert!(
        app.world()
            .get::<FeathersGraphViewport>(mounted.view)
            .unwrap()
            .frame_request
            .is_none()
    );
}

#[test]
fn real_post_layout_collection_and_framing_settle_without_reentering_layout() {
    use super::super::geometry::{
        self, GraphDocumentKey, GraphGeometryNode, GraphGeometryView, GraphNodeKey, GraphViewKey,
    };
    let (mut app, root) = geometry::tests::layout_app(1.25);
    geometry::tests::enable_overlays(&mut app);
    let m = mount(&mut app, "measured");
    let camera = app.world().get::<UiTargetCamera>(root).unwrap().clone();
    app.world_mut().entity_mut(m.view).insert((
        camera,
        GraphGeometryView {
            key: GraphViewKey {
                document: GraphDocumentKey {
                    project: "project".into(),
                    asset: crate::document::DocumentKey::MaterialProgram(
                        aestra_core::MaterialProgramId::from_u128(1),
                    ),
                },
                view: None,
            },
            nodes: [GraphNodeKey::MaterialOutputs].into_iter().collect(),
        },
    ));
    app.world_mut()
        .entity_mut(m.node)
        .insert(GraphGeometryNode::new(
            GraphNodeKey::MaterialOutputs,
            &"body",
            false,
        ));
    app.world_mut()
        .get_mut::<FeathersGraphViewport>(m.view)
        .unwrap()
        .frame_request = Some(GraphFrameTarget::All);
    app.world_mut()
        .spawn(GraphFrameAction::new("measured", GraphFrameTarget::All));
    for _ in 0..8 {
        app.update();
    }
    geometry::tests::assert_frame_all(app.world_mut());
}

#[test]
fn complete_registration_stages_measured_frame_then_updates_canvas_and_grid_next_update() {
    use super::super::geometry::{
        GraphDocumentKey, GraphGeometryNode, GraphGeometryView, GraphNodeKey, GraphViewKey,
    };
    let mut app = app();
    let camera = app.world_mut().spawn(Camera2d).id();
    let m = mount(&mut app, "measured");
    let root = app.world().get::<ChildOf>(m.view).unwrap().parent();
    app.world_mut().entity_mut(root).insert(Node {
        width: px(960.0),
        height: px(640.0),
        ..default()
    });
    app.world_mut().entity_mut(m.view).insert((
        UiTargetCamera(camera),
        GraphGeometryView {
            key: GraphViewKey {
                document: GraphDocumentKey {
                    project: "project".into(),
                    asset: crate::document::DocumentKey::MaterialProgram(
                        aestra_core::MaterialProgramId::from_u128(1),
                    ),
                },
                view: None,
            },
            nodes: [GraphNodeKey::MaterialOutputs].into_iter().collect(),
        },
    ));
    app.world_mut()
        .entity_mut(m.node)
        .insert(GraphGeometryNode::new(
            GraphNodeKey::MaterialOutputs,
            &"body",
            false,
        ));
    for _ in 0..8 {
        app.update();
    }
    app.world_mut()
        .get_mut::<FeathersGraphViewport>(m.view)
        .unwrap()
        .frame_request = Some(GraphFrameTarget::All);
    app.update();
    let view = app.world().get::<FeathersGraphViewport>(m.view).unwrap();
    assert_eq!((view.pan, view.zoom), (Vec2::new(15.0, 25.0), 0.75));
    let staged = view
        .measured_frame
        .expect("stable post-layout collector must stage a frame")
        .1;
    app.update();
    let view = app.world().get::<FeathersGraphViewport>(m.view).unwrap();
    assert_eq!((view.pan, view.zoom), (staged.pan, staged.zoom));
    assert!(view.frame_request.is_none() && view.measured_frame.is_none());
    assert_eq!(
        app.world().get::<UiTransform>(m.canvas).unwrap(),
        &graph_canvas_transform(staged.pan, staged.zoom, Vec2::new(1000.0, 800.0))
    );
    let handle = &app
        .world()
        .get::<MaterialNode<GraphGridMaterial>>(m.grid)
        .unwrap()
        .0;
    let grid = app
        .world()
        .resource::<Assets<GraphGridMaterial>>()
        .get(handle)
        .unwrap();
    assert_eq!((grid.pan, grid.zoom), (staged.pan, staged.zoom));
    assert_eq!(app.world().resource::<Edits>().placement, 0);
}
