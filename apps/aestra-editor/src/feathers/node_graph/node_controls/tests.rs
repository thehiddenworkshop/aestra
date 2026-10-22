//! Actual composed graph controls, memory synchronization and native state transitions.
use super::super::composition::{
    GraphNodeProps, GraphPortProps, spawn_graph_node, spawn_graph_port,
};
use super::super::socket::GraphSocketSide;
use super::*;
use crate::feathers::{
    icon::{SvgPlugin, load_svg_icon},
    text_input::tests::app_with,
};
use bevy::ecs::system::RunSystemOnce;

#[derive(Resource, Default)]
struct Observed {
    pins: Vec<GraphPinEdit>,
    presentation: Vec<GraphPresentationEdit>,
}
#[derive(Clone, Copy)]
struct Mounted {
    node: Entity,
    pin: Entity,
    pin_icon: Entity,
    collapse: Entity,
    collapse_icon: Entity,
    body: Entity,
    socket: Entity,
    dot: Entity,
}
fn app() -> App {
    app_with(|a| {
        a.add_plugins(SvgPlugin)
            .init_resource::<GraphViewportMemory>()
            .init_resource::<Observed>()
            .add_observer(queue_graph_pin_activation)
            .add_observer(queue_graph_collapse_activation)
            .add_observer(|event: On<GraphPinEdit>, mut observed: ResMut<Observed>| {
                observed.pins.push(event.event().clone());
            })
            .add_observer(
                |event: On<GraphPresentationEdit>, mut observed: ResMut<Observed>| {
                    observed.presentation.push(event.event().clone());
                },
            )
            .add_systems(
                Update,
                (
                    restore_graph_nodes,
                    sync_graph_nodes_from_memory,
                    handle_graph_collapse_buttons,
                    handle_graph_pin_buttons,
                    update_socket_visuals,
                )
                    .chain(),
            );
    })
}
fn mount(a: &mut App, graph: &str, key: &str, position: Vec2) -> Mounted {
    let server = a.world().resource::<AssetServer>().clone();
    let mut node = Entity::PLACEHOLDER;
    let mut socket = Entity::PLACEHOLDER;
    a.world_mut()
        .commands()
        .spawn(Node::default())
        .with_children(|p| {
            node = spawn_graph_node(
                p,
                GraphNodeProps {
                    graph_key: graph.into(),
                    node_key: key.into(),
                    title: "Actual node".into(),
                    position,
                    selected: false,
                    pinned: false,
                    muted: false,
                    collapse_icon: load_svg_icon(&server, "icons/chevron-down.svg"),
                    expand_icon: load_svg_icon(&server, "icons/chevron-right.svg"),
                    pin_icon: load_svg_icon(&server, "icons/pin.svg"),
                    collapse_label: "Collapse actual node".into(),
                    expand_label: "Expand actual node".into(),
                },
                (),
                |_, body| {
                    socket = spawn_graph_port(
                        body,
                        GraphPortProps {
                            label: Some("Value".into()),
                            tooltip_title: "Value [Float]".into(),
                            tooltip_description: "Actual typed input".into(),
                            side: GraphSocketSide::Input,
                            color: Color::srgb(0.45, 0.78, 1.0),
                        },
                        (),
                    );
                },
            );
        });
    a.world_mut().flush();
    let pin = a
        .world_mut()
        .query::<(Entity, &GraphPinAction)>()
        .iter(a.world())
        .find(|(_, action)| action.node == node)
        .unwrap()
        .0;
    let collapse = a
        .world_mut()
        .query::<(Entity, &GraphCollapseAction)>()
        .iter(a.world())
        .find(|(_, action)| action.node == node)
        .unwrap()
        .0;
    let pin_icon = a
        .world_mut()
        .query::<(Entity, &GraphPinIcon)>()
        .iter(a.world())
        .find(|(_, icon)| icon.node == node)
        .unwrap()
        .0;
    let collapse_icon = a
        .world_mut()
        .query::<(Entity, &GraphCollapseIcon)>()
        .iter(a.world())
        .find(|(_, icon)| icon.node == node)
        .unwrap()
        .0;
    let body = a
        .world_mut()
        .query::<(Entity, &FeathersGraphNodeBody)>()
        .iter(a.world())
        .find(|(_, body)| body.node == node)
        .unwrap()
        .0;
    let dot = a.world().get::<Children>(socket).unwrap()[0];
    Mounted {
        node,
        pin,
        pin_icon,
        collapse,
        collapse_icon,
        body,
        socket,
        dot,
    }
}
fn activate(a: &mut App, entity: Entity) {
    a.world_mut().trigger(Activate { entity });
    a.update();
}
fn collapsed(a: &App, m: Mounted) -> bool {
    a.world()
        .get::<FeathersGraphNode>(m.node)
        .unwrap()
        .collapsed()
}
fn node_style(a: &App, m: Mounted) -> &Node {
    a.world().get::<Node>(m.node).unwrap()
}
fn color(a: &App, m: Mounted) -> (Color, BorderColor) {
    (
        a.world().get::<BackgroundColor>(m.dot).unwrap().0,
        *a.world().get::<BorderColor>(m.dot).unwrap(),
    )
}
fn memory(a: &App) -> &GraphViewportMemory {
    a.world().resource::<GraphViewportMemory>()
}

#[test]
fn actual_pin_requires_activate_consumes_once_and_materializes_bootstrap_anchor() {
    let mut a = app();
    let m = mount(&mut a, "material:p", "node", Vec2::new(71.0, 29.0));
    a.update();
    node_control_input::set_phase(a.world_mut(), m.pin, SocketPhase::Pressed);
    a.update();
    assert!(!memory(&a).is_pinned("material:p", "node"));
    assert!(memory(&a).node("material:p", "node").is_none());
    activate(&mut a, m.pin);
    assert!(memory(&a).is_pinned("material:p", "node"));
    assert_eq!(
        memory(&a).node("material:p", "node"),
        Some((Vec2::new(71.0, 29.0), false))
    );
    assert!(a.world().get::<PendingFeathersActivation>(m.pin).is_none());
    assert!(!node_control_input::is_pressed(a.world(), m.pin));
    assert_eq!(
        a.world().get::<AccessibleLabel>(m.pin).unwrap().0,
        "Unpin node"
    );
    assert_eq!(
        a.world().get::<SvgColor>(m.pin_icon).unwrap().0,
        theme::ACCENT
    );
    let e = &a.world().resource::<Observed>().pins[0];
    assert_eq!(
        (&*e.graph, &*e.node, e.before, e.after, e.origin),
        ("material:p", "node", false, true, Some(m.node))
    );
    a.update();
    a.update();
    assert_eq!(a.world().resource::<Observed>().pins.len(), 1);
}
#[test]
fn actual_pin_round_trip_preserves_base_and_temporary_offset() {
    let mut a = app();
    let base = Vec2::new(30.0, 40.0);
    let offset = Vec2::new(100.0, 200.0);
    {
        let mut memory = a.world_mut().resource_mut::<GraphViewportMemory>();
        memory.set_node("function:f", "node", base, false);
        memory.set_temporary_offset("function:f", "node", offset);
    }
    let m = mount(&mut a, "function:f", "node", Vec2::ZERO);
    a.update();
    for pinned in [true, false] {
        activate(&mut a, m.pin);
        assert_eq!(memory(&a).is_pinned("function:f", "node"), pinned);
        assert_eq!(memory(&a).node("function:f", "node"), Some((base, false)));
        assert_eq!(
            memory(&a).node_position("function:f", "node"),
            Some(base + offset)
        );
        assert_eq!(
            a.world().get::<SvgColor>(m.pin_icon).unwrap().0,
            if pinned {
                theme::ACCENT
            } else {
                theme::TEXT_FAINT
            }
        );
    }
    assert_eq!(a.world().resource::<Observed>().pins.len(), 2);
    assert!(a.world().resource::<Observed>().presentation.is_empty());
}
#[test]
fn actual_collapse_requires_activate_and_updates_body_icon_label_and_edit_payload() {
    let mut a = app();
    let position = Vec2::new(10.0, 20.0);
    let m = mount(&mut a, "material:p", "node", position);
    a.update();
    node_control_input::set_phase(a.world_mut(), m.collapse, SocketPhase::Pressed);
    a.update();
    assert!(!collapsed(&a, m));
    for after in [true, false] {
        activate(&mut a, m.collapse);
        assert_eq!(collapsed(&a, m), after);
        assert_eq!(
            a.world().get::<Node>(m.body).unwrap().display,
            if after { Display::None } else { Display::Flex }
        );
        let icon = a.world().get::<GraphCollapseIcon>(m.collapse_icon).unwrap();
        assert_eq!(
            a.world().get::<UiSvg>(m.collapse_icon).unwrap().0,
            if after {
                icon.expand_icon.clone()
            } else {
                icon.collapse_icon.clone()
            }
        );
        assert_eq!(
            a.world().get::<AccessibleLabel>(m.collapse).unwrap().0,
            if after {
                "Expand actual node"
            } else {
                "Collapse actual node"
            }
        );
        assert!(
            a.world()
                .get::<PendingFeathersActivation>(m.collapse)
                .is_none()
        );
        assert!(!node_control_input::is_pressed(a.world(), m.collapse));
    }
    let o = a.world().resource::<Observed>();
    assert_eq!(o.presentation.len(), 2);
    let first = &o.presentation[0];
    assert_eq!((&*first.graph, &*first.node), ("material:p", "node"));
    assert_eq!(first.before, (position, false));
    assert_eq!(first.after, (position, true));
    assert_eq!(first.origin, Some(m.node));
    assert!(o.pins.is_empty());
}
#[test]
fn actual_collapse_retains_authored_base_under_temporary_displacement() {
    let mut a = app();
    let base = Vec2::new(40.0, 80.0);
    let offset = Vec2::new(90.0, 120.0);
    {
        let mut memory = a.world_mut().resource_mut::<GraphViewportMemory>();
        memory.set_node("material:p", "node", base, false);
        memory.set_temporary_offset("material:p", "node", offset);
    }
    let m = mount(&mut a, "material:p", "node", Vec2::ZERO);
    a.update();
    activate(&mut a, m.collapse);
    assert_eq!(memory(&a).node("material:p", "node"), Some((base, true)));
    assert_eq!(
        memory(&a).node_position("material:p", "node"),
        Some(base + offset)
    );
    let e = &a.world().resource::<Observed>().presentation[0];
    assert_eq!(e.before, (base, false));
    assert_eq!(e.after, (base, true));
    assert_eq!(
        a.world()
            .get::<FeathersGraphNode>(m.node)
            .unwrap()
            .position(),
        base + offset
    );
}
#[test]
fn duplicate_same_frame_activate_is_coalesced_like_the_shipping_queue() {
    for pin in [false, true] {
        let mut a = app();
        let m = mount(&mut a, "material:p", "node", Vec2::ZERO);
        a.update();
        let entity = if pin { m.pin } else { m.collapse };
        a.world_mut().trigger(Activate { entity });
        a.world_mut().trigger(Activate { entity });
        a.update();
        a.update();
        let o = a.world().resource::<Observed>();
        assert_eq!(
            if pin {
                o.pins.len()
            } else {
                o.presentation.len()
            },
            1
        );
    }
}
#[test]
fn unclassified_composed_controls_cannot_queue_edits() {
    for pin in [false, true] {
        let mut a = app();
        let m = mount(&mut a, "material:p", "node", Vec2::ZERO);
        a.update();
        let entity = if pin { m.pin } else { m.collapse };
        a.world_mut()
            .entity_mut(entity)
            .remove::<FeathersActionButton>();
        activate(&mut a, entity);
        assert!(a.world().get::<PendingFeathersActivation>(entity).is_none());
        assert!(!collapsed(&a, m));
        assert!(!memory(&a).is_pinned("material:p", "node"));
        assert!(a.world().resource::<Observed>().pins.is_empty());
        assert!(a.world().resource::<Observed>().presentation.is_empty());
    }
}
#[test]
fn missing_node_consumes_retained_control_activation_without_a_ghost_edit() {
    for pin in [false, true] {
        let mut a = app();
        let m = mount(&mut a, "material:p", "node", Vec2::ZERO);
        a.update();
        let entity = if pin { m.pin } else { m.collapse };
        a.world_mut().entity_mut(entity).remove::<ChildOf>();
        a.world_mut().despawn(m.node);
        activate(&mut a, entity);
        assert!(a.world().get::<PendingFeathersActivation>(entity).is_none());
        assert!(!node_control_input::is_pressed(a.world(), entity));
        assert!(a.world().resource::<Observed>().pins.is_empty());
        assert!(a.world().resource::<Observed>().presentation.is_empty());
        assert!(memory(&a).node("material:p", "node").is_none());
    }
}
#[test]
fn actual_restore_rebuilds_saved_position_collapse_and_pin_for_each_graph_kind() {
    for graph in ["material:p", "function:f"] {
        let mut a = app();
        let base = Vec2::new(20.0, 30.0);
        let offset = Vec2::new(4.0, 6.0);
        {
            let mut memory = a.world_mut().resource_mut::<GraphViewportMemory>();
            memory.set_node(graph, "node", base, true);
            memory.set_pinned(graph, "node", true);
            memory.set_temporary_offset(graph, "node", offset);
        }
        for _ in 0..2 {
            let m = mount(&mut a, graph, "node", Vec2::ZERO);
            a.update();
            assert_eq!(
                a.world()
                    .get::<FeathersGraphNode>(m.node)
                    .unwrap()
                    .position(),
                base + offset
            );
            assert_eq!(
                (node_style(&a, m).left, node_style(&a, m).top),
                (Val::Px(24.0), Val::Px(36.0))
            );
            assert!(collapsed(&a, m));
            assert_eq!(
                a.world().get::<Node>(m.body).unwrap().display,
                Display::None
            );
            assert_eq!(
                a.world().get::<SvgColor>(m.pin_icon).unwrap().0,
                theme::ACCENT
            );
            assert_eq!(memory(&a).node(graph, "node"), Some((base, true)));
            a.world_mut().despawn(m.node);
        }
        assert!(a.world().resource::<Observed>().pins.is_empty());
        assert!(a.world().resource::<Observed>().presentation.is_empty());
    }
}
#[test]
fn memory_synchronization_updates_same_document_views_without_touching_other_documents() {
    let mut a = app();
    let left = mount(&mut a, "material:p", "node", Vec2::ZERO);
    let right = mount(&mut a, "material:p", "node", Vec2::ZERO);
    let other = mount(&mut a, "material:q", "node", Vec2::splat(9.0));
    a.update();
    activate(&mut a, left.pin);
    activate(&mut a, left.collapse);
    a.update();
    for m in [left, right] {
        assert!(collapsed(&a, m));
        assert_eq!(
            a.world().get::<SvgColor>(m.pin_icon).unwrap().0,
            theme::ACCENT
        );
    }
    assert!(!collapsed(&a, other));
    assert_eq!(
        a.world()
            .get::<FeathersGraphNode>(other.node)
            .unwrap()
            .position(),
        Vec2::splat(9.0)
    );
    assert_eq!(
        a.world().get::<SvgColor>(other.pin_icon).unwrap().0,
        theme::TEXT_FAINT
    );
    let mut memory = a.world_mut().resource_mut::<GraphViewportMemory>();
    memory.set_node("material:p", "node", Vec2::new(70.0, 80.0), false);
    memory.set_pinned("material:p", "node", false);
    a.update();
    for m in [left, right] {
        assert!(!collapsed(&a, m));
        assert_eq!(
            (node_style(&a, m).left, node_style(&a, m).top),
            (Val::Px(70.0), Val::Px(80.0))
        );
        assert_eq!(
            a.world().get::<SvgColor>(m.pin_icon).unwrap().0,
            theme::TEXT_FAINT
        );
    }
    assert_eq!(a.world().resource::<Observed>().pins.len(), 1);
    assert_eq!(a.world().resource::<Observed>().presentation.len(), 1);
}
#[test]
fn unchanged_memory_does_not_rewrite_existing_node_style_or_icons() {
    let mut a = app();
    a.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node("material:p", "node", Vec2::ONE, true);
    let m = mount(&mut a, "material:p", "node", Vec2::ZERO);
    a.update();
    a.update();
    let tick = a
        .world()
        .entity(m.node)
        .get_ref::<Node>()
        .unwrap()
        .last_changed();
    let icon_tick = a
        .world()
        .entity(m.collapse_icon)
        .get_ref::<UiSvg>()
        .unwrap()
        .last_changed();
    a.update();
    a.update();
    assert_eq!(
        a.world()
            .entity(m.node)
            .get_ref::<Node>()
            .unwrap()
            .last_changed(),
        tick
    );
    assert_eq!(
        a.world()
            .entity(m.collapse_icon)
            .get_ref::<UiSvg>()
            .unwrap()
            .last_changed(),
        icon_tick
    );
}
#[test]
fn actual_socket_hover_press_release_and_hover_removal_restore_exact_colors() {
    let mut a = app();
    let m = mount(&mut a, "material:p", "node", Vec2::ZERO);
    a.world_mut().run_schedule(Update);
    let tint = a
        .world()
        .get::<super::super::composition::FeathersGraphSocket>(m.socket)
        .unwrap()
        .color;
    assert_eq!(color(&a, m), (theme::PANEL_DARK, BorderColor::all(tint)));
    // The real headless picking pass has no pointer and resets Hovered in PreUpdate.
    // Injected marker transitions run through the retained Update schedule instead:
    // native marker changes/removals and system change ticks remain real, without
    // claiming that these synthetic state tests deliver an OS pointer event.
    node_control_input::set_phase(a.world_mut(), m.socket, SocketPhase::Hovered);
    a.world_mut().run_schedule(Update);
    assert_eq!(color(&a, m), (tint, BorderColor::all(theme::TEXT)));
    node_control_input::set_phase(a.world_mut(), m.socket, SocketPhase::Pressed);
    a.world_mut().run_schedule(Update);
    assert_eq!(color(&a, m), (theme::TEXT, BorderColor::all(tint)));
    node_control_input::release_pressed(a.world_mut(), m.socket);
    a.world_mut().run_schedule(Update);
    assert_eq!(color(&a, m), (tint, BorderColor::all(theme::TEXT)));
    node_control_input::clear_hover(a.world_mut(), m.socket);
    a.world_mut().run_schedule(Update);
    assert_eq!(color(&a, m), (theme::PANEL_DARK, BorderColor::all(tint)));
}
#[test]
fn native_press_wins_over_hover_removal_until_the_press_is_released() {
    let mut a = app();
    let m = mount(&mut a, "material:p", "node", Vec2::ZERO);
    a.world_mut().run_schedule(Update);
    node_control_input::set_phase(a.world_mut(), m.socket, SocketPhase::Pressed);
    a.world_mut().run_schedule(Update);
    // A native hover departure while pressed must not turn the pressed dot into an idle dot.
    // Shipping encodes both in one enum; keep Pressed until its release there too.
    node_control_input::clear_hover_while_pressed(a.world_mut(), m.socket);
    a.world_mut().run_schedule(Update);
    assert_eq!(color(&a, m).0, theme::TEXT);
    node_control_input::set_phase(a.world_mut(), m.socket, SocketPhase::None);
    a.world_mut().run_schedule(Update);
    assert_eq!(color(&a, m).0, theme::PANEL_DARK);
}
#[test]
fn idle_socket_updates_do_not_rewrite_dot_color_or_geometry() {
    let mut a = app();
    let m = mount(&mut a, "material:p", "node", Vec2::ZERO);
    a.world_mut().run_schedule(Update);
    a.world_mut().run_schedule(Update);
    let color_tick = a
        .world()
        .entity(m.dot)
        .get_ref::<BackgroundColor>()
        .unwrap()
        .last_changed();
    let border_tick = a
        .world()
        .entity(m.dot)
        .get_ref::<BorderColor>()
        .unwrap()
        .last_changed();
    let geometry = a.world().get::<Node>(m.socket).unwrap().clone();
    a.world_mut().run_schedule(Update);
    a.world_mut().run_schedule(Update);
    assert_eq!(
        a.world()
            .entity(m.dot)
            .get_ref::<BackgroundColor>()
            .unwrap()
            .last_changed(),
        color_tick
    );
    assert_eq!(
        a.world()
            .entity(m.dot)
            .get_ref::<BorderColor>()
            .unwrap()
            .last_changed(),
        border_tick
    );
    assert_eq!(a.world().get::<Node>(m.socket).unwrap(), &geometry);
}
#[test]
fn socket_updates_are_local_and_skip_unmarked_children_and_unclassified_entities() {
    let mut a = app();
    let left = mount(&mut a, "material:p", "left", Vec2::ZERO);
    let right = mount(&mut a, "material:p", "right", Vec2::ZERO);
    let child = a
        .world_mut()
        .spawn((
            BackgroundColor(Color::WHITE),
            BorderColor::all(Color::BLACK),
            ChildOf(left.socket),
        ))
        .id();
    let foreign = a
        .world_mut()
        .spawn((
            super::super::composition_input::NodeButton,
            BackgroundColor(Color::BLACK),
        ))
        .id();
    a.world_mut().run_schedule(Update);
    let right_before = color(&a, right);
    node_control_input::set_phase(a.world_mut(), left.socket, SocketPhase::Hovered);
    node_control_input::set_phase(a.world_mut(), foreign, SocketPhase::Hovered);
    a.world_mut().run_schedule(Update);
    assert_eq!(color(&a, right), right_before);
    assert_eq!(
        a.world().get::<BackgroundColor>(child).unwrap().0,
        Color::WHITE
    );
    assert_eq!(
        a.world().get::<BackgroundColor>(foreign).unwrap().0,
        Color::BLACK
    );
}
#[test]
fn socket_without_children_and_removed_socket_entity_are_safe() {
    let mut a = app();
    let m = mount(&mut a, "material:p", "node", Vec2::ZERO);
    a.world_mut().run_schedule(Update);
    a.world_mut().despawn(m.dot);
    node_control_input::set_phase(a.world_mut(), m.socket, SocketPhase::Hovered);
    a.world_mut().run_schedule(Update);
    a.world_mut().despawn(m.socket);
    a.world_mut().run_schedule(Update);
    a.world_mut()
        .run_system_once(update_socket_visuals)
        .unwrap();
}
