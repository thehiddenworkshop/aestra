//! Production property-curve Insert/Delete routing and text-edit focus guard.
use super::property_editing::{ComplexKeyEdit, add_complex_key_at_pointer, edit_complex_key};
use super::state::CurvesState;
use crate::{EditorModuleRegistry, EditorSession};
use bevy::{input_focus::InputFocus, prelude::*, text::EditableText, ui::RelativeCursorPosition};

#[derive(Component)]
pub(super) struct CurveGraph;

pub(super) fn curves_keyboard_input(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    graphs: Query<&RelativeCursorPosition, With<CurveGraph>>,
    focus: Option<Res<InputFocus>>,
    editable_text: Query<(), With<EditableText>>,
    mut session: ResMut<EditorSession>,
    registry: Res<EditorModuleRegistry>,
    mut state: ResMut<CurvesState>,
) {
    let Some(keys) = keys else {
        return;
    };
    let editing_text = focus
        .as_ref()
        .and_then(|focus| focus.get())
        .is_some_and(|entity| editable_text.contains(entity));
    if editing_text {
        return;
    }
    let cursor = graphs
        .iter()
        .find_map(|cursor| cursor.normalized.filter(|_| cursor.cursor_over()));
    let Some(cursor) = cursor else {
        return;
    };
    if keys.just_pressed(KeyCode::Insert) {
        add_complex_key_at_pointer(&mut session, &registry.0, &mut state, cursor);
    } else if keys.just_pressed(KeyCode::Delete) {
        edit_complex_key(
            &mut session,
            &registry.0,
            &mut state,
            ComplexKeyEdit::Delete,
        );
    }
}
