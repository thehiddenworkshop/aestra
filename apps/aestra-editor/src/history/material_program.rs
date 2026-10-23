//! Production material history storage and failure-safe stack transitions.
//! Catalog publication/rollback and session ordering remain in the host adapter.
use aestra_core::{
    MaterialProgramId,
    material::{MaterialFunction, MaterialProgram},
};
use bevy::prelude::Resource;
use std::{
    collections::{BTreeMap, VecDeque},
    path::{Path, PathBuf},
};

pub(super) const MATERIAL_HISTORY_LIMIT: usize = 256;

#[derive(Debug, Clone)]
pub(super) struct MaterialProgramHistoryEntry {
    pub(super) label: String,
    pub(super) before: MaterialProgram,
    pub(super) after: MaterialProgram,
    pub(super) created_function: Option<MaterialFunction>,
}

#[derive(Resource, Debug, Default)]
pub(crate) struct MaterialProgramEditHistory {
    pub(super) undo: VecDeque<MaterialProgramHistoryEntry>,
    pub(super) redo: Vec<MaterialProgramHistoryEntry>,
    pub(super) standalone: BTreeMap<(PathBuf, MaterialProgramId), MaterialProgramEditHistory>,
}

impl MaterialProgramEditHistory {
    pub(crate) fn clear_program(&mut self, root: &Path, id: MaterialProgramId) {
        self.standalone.remove(&(root.to_owned(), id));
    }

    pub(super) fn available_for_program(
        &self,
        root: &Path,
        id: MaterialProgramId,
        undo: bool,
    ) -> bool {
        self.standalone
            .get(&(root.to_owned(), id))
            .is_some_and(|history| {
                if undo {
                    !history.undo.is_empty()
                } else {
                    !history.redo.is_empty()
                }
            })
    }

    /// Called only after the host successfully publishes the edit (including extraction assets).
    pub(super) fn record_applied(&mut self, entry: MaterialProgramHistoryEntry) {
        self.undo.push_back(entry);
        while self.undo.len() > MATERIAL_HISTORY_LIMIT {
            self.undo.pop_front();
        }
        self.redo.clear();
    }

    /// Failed host publication leaves the entry on its original stack for a safe retry.
    pub(super) fn step_applied(
        &mut self,
        undo: bool,
        apply: impl FnOnce(&MaterialProgramHistoryEntry) -> Result<(), String>,
    ) -> Result<Option<String>, String> {
        let entry = if undo {
            self.undo.pop_back()
        } else {
            self.redo.pop()
        };
        let Some(entry) = entry else {
            return Ok(None);
        };
        if let Err(error) = apply(&entry) {
            if undo {
                self.undo.push_back(entry);
            } else {
                self.redo.push(entry);
            }
            return Err(error);
        }
        let label = entry.label.clone();
        if undo {
            self.redo.push(entry);
        } else {
            self.undo.push_back(entry);
        }
        Ok(Some(label))
    }

    pub(super) fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.standalone.clear();
    }

    pub(super) fn clear_redo(&mut self) {
        self.redo.clear();
    }
}

#[cfg(test)]
#[path = "tests/material_program.rs"]
mod tests;
