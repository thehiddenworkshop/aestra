//! Real production scene, numeric bridge, scrub, arrows, text and focus together.
use super::super::{
    arrows::{CustomNumberStep, NumberArrowsPlugin, NumberArrowsSet, StepNumber},
    pointer::tests::{click, drag, end, start},
    scrub::{NumberScrubPlugin, ScrubbableNumber},
};
use super::*;
use crate::feathers::{
    icon::SvgFile,
    text_input::tests::{app_with, focus, tap, value},
};
use bevy::{
    input::keyboard::Key,
    picking::{
        backend::HitData,
        hover::HoverMap,
        pointer::{PointerButton, PointerId},
    },
    ui_widgets::{Activate, Button},
    window::{CursorIcon, CursorOptions, PrimaryWindow},
};

#[derive(Resource, Default)]
struct Values {
    floats: Vec<(Entity, f32, bool)>,
    doubles: Vec<(Entity, f64, bool)>,
    integers: Vec<(Entity, i32, bool)>,
    large: Vec<(Entity, i64, bool)>,
}

#[derive(Resource, Default)]
struct TestHover(Option<Entity>);

fn app() -> App {
    app_with(|app| {
        app.init_asset::<SvgFile>()
            .insert_resource(crate::Localizer::new("en-US").unwrap())
            .init_resource::<Values>()
            .init_resource::<TestHover>()
            .add_plugins((NumberArrowsPlugin, NumberScrubPlugin))
            .add_systems(
                Update,
                (|test: Res<TestHover>, mut hover: ResMut<HoverMap>| {
                    hover.clear();
                    if let Some(entity) = test.0 {
                        hover
                            .entry(PointerId::Mouse)
                            .or_default()
                            .insert(entity, HitData::new(Entity::PLACEHOLDER, 0.0, None, None));
                    }
                })
                .before(NumberArrowsSet),
            )
            .add_observer(|e: On<ValueChange<f32>>, mut v: ResMut<Values>| {
                v.floats.push((e.source, e.value, e.is_final));
            })
            .add_observer(|e: On<ValueChange<f64>>, mut v: ResMut<Values>| {
                v.doubles.push((e.source, e.value, e.is_final));
            })
            .add_observer(|e: On<ValueChange<i32>>, mut v: ResMut<Values>| {
                v.integers.push((e.source, e.value, e.is_final));
            })
            .add_observer(|e: On<ValueChange<i64>>, mut v: ResMut<Values>| {
                v.large.push((e.source, e.value, e.is_final));
            });
    })
}

fn settle(app: &mut App) {
    for _ in 0..3 {
        app.update();
    }
}

fn spawn(app: &mut App, format: NumberFormat, initial: NumberInputValue) -> (Entity, Entity) {
    let mut commands = app.world_mut().commands();
    let mut root = commands.spawn_empty();
    root.apply_scene(bsn! { @EditorNumberInput { @number_format: format } });
    let owner = root.id();
    app.world_mut().flush();
    settle(app);
    app.world_mut().trigger(UpdateNumberInput {
        entity: owner,
        value: initial,
    });
    settle(app);
    let input = app
        .world()
        .get::<Children>(owner)
        .unwrap()
        .iter()
        .find(|e| app.world().get::<EditableText>(*e).is_some())
        .unwrap();
    (owner, input)
}

fn replace(app: &mut App, input: Entity, text: &str) {
    let mut editable = app.world_mut().get_mut::<EditableText>(input).unwrap();
    editable.queue_edit(TextEdit::SelectAll);
    editable.queue_edit(TextEdit::Insert(text.into()));
}

fn arrow(app: &App, owner: Entity, increase: bool) -> Entity {
    app.world()
        .get::<Children>(owner)
        .unwrap()
        .iter()
        .find(|e| {
            app.world().get::<AccessibleLabel>(*e).is_some_and(|label| {
                label.0
                    == if increase {
                        "Increase value"
                    } else {
                        "Decrease value"
                    }
            })
        })
        .unwrap()
}

fn hover(app: &mut App, entity: Entity) {
    app.world_mut().resource_mut::<TestHover>().0 = Some(entity);
}

fn window(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .unwrap()
}

#[test]
fn numeric_scenes_keep_label_sigil_direct_text_and_exactly_two_hover_arrows() {
    let mut app = app();
    let mut commands = app.world_mut().commands();
    let scalar = commands
        .spawn_empty()
        .apply_scene(feathers_scalar_input())
        .id();
    let integer = commands
        .spawn_empty()
        .apply_scene(feathers_integer_input())
        .id();
    let labeled = commands
        .spawn_empty()
        .apply_scene(feathers_labeled_scalar_input(
            "X",
            tokens::TEXT_INPUT_X_AXIS,
        ))
        .id();
    app.world_mut().flush();
    settle(&mut app);
    assert_eq!(
        *app.world().get::<NumberFormat>(integer).unwrap(),
        NumberFormat::I32
    );
    assert_eq!(
        app.world().get::<ThemeBorderColor>(labeled).unwrap().0,
        tokens::TEXT_INPUT_X_AXIS
    );
    for owner in [scalar, integer, labeled] {
        let children = app.world().get::<Children>(owner).unwrap();
        assert_eq!(
            children
                .iter()
                .filter(|e| app.world().get::<Button>(*e).is_some())
                .count(),
            2
        );
        let input = children
            .iter()
            .find(|e| app.world().get::<EditableText>(*e).is_some())
            .unwrap();
        // No upstream scrubber overlay or extra chevrons.
        assert!(
            app.world()
                .get::<bevy::feathers::controls::FeathersNumberInput>(owner)
                .is_none()
        );
        let surface = app.world().get::<Children>(input).unwrap();
        assert_eq!(surface.len(), 1);
        assert!(app.world().get::<NumericSurface>(surface[0]).is_some());
        for increase in [false, true] {
            let arrow = arrow(&app, owner, increase);
            assert_eq!(
                app.world().get::<Visibility>(arrow),
                Some(&Visibility::Hidden)
            );
        }
        hover(&mut app, input);
        settle(&mut app);
        for increase in [false, true] {
            let arrow = arrow(&app, owner, increase);
            assert_eq!(
                app.world().get::<Visibility>(arrow),
                Some(&Visibility::Visible)
            );
            assert_eq!(app.world().get::<Node>(arrow).unwrap().width, px(12));
            assert_eq!(
                app.world().get::<BackgroundColor>(arrow).unwrap().0,
                crate::theme::MENU_ITEM_HOVER
            );
        }
        app.world_mut().resource_mut::<TestHover>().0 = None;
    }
}

#[test]
fn programmatic_updates_are_silent_typed_and_preserve_a_focused_edit() {
    let mut app = app();
    let (owner, input) = spawn(&mut app, NumberFormat::F32, NumberInputValue::F32(0.5));
    assert_eq!(value(&app, input), "0.5");
    assert!(app.world().resource::<Values>().floats.is_empty());
    focus(&mut app, input);
    replace(&mut app, input, "0.75");
    settle(&mut app);
    app.world_mut().resource_mut::<Values>().floats.clear();
    app.world_mut().trigger(UpdateNumberInput {
        entity: owner,
        value: NumberInputValue::F32(0.1),
    });
    settle(&mut app);
    assert_eq!(value(&app, input), "0.75");
    assert!(app.world().resource::<Values>().floats.is_empty());
}

#[test]
fn numeric_text_keeps_the_filter_and_twenty_character_limit() {
    let mut app = app();
    let (_, input) = spawn(&mut app, NumberFormat::F32, NumberInputValue::F32(0.5));
    focus(&mut app, input);
    tap(
        &mut app,
        KeyCode::KeyA,
        Key::Character("xyz".into()),
        Some("xyz"),
    );
    assert_eq!(value(&app, input), "0.5");
    assert!(app.world().resource::<Values>().floats.is_empty());
    replace(&mut app, input, "1234567890123456789012345");
    settle(&mut app);
    // Feathers rejects an over-limit insertion atomically, rather than truncating.
    assert_eq!(value(&app, input), "0.5");
    replace(&mut app, input, "12345678901234567890");
    settle(&mut app);
    assert_eq!(value(&app, input), "12345678901234567890");
}

#[test]
fn owned_surface_defers_press_focus_and_click_does_not_end_a_scrub_as_text_editing() {
    let mut app = app();
    let (owner, input) = spawn(&mut app, NumberFormat::F32, NumberInputValue::F32(0.5));
    app.world_mut()
        .entity_mut(owner)
        .insert(ScrubbableNumber::new(0.5, 0.0, 1.0, 0.01));
    let surface = app.world().get::<Children>(input).unwrap()[0];
    let window = window(&mut app);
    app.world_mut().trigger(AcquireFocus {
        focused_entity: surface,
        window,
    });
    settle(&mut app);
    assert_ne!(app.world().resource::<InputFocus>().get(), Some(input));
    app.world_mut()
        .trigger(start(surface, window, PointerButton::Primary));
    app.world_mut().trigger(drag(
        surface,
        window,
        PointerButton::Primary,
        Vec2::new(8.0, 0.0),
    ));
    // The same order as Bevy picking on release: Click, then DragEnd.
    app.world_mut().trigger(click(surface, window));
    app.world_mut()
        .trigger(end(surface, window, PointerButton::Primary));
    settle(&mut app);
    assert_eq!(
        app.world().resource::<Values>().floats,
        [(owner, 0.51, false), (owner, 0.51, true)]
    );
    assert_ne!(app.world().resource::<InputFocus>().get(), Some(input));
    app.world_mut().trigger(click(surface, window));
    settle(&mut app);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(input));
    assert_eq!(
        *app.world().get::<Pickable>(surface).unwrap(),
        Pickable::IGNORE
    );
    assert_eq!(app.world().resource::<Values>().floats.len(), 2);
    app.world_mut().resource_mut::<InputFocus>().clear();
    settle(&mut app);
    assert_eq!(
        *app.world().get::<Pickable>(surface).unwrap(),
        Pickable::default()
    );
}

#[test]
fn keyboard_typing_enter_release_and_blur_have_one_typed_event_each() {
    let mut app = app();
    let (owner, input) = spawn(&mut app, NumberFormat::F32, NumberInputValue::F32(0.0));
    focus(&mut app, input);
    tap(
        &mut app,
        KeyCode::Digit2,
        Key::Character("2".into()),
        Some("2"),
    );
    assert_eq!(
        app.world().resource::<Values>().floats,
        [(owner, 2.0, false)]
    );
    tap(&mut app, KeyCode::Enter, Key::Enter, None);
    assert_eq!(
        app.world().resource::<Values>().floats,
        [(owner, 2.0, false), (owner, 2.0, true)]
    );
    app.world_mut().resource_mut::<InputFocus>().clear();
    settle(&mut app);
    assert_eq!(
        app.world().resource::<Values>().floats,
        [(owner, 2.0, false), (owner, 2.0, true), (owner, 2.0, true)]
    );
}

#[test]
fn real_scene_scrub_emits_no_duplicate_value_from_display_text_updates() {
    let mut app = app();
    let (owner, input) = spawn(&mut app, NumberFormat::F32, NumberInputValue::F32(0.5));
    app.world_mut()
        .entity_mut(owner)
        .insert(ScrubbableNumber::new(0.5, 0.0, 1.0, 0.01));
    let window = window(&mut app);
    app.world_mut()
        .entity_mut(window)
        .insert((CursorIcon::default(), CursorOptions::default()));
    app.world_mut()
        .trigger(start(input, window, PointerButton::Primary));
    app.world_mut().trigger(drag(
        input,
        window,
        PointerButton::Primary,
        Vec2::new(8.0, 0.0),
    ));
    app.world_mut()
        .trigger(end(input, window, PointerButton::Primary));
    settle(&mut app);
    assert_eq!(value(&app, input), "0.51");
    assert_eq!(
        app.world().resource::<Values>().floats,
        [(owner, 0.51, false), (owner, 0.51, true)]
    );
    assert!(app.world().get::<CursorOptions>(window).unwrap().visible);
}

#[test]
fn real_scene_arrow_activation_is_single_final_and_custom_steps_stay_external() {
    let mut app = app();
    let (owner, input) = spawn(&mut app, NumberFormat::F32, NumberInputValue::F32(0.5));
    app.world_mut()
        .entity_mut(owner)
        .insert(ScrubbableNumber::new(0.5, 0.0, 1.0, 0.1));
    let right = arrow(&app, owner, true);
    app.world_mut().trigger(Activate { entity: right });
    settle(&mut app);
    assert_eq!(value(&app, input), "0.6");
    assert_eq!(
        app.world().resource::<Values>().floats,
        [(owner, 0.6, true)]
    );
    app.world_mut().entity_mut(owner).insert(CustomNumberStep);
    app.world_mut().trigger(Activate { entity: right });
    settle(&mut app);
    assert_eq!(
        app.world().resource::<Values>().floats,
        [(owner, 0.6, true)]
    );
}

#[test]
fn typing_and_arrows_keep_f64_i32_and_large_i64_values_exact() {
    let mut app = app();
    let (double, d) = spawn(&mut app, NumberFormat::F64, NumberInputValue::F64(0.0));
    focus(&mut app, d);
    replace(&mut app, d, "1.2345678901234567");
    settle(&mut app);
    assert_eq!(
        app.world().resource::<Values>().doubles,
        [(double, 1.234_567_890_123_456_7, false)]
    );
    let (int, i) = spawn(&mut app, NumberFormat::I32, NumberInputValue::I32(i32::MAX));
    app.world_mut().trigger(StepNumber {
        entity: int,
        direction: 1,
    });
    settle(&mut app);
    assert_eq!(value(&app, i), i32::MAX.to_string());
    assert_eq!(
        app.world().resource::<Values>().integers,
        [(int, i32::MAX, true)]
    );
    let (large, l) = spawn(
        &mut app,
        NumberFormat::I64,
        NumberInputValue::I64(9_007_199_254_740_993),
    );
    app.world_mut().trigger(StepNumber {
        entity: large,
        direction: 1,
    });
    settle(&mut app);
    assert_eq!(value(&app, l), "9007199254740994");
    assert_eq!(
        app.world().resource::<Values>().large,
        [(large, 9_007_199_254_740_994, true)]
    );
}

#[test]
fn invalid_partial_nonfinite_and_mismatched_values_do_not_emit_or_replace() {
    let mut app = app();
    let (owner, input) = spawn(&mut app, NumberFormat::F32, NumberInputValue::F32(0.5));
    focus(&mut app, input);
    for text in ["-", "1e", "1e999"] {
        replace(&mut app, input, text);
        settle(&mut app);
        tap(&mut app, KeyCode::Enter, Key::Enter, None);
    }
    assert!(app.world().resource::<Values>().floats.is_empty());
    app.world_mut().resource_mut::<InputFocus>().clear();
    settle(&mut app);
    app.world_mut().trigger(UpdateNumberInput {
        entity: owner,
        value: NumberInputValue::I64(12),
    });
    settle(&mut app);
    assert_eq!(value(&app, input), "1e999");
}

#[test]
fn disabled_ancestors_and_text_editing_block_scrub_and_arrow_commits() {
    let mut app = app();
    let (owner, input) = spawn(&mut app, NumberFormat::F32, NumberInputValue::F32(0.5));
    app.world_mut()
        .entity_mut(owner)
        .insert(ScrubbableNumber::new(0.5, 0.0, 1.0, 0.1));
    let window = window(&mut app);
    focus(&mut app, input);
    app.world_mut()
        .trigger(start(input, window, PointerButton::Primary));
    app.world_mut().trigger(drag(
        input,
        window,
        PointerButton::Primary,
        Vec2::new(8.0, 0.0),
    ));
    settle(&mut app);
    assert!(app.world().resource::<Values>().floats.is_empty());
    app.world_mut().resource_mut::<InputFocus>().clear();
    settle(&mut app);
    app.world_mut().resource_mut::<Values>().floats.clear();
    let parent = app.world_mut().spawn(InteractionDisabled).id();
    app.world_mut().entity_mut(parent).add_child(owner);
    app.world_mut()
        .trigger(start(input, window, PointerButton::Primary));
    app.world_mut().trigger(drag(
        input,
        window,
        PointerButton::Primary,
        Vec2::new(8.0, 0.0),
    ));
    let right = arrow(&app, owner, true);
    app.world_mut().trigger(Activate { entity: right });
    settle(&mut app);
    assert!(app.world().resource::<Values>().floats.is_empty());
    assert_eq!(value(&app, input), "0.5");
}
