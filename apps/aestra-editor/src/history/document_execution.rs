//! Actual effect/material/function undo-redo domain execution.
use crate::history::{
    HistoryAction, MaterialProgramEditHistory, material_program::MATERIAL_HISTORY_LIMIT,
};
use crate::{ProjectEffectCatalog, session::EditorSession};
use aestra_core::EffectId;
use bevy::prelude::Resource;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HistoryDomain {
    Effect,
    MaterialProgram,
}

#[derive(Resource, Debug, Default)]
pub(crate) struct EditorHistoryLedger {
    pub(super) undo: Vec<HistoryDomain>,
    pub(super) redo: Vec<HistoryDomain>,
    pub(super) effect: Option<EffectId>,
    pub(super) observed_generation: u64,
    pub(super) observed_edit_serial: u64,
    pub(super) observed_effect_undo: usize,
    pub(super) observed_effect_redo: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EffectHistoryChange {
    None,
    Edited,
    Reset,
}

impl EditorHistoryLedger {
    pub(super) fn capture_effect_changes(
        &mut self,
        session: &EditorSession,
    ) -> EffectHistoryChange {
        let effect_changed = self
            .effect
            .is_some_and(|effect| effect != session.effect.id);
        let generation_changed =
            self.effect.is_some() && self.observed_generation != session.history_generation();
        let history_cleared =
            session.effect_undo_len() < self.observed_effect_undo && session.effect_redo_len() == 0;
        if effect_changed || generation_changed || history_cleared {
            self.undo.clear();
            self.undo.extend(std::iter::repeat_n(
                HistoryDomain::Effect,
                session.effect_undo_len(),
            ));
            self.redo.clear();
            self.effect = Some(session.effect.id);
            self.observed_generation = session.history_generation();
            self.observed_edit_serial = session.effect_edit_serial();
            self.observed_effect_undo = session.effect_undo_len();
            self.observed_effect_redo = session.effect_redo_len();
            return EffectHistoryChange::Reset;
        }
        self.effect.get_or_insert(session.effect.id);
        self.observed_generation = session.history_generation();
        if session.effect_edit_serial() > self.observed_edit_serial {
            self.undo.extend(std::iter::repeat_n(
                HistoryDomain::Effect,
                ((session.effect_edit_serial() - self.observed_edit_serial) as usize)
                    .min(session.effect_undo_len()),
            ));
            self.trim_domain(HistoryDomain::Effect, session.effect_undo_len());
            self.observed_edit_serial = session.effect_edit_serial();
            self.redo.clear();
            self.observed_effect_undo = session.effect_undo_len();
            self.observed_effect_redo = session.effect_redo_len();
            return EffectHistoryChange::Edited;
        }
        self.observed_effect_undo = session.effect_undo_len();
        self.observed_effect_redo = session.effect_redo_len();
        EffectHistoryChange::None
    }

    pub(crate) fn record_material_edit(&mut self, session: &mut EditorSession) {
        if session.standalone_material().is_some() && session.material_history_active {
            return;
        }
        self.capture_effect_changes(session);
        session.clear_effect_redo();
        self.observe_effect_history(session);
        self.undo.push(HistoryDomain::MaterialProgram);
        self.trim_domain(HistoryDomain::MaterialProgram, MATERIAL_HISTORY_LIMIT);
        self.redo.clear();
    }

    pub(super) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    fn trim_domain(&mut self, domain: HistoryDomain, retained: usize) {
        let mut excess = self
            .undo
            .iter()
            .filter(|entry| **entry == domain)
            .count()
            .saturating_sub(retained);
        self.undo.retain(|entry| {
            if *entry == domain && excess > 0 {
                excess -= 1;
                false
            } else {
                true
            }
        });
    }

    pub(super) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub(super) fn observe_effect_history(&mut self, session: &EditorSession) {
        self.observed_edit_serial = session.effect_edit_serial();
        self.observed_effect_undo = session.effect_undo_len();
        self.observed_effect_redo = session.effect_redo_len();
    }
}

pub(super) fn execute_document_history(
    action: HistoryAction,
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    material_history: &mut MaterialProgramEditHistory,
    ledger: &mut EditorHistoryLedger,
    functions: &mut crate::material_function_editor::FunctionEditor,
) -> bool {
    if session.standalone_function().is_some() && session.material_history_active {
        let result = functions.step(session, catalog, action == HistoryAction::Undo);
        let succeeded = result.is_ok();
        session.status = match result {
            Ok(()) => "Function history applied".into(),
            Err(error) => error,
        };
        session.ui_revision += 1;
        return succeeded;
    }
    match ledger.capture_effect_changes(session) {
        EffectHistoryChange::Reset => material_history.clear(),
        EffectHistoryChange::Edited => material_history.clear_redo(),
        EffectHistoryChange::None => {}
    }
    if session.standalone_material().is_some() && session.material_history_active {
        let history = material_history.for_target_mut(session);
        let result = match action {
            HistoryAction::Undo => history.undo(session, catalog),
            HistoryAction::Redo => history.redo(session, catalog),
        };
        let succeeded = matches!(&result, Ok(Some(_)));
        session.status = match result {
            Ok(Some(label)) => format!(
                "{} {label}",
                if action == HistoryAction::Undo {
                    "Undid"
                } else {
                    "Redid"
                }
            ),
            Ok(None) => "No material history in this direction".into(),
            Err(error) => format!("Material history failed: {error}"),
        };
        session.ui_revision += 1;
        return succeeded;
    }
    let domain = match action {
        HistoryAction::Undo => ledger.undo.pop(),
        HistoryAction::Redo => ledger.redo.pop(),
    };
    let Some(domain) = domain else {
        session.status = match action {
            HistoryAction::Undo => "Nothing to undo".into(),
            HistoryAction::Redo => "Nothing to redo".into(),
        };
        return false;
    };

    let result = match (action, domain) {
        (HistoryAction::Undo, HistoryDomain::Effect) => {
            let before = session.effect_undo_len();
            session.undo();
            (session.effect_undo_len() < before).then(|| {
                ledger.redo.push(domain);
            })
        }
        (HistoryAction::Redo, HistoryDomain::Effect) => {
            let before = session.effect_redo_len();
            session.redo();
            (session.effect_redo_len() < before).then(|| {
                ledger.undo.push(domain);
            })
        }
        (HistoryAction::Undo, HistoryDomain::MaterialProgram) => {
            match material_history.undo(session, catalog) {
                Ok(Some(label)) => {
                    session.status = format!("Undid {label}");
                    session.ui_revision += 1;
                    ledger.redo.push(domain);
                    Some(())
                }
                Ok(None) => None,
                Err(error) => {
                    session.status = format!("Material undo failed: {error}");
                    None
                }
            }
        }
        (HistoryAction::Redo, HistoryDomain::MaterialProgram) => {
            match material_history.redo(session, catalog) {
                Ok(Some(label)) => {
                    session.status = format!("Redid {label}");
                    session.ui_revision += 1;
                    ledger.undo.push(domain);
                    Some(())
                }
                Ok(None) => None,
                Err(error) => {
                    session.status = format!("Material redo failed: {error}");
                    None
                }
            }
        }
    };
    if result.is_none() {
        match action {
            HistoryAction::Undo => ledger.undo.push(domain),
            HistoryAction::Redo => ledger.redo.push(domain),
        }
    }
    ledger.observe_effect_history(session);
    result.is_some()
}

#[cfg(test)]
#[path = "document_execution/tests.rs"]
mod tests;
