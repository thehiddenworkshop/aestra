//! A generic, reusable multi-line code editor widget for the editor's feathers layer.
//!
//! [`CodeEditor`] owns an editable text buffer with a caret and selection, rendered as monospace
//! coloured runs (via a pluggable [`CodeEditorHighlighter`]) with overlay caret, selection
//! highlights, and optional line markers. It handles click/drag selection, keyboard navigation and
//! editing (with shift-selection and Ctrl+A), and emits [`CodeEditorChanged`] when the text changes
//! so a host can persist it. Clipboard (cut/copy/paste) is layered on in a later stage.
#![allow(dead_code)] // Reusable widget API; not every hook is used by the current single consumer.

use crate::theme;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::{FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::text::{FontSize, LineHeight, TextSpan};
use bevy::ui::{ComputedNode, RelativeCursorPosition};
use std::sync::Arc;

pub(crate) const CODE_FONT_SIZE: f32 = 13.0;
// Fira Mono advance is ~0.6em; the host sets the monospace font, so these metrics place the caret,
// selection, and markers, and map clicks to a character.
pub(crate) const CODE_CHAR_WIDTH: f32 = CODE_FONT_SIZE * 0.6;
pub(crate) const CODE_LINE_HEIGHT: f32 = 19.0;

const CARET_COLOR: Color = Color::srgb(0.85, 0.85, 0.95);
const SELECTION_COLOR: Color = Color::srgba(0.36, 0.45, 0.85, 0.35);

/// The editable state of a code editor: its text, caret (character index), and selection anchor.
/// A selection spans the range between `anchor` and `cursor`; they are equal when nothing is
/// selected.
#[derive(Component, Debug, Clone)]
pub(crate) struct CodeEditor {
    pub(crate) text: String,
    pub(crate) cursor: usize,
    pub(crate) anchor: usize,
}

impl CodeEditor {
    pub(crate) fn new(text: String) -> Self {
        let end = text.chars().count();
        Self {
            text,
            cursor: end,
            anchor: end,
        }
    }

    /// Restores a caret/selection captured earlier, clamped to the current text length.
    pub(crate) fn with_selection(mut self, cursor: usize, anchor: usize) -> Self {
        let end = self.text.chars().count();
        self.cursor = cursor.min(end);
        self.anchor = anchor.min(end);
        self
    }

    fn selection(&self) -> Option<(usize, usize)> {
        (self.cursor != self.anchor)
            .then(|| (self.cursor.min(self.anchor), self.cursor.max(self.anchor)))
    }

    /// The currently selected text, if any.
    fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection()?;
        Some(self.text.chars().skip(start).take(end - start).collect())
    }

    fn set_cursor(&mut self, cursor: usize, extend: bool) {
        self.cursor = cursor;
        if !extend {
            self.anchor = cursor;
        }
    }

    /// Removes the selected range, collapsing the caret to its start. Returns whether it changed.
    fn delete_selection(&mut self) -> bool {
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

    fn insert(&mut self, insert: &str) {
        self.delete_selection();
        let mut chars: Vec<char> = self.text.chars().collect();
        for (offset, character) in insert.chars().enumerate() {
            chars.insert(self.cursor + offset, character);
        }
        self.cursor += insert.chars().count();
        self.text = chars.into_iter().collect();
        self.anchor = self.cursor;
    }
}

/// The pluggable syntax highlighter: maps the buffer text to coloured runs that reproduce it.
#[derive(Component, Clone)]
pub(crate) struct CodeEditorHighlighter(
    pub(crate) Arc<dyn Fn(&str) -> Vec<(String, Color)> + Send + Sync>,
);

/// Optional extra decorations a host can attach (e.g. a compile-error underline on one line).
#[derive(Component, Debug, Clone, Default)]
pub(crate) struct CodeEditorMarkers {
    /// A zero-based line to underline in the error colour, if any.
    pub(crate) error_line: Option<usize>,
    pub(crate) error_color: Color,
}

/// Fired when a code editor's text changes, so the host can persist it (and its caret/selection).
#[derive(Event, Debug, Clone, Copy)]
pub(crate) struct CodeEditorChanged(pub(crate) Entity);

/// Registers the code-editor rendering and input handling.
pub(crate) struct CodeEditorPlugin;

impl Plugin for CodeEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(focus_code_editor)
            .add_observer(drag_code_editor)
            .add_observer(edit_code_editor)
            .add_systems(Update, render_code_editors);
    }
}

/// Spawns a code-editor surface as a child of `parent`. The caller supplies the highlighter and any
/// markers, and observes [`CodeEditorChanged`] to persist edits.
pub(crate) fn spawn_code_editor(
    parent: &mut ChildSpawnerCommands,
    editor: CodeEditor,
    highlighter: CodeEditorHighlighter,
    markers: CodeEditorMarkers,
    tag: impl Bundle,
) -> Entity {
    parent
        .spawn((
            editor,
            highlighter,
            markers,
            tag,
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Percent(100.0),
                position_type: PositionType::Relative,
                ..default()
            },
            RelativeCursorPosition::default(),
        ))
        .id()
}

// ---- geometry helpers -----------------------------------------------------------------------

fn line_col(source: &str, caret: usize) -> (usize, usize) {
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

fn char_index(source: &str, line: usize, column: usize) -> usize {
    let mut current_line = 0;
    let mut index = 0;
    let mut line_column = 0;
    for character in source.chars() {
        if current_line == line && line_column == column {
            return index;
        }
        if character == '\n' {
            if current_line == line {
                return index;
            }
            current_line += 1;
            line_column = 0;
        } else {
            line_column += 1;
        }
        index += 1;
    }
    index
}

fn line_length(source: &str, line: usize) -> usize {
    source
        .split('\n')
        .nth(line)
        .map_or(0, |text| text.chars().count())
}

/// Moves the caret up/down one visual line, keeping the column.
fn vertical(source: &str, caret: usize, down: bool) -> usize {
    let (line, column) = line_col(source, caret);
    if down {
        let lines = source.split('\n').count();
        if line + 1 >= lines {
            return caret;
        }
        char_index(source, line + 1, column)
    } else if line == 0 {
        caret
    } else {
        char_index(source, line - 1, column)
    }
}

fn caret_from_local(source: &str, x: f32, y: f32) -> usize {
    let line = (y / CODE_LINE_HEIGHT).floor().max(0.0) as usize;
    let column = (x / CODE_CHAR_WIDTH).round().max(0.0) as usize;
    char_index(source, line, column)
}

// ---- rendering ------------------------------------------------------------------------------

fn text_font() -> TextFont {
    TextFont {
        font_size: FontSize::Px(CODE_FONT_SIZE),
        ..default()
    }
}

fn overlay(left: f32, top: f32, width: f32, height: f32, color: Color) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(left),
            top: Val::Px(top),
            width: Val::Px(width),
            height: Val::Px(height),
            ..default()
        },
        BackgroundColor(color),
        Pickable::IGNORE,
    )
}

fn spawn_children(
    surface: &mut ChildSpawnerCommands,
    editor: &CodeEditor,
    highlighter: &CodeEditorHighlighter,
    markers: &CodeEditorMarkers,
    focused: bool,
) {
    let source = &editor.text;

    // Selection highlights, one bar per covered line, behind the text.
    if let Some((start, end)) = editor.selection() {
        let (l0, c0) = line_col(source, start);
        let (l1, c1) = line_col(source, end);
        for line in l0..=l1 {
            let from = if line == l0 { c0 } else { 0 };
            let to = if line == l1 {
                c1
            } else {
                line_length(source, line) + 1
            };
            let width = ((to.saturating_sub(from)) as f32 * CODE_CHAR_WIDTH).max(2.0);
            surface.spawn(overlay(
                from as f32 * CODE_CHAR_WIDTH,
                line as f32 * CODE_LINE_HEIGHT,
                width,
                CODE_LINE_HEIGHT,
                SELECTION_COLOR,
            ));
        }
    }

    // Coloured text.
    surface
        .spawn((
            Text::new(""),
            text_font(),
            LineHeight::Px(CODE_LINE_HEIGHT),
            TextColor(theme::TEXT),
            Node {
                width: Val::Percent(100.0),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|text| {
            let mut runs = (highlighter.0)(source);
            if runs.is_empty() {
                runs.push((String::new(), theme::TEXT));
            }
            for (run, color) in runs {
                text.spawn((
                    TextSpan::new(run),
                    text_font(),
                    LineHeight::Px(CODE_LINE_HEIGHT),
                    TextColor(color),
                ));
            }
        });

    // Error underline marker.
    if let Some(line) = markers.error_line {
        let width = (line_length(source, line).max(1) as f32) * CODE_CHAR_WIDTH;
        surface.spawn(overlay(
            0.0,
            line as f32 * CODE_LINE_HEIGHT + CODE_LINE_HEIGHT - 2.0,
            width,
            2.0,
            markers.error_color,
        ));
    }

    // Caret.
    if focused {
        let (line, column) = line_col(source, editor.cursor);
        surface.spawn(overlay(
            column as f32 * CODE_CHAR_WIDTH,
            line as f32 * CODE_LINE_HEIGHT,
            2.0,
            CODE_LINE_HEIGHT,
            CARET_COLOR,
        ));
    }
}

/// Rebuilds each editor's children when its text/selection, markers, or focus change.
fn render_code_editors(
    focus: Res<InputFocus>,
    editors: Query<(
        Entity,
        Ref<CodeEditor>,
        &CodeEditorHighlighter,
        Ref<CodeEditorMarkers>,
        Option<&Children>,
    )>,
    mut commands: Commands,
) {
    let focus_changed = focus.is_changed();
    for (entity, editor, highlighter, markers, children) in &editors {
        if !focus_changed && !editor.is_changed() && !markers.is_changed() {
            continue;
        }
        if let Some(children) = children {
            for &child in children {
                commands.entity(child).despawn();
            }
        }
        let focused = focus.get() == Some(entity);
        commands.entity(entity).with_children(|surface| {
            spawn_children(surface, &editor, highlighter, &markers, focused);
        });
    }
}

// ---- input ----------------------------------------------------------------------------------

fn focus_code_editor(
    mut click: On<Pointer<Click>>,
    mut editors: Query<(&mut CodeEditor, &RelativeCursorPosition, &ComputedNode)>,
    keys: Res<ButtonInput<KeyCode>>,
    mut focus: ResMut<InputFocus>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let target = click.event_target();
    let Ok((mut editor, relative, node)) = editors.get_mut(target) else {
        return;
    };
    click.propagate(false);
    *focus = InputFocus::from_entity(target);
    if let Some(normalized) = relative.normalized {
        let size = node.size() * node.inverse_scale_factor;
        let caret = caret_from_local(&editor.text, normalized.x * size.x, normalized.y * size.y);
        let extend = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        editor.set_cursor(caret, extend);
    }
}

fn drag_code_editor(
    drag: On<Pointer<Drag>>,
    mut editors: Query<(&mut CodeEditor, &RelativeCursorPosition, &ComputedNode)>,
) {
    if drag.button != PointerButton::Primary {
        return;
    }
    let Ok((mut editor, relative, node)) = editors.get_mut(drag.event_target()) else {
        return;
    };
    if let Some(normalized) = relative.normalized {
        let size = node.size() * node.inverse_scale_factor;
        let caret = caret_from_local(&editor.text, normalized.x * size.x, normalized.y * size.y);
        editor.set_cursor(caret, true); // drag extends the selection from the press point
    }
}

fn edit_code_editor(
    key: On<FocusedInput<KeyboardInput>>,
    mut editors: Query<&mut CodeEditor>,
    keys: Res<ButtonInput<KeyCode>>,
    mut clipboard: ResMut<Clipboard>,
    mut commands: Commands,
) {
    let entity = key.event_target();
    let Ok(mut editor) = editors.get_mut(entity) else {
        return;
    };
    if key.input.state != ButtonState::Pressed {
        return;
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let count = editor.text.chars().count();
    let mut changed = false;

    match key.input.key_code {
        KeyCode::ArrowLeft => {
            let target = editor.cursor.saturating_sub(1);
            editor.set_cursor(target, shift);
        }
        KeyCode::ArrowRight => {
            let target = (editor.cursor + 1).min(count);
            editor.set_cursor(target, shift);
        }
        KeyCode::ArrowUp => {
            let target = vertical(&editor.text, editor.cursor, false);
            editor.set_cursor(target, shift);
        }
        KeyCode::ArrowDown => {
            let target = vertical(&editor.text, editor.cursor, true);
            editor.set_cursor(target, shift);
        }
        KeyCode::Home => {
            let (line, _) = line_col(&editor.text, editor.cursor);
            let target = char_index(&editor.text, line, 0);
            editor.set_cursor(target, shift);
        }
        KeyCode::End => {
            let (line, _) = line_col(&editor.text, editor.cursor);
            let target = char_index(&editor.text, line, line_length(&editor.text, line));
            editor.set_cursor(target, shift);
        }
        KeyCode::Backspace => {
            if editor.delete_selection() {
                changed = true;
            } else if editor.cursor > 0 {
                let mut chars: Vec<char> = editor.text.chars().collect();
                chars.remove(editor.cursor - 1);
                editor.cursor -= 1;
                editor.anchor = editor.cursor;
                editor.text = chars.into_iter().collect();
                changed = true;
            }
        }
        KeyCode::Delete => {
            if editor.delete_selection() {
                changed = true;
            } else if editor.cursor < count {
                let mut chars: Vec<char> = editor.text.chars().collect();
                chars.remove(editor.cursor);
                editor.text = chars.into_iter().collect();
                changed = true;
            }
        }
        KeyCode::Enter | KeyCode::NumpadEnter => {
            editor.insert("\n");
            changed = true;
        }
        KeyCode::Tab => {
            editor.insert("    ");
            changed = true;
        }
        KeyCode::KeyA if ctrl => {
            editor.anchor = 0;
            editor.cursor = count;
        }
        KeyCode::KeyC if ctrl => {
            if let Some(text) = editor.selected_text() {
                let _ = clipboard.set_text(text);
            }
        }
        KeyCode::KeyX if ctrl => {
            if let Some(text) = editor.selected_text() {
                let _ = clipboard.set_text(text);
                changed = editor.delete_selection();
            }
        }
        KeyCode::KeyV if ctrl => {
            if let Some(Ok(text)) = clipboard.fetch_text().poll_result() {
                // Clipboards on Windows carry CRLF; the buffer stores LF only.
                let text = text.replace("\r\n", "\n").replace('\r', "\n");
                if !text.is_empty() {
                    editor.insert(&text);
                    changed = true;
                }
            }
        }
        _ => {
            if ctrl {
                return; // other Ctrl combinations are not editor shortcuts
            }
            if let Key::Character(input) = &key.input.logical_key {
                editor.insert(input);
                changed = true;
            } else if key.input.logical_key == Key::Space {
                editor.insert(" ");
                changed = true;
            } else {
                return;
            }
        }
    }

    if changed {
        commands.trigger(CodeEditorChanged(entity));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_replaces_the_selection() {
        let mut editor = CodeEditor::new("hello world".into());
        editor.anchor = 0;
        editor.cursor = 5; // select "hello"
        editor.insert("bye");
        assert_eq!(editor.text, "bye world");
        assert_eq!(editor.cursor, 3);
        assert_eq!(editor.selection(), None);
    }

    #[test]
    fn caret_maps_to_line_and_column() {
        let source = "abc\ndefg\nhi";
        assert_eq!(line_col(source, 6), (1, 2));
        assert_eq!(char_index(source, 1, 2), 6);
        assert_eq!(char_index(source, 1, 99), 8);
    }

    #[test]
    fn selected_text_returns_the_covered_range() {
        let mut editor = CodeEditor::new("hello world".into());
        assert_eq!(editor.selected_text(), None);
        editor.anchor = 6;
        editor.cursor = 11; // select "world"
        assert_eq!(editor.selected_text().as_deref(), Some("world"));
        // The order of anchor/cursor should not matter.
        editor.anchor = 11;
        editor.cursor = 6;
        assert_eq!(editor.selected_text().as_deref(), Some("world"));
    }

    #[test]
    fn backspace_deletes_a_selection_in_one_step() {
        let mut editor = CodeEditor::new("abcdef".into());
        editor.anchor = 1;
        editor.cursor = 4; // select "bcd"
        assert!(editor.delete_selection());
        assert_eq!(editor.text, "aef");
        assert_eq!(editor.cursor, 1);
    }
}
