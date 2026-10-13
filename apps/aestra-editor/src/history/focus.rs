//! Production pointer/keyboard focus ownership for document history.
use super::focus_pointer::PointerPress;
use crate::session::EditorSession;
use bevy::prelude::*;

/// Neutral panels (menus, Assets, diagnostics) preserve the last editing context.
#[derive(Component, Clone, Copy)]
pub(crate) enum HistoryScope {
    Effect,
    Material,
    Neutral,
}

pub(super) fn capture_history_focus(
    event: On<PointerPress>,
    scopes: Query<&HistoryScope>,
    parents: Query<&ChildOf>,
    mut session: ResMut<EditorSession>,
) {
    if event.entity != event.original_event_target() {
        return;
    }
    apply_history_focus(event.entity, &scopes, &parents, &mut session);
}

pub(super) fn capture_history_keyboard_focus(
    focus: Option<Res<bevy::input_focus::InputFocus>>,
    scopes: Query<&HistoryScope>,
    parents: Query<&ChildOf>,
    mut session: ResMut<EditorSession>,
) {
    if let Some(focus) = focus
        && focus.is_changed()
        && let Some(entity) = focus.get()
    {
        apply_history_focus(entity, &scopes, &parents, &mut session);
    }
}

fn apply_history_focus(
    target: Entity,
    scopes: &Query<&HistoryScope>,
    parents: &Query<&ChildOf>,
    session: &mut EditorSession,
) {
    let mut entity = Some(target);
    while let Some(current) = entity {
        if let Ok(scope) = scopes.get(current) {
            let active = match scope {
                HistoryScope::Effect => false,
                HistoryScope::Material => {
                    session.standalone_material().is_some()
                        || session.standalone_function().is_some()
                }
                HistoryScope::Neutral => return,
            };
            if session.material_history_active != active {
                session.material_history_active = active;
            }
            return;
        }
        entity = parents.get(current).ok().map(ChildOf::parent);
    }
}

#[cfg(test)]
#[path = "focus/tests.rs"]
mod tests;
