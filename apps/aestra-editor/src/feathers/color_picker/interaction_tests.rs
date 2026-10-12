//! Actual native scene and pointer observers, shared by shipping and candidate.
use super::super::color_pointer::tests::{cancel, channel_y, click, drag, end, press, start};
use super::*;
use crate::feathers::text_input::tests::app_with;
use bevy::{feathers::controls::FeathersColorPlane, ui::UiGlobalTransform, window::PrimaryWindow};

#[derive(Resource, Default)]
struct Events(Vec<(Entity, Option<[f32; 4]>, bool)>);

fn app() -> App {
    app_with(|app| {
        app.init_resource::<Events>()
            .add_observer(handle_color_plane_change)
            .add_observer(begin_color_plane)
            .add_observer(track_color_plane_drag)
            .add_observer(cancel_color_plane)
            .add_observer(handle_color_lightness_change)
            .add_observer(handle_color_alpha_change)
            .add_observer(handle_color_channel_change)
            .add_observer(handle_color_hex_change)
            .add_observer(
                |e: On<ValueChange<Option<[f32; 4]>>>, mut events: ResMut<Events>| {
                    events.0.push((e.source, e.value, e.is_final));
                },
            )
            .add_systems(Update, sync_color_picker_visuals);
    })
}

fn labels() -> ColorPickerLabels {
    ColorPickerLabels {
        accessible: "Color".into(),
        hue_saturation: "Hue / saturation".into(),
        lightness: "Lightness".into(),
        alpha: "Alpha".into(),
        automatic: "Automatic".into(),
        rgb: "RGB".into(),
        hsl: "HSL".into(),
        red: "Red".into(),
        green: "Green".into(),
        blue: "Blue".into(),
        hue: "Hue".into(),
        saturation: "Saturation".into(),
        hex: "Hex".into(),
    }
}

fn spawn(app: &mut App, authored: Option<[f32; 4]>) -> (Entity, Entity, Entity, Entity) {
    let mut root = Entity::PLACEHOLDER;
    app.world_mut()
        .commands()
        .spawn(Node::default())
        .with_children(|parent| {
            root = spawn_color_picker(parent, authored, [0.2, 0.4, 0.6, 0.8], labels(), ());
        });
    app.world_mut().flush();
    for _ in 0..3 {
        app.update();
    }
    let world = app.world_mut();
    let plane = world
        .query_filtered::<Entity, With<ColorPickerPlane>>()
        .iter(world)
        .find(|entity| {
            world
                .get::<ChildOf>(*entity)
                .and_then(|p| world.get::<ChildOf>(p.parent()))
                .is_some_and(|p| p.parent() == root)
        })
        .unwrap();
    assert_eq!(
        *world.get::<FeathersColorPlane>(plane).unwrap(),
        FeathersColorPlane::HueSaturation
    );
    let inner = world.get::<Children>(plane).unwrap()[0];
    assert_eq!(
        world.get::<Children>(inner).unwrap().len(),
        1,
        "native thumb exists"
    );
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .unwrap();
    // Deterministic input geometry, not a replacement widget or picking backend.
    world.entity_mut(inner).insert((
        ComputedNode {
            size: Vec2::splat(100.0),
            ..default()
        },
        UiGlobalTransform::default(),
    ));
    world.resource_mut::<Events>().0.clear();
    (root, plane, inner, window)
}

fn at(app: &mut App, inner: Entity, window: Entity, screen: Vec2) {
    // Identity transform puts the center at (0, 0), with edges at +/-50.
    app.world_mut()
        .trigger(press(inner, window, screen * 100.0 - Vec2::splat(50.0)));
    app.world_mut().flush();
}

#[test]
fn native_plane_corners_and_outside_clamping_use_channel_values_once() {
    let mut app = app();
    let (root, _, inner, window) = spawn(&mut app, Some([1.0, 0.0, 0.0, 0.4]));
    for screen in [
        Vec2::ZERO,
        Vec2::ONE,
        Vec2::new(0.25, 0.75),
        Vec2::new(-1.0, 2.0),
    ] {
        at(&mut app, inner, window, screen);
        let picker = app.world().get::<ColorPickerState>(root).unwrap();
        assert!((picker.hsla.hue - screen.x.clamp(0.0, 1.0) * 360.0).abs() < 0.0001);
        assert!((picker.hsla.saturation - channel_y(screen.y.clamp(0.0, 1.0))).abs() < 0.0001);
        assert_eq!(picker.hsla.lightness, 0.5);
        assert_eq!(picker.hsla.alpha, 0.4);
    }
    assert_eq!(app.world().resource::<Events>().0.len(), 4);
    assert!(
        app.world()
            .resource::<Events>()
            .0
            .iter()
            .all(|e| e.0 == root && !e.2)
    );
}

#[test]
fn upstream_thumb_tracks_programmatic_channel_values() {
    let mut app = app();
    let (_, plane, inner, _) = spawn(&mut app, None);
    for value in [Vec2::ZERO, Vec2::new(0.25, 0.75), Vec2::ONE] {
        app.world_mut().entity_mut(inner).insert(ComputedNode {
            size: Vec2::splat(100.0),
            ..default()
        });
        app.world_mut()
            .entity_mut(plane)
            .insert(ColorPlaneValue(value.extend(0.5)));
        app.update();
        super::super::color_pointer::tests::assert_thumb(app.world(), inner, value);
    }
}

#[test]
fn click_commits_once_and_idle_click_does_not_commit() {
    let mut app = app();
    let (_, _, inner, window) = spawn(&mut app, None);
    app.world_mut().trigger(click(inner, window));
    assert!(app.world().resource::<Events>().0.is_empty());
    at(&mut app, inner, window, Vec2::splat(0.5));
    app.world_mut().trigger(click(inner, window));
    app.world_mut().trigger(click(inner, window));
    app.world_mut().flush();
    let events = &app.world().resource::<Events>().0;
    assert_eq!(events.len(), 2);
    assert!(!events[0].2);
    assert!(events[1].2);
}

#[test]
fn native_drag_owns_final_commit_even_when_click_arrives_before_drag_end() {
    let mut app = app();
    let (_, _, inner, window) = spawn(&mut app, None);
    at(&mut app, inner, window, Vec2::splat(0.5));
    app.world_mut().trigger(start(inner, window));
    app.world_mut()
        .trigger(drag(inner, window, Vec2::new(25.0, -25.0)));
    app.world_mut().trigger(click(inner, window));
    app.world_mut().flush();
    assert_eq!(app.world().resource::<Events>().0.len(), 2);
    app.world_mut()
        .trigger(end(inner, window, Vec2::new(25.0, -25.0)));
    app.world_mut().flush();
    app.world_mut().trigger(click(inner, window));
    app.world_mut().flush();
    let events = &app.world().resource::<Events>().0;
    assert_eq!(
        events.iter().map(|e| e.2).collect::<Vec<_>>(),
        [false, false, true]
    );
}

#[test]
fn canceled_drag_does_not_commit_and_the_next_click_still_works() {
    let mut app = app();
    let (_, _, inner, window) = spawn(&mut app, None);
    at(&mut app, inner, window, Vec2::splat(0.5));
    app.world_mut().trigger(start(inner, window));
    app.world_mut().trigger(cancel(inner, window));
    app.world_mut().trigger(click(inner, window));
    app.world_mut().trigger(end(inner, window, Vec2::ZERO));
    app.world_mut().flush();
    assert_eq!(app.world().resource::<Events>().0.len(), 1);
    at(&mut app, inner, window, Vec2::splat(0.25));
    app.world_mut().trigger(click(inner, window));
    app.world_mut().flush();
    assert_eq!(
        app.world()
            .resource::<Events>()
            .0
            .iter()
            .filter(|e| e.2)
            .count(),
        1
    );
}

#[test]
fn disabled_plane_or_picker_cannot_change_or_commit() {
    for ancestor in [false, true] {
        let mut app = app();
        let (root, plane, inner, window) = spawn(&mut app, None);
        let original = *app.world().get::<ColorPickerState>(root).unwrap();
        app.world_mut()
            .entity_mut(if ancestor { root } else { plane })
            .insert(InteractionDisabled);
        at(&mut app, inner, window, Vec2::splat(0.75));
        app.world_mut().trigger(start(inner, window));
        app.world_mut().trigger(drag(inner, window, Vec2::ZERO));
        app.world_mut().trigger(click(inner, window));
        app.world_mut().trigger(end(inner, window, Vec2::ZERO));
        app.world_mut().flush();
        assert!(app.world().resource::<Events>().0.is_empty());
        assert_eq!(
            *app.world().get::<ColorPickerState>(root).unwrap(),
            original
        );
    }
}

#[test]
fn disabling_during_a_click_clears_the_pending_commit() {
    let mut app = app();
    let (root, _, inner, window) = spawn(&mut app, None);
    at(&mut app, inner, window, Vec2::splat(0.5));
    app.world_mut().entity_mut(root).insert(InteractionDisabled);
    app.world_mut().trigger(click(inner, window));
    app.world_mut()
        .entity_mut(root)
        .remove::<InteractionDisabled>();
    app.world_mut().trigger(click(inner, window));
    app.world_mut().flush();
    assert_eq!(app.world().resource::<Events>().0.len(), 1);
}

#[test]
fn plane_events_are_owner_scoped_and_nonfinite_values_are_ignored() {
    let mut app = app();
    let (first, plane, _, _) = spawn(&mut app, None);
    let (second, _, _, _) = spawn(&mut app, Some([1.0, 0.0, 0.0, 1.0]));
    let original = *app.world().get::<ColorPickerState>(second).unwrap();
    for value in [Vec2::new(f32::NAN, 0.5), Vec2::new(0.5, f32::INFINITY)] {
        app.world_mut().trigger(ValueChange {
            source: plane,
            value,
            is_final: true,
        });
    }
    app.world_mut().trigger(ValueChange {
        source: second,
        value: Vec2::ONE,
        is_final: true,
    });
    assert!(app.world().resource::<Events>().0.is_empty());
    app.world_mut().trigger(ValueChange {
        source: plane,
        value: Vec2::new(0.5, 0.75),
        is_final: true,
    });
    app.world_mut().flush();
    assert_eq!(app.world().resource::<Events>().0.len(), 1);
    assert_eq!(app.world().resource::<Events>().0[0].0, first);
    assert_eq!(
        *app.world().get::<ColorPickerState>(second).unwrap(),
        original
    );
    assert!(
        !app.world()
            .get::<ColorPickerState>(first)
            .unwrap()
            .automatic
    );
}

#[test]
fn programmatic_sync_preserves_hsl_alpha_channels_and_does_not_emit() {
    let mut app = app();
    let (root, plane, _, _) = spawn(&mut app, None);
    let mut state = ColorPickerState::from_srgba([0.2, 0.6, 0.8, 0.4]);
    state.mode = ColorPickerMode::Hsl;
    app.world_mut().entity_mut(root).insert(state);
    for _ in 0..3 {
        app.update();
    }
    let value = app.world().get::<ColorPlaneValue>(plane).unwrap().0;
    assert_eq!(
        value,
        Vec3::new(
            state.hsla.hue / 360.0,
            state.hsla.saturation,
            state.hsla.lightness
        )
    );
    let world = app.world_mut();
    assert_eq!(
        world
            .query::<&ColorPickerChannelInput>()
            .iter(world)
            .count(),
        8
    );
    for (channel, scrub) in world
        .query::<(&ColorPickerChannelInput, &number_input::ScrubbableNumber)>()
        .iter(world)
    {
        assert_eq!(scrub.value, channel_display_value(*channel, state));
    }
    for (panel, node) in world
        .query::<(&ColorPickerChannelPanel, &Node)>()
        .iter(world)
    {
        assert_eq!(
            node.display,
            if panel.0 == ColorPickerMode::Hsl {
                Display::Flex
            } else {
                Display::None
            }
        );
    }
    for (base, alpha, lightness, slider) in world
        .query::<(
            &SliderBaseColor,
            Has<ColorPickerAlpha>,
            Has<ColorPickerLightness>,
            &SliderValue,
        )>()
        .iter(world)
    {
        assert_eq!(base.0, Color::Hsla(state.hsla));
        assert_eq!(
            slider.0,
            if alpha {
                0.4
            } else {
                assert!(lightness);
                state.hsla.lightness
            }
        );
    }
    assert!(world.resource::<Events>().0.is_empty());
}

#[test]
fn invalid_hex_is_safe_and_automatic_reset_emits_one_none() {
    for invalid in ["💥ab", "ééé", "#nothex", "", "#12345"] {
        assert_eq!(parse_hex(invalid), None);
    }
    let mut app = app();
    let (root, _, _, _) = spawn(&mut app, Some([1.0, 0.0, 0.0, 1.0]));
    let world = app.world_mut();
    let automatic = world
        .query_filtered::<Entity, With<ColorPickerAutomatic>>()
        .single(world)
        .unwrap();
    world.trigger(Activate { entity: automatic });
    world.flush();
    assert_eq!(world.resource::<Events>().0, [(root, None, true)]);
    let state = world.get::<ColorPickerState>(root).unwrap();
    assert!(state.automatic);
    for (actual, expected) in state.srgba().into_iter().zip([0.2, 0.4, 0.6, 0.8]) {
        assert!((actual - expected).abs() < 0.0001);
    }
}

#[test]
fn real_hex_typing_still_emits_live_and_enter_commits_once() {
    use crate::feathers::text_input::tests::{focus, tap};
    use bevy::input::keyboard::Key;
    let mut app = app();
    let (root, _, _, _) = spawn(&mut app, None);
    let world = app.world_mut();
    let container = world
        .query_filtered::<Entity, With<ColorPickerHexInput>>()
        .single(world)
        .unwrap();
    let input = world.get::<Children>(container).unwrap()[0];
    focus(&mut app, input);
    assert!(app.world().resource::<Events>().0.is_empty());
    assert!(app.world().get::<ColorPickerState>(root).unwrap().automatic);
    app.world_mut()
        .get_mut::<EditableText>(input)
        .unwrap()
        .queue_edit(TextEdit::SelectAll);
    app.update();
    tap(
        &mut app,
        KeyCode::KeyA,
        Key::Character("#80402080".into()),
        Some("#80402080"),
    );
    let events = &app.world().resource::<Events>().0;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0, root);
    assert!(!events[0].2);
    assert!(!app.world().get::<ColorPickerState>(root).unwrap().automatic);
    tap(&mut app, KeyCode::Enter, Key::Enter, None);
    let events = &app.world().resource::<Events>().0;
    assert_eq!(events.len(), 2);
    assert!(events[1].2);
    for (actual, byte) in events[1]
        .1
        .unwrap()
        .into_iter()
        .zip([0x80_u8, 0x40, 0x20, 0x80])
    {
        assert!((actual - f32::from(byte) / 255.0).abs() < 0.0001);
    }
}

#[test]
fn fresh_press_recovers_after_a_drag_ended_while_disabled() {
    let mut app = app();
    let (_, plane, inner, window) = spawn(&mut app, None);
    at(&mut app, inner, window, Vec2::splat(0.5));
    app.world_mut().trigger(start(inner, window));
    app.world_mut()
        .entity_mut(plane)
        .insert(InteractionDisabled);
    app.world_mut().trigger(end(inner, window, Vec2::ZERO));
    app.world_mut().flush();
    app.world_mut()
        .entity_mut(plane)
        .remove::<InteractionDisabled>();
    at(&mut app, inner, window, Vec2::splat(0.25));
    app.world_mut().trigger(click(inner, window));
    app.world_mut().flush();
    assert_eq!(
        app.world()
            .resource::<Events>()
            .0
            .iter()
            .map(|e| e.2)
            .collect::<Vec<_>>(),
        [false, false, true]
    );
}
