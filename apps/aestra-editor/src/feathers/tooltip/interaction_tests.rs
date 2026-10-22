//! Shared production lifecycle tests; no wall-clock sleeps or OS pointer input.
use super::super::pointer::tests::{drag_start, over};
use super::*;
use bevy::time::TimeUpdateStrategy;

fn app() -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::text::TextPlugin,
    ))
    .init_resource::<UiScale>()
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO))
    .init_resource::<TooltipState>()
    .add_observer(begin_tooltip)
    .add_observer(dismiss_tooltip_on_drag)
    .add_systems(Update, update_tooltip)
    .add_systems(Last, |mut time: ResMut<TimeUpdateStrategy>| {
        // Tests advance one frame at a time; unrelated work cannot expire the next hover.
        *time = TimeUpdateStrategy::ManualDuration(Duration::ZERO);
    });
    let window = app
        .world_mut()
        .spawn((Window::default(), PrimaryWindow))
        .id();
    // Initialize the real-time clock before the first controlled delta.
    app.update();
    (app, window)
}

fn target(app: &mut App, tooltip: EditorTooltip) -> Entity {
    app.world_mut()
        .spawn((
            tooltip,
            Node::default(),
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::ZERO),
            },
        ))
        .id()
}

fn mature(app: &mut App) {
    let delay = app
        .world()
        .resource::<TooltipState>()
        .target
        .and_then(|target| app.world().get::<EditorTooltip>(target))
        .map_or(DEFAULT_TOOLTIP_DELAY, |tooltip| tooltip.delay);
    app.insert_resource(TimeUpdateStrategy::ManualDuration(delay));
}

fn advance(app: &mut App, duration: Duration) {
    app.insert_resource(TimeUpdateStrategy::ManualDuration(duration));
    app.update();
}

#[test]
fn tooltip_delay_uses_unpaused_real_time_and_opens_at_the_exact_boundary() {
    let (mut app, window) = app();
    let delay = Duration::from_millis(1200);
    let target = target(
        &mut app,
        EditorTooltip::description("help").with_delay(delay),
    );
    app.world_mut().resource_mut::<Time<Virtual>>().pause();
    app.world_mut().trigger(over(target, window));
    advance(&mut app, delay - Duration::from_nanos(1));
    assert!(app.world().resource::<TooltipState>().popup.is_none());
    advance(&mut app, Duration::from_nanos(1));
    assert!(app.world().resource::<TooltipState>().popup.is_some());
    assert_eq!(
        app.world().resource::<Time<Virtual>>().elapsed(),
        Duration::ZERO
    );
}

#[test]
fn switching_targets_restarts_the_full_delay_after_external_popup_removal() {
    let (mut app, window) = app();
    let first = target(&mut app, EditorTooltip::description("first"));
    let second = target(&mut app, EditorTooltip::description("second"));
    app.world_mut().trigger(over(first, window));
    mature(&mut app);
    app.update();
    let old = popup(&app);
    app.world_mut().despawn(old);
    app.world_mut().trigger(over(second, window));
    app.world_mut().flush();
    advance(&mut app, DEFAULT_TOOLTIP_DELAY - Duration::from_nanos(1));
    assert!(app.world().resource::<TooltipState>().popup.is_none());
    advance(&mut app, Duration::from_nanos(1));
    assert_eq!(
        app.world().get::<AccessibleLabel>(popup(&app)).unwrap().0,
        "second"
    );
}

fn popup(app: &App) -> Entity {
    app.world()
        .resource::<TooltipState>()
        .popup
        .expect("visible tooltip")
}

#[test]
fn delay_preserves_one_noninteractive_accessible_popup_and_repeated_hover_does_not_reset_it() {
    let (mut app, window) = app();
    let target = target(
        &mut app,
        EditorTooltip::titled("Lifetime", "Particle life")
            .with_shortcut("F")
            .with_footer("Precision"),
    );
    app.world_mut().trigger(over(target, window));
    app.update();
    assert!(app.world().resource::<TooltipState>().popup.is_none());
    let started = app.world().resource::<TooltipState>().hovered_at;
    app.world_mut().trigger(over(target, window));
    assert_eq!(app.world().resource::<TooltipState>().hovered_at, started);
    mature(&mut app);
    app.update();
    let body = popup(&app);
    assert_eq!(app.world().get::<ChildOf>(body).unwrap().parent(), target);
    assert_eq!(
        app.world().get::<AccessibleLabel>(body).unwrap().0,
        "Lifetime. Particle life. F. Precision"
    );
    assert_eq!(app.world().get::<Pickable>(body), Some(&Pickable::IGNORE));
    assert!(app.world().get::<OverrideClip>(body).is_some());
    app.world_mut().trigger(over(target, window));
    app.update();
    assert_eq!(popup(&app), body);
}

#[test]
fn hover_switch_retires_old_popup_and_restarts_the_new_targets_delay() {
    let (mut app, window) = app();
    let first = target(&mut app, EditorTooltip::description("first"));
    let second = target(&mut app, EditorTooltip::description("second"));
    app.world_mut().trigger(over(first, window));
    mature(&mut app);
    app.update();
    let old = popup(&app);
    app.world_mut().trigger(over(second, window));
    app.world_mut().flush();
    assert!(app.world().get_entity(old).is_err());
    app.update();
    assert!(app.world().resource::<TooltipState>().popup.is_none());
    mature(&mut app);
    app.update();
    assert_eq!(
        app.world().get::<AccessibleLabel>(popup(&app)).unwrap().0,
        "second"
    );
}

#[test]
fn leaving_removing_or_despawning_a_target_clears_its_popup_and_timer() {
    for removal in 0..3 {
        let (mut app, window) = app();
        let target = target(&mut app, EditorTooltip::description("help"));
        app.world_mut().trigger(over(target, window));
        mature(&mut app);
        app.update();
        let old = popup(&app);
        match removal {
            0 => {
                app.world_mut()
                    .get_mut::<RelativeCursorPosition>(target)
                    .unwrap()
                    .cursor_over = false
            }
            1 => {
                app.world_mut().entity_mut(target).remove::<EditorTooltip>();
            }
            _ => {
                app.world_mut().despawn(target);
            }
        }
        app.update();
        assert!(app.world().get_entity(old).is_err());
        let state = app.world().resource::<TooltipState>();
        assert!(state.popup.is_none() && state.target.is_none() && state.hovered_at.is_none());
    }
}

#[test]
fn dragging_clears_pending_or_visible_help_and_updates_do_not_resurrect_it() {
    for visible in [false, true] {
        let (mut app, window) = app();
        let target = target(&mut app, EditorTooltip::description("help"));
        app.world_mut().trigger(over(target, window));
        if visible {
            mature(&mut app);
            app.update();
        }
        let old = app.world().resource::<TooltipState>().popup;
        // A drag anywhere starts an editing/navigation gesture, not just on the tooltip owner.
        let dragged = app.world_mut().spawn_empty().id();
        app.world_mut().trigger(drag_start(dragged, window));
        app.world_mut().flush();
        mature(&mut app);
        app.update();
        assert!(app.world().resource::<TooltipState>().target.is_none());
        assert!(app.world().resource::<TooltipState>().popup.is_none());
        if let Some(old) = old {
            assert!(app.world().get_entity(old).is_err());
        }
    }
}

#[test]
fn externally_removed_popup_is_rebuilt_without_duplicates_while_target_remains_hovered() {
    let (mut app, window) = app();
    let target = target(&mut app, EditorTooltip::description("help"));
    app.world_mut().trigger(over(target, window));
    mature(&mut app);
    app.update();
    let old = popup(&app);
    app.world_mut().despawn(old);
    app.update();
    assert_ne!(popup(&app), old);
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<TooltipPopup>>()
            .iter(app.world())
            .count(),
        1
    );
}

#[test]
fn replacing_or_dragging_after_external_popup_removal_is_safe_before_the_next_update() {
    for dragging in [false, true] {
        let (mut app, window) = app();
        let first = target(&mut app, EditorTooltip::description("first"));
        let second = target(&mut app, EditorTooltip::description("second"));
        app.world_mut().trigger(over(first, window));
        mature(&mut app);
        app.update();
        let old = popup(&app);
        app.world_mut().despawn(old);
        if dragging {
            app.world_mut().trigger(drag_start(first, window));
        } else {
            app.world_mut().trigger(over(second, window));
        }
        app.world_mut().flush();
        app.update();
        assert!(app.world().resource::<TooltipState>().popup.is_none());
        assert_eq!(
            app.world().resource::<TooltipState>().target,
            (!dragging).then_some(second)
        );
    }
}

#[test]
fn cursor_anchor_is_detached_and_inherits_the_nearest_camera_at_each_ui_and_display_scale() {
    use bevy::camera::Viewport;
    for ui_scale in [1.0, 1.5, 2.0] {
        for display_scale in [1.0, 1.5, 2.0] {
            let (mut app, window) = app();
            app.world_mut().resource_mut::<UiScale>().0 = ui_scale;
            {
                let mut window = app.world_mut().get_mut::<Window>(window).unwrap();
                window
                    .resolution
                    .set_scale_factor_override(Some(display_scale));
                window.set_cursor_position(Some(Vec2::new(300.0, 180.0)));
            }
            let far = app.world_mut().spawn(Camera::default()).id();
            let nearest = app
                .world_mut()
                .spawn(Camera {
                    viewport: Some(Viewport {
                        physical_position: UVec2::new(60, 30),
                        physical_size: UVec2::new(640, 480),
                        ..default()
                    }),
                    ..default()
                })
                .id();
            let root = app
                .world_mut()
                .spawn((Node::default(), UiTargetCamera(far)))
                .id();
            let parent = app
                .world_mut()
                .spawn((Node::default(), UiTargetCamera(nearest), ChildOf(root)))
                .id();
            let target = target(
                &mut app,
                EditorTooltip::description("graph help").anchored_to_cursor(),
            );
            app.world_mut().entity_mut(target).insert(ChildOf(parent));
            app.world_mut().trigger(over(target, window));
            mature(&mut app);
            app.update();
            let anchor = popup(&app);
            assert!(app.world().get::<ChildOf>(anchor).is_none());
            assert_eq!(
                app.world().get::<UiTargetCamera>(anchor).unwrap().entity(),
                nearest
            );
            let node = app.world().get::<Node>(anchor).unwrap();
            // Window cursor positions are logical pixels. Only UI scale divides
            // them; the camera viewport origin is physical and needs DPI conversion.
            let expected =
                (Vec2::new(300.0, 180.0) - Vec2::new(60.0, 30.0) / display_scale) / ui_scale;
            assert_eq!(
                (node.left, node.top),
                (Val::Px(expected.x), Val::Px(expected.y))
            );
            let body = app.world().get::<Children>(anchor).unwrap()[0];
            assert!(app.world().get::<Popover>(body).is_some());
            assert_eq!(app.world().get::<Pickable>(body), Some(&Pickable::IGNORE));
        }
    }
}

#[test]
fn cursor_anchor_waits_for_cursor_then_uses_the_default_camera_and_cleans_up_its_body() {
    let (mut app, window) = app();
    let camera = app
        .world_mut()
        .spawn((Camera::default(), IsDefaultUiCamera))
        .id();
    let target = target(
        &mut app,
        EditorTooltip::description("help").anchored_to_cursor(),
    );
    app.world_mut().trigger(over(target, window));
    mature(&mut app);
    app.update();
    assert!(app.world().resource::<TooltipState>().popup.is_none());
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(Vec2::new(100.0, 80.0)));
    app.update();
    let anchor = popup(&app);
    let body = app.world().get::<Children>(anchor).unwrap()[0];
    assert_eq!(
        app.world().get::<UiTargetCamera>(anchor).unwrap().entity(),
        camera
    );
    app.world_mut()
        .get_mut::<RelativeCursorPosition>(target)
        .unwrap()
        .cursor_over = false;
    app.update();
    assert!(app.world().get_entity(anchor).is_err());
    assert!(app.world().get_entity(body).is_err());
}

#[test]
fn unrelated_hover_does_not_replace_a_tooltip_owner_or_restart_its_timer() {
    let (mut app, window) = app();
    let target = target(&mut app, EditorTooltip::description("help"));
    let unrelated = app.world_mut().spawn_empty().id();
    app.world_mut().trigger(over(target, window));
    let started = app.world().resource::<TooltipState>().hovered_at;
    app.world_mut().trigger(over(unrelated, window));
    assert_eq!(app.world().resource::<TooltipState>().target, Some(target));
    assert_eq!(app.world().resource::<TooltipState>().hovered_at, started);
}
