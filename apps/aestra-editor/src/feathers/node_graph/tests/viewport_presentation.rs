//! Real production camera/grid/outline consumers over the actual composed graph hierarchy.
use super::super::{
    composition::{
        GraphNodeProps, GraphPortProps, GraphViewportProps, spawn_graph_node, spawn_graph_port,
        spawn_graph_viewport,
    },
    navigation::{GraphFrameTarget, graph_canvas_transform, sync_graph_viewport_transforms},
    socket::GraphSocketSide,
};
use super::*;
use crate::feathers::{
    icon::{SvgPlugin, load_svg_icon},
    text_input::tests::app_with,
};
use bevy::ecs::system::RunSystemOnce;

#[derive(Clone, Copy)]
struct Mounted {
    viewport: Entity,
    grid: Entity,
    canvas: Entity,
    node: Entity,
    socket: Entity,
}
fn app() -> App {
    app_with(|a| {
        a.add_plugins(SvgPlugin)
            .init_asset::<GraphGridMaterial>()
            .init_resource::<GraphViewportMemory>()
            .add_systems(
                Update,
                (
                    attach_graph_grid_materials,
                    remember_graph_viewports,
                    restore_graph_viewports,
                    sync_graph_viewport_transforms,
                    sync_graph_selection_outlines,
                )
                    .chain(),
            )
            .add_systems(
                PostUpdate,
                sync_graph_grid_materials.after(bevy::ui::UiSystems::Layout),
            );
    })
}
fn mount(a: &mut App, key: &str, seed: Option<(Vec2, f32)>, selected: bool) -> Mounted {
    let assets = a.world().resource::<AssetServer>().clone();
    let mut viewport = Entity::PLACEHOLDER;
    let mut node = Entity::PLACEHOLDER;
    let mut socket = Entity::PLACEHOLDER;
    a.world_mut()
        .commands()
        .spawn(Node::default())
        .with_children(|parent| {
            viewport = spawn_graph_viewport(
                parent,
                GraphViewportProps {
                    key: key.into(),
                    content_size: Vec2::new(300.0, 200.0),
                    selection_bounds: None,
                    initial_view: seed,
                },
                (),
                |_| {},
                |canvas| {
                    node = spawn_graph_node(
                        canvas,
                        GraphNodeProps {
                            graph_key: "material:p".into(),
                            node_key: "expression:n".into(),
                            title: "Actual graph node".into(),
                            position: Vec2::new(34.0, 68.0),
                            selected,
                            pinned: false,
                            muted: false,
                            collapse_icon: load_svg_icon(&assets, "icons/chevron-down.svg"),
                            expand_icon: load_svg_icon(&assets, "icons/chevron-right.svg"),
                            pin_icon: load_svg_icon(&assets, "icons/pin.svg"),
                            collapse_label: "Collapse".into(),
                            expand_label: "Expand".into(),
                        },
                        (),
                        |_, body| {
                            socket = spawn_graph_port(
                                body,
                                GraphPortProps {
                                    label: Some("Input".into()),
                                    tooltip_title: "Input [Float]".into(),
                                    tooltip_description: "Actual typed port".into(),
                                    side: GraphSocketSide::Input,
                                    color: Color::WHITE,
                                },
                                (),
                            );
                        },
                    );
                },
            );
        });
    a.world_mut().flush();
    let children = a.world().get::<Children>(viewport).unwrap();
    Mounted {
        viewport,
        grid: children[0],
        canvas: children[2],
        node,
        socket,
    }
}
fn camera(a: &App, m: Mounted) -> (Vec2, f32) {
    let v = a.world().get::<FeathersGraphViewport>(m.viewport).unwrap();
    (v.pan, v.zoom)
}
fn set_camera(a: &mut App, m: Mounted, pan: Vec2, zoom: f32) {
    let mut viewport = a
        .world_mut()
        .get_mut::<FeathersGraphViewport>(m.viewport)
        .unwrap();
    viewport.pan = pan;
    viewport.zoom = zoom;
}
fn grid_handle(a: &App, m: Mounted) -> Handle<GraphGridMaterial> {
    a.world()
        .get::<MaterialNode<GraphGridMaterial>>(m.grid)
        .unwrap()
        .0
        .clone()
}
fn outline(a: &App, m: Mounted) -> &Outline {
    a.world().get::<Outline>(m.node).unwrap()
}

#[test]
fn new_viewports_do_not_overwrite_saved_camera_before_restoration() {
    let mut a = app();
    let saved = (Vec2::new(90.0, -70.0), 0.25);
    a.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_view("material:p#1", saved.0, saved.1);
    let m = mount(&mut a, "material:p#1", Some((Vec2::ZERO, 1.0)), false);
    a.update();
    assert_eq!(camera(&a, m), saved);
    assert_eq!(
        a.world()
            .resource::<GraphViewportMemory>()
            .view("material:p#1"),
        Some(saved)
    );
    assert_eq!(
        a.world()
            .get::<FeathersGraphViewport>(m.viewport)
            .unwrap()
            .frame_request,
        None
    );
}
#[test]
fn uncached_viewports_keep_seeded_camera_or_original_auto_frame_request() {
    for seed in [None, Some((Vec2::new(30.0, -40.0), 0.5))] {
        let mut a = app();
        let m = mount(&mut a, "new-view", seed, false);
        a.update();
        assert_eq!(camera(&a, m), seed.unwrap_or((Vec2::ZERO, 1.0)));
        assert_eq!(
            a.world()
                .get::<FeathersGraphViewport>(m.viewport)
                .unwrap()
                .frame_request,
            if seed.is_none() {
                Some(GraphFrameTarget::All)
            } else {
                None
            }
        );
        assert_eq!(
            a.world().resource::<GraphViewportMemory>().view("new-view"),
            None
        );
    }
}
#[test]
fn split_camera_restoration_is_keyed_per_view_and_does_not_change_document_nodes() {
    let mut a = app();
    {
        let mut memory = a.world_mut().resource_mut::<GraphViewportMemory>();
        memory.set_view("material:p#1", Vec2::new(10.0, 20.0), 0.5);
        memory.set_view("material:p#2", Vec2::new(-40.0, 70.0), 1.5);
        memory.set_node("material:p", "expression:n", Vec2::new(90.0, 30.0), true);
    }
    let first = mount(&mut a, "material:p#1", None, false);
    let second = mount(&mut a, "material:p#2", None, false);
    a.update();
    assert_eq!(camera(&a, first), (Vec2::new(10.0, 20.0), 0.5));
    assert_eq!(camera(&a, second), (Vec2::new(-40.0, 70.0), 1.5));
    assert_eq!(
        a.world()
            .resource::<GraphViewportMemory>()
            .node("material:p", "expression:n"),
        Some((Vec2::new(90.0, 30.0), true))
    );
}
#[test]
fn restoration_only_applies_to_added_viewports_not_already_live_cameras() {
    let mut a = App::new();
    a.init_resource::<GraphViewportMemory>()
        .add_systems(Update, restore_graph_viewports);
    let first = a
        .world_mut()
        .spawn(FeathersGraphViewport::clipboard_fixture(
            "view".into(),
            Vec2::new(5.0, 8.0),
            1.0,
        ))
        .id();
    a.update();
    a.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_view("view", Vec2::new(70.0, 80.0), 0.25);
    let second = a
        .world_mut()
        .spawn(FeathersGraphViewport::clipboard_fixture(
            "view".into(),
            Vec2::ZERO,
            1.0,
        ))
        .id();
    a.update();
    assert_eq!(
        a.world().get::<FeathersGraphViewport>(first).unwrap().pan,
        Vec2::new(5.0, 8.0)
    );
    assert_eq!(
        a.world().get::<FeathersGraphViewport>(second).unwrap().pan,
        Vec2::new(70.0, 80.0)
    );
}
#[test]
fn camera_changes_are_remembered_without_rewriting_idle_memory_or_node_placement() {
    let mut a = app();
    let m = mount(&mut a, "view", None, false);
    a.update();
    set_camera(&mut a, m, Vec2::new(10.0, 30.0), 0.5);
    a.update();
    assert_eq!(
        a.world().resource::<GraphViewportMemory>().view("view"),
        Some((Vec2::new(10.0, 30.0), 0.5))
    );
    assert!(
        a.world()
            .resource::<GraphViewportMemory>()
            .base_nodes("material:p")
            .is_empty()
    );
    let tick = a
        .world()
        .get_resource_ref::<GraphViewportMemory>()
        .unwrap()
        .last_changed();
    a.update();
    a.update();
    assert_eq!(
        a.world()
            .get_resource_ref::<GraphViewportMemory>()
            .unwrap()
            .last_changed(),
        tick
    );
}
#[test]
fn rebuilt_viewports_restore_last_live_camera_without_accumulating_offsets() {
    let mut a = app();
    let first = mount(&mut a, "view", None, false);
    a.update();
    set_camera(&mut a, first, Vec2::new(90.0, -60.0), 0.4);
    a.update();
    a.world_mut().despawn(first.viewport);
    let second = mount(&mut a, "view", Some((Vec2::new(999.0, 999.0), 2.0)), false);
    a.update();
    assert_eq!(camera(&a, second), (Vec2::new(90.0, -60.0), 0.4));
    a.update();
    assert_eq!(camera(&a, second), (Vec2::new(90.0, -60.0), 0.4));
}
#[test]
fn actual_grid_attachment_creates_independent_assets_once_per_grid() {
    let mut a = app();
    let first = mount(&mut a, "first", None, false);
    let second = mount(&mut a, "second", None, false);
    assert!(
        a.world()
            .get::<MaterialNode<GraphGridMaterial>>(first.grid)
            .is_none()
    );
    a.update();
    let first_handle = grid_handle(&a, first);
    let second_handle = grid_handle(&a, second);
    assert_ne!(first_handle, second_handle);
    assert_eq!(a.world().resource::<Assets<GraphGridMaterial>>().len(), 2);
    for handle in [&first_handle, &second_handle] {
        let grid = a
            .world()
            .resource::<Assets<GraphGridMaterial>>()
            .get(handle)
            .unwrap();
        assert_eq!((grid.pan, grid.zoom, grid.spacing), (Vec2::ZERO, 1.0, 32.0));
    }
    a.update();
    a.update();
    assert_eq!(grid_handle(&a, first), first_handle);
    assert_eq!(grid_handle(&a, second), second_handle);
    assert_eq!(a.world().resource::<Assets<GraphGridMaterial>>().len(), 2);
}
#[test]
fn existing_grid_handles_and_unclassified_nodes_are_not_replaced() {
    let mut a = app();
    let m = mount(&mut a, "view", None, false);
    let handle = a
        .world_mut()
        .resource_mut::<Assets<GraphGridMaterial>>()
        .add(GraphGridMaterial {
            spacing: 48.0,
            ..default()
        });
    a.world_mut()
        .entity_mut(m.grid)
        .insert(MaterialNode(handle.clone()));
    let unrelated = a.world_mut().spawn(Node::default()).id();
    a.update();
    assert_eq!(grid_handle(&a, m), handle);
    assert_eq!(a.world().resource::<Assets<GraphGridMaterial>>().len(), 1);
    assert_eq!(
        a.world()
            .resource::<Assets<GraphGridMaterial>>()
            .get(&handle)
            .unwrap()
            .spacing,
        48.0
    );
    assert!(
        a.world()
            .get::<MaterialNode<GraphGridMaterial>>(unrelated)
            .is_none()
    );
}
#[test]
fn post_layout_grid_uniforms_track_their_own_camera_and_preserve_spacing() {
    let mut a = app();
    let first = mount(&mut a, "first", Some((Vec2::new(20.0, 40.0), 0.5)), false);
    let second = mount(&mut a, "second", Some((Vec2::new(-80.0, 15.0), 1.5)), false);
    a.update();
    let first_handle = grid_handle(&a, first);
    let second_handle = grid_handle(&a, second);
    {
        let grids = a.world().resource::<Assets<GraphGridMaterial>>();
        assert_eq!(
            (
                grids.get(&first_handle).unwrap().pan,
                grids.get(&first_handle).unwrap().zoom
            ),
            camera(&a, first)
        );
        assert_eq!(
            (
                grids.get(&second_handle).unwrap().pan,
                grids.get(&second_handle).unwrap().zoom
            ),
            camera(&a, second)
        );
    }
    a.world_mut()
        .resource_mut::<Assets<GraphGridMaterial>>()
        .get_mut(&first_handle)
        .unwrap()
        .spacing = 64.0;
    set_camera(&mut a, first, Vec2::new(11.0, -12.0), 0.08);
    a.update();
    let grids = a.world().resource::<Assets<GraphGridMaterial>>();
    assert_eq!(
        (
            grids.get(&first_handle).unwrap().pan,
            grids.get(&first_handle).unwrap().zoom
        ),
        camera(&a, first)
    );
    assert_eq!(grids.get(&first_handle).unwrap().spacing, 64.0);
    assert_eq!(
        (
            grids.get(&second_handle).unwrap().pan,
            grids.get(&second_handle).unwrap().zoom
        ),
        camera(&a, second)
    );
}
#[test]
fn missing_grid_owner_or_removed_material_is_skipped_without_allocating_a_replacement() {
    let mut a = app();
    let m = mount(&mut a, "view", None, false);
    a.update();
    let handle = grid_handle(&a, m);
    let missing = a.world_mut().spawn_empty().id();
    a.world_mut().despawn(missing);
    let orphan_handle = a
        .world_mut()
        .resource_mut::<Assets<GraphGridMaterial>>()
        .add(GraphGridMaterial {
            pan: Vec2::ONE,
            zoom: 1.5,
            spacing: 48.0,
        });
    a.world_mut().spawn((
        FeathersGraphGrid { viewport: missing },
        MaterialNode(orphan_handle.clone()),
    ));
    a.world_mut()
        .resource_mut::<Assets<GraphGridMaterial>>()
        .remove(handle.id());
    a.update();
    assert_eq!(grid_handle(&a, m), handle);
    let grids = a.world().resource::<Assets<GraphGridMaterial>>();
    assert!(grids.get(&handle).is_none());
    let orphan = grids.get(&orphan_handle).unwrap();
    assert_eq!(
        (orphan.pan, orphan.zoom, orphan.spacing),
        (Vec2::ONE, 1.5, 48.0)
    );
    assert_eq!(grids.len(), 1);
}
#[test]
fn restored_camera_drives_real_canvas_transform_and_grid_in_the_same_frame() {
    let mut a = app();
    a.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_view("view", Vec2::new(20.0, 40.0), 0.5);
    let m = mount(&mut a, "view", None, false);
    a.update();
    let transform = a.world().get::<UiTransform>(m.canvas).unwrap();
    assert_eq!(
        *transform,
        graph_canvas_transform(Vec2::new(20.0, 40.0), 0.5, Vec2::new(300.0, 200.0))
    );
    assert_eq!(transform.translation, Val2::px(-55.0, -10.0));
    assert_eq!(transform.scale, Vec2::splat(0.5));
    let grids = a.world().resource::<Assets<GraphGridMaterial>>();
    let grid = grids.get(&grid_handle(&a, m)).unwrap();
    assert_eq!((grid.pan, grid.zoom), camera(&a, m));
}
#[test]
fn selection_outline_keeps_minimum_visible_edge_and_clamps_invalid_small_zoom() {
    for zoom in [0.08, 0.25, 0.5, 1.0, 2.0] {
        let screen_width = (SELECTED_BORDER_WIDTH + selection_outline_width(zoom)) * zoom;
        assert!(screen_width >= MIN_SELECTED_BORDER_SCREEN_WIDTH - 0.001);
        if zoom <= 1.0 {
            assert!((screen_width - 2.0).abs() < 0.001);
        }
    }
    for zoom in [-1.0, 0.0, 0.001] {
        assert_eq!(
            selection_outline_width(zoom),
            selection_outline_width(MIN_ZOOM)
        );
    }
    assert_eq!(selection_outline_width(2.0), 0.0);
}
#[test]
fn selected_nodes_resolve_the_nearest_actual_viewport_not_the_shared_document_key() {
    let mut a = app();
    let outer = mount(&mut a, "material:p#outer", Some((Vec2::ZERO, 2.0)), true);
    let inner = mount(&mut a, "material:p#inner", Some((Vec2::ZERO, 0.25)), true);
    a.world_mut()
        .entity_mut(inner.viewport)
        .insert(ChildOf(outer.canvas));
    a.update();
    assert_eq!(outline(&a, outer).width, Val::Px(0.0));
    assert_eq!(outline(&a, inner).width, Val::Px(6.0));
    set_camera(&mut a, inner, Vec2::ZERO, 0.5);
    a.update();
    assert_eq!(outline(&a, outer).width, Val::Px(0.0));
    assert_eq!(outline(&a, inner).width, Val::Px(2.0));
}
#[test]
fn selection_changes_hide_outline_without_mutating_node_or_socket_geometry() {
    let mut a = app();
    let m = mount(&mut a, "view", Some((Vec2::ZERO, 0.25)), true);
    a.update();
    let node = a.world().get::<Node>(m.node).unwrap().clone();
    let socket = a.world().get::<Node>(m.socket).unwrap().clone();
    assert_eq!(outline(&a, m).width, Val::Px(6.0));
    a.world_mut()
        .get_mut::<FeathersGraphNode>(m.node)
        .unwrap()
        .selected = false;
    a.update();
    assert_eq!(outline(&a, m).color, Color::NONE);
    assert_eq!(outline(&a, m).width, Val::Px(6.0));
    assert_eq!(a.world().get::<Node>(m.node).unwrap(), &node);
    assert_eq!(a.world().get::<Node>(m.socket).unwrap(), &socket);
    a.world_mut()
        .get_mut::<FeathersGraphNode>(m.node)
        .unwrap()
        .selected = true;
    a.update();
    assert_eq!(outline(&a, m).color, theme::ACCENT);
    assert_eq!(a.world().get::<Node>(m.node).unwrap(), &node);
}
#[test]
fn orphan_nodes_keep_width_and_missing_outlines_do_not_break_the_consumer() {
    let mut a = App::new();
    a.add_systems(Update, sync_graph_selection_outlines);
    let mut node = FeathersGraphNode::clipboard_fixture("graph".into(), "node".into(), Vec2::ZERO);
    node.selected = true;
    let e = a
        .world_mut()
        .spawn((
            node.clone(),
            Outline::new(Val::Px(9.0), Val::ZERO, Color::NONE),
        ))
        .id();
    let no_outline = a.world_mut().spawn(node).id();
    a.update();
    assert_eq!(a.world().get::<Outline>(e).unwrap().width, Val::Px(9.0));
    assert_eq!(a.world().get::<Outline>(e).unwrap().color, theme::ACCENT);
    assert!(a.world().get::<Outline>(no_outline).is_none());
    a.world_mut()
        .run_system_once(sync_graph_selection_outlines)
        .unwrap();
}
#[test]
fn idle_outline_updates_do_not_rewrite_the_component() {
    let mut a = app();
    let m = mount(&mut a, "view", Some((Vec2::ZERO, 0.5)), true);
    a.update();
    a.update();
    let tick = a
        .world()
        .entity(m.node)
        .get_ref::<Outline>()
        .unwrap()
        .last_changed();
    a.update();
    a.update();
    assert_eq!(
        a.world()
            .entity(m.node)
            .get_ref::<Outline>()
            .unwrap()
            .last_changed(),
        tick
    );
}
