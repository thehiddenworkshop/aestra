//! The early ownership gate shared by action execution and menu availability.
use super::HistoryAction;
use crate::{
    feathers::code_editor::{ActiveCodeEditor, CodeEditor},
    persistence::DocumentProtectionState,
    project_content::io::{self, ProjectIoTasks},
};
use bevy::{ecs::system::SystemParam, prelude::*};

#[derive(SystemParam)]
pub(super) struct Gate<'w> {
    tasks: Option<Res<'w, ProjectIoTasks>>,
    protection: Option<Res<'w, DocumentProtectionState>>,
}

impl Gate<'_> {
    pub(super) fn blocked(&self) -> bool {
        !io::idle(self.tasks.as_ref().map(Res::clone))
            || self
                .protection
                .as_deref()
                .is_some_and(|state| state.is_open())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Route {
    Blocked,
    Code(&'static str),
    Document,
}

#[derive(SystemParam)]
pub(crate) struct Ownership<'w, 's> {
    gate: Gate<'w>,
    active: Option<Res<'w, ActiveCodeEditor>>,
    editors: Query<'w, 's, &'static mut CodeEditor>,
}

impl Ownership<'_, '_> {
    pub(super) fn route(&mut self, action: HistoryAction) -> Route {
        if self.gate.blocked() {
            return Route::Blocked;
        }
        // Keep an active code editor authoritative even if its own stack is empty.
        // Deleted/stale entities cannot capture the document's history indefinitely.
        let Some(entity) = self.active.as_deref().and_then(|active| active.0) else {
            return Route::Document;
        };
        let Ok(mut editor) = self.editors.get_mut(entity) else {
            return Route::Document;
        };
        let undo = action == HistoryAction::Undo;
        let changed = if undo { editor.undo() } else { editor.redo() };
        Route::Code(match (undo, changed) {
            (true, true) => "Undid edit",
            (false, true) => "Redid edit",
            (true, false) => "Nothing to undo",
            (false, false) => "Nothing to redo",
        })
    }
}

#[cfg(test)]
#[path = "tests/ownership.rs"]
mod tests;
