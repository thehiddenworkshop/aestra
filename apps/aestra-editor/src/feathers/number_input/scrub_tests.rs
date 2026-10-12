//! Shared event-dispatch regression suite; no native pointer hit-testing is claimed.
use super::super::pointer::tests::{drag, end, start};
use super::*;
use crate::feathers::text_input::tests::{app_with, field, value};

#[derive(Resource, Default)]
struct Changes(Vec<(Entity, f32, bool)>);

fn fixture(initial: f32, min: f32, max: f32, step: f32) -> (App, Entity, Entity, Entity) {
    let mut app = app_with(|app| {
        app.add_plugins(NumberScrubPlugin)
            .init_resource::<Changes>()
            .add_observer(
                |change: On<ValueChange<f32>>, mut changes: ResMut<Changes>| {
                    changes
                        .0
                        .push((change.source, change.value, change.is_final));
                },
            );
    });
    let (owner, input) = field(&mut app, &formatted(initial, 3));
    app.world_mut()
        .entity_mut(owner)
        .insert(ScrubbableNumber::new(initial, min, max, step));
    let window = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(world)
            .unwrap()
    };
    app.world_mut()
        .entity_mut(window)
        .insert((CursorIcon::default(), CursorOptions::default()));
    app.update();
    (app, owner, input, window)
}

fn changes(app: &App) -> &[(Entity, f32, bool)] {
    &app.world().resource::<Changes>().0
}

fn finish(app: &mut App, target: Entity, window: Entity, button: PointerButton) {
    app.world_mut().trigger(end(target, window, button));
    // ValueChange is queued by the real observer, just as in pointer dispatch.
    app.world_mut().flush();
}

#[test]
fn modifiers_change_scrub_precision() {
    assert_eq!(scrub_multiplier(false, false), 1.0);
    assert_eq!(scrub_multiplier(true, false), 0.1);
    assert_eq!(scrub_multiplier(false, true), 10.0);
    assert_eq!(decimal_places(0.005), 3);
    assert_eq!(formatted(1.2301, 2), "1.23");
}

#[test]
fn scrubbable_number_clamps_and_uses_modifier_precision() {
    let input = ScrubbableNumber::new(0.5, 0.0, 1.0, 0.01);
    assert_eq!(input.normalize(1.5, 1.0), 1.0);
    assert_eq!(input.normalize(-0.5, 1.0), 0.0);
    assert_eq!(input.normalize(0.123_6, 1.0), 0.12);
    assert_eq!(input.normalize(0.123_6, 0.1), 0.124);
}

#[test]
fn cursor_decoration_does_not_rebuild_the_field() {
    let (mut app, owner, input, _) = fixture(0.5, 0.0, 1.0, 0.01);
    let children = app.world().get::<Children>(owner).unwrap().to_vec();
    for entity in [owner, input] {
        assert_eq!(
            app.world().get::<EntityCursor>(entity),
            Some(&EntityCursor::System(SystemCursorIcon::EwResize))
        );
    }
    app.update();
    assert_eq!(
        app.world().get::<Children>(owner).unwrap().to_vec(),
        children
    );
    assert_eq!(value(&app, input), "0.5");
    assert!(changes(&app).is_empty());
}

#[test]
fn descendant_drag_updates_real_text_and_submits_once_on_release() {
    let (mut app, owner, input, window) = fixture(0.5, 0.0, 1.0, 0.01);
    app.world_mut()
        .trigger(start(input, window, PointerButton::Primary));
    assert!(!app.world().get::<CursorOptions>(window).unwrap().visible);
    assert_eq!(
        app.world().resource::<OverrideCursor>().0,
        Some(EntityCursor::System(SystemCursorIcon::EwResize))
    );
    app.world_mut().trigger(drag(
        input,
        window,
        PointerButton::Primary,
        Vec2::new(8.0, 0.0),
    ));
    app.update();
    assert_eq!(value(&app, input), "0.51");
    assert_eq!(changes(&app), [(owner, 0.51, false)]);
    finish(&mut app, input, window, PointerButton::Primary);
    assert!(app.world().get::<CursorOptions>(window).unwrap().visible);
    assert!(app.world().resource::<OverrideCursor>().0.is_none());
    assert_eq!(changes(&app), [(owner, 0.51, false), (owner, 0.51, true)]);
    finish(&mut app, input, window, PointerButton::Primary);
    assert_eq!(changes(&app).len(), 2);
}

#[test]
fn shift_and_control_keep_precision_and_shift_precedence() {
    for (shift, control, expected, text) in [
        (true, false, 0.501, "0.501"),
        (false, true, 0.6, "0.6"),
        (true, true, 0.501, "0.501"),
    ] {
        let (mut app, owner, input, window) = fixture(0.5, 0.0, 1.0, 0.01);
        app.world_mut()
            .trigger(start(input, window, PointerButton::Primary));
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        if shift {
            keys.press(KeyCode::ShiftRight);
        }
        if control {
            keys.press(KeyCode::ControlRight);
        }
        app.world_mut().trigger(drag(
            input,
            window,
            PointerButton::Primary,
            Vec2::new(8.0, 0.0),
        ));
        app.update();
        assert_eq!(changes(&app), [(owner, expected, false)]);
        assert_eq!(value(&app, input), text);
    }
}

#[test]
fn clamping_and_vertical_only_drag_preserve_value_policy() {
    let (mut app, owner, input, window) = fixture(0.5, 0.0, 1.0, 0.01);
    app.world_mut()
        .trigger(start(input, window, PointerButton::Primary));
    app.world_mut().trigger(drag(
        input,
        window,
        PointerButton::Primary,
        Vec2::new(0.0, 200.0),
    ));
    assert!(changes(&app).is_empty());
    app.world_mut().trigger(drag(
        input,
        window,
        PointerButton::Primary,
        Vec2::new(800.0, 0.0),
    ));
    app.update();
    assert_eq!(value(&app, input), "1");
    app.world_mut().trigger(drag(
        input,
        window,
        PointerButton::Primary,
        Vec2::new(-1600.0, 0.0),
    ));
    app.update();
    assert_eq!(value(&app, input), "0");
    finish(&mut app, input, window, PointerButton::Primary);
    assert_eq!(
        changes(&app),
        [(owner, 1.0, false), (owner, 0.0, false), (owner, 0.0, true)]
    );
}

#[test]
fn other_buttons_and_unrelated_targets_cannot_finish_an_active_scrub() {
    let (mut app, owner, input, window) = fixture(0.5, 0.0, 1.0, 0.01);
    let unrelated = app.world_mut().spawn(Node::default()).id();
    app.world_mut()
        .trigger(start(input, window, PointerButton::Secondary));
    app.world_mut().trigger(drag(
        input,
        window,
        PointerButton::Primary,
        Vec2::new(8.0, 0.0),
    ));
    assert!(app.world().resource::<NumberScrubState>().active.is_none());
    assert!(changes(&app).is_empty());
    app.world_mut()
        .trigger(start(input, window, PointerButton::Primary));
    app.world_mut().trigger(drag(
        input,
        window,
        PointerButton::Secondary,
        Vec2::new(8.0, 0.0),
    ));
    app.world_mut().trigger(drag(
        unrelated,
        window,
        PointerButton::Primary,
        Vec2::new(8.0, 0.0),
    ));
    finish(&mut app, unrelated, window, PointerButton::Primary);
    finish(&mut app, input, window, PointerButton::Secondary);
    assert!(changes(&app).is_empty());
    assert!(!app.world().get::<CursorOptions>(window).unwrap().visible);
    finish(&mut app, input, window, PointerButton::Primary);
    assert_eq!(changes(&app), [(owner, 0.5, true)]);
    assert!(app.world().get::<CursorOptions>(window).unwrap().visible);
}

#[test]
fn active_scrub_stops_pointer_propagation_to_parent_controls() {
    #[derive(Resource, Default)]
    struct ParentDrags(usize);
    let (mut app, _, input, window) = fixture(0.5, 0.0, 1.0, 0.01);
    app.init_resource::<ParentDrags>();
    let ancestor = app
        .world_mut()
        .spawn(Node::default())
        .observe(|_: On<PointerDrag>, mut seen: ResMut<ParentDrags>| {
            seen.0 += 1;
        })
        .id();
    // A non-numeric ancestor must not also respond to this numeric drag.
    let owner = app.world().get::<ChildOf>(input).unwrap().parent();
    app.world_mut().entity_mut(ancestor).add_child(owner);
    app.world_mut()
        .trigger(start(input, window, PointerButton::Primary));
    app.world_mut().trigger(drag(
        input,
        window,
        PointerButton::Primary,
        Vec2::new(8.0, 0.0),
    ));
    assert_eq!(app.world().resource::<ParentDrags>().0, 0);
}
