//! Real plugin/keyboard tests shared by the shipping and candidate graphs.
use super::*;
use crate::feathers::text_input::tests::{app_with, focus, tap};

#[derive(Resource, Default)]
struct Edits(Vec<Entity>);

fn app() -> App {
    app_with(|app| {
        app.init_resource::<Clipboard>()
            .init_resource::<Edits>()
            .add_plugins(CodeEditorPlugin)
            .add_observer(|change: On<CodeEditorChanged>, mut edits: ResMut<Edits>| {
                edits.0.push(change.0);
            });
    })
}

fn editor(app: &mut App, text: &str) -> Entity {
    let mut entity = Entity::PLACEHOLDER;
    app.world_mut()
        .commands()
        .spawn(Node::default())
        .with_children(|parent| {
            entity = spawn_code_editor(
                parent,
                CodeEditor::new(text.into()),
                CodeEditorHighlighter(Arc::new(|text| vec![(text.into(), Color::WHITE)])),
                CodeEditorMarkers::default(),
                (),
            );
            spawn_code_gutter(parent, entity);
        });
    app.world_mut().flush();
    app.update();
    focus(app, entity);
    entity
}

#[test]
fn real_keyboard_edits_unicode_and_multiline_without_cross_editor_changes() {
    let mut app = app();
    let other = editor(&mut app, "untouched");
    let input = editor(&mut app, "");
    tap(
        &mut app,
        KeyCode::KeyA,
        Key::Character("é火🙂".into()),
        Some("é火🙂"),
    );
    tap(&mut app, KeyCode::Enter, Key::Enter, None);
    tap(&mut app, KeyCode::Tab, Key::Tab, None);
    tap(&mut app, KeyCode::Backspace, Key::Backspace, None);
    assert_eq!(
        app.world().get::<CodeEditor>(input).unwrap().text,
        "é火🙂\n   "
    );
    assert_eq!(app.world().get::<CodeEditor>(input).unwrap().cursor, 7);
    assert_eq!(
        app.world().get::<CodeEditor>(other).unwrap().text,
        "untouched"
    );
    assert_eq!(app.world().resource::<Edits>().0, vec![input; 4]);
    assert!(!app.world().get::<Children>(input).unwrap().is_empty());
}

fn chord(app: &mut App, modifier: KeyCode, logical_modifier: Key, code: KeyCode, logical: Key) {
    let window = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(world)
            .unwrap()
    };
    // All four messages arrive between rendered frames. The dispatcher's
    // event-time modifier snapshot must reach this custom consumer too.
    for (key_code, logical_key, state) in [
        (modifier, logical_modifier.clone(), ButtonState::Pressed),
        (code, logical.clone(), ButtonState::Pressed),
        (code, logical, ButtonState::Released),
        (modifier, logical_modifier, ButtonState::Released),
    ] {
        app.world_mut().write_message(KeyboardInput {
            key_code,
            logical_key,
            state,
            text: None,
            repeat: false,
            window,
        });
    }
    for _ in 0..3 {
        app.update();
    }
}

#[test]
fn batched_layout_aware_undo_redo_and_shift_selection_preserve_text() {
    let mut app = app();
    let input = editor(&mut app, "é火🙂");
    tap(
        &mut app,
        KeyCode::KeyA,
        Key::Character("!".into()),
        Some("!"),
    );
    chord(
        &mut app,
        KeyCode::ControlLeft,
        Key::Control,
        KeyCode::KeyW,
        Key::Character("z".into()),
    );
    assert_eq!(app.world().get::<CodeEditor>(input).unwrap().text, "é火🙂");
    chord(
        &mut app,
        KeyCode::ControlLeft,
        Key::Control,
        KeyCode::KeyY,
        Key::Character("y".into()),
    );
    assert_eq!(app.world().get::<CodeEditor>(input).unwrap().text, "é火🙂!");
    chord(
        &mut app,
        KeyCode::ShiftLeft,
        Key::Shift,
        KeyCode::ArrowLeft,
        Key::ArrowLeft,
    );
    let editor = app.world().get::<CodeEditor>(input).unwrap();
    assert_eq!(editor.selected_text().as_deref(), Some("!"));
    assert_eq!(app.world().resource::<Edits>().0, vec![input; 3]);
}

#[test]
fn active_editor_survives_menu_focus_but_not_a_different_history_scope() {
    let mut app = app();
    let input = editor(&mut app, "code");
    let menu = app
        .world_mut()
        .spawn(crate::feathers::context_menu::PointerContextMenuItem)
        .id();
    focus(&mut app, menu);
    assert_eq!(app.world().resource::<ActiveCodeEditor>().0, Some(input));
    let scope = app
        .world_mut()
        .spawn(crate::history::HistoryScope::Effect)
        .id();
    focus(&mut app, scope);
    assert_eq!(app.world().resource::<ActiveCodeEditor>().0, None);
}

#[test]
fn pointer_focus_preserves_pending_focus_loss_submission_of_the_previous_text_field() {
    use crate::feathers::text_input::tests::{Changes, field};
    use bevy::ecs::system::RunSystemOnce;
    let mut app = app();
    let code = editor(&mut app, "code");
    let (container, text) = field(&mut app, "Effect name");
    focus(&mut app, text);
    app.world_mut()
        .get_mut::<RelativeCursorPosition>(code)
        .unwrap()
        .cursor_over = true;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut()
        .run_system_once(code_editor_pointer_input)
        .unwrap();
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(code));
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        app.world().resource::<Changes>().0,
        [(container, "Effect name".into(), true)]
    );
}

#[test]
fn context_actions_close_the_menu_and_emit_only_actual_text_edits() {
    let mut app = app();
    let input = editor(&mut app, "abcd");
    app.world_mut()
        .get_mut::<CodeEditor>(input)
        .unwrap()
        .set_caret(2);
    let anchor = app.world_mut().spawn(CodeEditorContextAnchor).id();
    let action = app
        .world_mut()
        .spawn(CodeEditorMenuAction {
            editor: input,
            command: CodeEditorCommand::SelectAll,
        })
        .id();
    app.world_mut().trigger(Activate { entity: action });
    app.world_mut().flush();
    assert!(app.world().get_entity(anchor).is_err());
    assert_eq!(
        app.world()
            .get::<CodeEditor>(input)
            .unwrap()
            .selected_text()
            .as_deref(),
        Some("abcd")
    );
    assert!(app.world().resource::<Edits>().0.is_empty());
    // Undo with no edits must not dirty the document either.
    app.world_mut()
        .get_mut::<CodeEditorMenuAction>(action)
        .unwrap()
        .command = CodeEditorCommand::Undo;
    app.world_mut().trigger(Activate { entity: action });
    assert!(app.world().resource::<Edits>().0.is_empty());
    tap(
        &mut app,
        KeyCode::KeyA,
        Key::Character("x".into()),
        Some("x"),
    );
    app.world_mut().trigger(Activate { entity: action });
    app.world_mut().flush();
    assert_eq!(app.world().get::<CodeEditor>(input).unwrap().text, "abcd");
    assert_eq!(app.world().resource::<Edits>().0, vec![input; 2]);
}
