use super::super::{graph_pointer::tests as pointer, node::begin_graph_node_press};
use super::*;
use bevy::math::Affine2;

// The observer intentionally accepts a host-supplied QueryFilter. These test controls exercise
// the routing policy without replacing any graph, geometry or document types.
#[derive(Component)]
struct Control;
#[derive(Resource, Default)]
struct Selections(Vec<GraphMarqueeSelection>);

struct Fixture {
    app: App,
    window: Entity,
    viewport: Entity,
    background: Entity,
    node: Entity,
    ui_scale: f32,
}

fn node(key: &str, position: Vec2) -> FeathersGraphNode {
    FeathersGraphNode {
        graph_key: "material".into(),
        node_key: key.into(),
        position,
        selected: false,
        pinned: false,
        collapsed: false,
        dragging: false,
        drag_before: None,
        drag_modifier: None,
        suppress_release_click: false,
    }
}

fn fixture(ui_scale: f32, display_scale: f32, zoom: f32) -> Fixture {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .insert_resource(UiScale(ui_scale))
        .init_resource::<GraphMarqueeGesture>()
        .init_resource::<Selections>()
        .add_observer(begin_graph_marquee::<With<Control>>)
        .add_observer(update_graph_marquee)
        .add_observer(end_graph_marquee)
        .add_observer(cancel_graph_marquee)
        .add_observer(begin_graph_node_press::<With<Control>>)
        .add_observer(
            |event: On<GraphMarqueeSelection>, mut log: ResMut<Selections>| {
                log.0.push(event.event().clone())
            },
        )
        .add_systems(Update, cleanup_graph_marquee);
    let window = app.world_mut().spawn_empty().id();
    let scale = ui_scale * display_scale;
    let viewport = app
        .world_mut()
        .spawn((
            FeathersGraphViewport {
                key: "material".into(),
                pan: Vec2::new(60.0, 30.0),
                zoom,
                content_size: Vec2::splat(1000.0),
                selection_bounds: None,
                frame_request: None,
                measured_frame: None,
            },
            ComputedNode {
                size: Vec2::new(800.0, 600.0) * scale,
                inverse_scale_factor: scale.recip(),
                ..default()
            },
            UiGlobalTransform::from(Affine2::from_translation(Vec2::new(500.0, 400.0) * scale)),
        ))
        .id();
    let background = app.world_mut().spawn(ChildOf(viewport)).id();
    let node = app
        .world_mut()
        .spawn((
            node("n", Vec2::new(100.0, 80.0)),
            ComputedNode {
                size: Vec2::new(80.0, 40.0) * scale,
                inverse_scale_factor: scale.recip(),
                ..default()
            },
            ChildOf(background),
        ))
        .id();
    Fixture {
        app,
        window,
        viewport,
        background,
        node,
        ui_scale,
    }
}

impl Fixture {
    fn position(&self, graph: Vec2) -> Vec2 {
        let view = self
            .app
            .world()
            .get::<FeathersGraphViewport>(self.viewport)
            .unwrap();
        (Vec2::splat(100.0) + view.project_graph_point(graph)) * self.ui_scale
    }
    fn start_at(&mut self, target: Entity, graph: Vec2) {
        let event = pointer::start(
            target,
            self.window,
            self.position(graph),
            PointerId::Mouse,
            PointerButton::Primary,
        );
        self.app.world_mut().trigger(event);
        self.app.world_mut().flush();
    }
    fn start(&mut self, graph: Vec2) {
        self.start_at(self.background, graph);
    }
    fn motion(&mut self, graph: Vec2) {
        let event = pointer::motion(
            self.background,
            self.window,
            self.position(graph),
            PointerId::Mouse,
            PointerButton::Primary,
        );
        self.app.world_mut().trigger(event);
        self.app.world_mut().flush();
    }
    fn end(&mut self, graph: Vec2) {
        let event = pointer::end(
            self.background,
            self.window,
            self.position(graph),
            PointerId::Mouse,
            PointerButton::Primary,
        );
        self.app.world_mut().trigger(event);
        self.app.world_mut().flush();
    }
    fn selections(&self) -> &[GraphMarqueeSelection] {
        &self.app.world().resource::<Selections>().0
    }
    fn overlay(&self) -> Option<Entity> {
        self.app.world().resource::<GraphMarqueeGesture>().overlay
    }
    fn assert_cleared(&mut self) {
        assert!(self.overlay().is_none());
        assert!(
            self.app
                .world()
                .resource::<GraphMarqueeGesture>()
                .viewport
                .is_none()
        );
        assert!(
            self.app
                .world()
                .resource::<GraphMarqueeGesture>()
                .pointer
                .is_none()
        );
        assert_eq!(
            self.app
                .world_mut()
                .query::<&GraphMarqueeOverlay>()
                .iter(self.app.world())
                .count(),
            0
        );
    }
}

#[test]
fn real_pointer_observers_select_at_every_display_ui_scale_and_zoom() {
    for ui in [1.0, 1.5, 2.0] {
        for display in [1.0, 1.5, 2.0] {
            for zoom in [0.08, 0.5, 2.0] {
                let mut f = fixture(ui, display, zoom);
                f.start(Vec2::new(90.0, 70.0));
                f.motion(Vec2::new(190.0, 130.0));
                let overlay = f.overlay().unwrap();
                let style = f.app.world().get::<Node>(overlay).unwrap();
                assert!(
                    matches!(style.width, Val::Px(width) if (width - 100.0 * zoom).abs() < 0.001)
                );
                assert!(
                    matches!(style.height, Val::Px(height) if (height - 60.0 * zoom).abs() < 0.001)
                );
                assert_eq!(
                    f.app.world().get::<Pickable>(overlay),
                    Some(&Pickable::IGNORE)
                );
                f.end(Vec2::new(190.0, 130.0));
                assert_eq!(
                    f.selections().len(),
                    1,
                    "ui={ui} display={display} zoom={zoom}"
                );
                assert_eq!(f.selections()[0].viewport, f.viewport);
                assert_eq!(f.selections()[0].nodes, BTreeSet::from(["n".into()]));
                assert_eq!(f.selections()[0].mode, GraphSelectionMode::Replace);
                f.assert_cleared();
            }
        }
    }
}

#[test]
fn release_applies_final_pointer_position_without_a_motion_event() {
    let mut f = fixture(1.5, 2.0, 0.5);
    f.start(Vec2::ZERO);
    f.end(Vec2::new(200.0, 150.0));
    assert_eq!(f.selections()[0].nodes, BTreeSet::from(["n".into()]));
    f.assert_cleared();
}

#[test]
fn reverse_drag_and_edge_intersections_select_but_other_views_do_not() {
    let mut f = fixture(1.0, 1.0, 1.0);
    let other = f.app.world_mut().spawn_empty().id();
    f.app.world_mut().spawn((
        node("foreign", Vec2::new(100.0, 80.0)),
        ComputedNode {
            size: Vec2::splat(20.0),
            ..default()
        },
        ChildOf(other),
    ));
    f.start(Vec2::new(200.0, 150.0));
    f.end(Vec2::new(180.0, 120.0));
    assert_eq!(f.selections()[0].nodes, BTreeSet::from(["n".into()]));
}

#[test]
fn selection_modifier_precedence_is_captured_at_start() {
    for (keys, expected) in [
        (vec![], GraphSelectionMode::Replace),
        (vec![KeyCode::ShiftRight], GraphSelectionMode::Add),
        (vec![KeyCode::ControlLeft], GraphSelectionMode::Toggle),
        (
            vec![KeyCode::ShiftLeft, KeyCode::ControlRight],
            GraphSelectionMode::Toggle,
        ),
    ] {
        let mut f = fixture(1.0, 1.0, 1.0);
        for key in keys {
            f.app
                .world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
        }
        f.start(Vec2::ZERO);
        f.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        f.end(Vec2::splat(200.0));
        assert_eq!(f.selections()[0].mode, expected);
    }
}

#[test]
fn node_and_nested_control_targets_do_not_start_background_selection() {
    let mut f = fixture(1.0, 1.0, 1.0);
    let node_child = f.app.world_mut().spawn(ChildOf(f.node)).id();
    let control = f
        .app
        .world_mut()
        .spawn((Control, ChildOf(f.background)))
        .id();
    let control_child = f.app.world_mut().spawn(ChildOf(control)).id();
    for target in [f.node, node_child, control, control_child] {
        f.start_at(target, Vec2::ZERO);
        assert!(f.overlay().is_none());
    }
    f.start(Vec2::ZERO);
    assert!(f.overlay().is_some());
}

#[test]
fn space_and_non_primary_buttons_leave_marquee_idle() {
    let mut f = fixture(1.0, 1.0, 1.0);
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    f.start(Vec2::ZERO);
    assert!(f.overlay().is_none());
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    for button in [PointerButton::Secondary, PointerButton::Middle] {
        let event = pointer::start(
            f.background,
            f.window,
            f.position(Vec2::ZERO),
            PointerId::Mouse,
            button,
        );
        f.app.world_mut().trigger(event);
        assert!(f.overlay().is_none());
    }
}

#[test]
fn captured_pointer_ignores_foreign_pointer_window_and_button_events() {
    let mut f = fixture(1.0, 1.0, 1.0);
    let foreign = f.app.world_mut().spawn_empty().id();
    f.start(Vec2::ZERO);
    let overlay = f.overlay();
    for (window, id, button) in [
        (f.window, PointerId::Touch(5), PointerButton::Primary),
        (foreign, PointerId::Mouse, PointerButton::Primary),
        (f.window, PointerId::Mouse, PointerButton::Secondary),
    ] {
        let position = f.position(Vec2::splat(500.0));
        f.app
            .world_mut()
            .trigger(pointer::start(f.background, window, position, id, button));
        f.app
            .world_mut()
            .trigger(pointer::motion(f.background, window, position, id, button));
        f.app
            .world_mut()
            .trigger(pointer::end(f.background, window, position, id, button));
        f.app.world_mut().flush();
        assert_eq!(f.overlay(), overlay);
        assert_eq!(
            f.app.world().resource::<GraphMarqueeGesture>().start,
            f.app.world().resource::<GraphMarqueeGesture>().current
        );
        assert!(f.selections().is_empty());
    }
    f.end(Vec2::splat(200.0));
    assert_eq!(f.selections().len(), 1);
}

#[test]
fn escape_owner_loss_and_overlay_loss_cancel_without_selection() {
    for reason in 0..3 {
        let mut f = fixture(1.0, 1.0, 1.0);
        f.start(Vec2::ZERO);
        match reason {
            0 => f
                .app
                .world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape),
            1 => {
                f.app.world_mut().despawn(f.viewport);
            }
            _ => {
                let overlay = f.overlay().unwrap();
                f.app.world_mut().despawn(overlay);
            }
        }
        f.app.update();
        f.assert_cleared();
        assert!(f.selections().is_empty());
    }
}

#[test]
fn pointer_cancel_only_clears_its_owner() {
    let mut f = fixture(1.0, 1.0, 1.0);
    f.start(Vec2::ZERO);
    f.app
        .world_mut()
        .trigger(pointer::cancel(f.background, f.window, PointerId::Touch(3)));
    assert!(f.overlay().is_some());
    f.app
        .world_mut()
        .trigger(pointer::cancel(f.background, f.window, PointerId::Mouse));
    f.app.world_mut().flush();
    f.assert_cleared();
    assert!(f.selections().is_empty());
}

#[test]
fn non_finite_pointer_and_invalid_node_measurements_do_not_select() {
    let mut f = fixture(1.0, 1.0, 1.0);
    f.start(Vec2::splat(f32::NAN));
    assert!(f.overlay().is_none());
    f.app
        .world_mut()
        .get_mut::<ComputedNode>(f.node)
        .unwrap()
        .size = Vec2::splat(f32::NAN);
    f.start(Vec2::ZERO);
    f.motion(Vec2::splat(f32::INFINITY));
    assert_eq!(
        f.app.world().resource::<GraphMarqueeGesture>().start,
        f.app.world().resource::<GraphMarqueeGesture>().current
    );
    f.end(Vec2::splat(200.0));
    assert!(f.selections()[0].nodes.is_empty());
}

#[test]
fn fresh_primary_press_clears_drag_release_guard_only_through_node_chrome() {
    let mut f = fixture(1.0, 1.0, 1.0);
    let child = f.app.world_mut().spawn(ChildOf(f.node)).id();
    let control = f.app.world_mut().spawn((Control, ChildOf(f.node))).id();
    for (target, button, cleared) in [
        (control, PointerButton::Primary, false),
        (child, PointerButton::Secondary, false),
        (child, PointerButton::Primary, true),
    ] {
        f.app
            .world_mut()
            .get_mut::<FeathersGraphNode>(f.node)
            .unwrap()
            .suppress_release_click = true;
        f.app.world_mut().trigger(pointer::press(
            target,
            f.window,
            Vec2::ZERO,
            PointerId::Mouse,
            button,
        ));
        assert_eq!(
            f.app
                .world_mut()
                .get_mut::<FeathersGraphNode>(f.node)
                .unwrap()
                .consume_suppressed_release_click(),
            !cleared
        );
    }
}

#[test]
fn real_node_state_keeps_release_suppression_until_consumed() {
    let mut node = node("n", Vec2::new(20.0, 50.0));
    node.begin_drag();
    assert!(!node.consume_suppressed_release_click());
    node.note_drag_motion();
    node.end_drag();
    assert!(!node.dragging);
    assert!(node.consume_suppressed_release_click());
    assert!(!node.consume_suppressed_release_click());
    assert_eq!(node.position(), Vec2::new(20.0, 50.0));
    assert_eq!(node.graph_key(), "material");
    assert_eq!(node.node_key(), "n");
    assert!(!node.collapsed());
}

#[test]
fn release_target_changes_do_not_transfer_captured_viewport_ownership() {
    let mut f = fixture(1.5, 2.0, 0.5);
    f.start(Vec2::ZERO);
    let foreign = f.app.world_mut().spawn_empty().id();
    let event = pointer::end(
        foreign,
        f.window,
        f.position(Vec2::splat(200.0)),
        PointerId::Mouse,
        PointerButton::Primary,
    );
    f.app.world_mut().trigger(event);
    f.app.world_mut().flush();
    assert_eq!(f.selections()[0].viewport, f.viewport);
    assert_eq!(f.selections()[0].nodes, BTreeSet::from(["n".into()]));
    f.assert_cleared();
}

#[test]
fn invalid_scale_or_singular_transform_cannot_capture_marquee() {
    for reason in 0..4 {
        let mut f = fixture(1.0, 1.0, 1.0);
        match reason {
            0 => f.app.world_mut().resource_mut::<UiScale>().0 = 0.0,
            1 => {
                f.app
                    .world_mut()
                    .get_mut::<ComputedNode>(f.viewport)
                    .unwrap()
                    .inverse_scale_factor = f32::NAN
            }
            2 => {
                f.app
                    .world_mut()
                    .get_mut::<ComputedNode>(f.viewport)
                    .unwrap()
                    .inverse_scale_factor = -1.0
            }
            _ => {
                *f.app
                    .world_mut()
                    .get_mut::<UiGlobalTransform>(f.viewport)
                    .unwrap() = UiGlobalTransform::from(Affine2::from_scale(Vec2::ZERO))
            }
        }
        f.start(Vec2::ZERO);
        assert!(f.overlay().is_none());
        assert!(f.selections().is_empty());
    }
}
