//! Actual editor scene wrappers and single-line text bridge on the candidate graph.
#[path = "../../../apps/aestra-editor/src/theme/feathers.rs"]
pub mod boundary;
#[path = "../../../apps/aestra-editor/src/theme/palette.rs"]
pub mod palette;
mod theme {
    pub(crate) use super::boundary::feathers_theme;
}
#[path = "../../../apps/aestra-editor/src/feathers/scenes/common.rs"]
mod scenes;
#[path = "../../../apps/aestra-editor/src/feathers/text_input.rs"]
mod text_input;

use bevy::{
    input::keyboard::{Key, KeyboardInput},
    input_focus::{FocusedInput, InputFocus},
    prelude::*,
    ui_widgets::TextInput,
};
use text_input::tests::{Changes, app, field, focus, tap, value};

#[derive(Resource, Default)]
struct Escapes(Vec<(Entity, Option<Entity>)>);

#[test]
fn escape_blurs_once_and_still_reaches_the_parent_dialog() {
    let mut app = app();
    app.init_resource::<Escapes>();
    let (container, input) = field(&mut app, "Spark");
    app.world_mut().entity_mut(container).observe(
        |event: On<FocusedInput<KeyboardInput>>,
         focus: Res<InputFocus>,
         mut escapes: ResMut<Escapes>| {
            if event.input.key_code == KeyCode::Escape && event.input.state.is_pressed() {
                escapes.0.push((event.original_event_target(), focus.get()));
            }
        },
    );
    focus(&mut app, input);
    tap(&mut app, KeyCode::Escape, Key::Escape, None);
    assert_eq!(app.world().resource::<InputFocus>().get(), None);
    assert_eq!(app.world().resource::<Escapes>().0, [(input, None)]);
    assert_eq!(
        app.world().resource::<Changes>().0,
        [(container, "Spark".into(), true)]
    );
    assert_eq!(value(&app, input), "Spark");
}

#[test]
fn feathers_scene_installs_the_new_keyboard_behavior_marker() {
    let mut app = app();
    let (container, input) = field(&mut app, "");
    assert!(app.world().get::<TextInput>(input).is_some());
    assert!(app.world().get::<TextInput>(container).is_none());
    focus(&mut app, input);
    app.world_mut().entity_mut(input).remove::<TextInput>();
    tap(
        &mut app,
        KeyCode::KeyA,
        Key::Character("a".into()),
        Some("a"),
    );
    assert_eq!(value(&app, input), "");
    assert!(app.world().resource::<Changes>().0.is_empty());
}
