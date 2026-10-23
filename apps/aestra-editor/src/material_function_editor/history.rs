//! Root/function-scoped production history; catalog publication stays in the host adapter.
use aestra_core::{MaterialFunctionId, material::MaterialFunction};
use bevy::prelude::Resource;
use std::{collections::BTreeMap, path::PathBuf};

pub(super) type Key = (PathBuf, MaterialFunctionId);
#[derive(Default)]
struct History {
    undo: Vec<(MaterialFunction, MaterialFunction)>,
    redo: Vec<(MaterialFunction, MaterialFunction)>,
}
#[derive(Resource, Default)]
pub(crate) struct FunctionEditor {
    histories: BTreeMap<Key, History>,
    pub(super) reports: BTreeMap<Key, String>,
}

impl FunctionEditor {
    pub(crate) fn clear_function_redo(&mut self, root: &std::path::Path, id: MaterialFunctionId) {
        if let Some(history) = self.histories.get_mut(&(root.to_owned(), id)) {
            history.redo.clear();
        }
    }
    pub(crate) fn clear_redo(&mut self) {
        for history in self.histories.values_mut() {
            history.redo.clear();
        }
    }
    pub(crate) fn clear_function(&mut self, root: &std::path::Path, id: MaterialFunctionId) {
        let key = (root.to_owned(), id);
        self.histories.remove(&key);
        self.reports.remove(&key);
    }
    pub(crate) fn available_for_function(
        &self,
        root: &std::path::Path,
        id: MaterialFunctionId,
        undo: bool,
    ) -> bool {
        self.histories
            .get(&(root.to_owned(), id))
            .is_some_and(|history| {
                if undo {
                    !history.undo.is_empty()
                } else {
                    !history.redo.is_empty()
                }
            })
    }

    /// Publish first; failed edits only update their diagnostic, never either stack.
    pub(super) fn edit_applied(
        &mut self,
        key: Key,
        before: MaterialFunction,
        after: MaterialFunction,
        apply: impl FnOnce(&MaterialFunction, &MaterialFunction) -> Result<String, String>,
    ) -> Result<Option<String>, String> {
        if before == after {
            return Ok(None);
        }
        if before.custom_wesl.is_some() {
            return Err("Custom WESL signatures remain read-only".into());
        }
        match apply(&before, &after) {
            Ok(report) => {
                self.reports.insert(key.clone(), report.clone());
                let history = self.histories.entry(key).or_default();
                history.undo.push((before, after));
                if history.undo.len() > 256 {
                    history.undo.remove(0);
                }
                history.redo.clear();
                Ok(Some(report))
            }
            Err(error) => {
                self.reports.insert(key, error.clone());
                Err(error)
            }
        }
    }

    /// Keep both stacks and the last report untouched until publication succeeds.
    pub(super) fn step_applied(
        &mut self,
        key: &Key,
        undo: bool,
        apply: impl FnOnce(&MaterialFunction, &MaterialFunction) -> Result<String, String>,
    ) -> Result<(), String> {
        let entry = self
            .histories
            .get(key)
            .and_then(|history| {
                if undo {
                    history.undo.last()
                } else {
                    history.redo.last()
                }
            })
            .cloned()
            .ok_or("No function history in this direction")?;
        let (before, after) = &entry;
        let report = if undo {
            apply(after, before)
        } else {
            apply(before, after)
        }?;
        self.reports.insert(key.clone(), report);
        let history = self.histories.get_mut(key).unwrap();
        if undo {
            history.undo.pop();
            history.redo.push(entry);
        } else {
            history.redo.pop();
            history.undo.push(entry);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests/history.rs"]
mod tests;
