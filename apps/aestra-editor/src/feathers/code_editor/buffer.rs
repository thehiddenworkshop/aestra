//! Production code buffer, selection and bounded undo/redo state.
use bevy::prelude::{Component, Entity, Resource};

/// The upper bound on retained undo history, so a long editing session cannot grow it without
/// limit.
const MAX_UNDO: usize = 256;

/// The kind of the last text edit, used to coalesce a run of same-kind edits into one undo step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EditKind {
    /// A single typed character (coalesces with adjacent typing).
    Type,
    /// A single-character deletion (coalesces with adjacent deletions).
    Erase,
    /// Any other edit (newline, tab, paste, cut, deleting a selection): never coalesces.
    Other,
}

/// A captured editor state for undo/redo.
#[derive(Debug, Clone)]
pub(super) struct EditSnapshot {
    text: String,
    cursor: usize,
    anchor: usize,
}

/// The editable state of a code editor: its text, caret (character index), and selection anchor.
/// A selection spans the range between `anchor` and `cursor`; they are equal when nothing is
/// selected.
#[derive(Component, Debug, Clone)]
pub(crate) struct CodeEditor {
    pub(crate) text: String,
    pub(crate) cursor: usize,
    pub(crate) anchor: usize,
    pub(super) undo: Vec<EditSnapshot>,
    pub(super) redo: Vec<EditSnapshot>,
    pub(super) last_edit: Option<EditKind>,
}

impl CodeEditor {
    /// Build real undo state for cross-module history ownership regressions.
    #[cfg(test)]
    pub(crate) fn insert_for_test(&mut self, text: &str) {
        self.record(EditKind::Other);
        self.insert(text);
    }
    pub(crate) fn new(text: String) -> Self {
        let end = text.chars().count();
        Self {
            text,
            cursor: end,
            anchor: end,
            undo: Vec::new(),
            redo: Vec::new(),
            last_edit: None,
        }
    }

    /// Restores a caret/selection captured earlier, clamped to the current text length.
    pub(crate) fn with_selection(mut self, cursor: usize, anchor: usize) -> Self {
        let end = self.text.chars().count();
        self.cursor = cursor.min(end);
        self.anchor = anchor.min(end);
        self
    }

    pub(super) fn selection(&self) -> Option<(usize, usize)> {
        (self.cursor != self.anchor)
            .then(|| (self.cursor.min(self.anchor), self.cursor.max(self.anchor)))
    }

    /// The currently selected text, if any.
    pub(super) fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection()?;
        Some(self.text.chars().skip(start).take(end - start).collect())
    }

    pub(super) fn set_cursor(&mut self, cursor: usize, extend: bool) {
        self.cursor = cursor;
        if !extend {
            self.anchor = cursor;
        }
    }

    /// Removes the selected range, collapsing the caret to its start. Returns whether it changed.
    pub(super) fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection() else {
            return false;
        };
        let mut chars: Vec<char> = self.text.chars().collect();
        chars.drain(start..end);
        self.text = chars.into_iter().collect();
        self.cursor = start;
        self.anchor = start;
        true
    }

    pub(super) fn insert(&mut self, insert: &str) {
        self.delete_selection();
        let mut chars: Vec<char> = self.text.chars().collect();
        for (offset, character) in insert.chars().enumerate() {
            chars.insert(self.cursor + offset, character);
        }
        self.cursor += insert.chars().count();
        self.text = chars.into_iter().collect();
        self.anchor = self.cursor;
    }

    pub(super) fn snapshot(&self) -> EditSnapshot {
        EditSnapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
        }
    }

    /// Records the pre-edit state onto the undo stack (called before mutating). Consecutive edits
    /// of the same coalescing kind fold into the one step so a run of typing undoes together.
    pub(super) fn record(&mut self, kind: EditKind) {
        let coalesce = kind != EditKind::Other && self.last_edit == Some(kind);
        if !coalesce {
            self.undo.push(self.snapshot());
            if self.undo.len() > MAX_UNDO {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.last_edit = Some(kind);
    }

    /// Places the caret (collapsing any selection) at a character index, clamped to the text length.
    pub(crate) fn set_caret(&mut self, index: usize) {
        let clamped = index.min(self.text.chars().count());
        self.cursor = clamped;
        self.anchor = clamped;
    }

    /// The zero-based line a character index falls on, for scrolling it into view.
    pub(crate) fn line_of(&self, index: usize) -> usize {
        line_col(&self.text, index.min(self.text.chars().count())).0
    }

    /// Whether there is anything to undo or redo, so a host (e.g. the Edit menu) can reflect it.
    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Reverts to the previous undo snapshot, pushing the current state onto the redo stack.
    pub(crate) fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else {
            return false;
        };
        self.redo.push(self.snapshot());
        self.apply(previous);
        true
    }

    /// Re-applies the most recently undone state, pushing the current state onto the undo stack.
    pub(crate) fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo.push(self.snapshot());
        self.apply(next);
        true
    }

    pub(super) fn apply(&mut self, snapshot: EditSnapshot) {
        self.text = snapshot.text;
        self.cursor = snapshot.cursor;
        self.anchor = snapshot.anchor;
        self.last_edit = None; // an undo/redo breaks any coalescing run
    }

    /// Selects the word (alphanumeric or `_` run) under `index`; if not on a word, collapses there.
    pub(super) fn select_word_at(&mut self, index: usize) {
        let chars: Vec<char> = self.text.chars().collect();
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let index = index.min(chars.len());
        let mut start = index;
        let mut end = index;
        while start > 0 && is_word(chars[start - 1]) {
            start -= 1;
        }
        while end < chars.len() && is_word(chars[end]) {
            end += 1;
        }
        self.anchor = start;
        self.cursor = end;
    }
}

/// The code editor the app's Edit-menu undo/redo should target: the last-focused editor, kept
/// through menu interaction and cleared when another editing context takes focus.
#[derive(Resource, Default)]
pub(crate) struct ActiveCodeEditor(pub(crate) Option<Entity>);

pub(super) fn line_col(source: &str, caret: usize) -> (usize, usize) {
    let mut line = 0;
    let mut column = 0;
    for character in source.chars().take(caret) {
        if character == '\n' {
            line += 1;
            column = 0;
        } else {
            column += 1;
        }
    }
    (line, column)
}

#[cfg(test)]
#[path = "tests/buffer.rs"]
mod tests;
