//! Exercise the actual settings row and upstream widget through public components
//! and real UI picking. Do not locate private scrubber/chevron child markers.
use super::*;
use crate::feathers::{
    icon::SvgFile,
    number_input::{
        EditorNumberInput, NumberScrubPlugin, ScrubbableNumber,
        arrows::{CustomNumberStep, NumberArrowsPlugin},
    },
    text_input::tests::{app_with, focus, tap, value},
};
use bevy::{
    camera::RenderTarget,
    input::keyboard::Key,
    picking::{
        hover::HoverMap,
        pointer::{Location, PointerAction, PointerButton, PointerId, PointerInput},
    },
    text::{TextEdit, TextLayoutInfo},
    window::{PrimaryWindow, WindowRef},
};

struct Fixture {
    app: App,
    owner: Entity,
    text: Entity,
    path: std::path::PathBuf,
    _directory: tempfile::TempDir,
}

fn settle(app: &mut App) {
    for _ in 0..5 {
        app.update();
    }
}

fn fixture() -> Fixture {
    fixture_with(30)
}

fn fixture_with(initial: u16) -> Fixture {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.ron");
    let mut app = app_with(|app| {
        app.init_asset::<SvgFile>()
            .insert_resource({
                let mut settings = EditorSettings::default();
                settings.general.autosave_interval_seconds = initial;
                settings
            })
            .insert_resource(EditorSession::for_unopened_project())
            .insert_resource(SettingsPersistence::for_test(path.clone()))
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_plugins((NumberScrubPlugin, NumberArrowsPlugin));
        install(app);
    });
    let camera = app.world_mut().spawn(Camera2d).id();
    app.world_mut()
        .commands()
        .spawn((
            Node {
                width: px(280),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            UiTargetCamera(camera),
        ))
        .with_children(|parent| {
            spawn_settings_integer(
                parent,
                "Recovery interval",
                "Seconds between snapshots",
                SettingsNumber::AutosaveInterval,
                Some("s"),
            );
            spawn_settings_integer(
                parent,
                "Columns",
                "Not migrated",
                SettingsNumber::ContactSheetColumns,
                None,
            );
        });
    app.world_mut().flush();
    settle(&mut app);
    let owner = app
        .world_mut()
        .query_filtered::<Entity, With<AutosaveNumber>>()
        .single(app.world())
        .unwrap();
    let text = app
        .world()
        .iter_entities()
        .find(|e| {
            e.contains::<EditableText>() && {
                let mut parent = e.get::<ChildOf>().map(ChildOf::parent);
                let mut found = false;
                while let Some(p) = parent {
                    if p == owner {
                        found = true;
                        break;
                    }
                    parent = app.world().get::<ChildOf>(p).map(ChildOf::parent);
                }
                found
            }
        })
        .unwrap()
        .id();
    Fixture {
        app,
        owner,
        text,
        path,
        _directory: directory,
    }
}

fn replace(f: &mut Fixture, digits: &str) {
    let mut text = f.app.world_mut().get_mut::<EditableText>(f.text).unwrap();
    text.queue_edit(TextEdit::SelectAll);
    text.queue_edit(TextEdit::Insert(digits.into()));
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

fn begin_drag(f: &mut Fixture) -> Vec2 {
    let p = point(f);
    pointer(f, p, PointerAction::Move { delta: Vec2::ZERO });
    assert!(
        !f.app.world().resource::<HoverMap>().is_empty(),
        "real UI picking must hit the row"
    );
    pointer(f, p, PointerAction::Press(PointerButton::Primary));
    p
}

fn model(f: &Fixture) -> u16 {
    f.app
        .world()
        .resource::<EditorSettings>()
        .general
        .autosave_interval_seconds
}

#[test]
fn production_pilot_has_native_limits_and_readable_stable_layout_without_legacy_handlers() {
    let mut f = fixture();
    let entity = f.app.world().entity(f.owner);
    assert!(entity.contains::<FeathersNumberInput>());
    assert!(!entity.contains::<EditorNumberInput>());
    assert!(!entity.contains::<ScrubbableNumber>());
    assert!(!entity.contains::<CustomNumberStep>());
    assert!(!entity.contains::<SettingsNumberControl>());
    assert_eq!(value(&f.app, f.text), "30");
    let node = f.app.world().get::<ComputedNode>(f.text).unwrap();
    let text = f.app.world().get::<TextLayoutInfo>(f.text).unwrap();
    assert_eq!(text.glyphs.len(), 2);
    assert!(
        node.size().y > 0.0 && node.content_box().width() >= text.size.x,
        "cell={:?}, text={:?}",
        node.size(),
        text.size
    );
    let geometry = node.size();
    let p = point(&f);
    pointer(&mut f, p, PointerAction::Move { delta: Vec2::ZERO });
    assert_eq!(
        f.app.world().get::<ComputedNode>(f.text).unwrap().size(),
        geometry
    );
    focus(&mut f.app, f.text);
    assert_eq!(
        f.app.world().get::<ComputedNode>(f.text).unwrap().size(),
        geometry
    );
    assert!(!f.path.exists());
}

#[test]
fn keyboard_commits_once_clamps_and_never_dirties_the_effect() {
    let mut f = fixture();
    let revision = f
        .app
        .world()
        .resource::<EditorSession>()
        .document_revision();
    let ui_revision = f.app.world().resource::<EditorSession>().ui_revision;
    let p = point(&f);
    pointer(&mut f, p, PointerAction::Move { delta: Vec2::ZERO });
    pointer(&mut f, p, PointerAction::Press(PointerButton::Primary));
    pointer(&mut f, p, PointerAction::Release(PointerButton::Primary));
    assert_eq!(
        f.app.world().resource::<InputFocus>().get(),
        Some(f.text),
        "real click enters native text editing"
    );
    replace(&mut f, "900");
    assert_eq!(model(&f), 30);
    assert!(!f.path.exists());
    tap(&mut f.app, KeyCode::Enter, Key::Enter, None);
    assert_eq!(model(&f), 600);
    assert_eq!(value(&f.app, f.text), "600");
    let saved: EditorSettings = ron::from_str(&std::fs::read_to_string(&f.path).unwrap()).unwrap();
    assert_eq!(saved.general.autosave_interval_seconds, 600);
    assert_eq!(
        f.app.world().resource::<EditorSession>().ui_revision,
        ui_revision + 1
    );
    tap(&mut f.app, KeyCode::Enter, Key::Enter, None);
    assert_eq!(
        f.app.world().resource::<EditorSession>().ui_revision,
        ui_revision + 1
    );
    assert_eq!(
        f.app
            .world()
            .resource::<EditorSession>()
            .document_revision(),
        revision
    );
}

#[test]
fn invalid_text_escape_and_external_updates_preserve_the_model() {
    let mut f = fixture();
    focus(&mut f.app, f.text);
    replace(&mut f, "77");
    tap(&mut f.app, KeyCode::Escape, Key::Escape, None);
    assert_eq!(model(&f), 30);
    assert_eq!(value(&f.app, f.text), "30");
    assert!(!f.path.exists());
    focus(&mut f.app, f.text);
    replace(&mut f, "invalid");
    tap(&mut f.app, KeyCode::Enter, Key::Enter, None);
    assert_eq!(value(&f.app, f.text), "30");
    assert!(!f.path.exists());
    f.app
        .world_mut()
        .resource_mut::<EditorSettings>()
        .general
        .autosave_interval_seconds = 45;
    settle(&mut f.app);
    assert_eq!(value(&f.app, f.text), "45");
    focus(&mut f.app, f.text);
    replace(&mut f, "77");
    f.app
        .world_mut()
        .resource_mut::<EditorSettings>()
        .general
        .autosave_interval_seconds = 50;
    settle(&mut f.app);
    assert_eq!(
        value(&f.app, f.text),
        "77",
        "external sync must not replace a focused draft"
    );
    tap(&mut f.app, KeyCode::Enter, Key::Enter, None);
    assert_eq!(
        model(&f),
        50,
        "stale typed commit must not overwrite a newer model"
    );
    assert_eq!(value(&f.app, f.text), "50");
    assert!(!f.path.exists());
}

#[test]
fn real_picking_scrub_displays_drafts_and_persists_only_on_release() {
    let mut f = fixture();
    let ui_revision = f.app.world().resource::<EditorSession>().ui_revision;
    let p = begin_drag(&mut f);
    pointer(
        &mut f,
        p + Vec2::X * 8.0,
        PointerAction::Move {
            delta: Vec2::X * 8.0,
        },
    );
    pointer(
        &mut f,
        p + Vec2::X * 16.0,
        PointerAction::Move {
            delta: Vec2::X * 8.0,
        },
    );
    let draft = *f.app.world().get::<NumberInputValue>(f.owner).unwrap();
    assert_ne!(
        draft,
        NumberInputValue::I32(30),
        "upstream drag must produce a real draft"
    );
    assert_eq!(model(&f), 30);
    assert!(!f.path.exists());
    assert_eq!(
        f.app.world().resource::<EditorSession>().ui_revision,
        ui_revision
    );
    pointer(
        &mut f,
        p + Vec2::X * 16.0,
        PointerAction::Release(PointerButton::Primary),
    );
    assert_eq!(NumberInputValue::I32(i32::from(model(&f))), draft);
    assert!(f.path.exists());
    assert_eq!(
        f.app.world().resource::<EditorSession>().ui_revision,
        ui_revision + 1
    );
}

#[test]
fn real_drag_cancel_restores_the_saved_value_and_shift_reduces_sensitivity() {
    let mut f = fixture();
    let p = begin_drag(&mut f);
    pointer(
        &mut f,
        p + Vec2::X * 8.0,
        PointerAction::Move {
            delta: Vec2::X * 8.0,
        },
    );
    pointer(
        &mut f,
        p + Vec2::X * 16.0,
        PointerAction::Move {
            delta: Vec2::X * 8.0,
        },
    );
    let NumberInputValue::I32(normal) = *f.app.world().get::<NumberInputValue>(f.owner).unwrap()
    else {
        panic!("integer input changed type")
    };
    pointer(&mut f, p + Vec2::X * 16.0, PointerAction::Cancel);
    assert_eq!(value(&f.app, f.text), "30");
    assert_eq!(model(&f), 30);
    assert!(!f.path.exists());
    let p = begin_drag(&mut f);
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<Key>>()
        .press(Key::Shift);
    pointer(
        &mut f,
        p + Vec2::X * 8.0,
        PointerAction::Move {
            delta: Vec2::X * 8.0,
        },
    );
    pointer(
        &mut f,
        p + Vec2::X * 16.0,
        PointerAction::Move {
            delta: Vec2::X * 8.0,
        },
    );
    let NumberInputValue::I32(precise) = *f.app.world().get::<NumberInputValue>(f.owner).unwrap()
    else {
        panic!("integer input changed type")
    };
    assert!(
        30 < precise && precise < normal,
        "shift={precise}, normal={normal}"
    );
    pointer(&mut f, p + Vec2::X * 16.0, PointerAction::Cancel);
    assert!(!f.path.exists());
}

#[test]
fn blur_commits_valid_text_but_reverts_invalid_text_without_a_write() {
    let mut f = fixture();
    focus(&mut f.app, f.text);
    replace(&mut f, "not-a-number");
    f.app.world_mut().resource_mut::<InputFocus>().clear();
    settle(&mut f.app);
    assert_eq!(value(&f.app, f.text), "30");
    assert!(!f.path.exists());
    focus(&mut f.app, f.text);
    replace(&mut f, "-10");
    f.app.world_mut().resource_mut::<InputFocus>().clear();
    settle(&mut f.app);
    assert_eq!(model(&f), 5);
    assert_eq!(value(&f.app, f.text), "5");
    assert!(f.path.exists());
}

#[test]
fn disabled_ancestors_reject_commits_and_drafts_without_side_effects() {
    let mut f = fixture();
    let parent = f.app.world().get::<ChildOf>(f.owner).unwrap().parent();
    f.app
        .world_mut()
        .entity_mut(parent)
        .insert(InteractionDisabled);
    for is_final in [false, true] {
        f.app.world_mut().trigger(ValueChange {
            source: f.owner,
            value: 100_i32,
            is_final,
        });
        settle(&mut f.app);
    }
    assert_eq!(model(&f), 30);
    assert_eq!(value(&f.app, f.text), "30");
    assert!(!f.path.exists());
}

#[test]
fn app_stepper_buttons_keep_native_interaction_and_do_not_write_at_limits() {
    let mut f = fixture();
    let p = point(&f);
    pointer(&mut f, p, PointerAction::Move { delta: Vec2::ZERO });
    let buttons: Vec<_> = f
        .app
        .world_mut()
        .query_filtered::<Entity, With<steppers::Step>>()
        .iter(f.app.world())
        .collect();
    assert_eq!(buttons.len(), 2);
    let right = *buttons
        .iter()
        .max_by(|a, b| {
            f.app
                .world()
                .get::<UiGlobalTransform>(**a)
                .unwrap()
                .translation
                .x
                .total_cmp(
                    &f.app
                        .world()
                        .get::<UiGlobalTransform>(**b)
                        .unwrap()
                        .translation
                        .x,
                )
        })
        .unwrap();
    assert_eq!(
        *f.app.world().get::<Visibility>(right).unwrap(),
        Visibility::Visible
    );
    assert!(f.app.world().get::<AccessibleLabel>(right).is_some());
    let button_point = f
        .app
        .world()
        .get::<UiGlobalTransform>(right)
        .unwrap()
        .translation;
    pointer(
        &mut f,
        button_point,
        PointerAction::Move {
            delta: button_point - p,
        },
    );
    pointer(
        &mut f,
        button_point,
        PointerAction::Press(PointerButton::Primary),
    );
    pointer(
        &mut f,
        button_point,
        PointerAction::Release(PointerButton::Primary),
    );
    assert_eq!(model(&f), 31, "clicking the app stepper commits one step");
    assert_eq!(value(&f.app, f.text), "31");
    let revision = f.app.world().resource::<EditorSession>().ui_revision;
    f.app
        .world_mut()
        .resource_mut::<EditorSettings>()
        .general
        .autosave_interval_seconds = 600;
    settle(&mut f.app);
    let saved = std::fs::read(&f.path).unwrap();
    f.app
        .world_mut()
        .trigger(bevy::ui_widgets::Activate { entity: right });
    settle(&mut f.app);
    assert_eq!(model(&f), 600);
    assert_eq!(
        f.app.world().resource::<EditorSession>().ui_revision,
        revision
    );
    assert_eq!(
        std::fs::read(&f.path).unwrap(),
        saved,
        "at-limit activation does not write settings"
    );
}

#[test]
fn initial_model_is_synchronized_after_deferred_scene_composition_without_writing() {
    let f = fixture_with(73);
    assert_eq!(value(&f.app, f.text), "73");
    assert_eq!(model(&f), 73);
    assert_eq!(
        *f.app.world().get::<NumberInputValue>(f.owner).unwrap(),
        NumberInputValue::I32(73)
    );
    assert!(!f.path.exists());
}

#[test]
fn escape_cancels_a_real_scrub_without_text_focus_or_a_later_release_commit() {
    let mut f = fixture();
    let p = begin_drag(&mut f);
    pointer(
        &mut f,
        p + Vec2::X * 8.0,
        PointerAction::Move {
            delta: Vec2::X * 8.0,
        },
    );
    pointer(
        &mut f,
        p + Vec2::X * 16.0,
        PointerAction::Move {
            delta: Vec2::X * 8.0,
        },
    );
    assert_ne!(value(&f.app, f.text), "30");
    tap(&mut f.app, KeyCode::Escape, Key::Escape, None);
    assert_eq!(model(&f), 30);
    assert_eq!(value(&f.app, f.text), "30");
    pointer(
        &mut f,
        p + Vec2::X * 16.0,
        PointerAction::Release(PointerButton::Primary),
    );
    assert_eq!(model(&f), 30);
    assert!(!f.path.exists());
}
