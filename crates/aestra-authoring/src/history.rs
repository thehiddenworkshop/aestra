use crate::{
    CommandError, CommandExecutor, EffectDiff, EffectTransaction, LockState, TransactionPreview,
};
use aestra_core::EffectAsset;
use std::collections::VecDeque;

const DEFAULT_HISTORY_LIMIT: usize = 256;

#[derive(Debug, Clone)]
struct HistoryEntry {
    label: String,
    forward: EffectTransaction,
    inverse: EffectTransaction,
}

#[derive(Debug, Clone)]
pub struct HistoryResult {
    pub label: String,
    pub diff: EffectDiff,
}

#[derive(Debug, Clone)]
pub struct CommandHistory {
    undo: VecDeque<HistoryEntry>,
    redo: Vec<HistoryEntry>,
    limit: usize,
}

impl Default for CommandHistory {
    fn default() -> Self {
        Self::with_limit(DEFAULT_HISTORY_LIMIT)
    }
}

impl CommandHistory {
    pub fn with_limit(limit: usize) -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            limit: limit.max(1),
        }
    }

    pub fn execute(
        &mut self,
        effect: &mut EffectAsset,
        locks: &LockState,
        transaction: EffectTransaction,
    ) -> Result<EffectDiff, CommandError> {
        let outcome = CommandExecutor::execute(effect, locks, &transaction)?;
        if outcome.diff.is_empty() {
            return Ok(outcome.diff);
        }
        let label = transaction.label.clone();
        self.undo.push_back(HistoryEntry {
            label,
            forward: transaction,
            inverse: outcome.inverse,
        });
        while self.undo.len() > self.limit {
            self.undo.pop_front();
        }
        self.redo.clear();
        Ok(outcome.diff)
    }

    pub fn commit_preview(
        &mut self,
        effect: &mut EffectAsset,
        locks: &LockState,
        preview: TransactionPreview,
    ) -> Result<EffectDiff, CommandError> {
        if !preview.source_matches(effect) {
            return Err(CommandError::StalePreview);
        }
        self.execute(effect, locks, preview.into_transaction())
    }

    pub fn undo(
        &mut self,
        effect: &mut EffectAsset,
    ) -> Result<Option<HistoryResult>, CommandError> {
        let Some(entry) = self.undo.pop_back() else {
            return Ok(None);
        };
        match CommandExecutor::execute(effect, &LockState::default(), &entry.inverse) {
            Ok(outcome) => {
                let result = HistoryResult {
                    label: entry.label.clone(),
                    diff: outcome.diff,
                };
                self.redo.push(entry);
                Ok(Some(result))
            }
            Err(error) => {
                self.undo.push_back(entry);
                Err(error)
            }
        }
    }

    pub fn redo(
        &mut self,
        effect: &mut EffectAsset,
    ) -> Result<Option<HistoryResult>, CommandError> {
        let Some(entry) = self.redo.pop() else {
            return Ok(None);
        };
        match CommandExecutor::execute(effect, &LockState::default(), &entry.forward) {
            Ok(outcome) => {
                let result = HistoryResult {
                    label: entry.label.clone(),
                    diff: outcome.diff,
                };
                self.undo.push_back(entry);
                Ok(Some(result))
            }
            Err(error) => {
                self.redo.push(entry);
                Err(error)
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    pub fn clear_redo(&mut self) {
        self.redo.clear();
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}
