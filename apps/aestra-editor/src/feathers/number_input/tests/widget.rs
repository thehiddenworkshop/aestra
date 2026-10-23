//! Production wrapper, released widget, real coordinate-based picking and typed events.
//! No tests rely on Bevy's private scrubber/chevron component or child ordering.
use super::super::{
    NumberScrubPlugin, ScrubbableNumber,
    arrows::{CustomNumberStep, NumberArrowsPlugin, StepNumber},
    native::{EditPhase, NumberEdit},
};
use super::*;
use crate::feathers::text_input::tests::{app_with, focus, tap, value};
use bevy::{
    camera::RenderTarget,
    input::keyboard::Key,
    input_focus::InputFocus,
    picking::pointer::{Location, PointerAction, PointerButton, PointerId, PointerInput},
    text::{EditableText, TextEdit, TextLayoutInfo},
    ui::InteractionDisabled,
    ui_widgets::{Activate, Button, ValueChange},
    window::{PrimaryWindow, WindowRef},
};

#[derive(Resource, Default)]
struct Values(Vec<(Entity, NumberInputValue, bool)>);
#[derive(Resource, Default)]
struct Edits(Vec<EditPhase>);

struct Fixture {
    app: App,
    owner: Entity,
    native: Entity,
    text: Entity,
}

fn settle(app: &mut App) {
    for _ in 0..5 {
        app.update();
    }
}

fn app() -> App {
    app_with(|app| {
        app.insert_resource(crate::Localizer::new("en-US").unwrap())
            .init_resource::<Values>()
            .init_resource::<Edits>()
            .add_plugins((NumberScrubPlugin, NumberArrowsPlugin))
            .add_observer(|e: On<NumberEdit>, mut edits: ResMut<Edits>| {
                edits.0.push(e.phase);
            });
        macro_rules! observe {
            ($ty:ty, $variant:ident) => {
                app.add_observer(
                    |e: On<ValueChange<$ty>>,
                     owners: Query<(), With<EditorNumberInput>>,
                     mut values: ResMut<Values>| {
                        if owners.contains(e.source) {
                            values.0.push((
                                e.source,
                                NumberInputValue::$variant(e.value),
                                e.is_final,
                            ));
                        }
                    },
                );
            };
        }
        observe!(f32, F32);
        observe!(f64, F64);
        observe!(i32, I32);
        observe!(i64, I64);
    })
}

fn fixture(format: NumberFormat, initial: NumberInputValue) -> Fixture {
    let mut app = app();
    let camera = app.world_mut().spawn(Camera2d).id();
    let parent = app
        .world_mut()
        .spawn((
            Node {
                width: px(240),
                ..default()
            },
            UiTargetCamera(camera),
        ))
        .id();
    let owner = app
        .world_mut()
        .commands()
        .spawn(ChildOf(parent))
        .apply_scene(bsn! { @EditorNumberInput { @number_format: format } })
        .id();
    app.world_mut().flush();
    app.world_mut().trigger(UpdateNumberInput {
        entity: owner,
        value: initial,
    });
    settle(&mut app);
    let native = app
        .world()
        .get::<Children>(owner)
        .unwrap()
        .iter()
        .find(|&e| app.world().get::<FeathersNumberInput>(e).is_some())
        .unwrap();
    let text = app
        .world()
        .get::<Children>(native)
        .unwrap()
        .iter()
        .find(|&e| app.world().get::<EditableText>(e).is_some())
        .unwrap();
    Fixture {
        app,
        owner,
        native,
        text,
    }
}

fn scalar() -> Fixture {
    fixture(NumberFormat::F32, NumberInputValue::F32(0.5))
}
fn replace(f: &mut Fixture, text: &str) {
    let mut editable = f.app.world_mut().get_mut::<EditableText>(f.text).unwrap();
    editable.queue_edit(TextEdit::SelectAll);
    editable.queue_edit(TextEdit::Insert(text.into()));
    settle(&mut f.app);
}
fn point(f: &Fixture) -> Vec2 {
    f.app
        .world()
        .get::<UiGlobalTransform>(f.text)
        .unwrap()
        .translation
}
fn pointer(f: &mut Fixture, position: Vec2, action: PointerAction) {
    let window = f
        .app
        .world_mut()
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(f.app.world())
        .unwrap();
    f.app.world_mut().write_message(PointerInput::new(
        PointerId::Mouse,
        Location {
            target: RenderTarget::Window(WindowRef::Entity(window))
                .normalize(None)
                .unwrap(),
            position,
        },
        action,
    ));
    settle(&mut f.app);
}
fn begin(f: &mut Fixture) -> Vec2 {
    let p = point(f);
    pointer(f, p, PointerAction::Move { delta: Vec2::ZERO });
    pointer(f, p, PointerAction::Press(PointerButton::Primary));
    p
}
fn drag(f: &mut Fixture, p: Vec2, delta: Vec2) {
    // One motion starts the native drag, the second supplies a measured delta.
    pointer(f, p + delta, PointerAction::Move { delta });
    pointer(f, p + delta * 2.0, PointerAction::Move { delta });
}
fn release(f: &mut Fixture, p: Vec2) {
    pointer(f, p, PointerAction::Release(PointerButton::Primary));
}
fn values(f: &Fixture) -> &[(Entity, NumberInputValue, bool)] {
    &f.app.world().resource::<Values>().0
}
fn arrow(f: &Fixture, increase: bool) -> Entity {
    f.app
        .world()
        .get::<Children>(f.owner)
        .unwrap()
        .iter()
        .find(|&e| {
            f.app
                .world()
                .get::<AccessibleLabel>(e)
                .is_some_and(|label| {
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

#[test]
fn every_constructor_composes_exactly_one_native_control_and_two_external_steppers() {
    let mut f = scalar();
    for scene in [0, 1, 2] {
        let mut commands = f.app.world_mut().commands();
        let mut root = commands.spawn_empty();
        match scene {
            0 => {
                root.apply_scene(feathers_scalar_input());
            }
            1 => {
                root.apply_scene(feathers_integer_input());
            }
            _ => {
                root.apply_scene(feathers_labeled_scalar_input(
                    "X",
                    tokens::TEXT_INPUT_X_AXIS,
                ));
            }
        }
        let owner = root.id();
        f.app.world_mut().flush();
        settle(&mut f.app);
        let children = f.app.world().get::<Children>(owner).unwrap();
        assert_eq!(
            children
                .iter()
                .filter(|&e| f.app.world().get::<FeathersNumberInput>(e).is_some())
                .count(),
            1
        );
        assert_eq!(
            children
                .iter()
                .filter(|&e| f.app.world().get::<Button>(e).is_some())
                .count(),
            2
        );
        assert!(
            f.app.world().get::<FeathersNumberInput>(owner).is_none(),
            "semantic source is a wrapper, never a second native widget"
        );
        if scene == 2 {
            assert_eq!(
                f.app.world().get::<ThemeBorderColor>(owner).unwrap().0,
                tokens::TEXT_INPUT_X_AXIS
            );
        }
    }
}

#[test]
fn deferred_model_initialization_is_silent_typed_and_keeps_the_latest_value() {
    let mut app = app();
    let owner = app.world_mut().spawn_empty().id();
    for value in [1.0, 17.5] {
        app.world_mut().trigger(UpdateNumberInput {
            entity: owner,
            value: NumberInputValue::F32(value),
        });
    }
    app.world_mut()
        .commands()
        .entity(owner)
        .apply_scene(feathers_scalar_input());
    app.world_mut().flush();
    settle(&mut app);
    let displayed = app
        .world()
        .iter_entities()
        .find_map(|e| e.get::<EditableText>().map(|t| t.value().to_string()))
        .unwrap();
    assert_eq!(displayed, "17.5");
    assert!(app.world().resource::<Values>().0.is_empty());
}

#[test]
fn real_native_click_enters_editing_and_enter_blur_commit_once() {
    let mut f = scalar();
    let p = begin(&mut f);
    release(&mut f, p);
    assert_eq!(f.app.world().resource::<InputFocus>().get(), Some(f.text));
    replace(&mut f, "2.75");
    assert_eq!(values(&f), [(f.owner, NumberInputValue::F32(2.75), false)]);
    tap(&mut f.app, KeyCode::Enter, Key::Enter, None);
    f.app.world_mut().resource_mut::<InputFocus>().clear();
    settle(&mut f.app);
    assert_eq!(
        values(&f),
        [
            (f.owner, NumberInputValue::F32(2.75), false),
            (f.owner, NumberInputValue::F32(2.75), true)
        ]
    );
}

#[test]
fn blur_without_enter_commits_once_and_escape_restores_without_a_commit() {
    let mut f = scalar();
    focus(&mut f.app, f.text);
    replace(&mut f, "2.75");
    f.app.world_mut().resource_mut::<InputFocus>().clear();
    settle(&mut f.app);
    assert_eq!(
        values(&f),
        [
            (f.owner, NumberInputValue::F32(2.75), false),
            (f.owner, NumberInputValue::F32(2.75), true)
        ]
    );
    focus(&mut f.app, f.text);
    replace(&mut f, "8");
    tap(&mut f.app, KeyCode::Escape, Key::Escape, None);
    assert_eq!(value(&f.app, f.text), "2.75");
    assert_eq!(values(&f).iter().filter(|event| event.2).count(), 1);
    assert_eq!(
        values(&f).last(),
        Some(&(f.owner, NumberInputValue::F32(2.75), false))
    );
}

#[test]
fn invalid_partial_and_nonfinite_input_roll_back_without_events() {
    let mut f = scalar();
    for text in ["-", "1e", "1e999", "NaN", "inf"] {
        focus(&mut f.app, f.text);
        replace(&mut f, text);
        tap(&mut f.app, KeyCode::Enter, Key::Enter, None);
        assert_eq!(value(&f.app, f.text), "0.5", "invalid {text}");
    }
    assert!(values(&f).is_empty());
}

#[test]
fn silent_model_sync_preserves_focused_text_but_rejects_a_stale_final() {
    let mut f = scalar();
    focus(&mut f.app, f.text);
    replace(&mut f, "0.75");
    f.app.world_mut().trigger(UpdateNumberInput {
        entity: f.owner,
        value: NumberInputValue::F32(0.1),
    });
    settle(&mut f.app);
    assert_eq!(value(&f.app, f.text), "0.75");
    tap(&mut f.app, KeyCode::Enter, Key::Enter, None);
    f.app.world_mut().resource_mut::<InputFocus>().clear();
    settle(&mut f.app);
    assert_eq!(value(&f.app, f.text), "0.1");
    assert_eq!(
        values(&f),
        [
            (f.owner, NumberInputValue::F32(0.75), false),
            (f.owner, NumberInputValue::F32(0.1), false)
        ]
    );
}

#[test]
fn native_scrub_displays_live_drafts_and_emits_one_final_without_focusing() {
    let mut f = scalar();
    f.app
        .world_mut()
        .entity_mut(f.owner)
        .insert(ScrubbableNumber::new(0.5, 0.0, 1.0, 0.01));
    settle(&mut f.app);
    let p = begin(&mut f);
    drag(&mut f, p, Vec2::X * 8.0);
    assert!(!values(&f).is_empty());
    assert!(values(&f).iter().all(|v| !v.2));
    assert_ne!(value(&f.app, f.text), "0.5");
    release(&mut f, p + Vec2::X * 16.0);
    assert_eq!(values(&f).iter().filter(|v| v.2).count(), 1);
    assert_ne!(
        *f.app
            .world()
            .get::<bevy::text::TextReadWriteMode>(f.text)
            .unwrap(),
        bevy::text::TextReadWriteMode::Editable,
        "a scrub must not enter text editing"
    );
}

#[test]
fn cancelled_drag_restores_baseline_and_late_release_cannot_commit() {
    for escape in [false, true] {
        let mut f = scalar();
        let p = begin(&mut f);
        drag(&mut f, p, Vec2::X * 40.0);
        assert_ne!(value(&f.app, f.text), "0.5");
        if escape {
            tap(&mut f.app, KeyCode::Escape, Key::Escape, None);
        } else {
            pointer(&mut f, p + Vec2::X * 1000.0, PointerAction::Cancel);
        }
        assert_eq!(value(&f.app, f.text), "0.5");
        release(&mut f, p + Vec2::X * 80.0);
        assert!(!values(&f).iter().any(|v| v.2));
        assert!(
            f.app
                .world()
                .resource::<Edits>()
                .0
                .contains(&EditPhase::Cancel)
        );
    }
}

#[test]
fn shift_uses_upstream_precision_and_vertical_motion_cannot_change_value() {
    let mut normal = scalar();
    let p = begin(&mut normal);
    drag(&mut normal, p, Vec2::X * 80.0);
    let NumberInputValue::F32(normal_value) = *normal
        .app
        .world()
        .get::<NumberInputValue>(normal.native)
        .unwrap()
    else {
        panic!()
    };
    let mut precise = scalar();
    precise
        .app
        .world_mut()
        .resource_mut::<ButtonInput<Key>>()
        .press(Key::Shift);
    let p = begin(&mut precise);
    drag(&mut precise, p, Vec2::X * 80.0);
    let NumberInputValue::F32(precise_value) = *precise
        .app
        .world()
        .get::<NumberInputValue>(precise.native)
        .unwrap()
    else {
        panic!()
    };
    assert!(precise_value > 0.5 && precise_value < normal_value);
    let mut f = scalar();
    let p = begin(&mut f);
    drag(&mut f, p, Vec2::Y * 80.0);
    release(&mut f, p + Vec2::Y * 160.0);
    assert!(values(&f).is_empty());
    assert_eq!(value(&f.app, f.text), "0.5");
}

#[test]
fn native_limits_apply_to_text_drag_and_step_buttons() {
    let mut f = scalar();
    f.app
        .world_mut()
        .entity_mut(f.owner)
        .insert(ScrubbableNumber::new(0.5, 0.0, 1.0, 0.1));
    settle(&mut f.app);
    focus(&mut f.app, f.text);
    replace(&mut f, "99");
    tap(&mut f.app, KeyCode::Enter, Key::Enter, None);
    assert_eq!(value(&f.app, f.text), "1");
    let count = values(&f).len();
    let right = arrow(&f, true);
    f.app.world_mut().trigger(Activate { entity: right });
    settle(&mut f.app);
    assert_eq!(values(&f).len(), count, "at-limit activation is a no-op");
    f.app.world_mut().resource_mut::<InputFocus>().clear();
    settle(&mut f.app);
    let p = begin(&mut f);
    drag(&mut f, p, -Vec2::X * 2000.0);
    release(&mut f, p - Vec2::X * 4000.0);
    assert_eq!(value(&f.app, f.text), "0");
}

#[test]
fn generic_and_custom_steppers_never_emit_duplicate_semantic_changes() {
    let mut f = scalar();
    f.app
        .world_mut()
        .entity_mut(f.owner)
        .insert(ScrubbableNumber::new(0.5, 0.0, 1.0, 0.1));
    settle(&mut f.app);
    let right = arrow(&f, true);
    f.app.world_mut().trigger(Activate { entity: right });
    settle(&mut f.app);
    assert_eq!(values(&f), [(f.owner, NumberInputValue::F32(0.6), true)]);
    f.app
        .world_mut()
        .entity_mut(f.owner)
        .insert(CustomNumberStep);
    f.app.world_mut().trigger(Activate { entity: right });
    settle(&mut f.app);
    assert_eq!(values(&f).len(), 1);
    let p = begin(&mut f);
    drag(&mut f, p, Vec2::X * 8.0);
    release(&mut f, p + Vec2::X * 16.0);
    assert_eq!(
        values(&f).len(),
        1,
        "custom controls receive NumberEdit, not a second ValueChange"
    );
}

#[test]
fn f64_and_large_i64_text_and_step_values_remain_exact() {
    let mut f = fixture(NumberFormat::F64, NumberInputValue::F64(0.0));
    focus(&mut f.app, f.text);
    replace(&mut f, "1.2345678901234567");
    tap(&mut f.app, KeyCode::Enter, Key::Enter, None);
    assert_eq!(
        values(&f),
        [
            (
                f.owner,
                NumberInputValue::F64(1.234_567_890_123_456_7),
                false
            ),
            (
                f.owner,
                NumberInputValue::F64(1.234_567_890_123_456_7),
                true
            )
        ]
    );
    let mut f = fixture(
        NumberFormat::I64,
        NumberInputValue::I64(9_007_199_254_740_993),
    );
    let owner = f.owner;
    f.app.world_mut().trigger(StepNumber {
        entity: owner,
        direction: 1,
    });
    settle(&mut f.app);
    assert_eq!(value(&f.app, f.text), "9007199254740994");
    focus(&mut f.app, f.text);
    replace(&mut f, "9223372036854775807");
    tap(&mut f.app, KeyCode::Enter, Key::Enter, None);
    assert_eq!(
        values(&f).last().unwrap().1,
        NumberInputValue::I64(i64::MAX)
    );
    let count = values(&f).len();
    f.app.world_mut().trigger(StepNumber {
        entity: owner,
        direction: 1,
    });
    settle(&mut f.app);
    assert_eq!(values(&f).len(), count);
}

#[test]
fn wrong_type_and_repeated_model_updates_are_silent_and_replace_not_append() {
    let mut f = scalar();
    for value in [0.51, 0.52] {
        f.app.world_mut().trigger(UpdateNumberInput {
            entity: f.owner,
            value: NumberInputValue::F32(value),
        });
    }
    settle(&mut f.app);
    assert_eq!(value(&f.app, f.text), "0.52");
    f.app.world_mut().trigger(UpdateNumberInput {
        entity: f.owner,
        value: NumberInputValue::I64(12),
    });
    settle(&mut f.app);
    assert_eq!(value(&f.app, f.text), "0.52");
    assert!(values(&f).is_empty());
}

#[test]
fn native_units_parse_suffixes_and_preserve_canonical_f64_values() {
    use bevy::feathers::controls::NumberInputUnits;
    let mut f = fixture(NumberFormat::F64, NumberInputValue::F64(1.0));
    f.app
        .world_mut()
        .entity_mut(f.owner)
        .insert(NumberInputUnits::new(
            &bevy::feathers::controls::LengthMeters,
        ));
    settle(&mut f.app);
    focus(&mut f.app, f.text);
    replace(&mut f, "250cm");
    tap(&mut f.app, KeyCode::Enter, Key::Enter, None);
    assert_eq!(
        values(&f).last(),
        Some(&(f.owner, NumberInputValue::F64(2.5), true))
    );
    assert_eq!(values(&f).iter().filter(|e| e.2).count(), 1);
}

#[test]
fn optional_number_can_be_cleared_without_a_numeric_sentinel_or_double_commit() {
    #[derive(Resource, Default)]
    struct Clears(usize);
    let mut f = scalar();
    f.app
        .world_mut()
        .entity_mut(f.owner)
        .insert(super::super::OptionalNumberInput);
    f.app.init_resource::<Clears>().add_observer(
        |e: On<ValueChange<String>>,
         owners: Query<(), With<EditorNumberInput>>,
         mut clears: ResMut<Clears>| {
            if owners.contains(e.source) && e.is_final && e.value.is_empty() {
                clears.0 += 1;
            }
        },
    );
    focus(&mut f.app, f.text);
    replace(&mut f, "");
    tap(&mut f.app, KeyCode::Enter, Key::Enter, None);
    f.app.world_mut().resource_mut::<InputFocus>().clear();
    settle(&mut f.app);
    assert_eq!(f.app.world().resource::<Clears>().0, 1);
    assert!(!values(&f).iter().any(|e| e.2));
}

#[test]
fn disabled_ancestors_reject_keyboard_drag_step_and_direct_native_events() {
    let mut f = scalar();
    let parent = f.app.world().get::<ChildOf>(f.owner).unwrap().parent();
    f.app
        .world_mut()
        .entity_mut(parent)
        .insert(InteractionDisabled);
    settle(&mut f.app);
    for is_final in [false, true] {
        f.app.world_mut().trigger(ValueChange {
            source: f.native,
            value: 0.75_f32,
            is_final,
        });
    }
    let right = arrow(&f, true);
    f.app.world_mut().trigger(Activate { entity: right });
    let p = begin(&mut f);
    drag(&mut f, p, Vec2::X * 80.0);
    release(&mut f, p + Vec2::X * 160.0);
    assert!(values(&f).is_empty());
    assert_eq!(value(&f.app, f.text), "0.5");
}

#[test]
fn hover_and_editing_preserve_measured_layout_and_readable_stepper_geometry() {
    let mut f = scalar();
    let geometry = |f: &Fixture| {
        [f.owner, f.native, f.text].map(|e| {
            (
                f.app.world().get::<ComputedNode>(e).unwrap().size(),
                f.app
                    .world()
                    .get::<UiGlobalTransform>(e)
                    .unwrap()
                    .translation,
            )
        })
    };
    let idle = geometry(&f);
    let p = point(&f);
    pointer(&mut f, p, PointerAction::Move { delta: Vec2::ZERO });
    assert_eq!(geometry(&f), idle);
    for increase in [false, true] {
        let arrow = arrow(&f, increase);
        assert_eq!(
            *f.app.world().get::<Visibility>(arrow).unwrap(),
            Visibility::Visible
        );
        assert_eq!(f.app.world().get::<Node>(arrow).unwrap().width, px(20));
        assert_eq!(
            f.app.world().get::<BackgroundColor>(arrow).unwrap().0,
            crate::theme::MENU_ITEM_HOVER
        );
    }
    assert!(
        f.app
            .world()
            .get::<TextLayoutInfo>(f.text)
            .unwrap()
            .glyphs
            .len()
            >= 3
    );
    focus(&mut f.app, f.text);
    assert_eq!(geometry(&f), idle);
    for increase in [false, true] {
        assert_eq!(
            *f.app
                .world()
                .get::<Visibility>(arrow(&f, increase))
                .unwrap(),
            Visibility::Hidden
        );
    }
}
