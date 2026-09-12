//! A generic, reusable multi-line code editor widget for the editor's feathers layer.
//!
//! [`CodeEditor`] owns an editable text buffer with a caret and selection, rendered as monospace
//! coloured runs (via a pluggable [`CodeEditorHighlighter`]) with overlay caret, selection
//! highlights, and optional line markers. It handles click/drag selection, keyboard navigation and
//! editing (with shift-selection and Ctrl+A), and emits [`CodeEditorChanged`] when the text changes
//! so a host can persist it. Clipboard (cut/copy/paste) is layered on in a later stage.
#![allow(dead_code)] // Reusable widget API; not every hook is used by the current single consumer.

use crate::feathers::context_menu::{
    pointer_position_in_node, should_dismiss_pointer_context_menu, spawn_pointer_context_menu,
    spawn_pointer_context_menu_item,
};
use crate::theme;
use bevy::asset::RenderAssetUsages;
use bevy::feathers::cursor::EntityCursor;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::{FocusedInput, InputFocus};
use bevy::math::BVec2;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::{FontSize, LineBreak, LineHeight, TextLayout, TextSpan};
use bevy::ui::{ComputedNode, IgnoreScroll, RelativeCursorPosition, UiGlobalTransform};
use bevy::ui_widgets::Activate;
use bevy::window::{PrimaryWindow, SystemCursorIcon};
use std::sync::Arc;

pub(crate) const CODE_FONT_SIZE: f32 = 13.0;
// Fira Mono advance is ~0.6em; the host sets the monospace font, so these metrics place the caret,
// selection, and markers, and map clicks to a character.
pub(crate) const CODE_CHAR_WIDTH: f32 = CODE_FONT_SIZE * 0.6;
pub(crate) const CODE_LINE_HEIGHT: f32 = 19.0;

const CARET_COLOR: Color = Color::srgb(0.85, 0.85, 0.95);
const SELECTION_COLOR: Color = Color::srgba(0.36, 0.45, 0.85, 0.35);

/// Width of the line-number gutter, and the gap between it and the code. Code content is offset
/// right by [`GUTTER_WIDTH`] so it clears the gutter, which stays pinned to the left while the code
/// scrolls horizontally.
const GUTTER_WIDTH: f32 = 44.0;
const GUTTER_TEXT_PADDING: f32 = 8.0;

/// The upper bound on retained undo history, so a long editing session cannot grow it without
/// limit.
const MAX_UNDO: usize = 256;

/// The kind of the last text edit, used to coalesce a run of same-kind edits into one undo step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditKind {
    /// A single typed character (coalesces with adjacent typing).
    Type,
    /// A single-character deletion (coalesces with adjacent deletions).
    Erase,
    /// Any other edit (newline, tab, paste, cut, deleting a selection): never coalesces.
    Other,
}

/// A captured editor state for undo/redo.
#[derive(Debug, Clone)]
struct EditSnapshot {
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
    undo: Vec<EditSnapshot>,
    redo: Vec<EditSnapshot>,
    last_edit: Option<EditKind>,
}

impl CodeEditor {
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

    fn snapshot(&self) -> EditSnapshot {
        EditSnapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
        }
    }

    /// Records the pre-edit state onto the undo stack (called before mutating). Consecutive edits
    /// of the same coalescing kind fold into the one step so a run of typing undoes together.
    fn record(&mut self, kind: EditKind) {
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

    fn apply(&mut self, snapshot: EditSnapshot) {
        self.text = snapshot.text;
        self.cursor = snapshot.cursor;
        self.anchor = snapshot.anchor;
        self.last_edit = None; // an undo/redo breaks any coalescing run
    }

    /// Selects the word (alphanumeric or `_` run) under `index`; if not on a word, collapses there.
    fn select_word_at(&mut self, index: usize) {
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

/// The pluggable syntax highlighter: maps the buffer text to coloured runs that reproduce it.
#[derive(Component, Clone)]
pub(crate) struct CodeEditorHighlighter(
    pub(crate) Arc<dyn Fn(&str) -> Vec<(String, Color)> + Send + Sync>,
);

/// Optional extra decorations a host can attach: a compile error underlined at the exact failing
/// characters, with an inline message.
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub(crate) struct CodeEditorMarkers {
    pub(crate) error: Option<CodeEditorError>,
    pub(crate) error_color: Color,
}

/// A compile error to mark in the editor: the failing character range and a short message shown
/// inline after it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CodeEditorError {
    /// The failing range as char indices `(start, end)`.
    pub(crate) span: (usize, usize),
    pub(crate) message: String,
}

/// Fired when a code editor's text changes, so the host can persist it (and its caret/selection).
#[derive(Event, Debug, Clone, Copy)]
pub(crate) struct CodeEditorChanged(pub(crate) Entity);

/// Registers the code-editor rendering and input handling.
pub(crate) struct CodeEditorPlugin;

/// A tiny tiled squiggle texture (white on transparent) tinted red under compile errors, for the
/// wavy "error underline" look. Generated once at startup.
#[derive(Resource)]
struct CodeEditorSquiggle(Handle<Image>);

/// Builds the 8×5 triangle-wave tile used for the error underline.
fn build_squiggle(images: &mut Assets<Image>) -> Handle<Image> {
    const W: usize = 8;
    const H: usize = 5;
    let mut data = vec![0u8; W * H * 4];
    for x in 0..W {
        // Triangle wave over the tile width: 0,1,2,3,4,3,2,1 (top row = 0).
        let y = if x < 4 { x } else { 8 - x };
        for row in [y, y.saturating_sub(1)] {
            let i = (row * W + x) * 4;
            data[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
    }
    let image = Image::new(
        Extent3d {
            width: W as u32,
            height: H as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    images.add(image)
}

fn init_code_editor_squiggle(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let handle = build_squiggle(&mut images);
    commands.insert_resource(CodeEditorSquiggle(handle));
}

impl Plugin for CodeEditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CodeEditorPointer>()
            .init_resource::<CaretBlink>()
            .init_resource::<ActiveCodeEditor>()
            .add_systems(Startup, init_code_editor_squiggle)
            .add_observer(edit_code_editor)
            .add_observer(activate_code_editor_menu)
            .add_systems(
                Update,
                (
                    code_editor_pointer_input,
                    open_code_editor_context_menu,
                    dismiss_code_editor_context_menu,
                    track_active_code_editor,
                    render_code_editors,
                    render_code_gutters,
                    blink_carets,
                )
                    .chain(),
            );
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
                // Width grows to the longest (unwrapped) line; height is set to the text height by
                // `render_code_editors` (with min_height as a floor so short files still fill the
                // viewport). flex_shrink: 0 stops the scroll container from shrinking the widget back
                // to the viewport height — it must keep its full content height and overflow, so its
                // own box (which `cursor_over` and click mapping use) covers every line.
                min_width: Val::Percent(100.0),
                min_height: Val::Percent(100.0),
                flex_shrink: 0.0,
                position_type: PositionType::Relative,
                ..default()
            },
            RelativeCursorPosition::default(),
            EntityCursor::System(SystemCursorIcon::Text), // I-beam over the editable text
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
    // Clicks land in the same frame the code is drawn in, which is inset past the gutter.
    let column = ((x - GUTTER_WIDTH) / CODE_CHAR_WIDTH).round().max(0.0) as usize;
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
    squiggle: &Handle<Image>,
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
                GUTTER_WIDTH + from as f32 * CODE_CHAR_WIDTH,
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
            // Code does not reflow: without this, narrowing the pane wraps long lines so the visual
            // rows stop matching the logical line/column the caret and selection are placed by.
            TextLayout {
                linebreak: LineBreak::NoWrap,
                ..default()
            },
            TextColor(theme::TEXT),
            // No width constraint: the text sizes to its longest line so the editor can scroll to
            // it. Offset right to clear the line-number gutter pinned on the left.
            Node {
                margin: UiRect::left(Val::Px(GUTTER_WIDTH)),
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

    // Error marker: underline the exact failing characters (per covered line) and show the message
    // inline after the last one.
    if let Some(error) = &markers.error {
        let (start, end) = error.span;
        let end = end.max(start + 1); // empty ranges still underline one character
        let (l0, c0) = line_col(source, start);
        let (l1, c1) = line_col(source, end);
        for line in l0..=l1 {
            let from = if line == l0 { c0 } else { 0 };
            let to = if line == l1 {
                c1
            } else {
                line_length(source, line)
            };
            let width = ((to.saturating_sub(from)).max(1) as f32) * CODE_CHAR_WIDTH;
            // A tiled squiggle image, tinted the error colour, for the wavy underline.
            surface.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(GUTTER_WIDTH + from as f32 * CODE_CHAR_WIDTH),
                    top: Val::Px(line as f32 * CODE_LINE_HEIGHT + CODE_LINE_HEIGHT - 5.0),
                    width: Val::Px(width),
                    height: Val::Px(5.0),
                    ..default()
                },
                ImageNode {
                    image: squiggle.clone(),
                    color: markers.error_color,
                    image_mode: NodeImageMode::Tiled {
                        tile_x: true,
                        tile_y: false,
                        stretch_value: 1.0,
                    },
                    ..default()
                },
                Pickable::IGNORE,
            ));
        }
        let inline_left = GUTTER_WIDTH + (line_length(source, l1) + 2) as f32 * CODE_CHAR_WIDTH;
        surface.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(inline_left),
                top: Val::Px(l1 as f32 * CODE_LINE_HEIGHT),
                ..default()
            },
            Text::new(error.message.clone()),
            text_font(),
            LineHeight::Px(CODE_LINE_HEIGHT),
            TextLayout {
                linebreak: LineBreak::NoWrap,
                ..default()
            },
            // Dimmed so the inline message reads as a hint, not competing with the code.
            TextColor(markers.error_color.with_alpha(0.6)),
            Pickable::IGNORE,
        ));
    }

    // Caret.
    if focused {
        let (line, column) = line_col(source, editor.cursor);
        surface.spawn((
            overlay(
                GUTTER_WIDTH + column as f32 * CODE_CHAR_WIDTH,
                line as f32 * CODE_LINE_HEIGHT,
                2.0,
                CODE_LINE_HEIGHT,
                CARET_COLOR,
            ),
            CaretVisual,
        ));
    }
}

/// Marks the caret bar so [`blink_carets`] can pulse its visibility.
#[derive(Component)]
struct CaretVisual;

/// The half-period of the caret blink, in seconds.
const CARET_BLINK_SECONDS: f32 = 0.53;

/// The time origin of the caret blink phase, reset whenever an editor changes so the caret shows
/// solid immediately after the caret moves or the text is edited.
#[derive(Resource, Default)]
struct CaretBlink {
    epoch: f32,
}

/// Pulses every caret's visibility on a fixed cadence, like a typical editor.
fn blink_carets(
    time: Res<Time>,
    blink: Res<CaretBlink>,
    mut carets: Query<&mut Visibility, With<CaretVisual>>,
) {
    let phase = ((time.elapsed_secs() - blink.epoch) / CARET_BLINK_SECONDS) as u64;
    let visible = phase.is_multiple_of(2);
    let next = if visible {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut carets {
        visibility.set_if_neq(next);
    }
}

/// A line-number gutter bound to a code editor. It is a sibling of the editor inside the same
/// scroll viewport, pinned horizontally (via [`IgnoreScroll`]) while it scrolls vertically with the
/// code, and re-rendered when the editor's line count changes.
#[derive(Component)]
struct CodeGutter {
    editor: Entity,
    lines: usize,
}

/// Spawns a line-number gutter for `editor`. Spawn it after the editor so it draws on top of any
/// code scrolled under it.
pub(crate) fn spawn_code_gutter(parent: &mut ChildSpawnerCommands, editor: Entity) -> Entity {
    parent
        .spawn((
            CodeGutter { editor, lines: 0 },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Px(GUTTER_WIDTH),
                min_height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            // Sticky on x only: stays put as the code scrolls horizontally, follows vertical scroll.
            IgnoreScroll(BVec2::new(true, false)),
            BackgroundColor(theme::PANEL),
            Pickable::IGNORE,
        ))
        .id()
}

/// Rebuilds a gutter's line numbers whenever its editor's line count changes.
fn render_code_gutters(
    editors: Query<&CodeEditor>,
    mut gutters: Query<(Entity, &mut CodeGutter, Option<&Children>)>,
    mut commands: Commands,
) {
    for (entity, mut gutter, children) in &mut gutters {
        let Ok(editor) = editors.get(gutter.editor) else {
            continue;
        };
        let lines = editor.text.split('\n').count();
        if lines == gutter.lines && children.is_some() {
            continue;
        }
        gutter.lines = lines;
        if let Some(children) = children {
            for &child in children {
                commands.entity(child).despawn();
            }
        }
        commands.entity(entity).with_children(|column| {
            for line in 0..lines {
                column
                    .spawn((
                        Node {
                            height: Val::Px(CODE_LINE_HEIGHT),
                            padding: UiRect::right(Val::Px(GUTTER_TEXT_PADDING)),
                            justify_content: JustifyContent::FlexEnd,
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .with_child((
                        Text::new((line + 1).to_string()),
                        text_font(),
                        LineHeight::Px(CODE_LINE_HEIGHT),
                        TextColor(theme::TEXT_FAINT),
                        Pickable::IGNORE,
                    ));
            }
        });
    }
}

/// Rebuilds each editor's children when its text/selection, markers, or focus change.
fn render_code_editors(
    focus: Res<InputFocus>,
    time: Res<Time>,
    mut blink: ResMut<CaretBlink>,
    squiggle: Res<CodeEditorSquiggle>,
    mut editors: Query<(
        Entity,
        Ref<CodeEditor>,
        &CodeEditorHighlighter,
        Ref<CodeEditorMarkers>,
        &mut Node,
        Option<&Children>,
    )>,
    mut commands: Commands,
) {
    let focus_changed = focus.is_changed();
    for (entity, editor, highlighter, markers, mut node, children) in &mut editors {
        if !focus_changed && !editor.is_changed() && !markers.is_changed() {
            continue;
        }
        // Size the widget's own box to the text height so `cursor_over` and click mapping cover every
        // line; min_height keeps it at least the viewport height for short files.
        if editor.is_changed() {
            let lines = editor.text.split('\n').count().max(1);
            node.height = Val::Px(lines as f32 * CODE_LINE_HEIGHT);
        }
        let focused = focus.get() == Some(entity);
        if focused && editor.is_changed() {
            blink.epoch = time.elapsed_secs(); // keep the caret solid right after it moves
        }
        if let Some(children) = children {
            for &child in children {
                commands.entity(child).despawn();
            }
        }
        commands.entity(entity).with_children(|surface| {
            spawn_children(
                surface,
                &editor,
                highlighter,
                &markers,
                &squiggle.0,
                focused,
            );
        });
    }
}

// ---- input ----------------------------------------------------------------------------------

/// The window seconds within which a second press on the same spot counts as a double-click.
const DOUBLE_CLICK_SECONDS: f32 = 0.4;

/// Drag/double-click bookkeeping for the pointer input system.
#[derive(Resource, Default)]
struct CodeEditorPointer {
    /// The editor currently being drag-selected (mouse held after a press over it).
    dragging: Option<Entity>,
    /// The last press: editor, elapsed time, and caret index, for double-click detection.
    last_press: Option<(Entity, f32, usize)>,
}

/// Places and extends the caret from the mouse, driven by `ButtonInput<MouseButton>` +
/// `RelativeCursorPosition` rather than pointer-pick events. The coloured text drawn over the
/// editor is `Pickable::IGNORE`, but `Click`/`Press` events still fail to fall through to the
/// widget over a glyph; `cursor_over`, computed for every node under the pointer, does not, so a
/// press anywhere over the editor — text or empty space — reliably moves the caret.
fn code_editor_pointer_input(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut pointer: ResMut<CodeEditorPointer>,
    mut focus: ResMut<InputFocus>,
    menus: Query<&RelativeCursorPosition, With<CodeEditorContextMenu>>,
    mut editors: Query<(
        Entity,
        &mut CodeEditor,
        &RelativeCursorPosition,
        &ComputedNode,
    )>,
) {
    if mouse.just_released(MouseButton::Left) {
        pointer.dragging = None;
    }

    // A click on an open context menu overlays the editor: let the menu's own action run (and the
    // dismiss handler close it) without the editor moving the caret and clearing the selection.
    if mouse.just_pressed(MouseButton::Left)
        && menus.iter().any(RelativeCursorPosition::cursor_over)
    {
        return;
    }

    if mouse.just_pressed(MouseButton::Left) {
        let hovered = editors
            .iter()
            .find_map(|(entity, _, relative, _)| relative.cursor_over.then_some(entity));
        if let Some(entity) = hovered {
            let (_, mut editor, relative, node) = editors.get_mut(entity).expect("hovered editor");
            *focus = InputFocus::from_entity(entity);
            pointer.dragging = Some(entity);
            if let Some((x, y)) = local_cursor(relative, node) {
                let caret = caret_from_local(&editor.text, x, y);
                let now = time.elapsed_secs();
                let double = pointer
                    .last_press
                    .is_some_and(|(last_entity, at, last_caret)| {
                        last_entity == entity
                            && now - at < DOUBLE_CLICK_SECONDS
                            && last_caret == caret
                    });
                if double {
                    editor.select_word_at(caret);
                } else {
                    let extend =
                        keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
                    editor.set_cursor(caret, extend);
                }
                pointer.last_press = Some((entity, now, caret));
            }
        }
    } else if mouse.pressed(MouseButton::Left)
        && let Some(entity) = pointer.dragging
        && let Ok((_, mut editor, relative, node)) = editors.get_mut(entity)
        && let Some((x, y)) = local_cursor(relative, node)
    {
        let caret = caret_from_local(&editor.text, x, y);
        editor.set_cursor(caret, true); // drag extends the selection from the press point
    }
}

/// The cursor position in the node's local logical pixels (origin at its top-left), or `None` when
/// the pointer is off the node. Bevy's `RelativeCursorPosition::normalized` centres the node at the
/// origin and ranges (-0.5, -0.5) top-left to (0.5, 0.5) bottom-right, so shift it into a top-left
/// origin before scaling by the node size.
fn local_cursor(relative: &RelativeCursorPosition, node: &ComputedNode) -> Option<(f32, f32)> {
    let normalized = relative.normalized?;
    let size = node.size() * node.inverse_scale_factor;
    Some(((normalized.x + 0.5) * size.x, (normalized.y + 0.5) * size.y))
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

    if ctrl {
        // Ctrl shortcuts are matched on the logical letter, not the physical `KeyCode`, so they work
        // on non-QWERTY layouts (e.g. AZERTY, where the Z key is at the physical W position).
        let letter = match &key.input.logical_key {
            Key::Character(text) => text.chars().next().map(|c| c.to_ascii_lowercase()),
            _ => None,
        };
        match letter {
            Some('a') => {
                editor.anchor = 0;
                editor.cursor = count;
            }
            Some('c') => {
                if let Some(text) = editor.selected_text() {
                    let _ = clipboard.set_text(text);
                }
            }
            Some('x') => {
                if let Some(text) = editor.selected_text() {
                    let _ = clipboard.set_text(text);
                    editor.record(EditKind::Other);
                    changed = editor.delete_selection();
                }
            }
            Some('v') => {
                if let Some(Ok(text)) = clipboard.fetch_text().poll_result() {
                    // Clipboards on Windows carry CRLF; the buffer stores LF only.
                    let text = text.replace("\r\n", "\n").replace('\r', "\n");
                    if !text.is_empty() {
                        editor.record(EditKind::Other);
                        editor.insert(&text);
                        changed = true;
                    }
                }
            }
            // Ctrl+Z undoes, Ctrl+Shift+Z and Ctrl+Y redo.
            Some('z') => changed = if shift { editor.redo() } else { editor.undo() },
            Some('y') => changed = editor.redo(),
            _ => {}
        }
        if changed {
            commands.trigger(CodeEditorChanged(entity));
        }
        return;
    }

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
            if editor.selection().is_some() {
                editor.record(EditKind::Other);
                changed = editor.delete_selection();
            } else if editor.cursor > 0 {
                editor.record(EditKind::Erase);
                let mut chars: Vec<char> = editor.text.chars().collect();
                chars.remove(editor.cursor - 1);
                editor.cursor -= 1;
                editor.anchor = editor.cursor;
                editor.text = chars.into_iter().collect();
                changed = true;
            }
        }
        KeyCode::Delete => {
            if editor.selection().is_some() {
                editor.record(EditKind::Other);
                changed = editor.delete_selection();
            } else if editor.cursor < count {
                editor.record(EditKind::Erase);
                let mut chars: Vec<char> = editor.text.chars().collect();
                chars.remove(editor.cursor);
                editor.text = chars.into_iter().collect();
                changed = true;
            }
        }
        KeyCode::Enter | KeyCode::NumpadEnter => {
            editor.record(EditKind::Other);
            editor.insert("\n");
            changed = true;
        }
        KeyCode::Tab => {
            editor.record(EditKind::Other);
            editor.insert("    ");
            changed = true;
        }
        _ => {
            if let Key::Character(input) = &key.input.logical_key {
                editor.record(EditKind::Type);
                editor.insert(input);
                changed = true;
            } else if key.input.logical_key == Key::Space {
                editor.record(EditKind::Type);
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

// ---- active editor tracking (for the app's Edit menu) -------------------------------------

/// The code editor the app's Edit-menu undo/redo should target: the last-focused editor, kept
/// through menu interaction and cleared when another editing context takes focus.
#[derive(Resource, Default)]
pub(crate) struct ActiveCodeEditor(pub(crate) Option<Entity>);

/// Tracks which code editor (if any) is the active editing context, so the app's history menu can
/// reflect and drive its undo/redo. Focusing the editor sets it; focusing another editing panel
/// (one carrying a `HistoryScope`) clears it; focusing menus, the toolbar, or other unscoped UI
/// keeps it, so the Edit menu's own items still act on the editor.
type MenuMarker = Or<(
    With<bevy::ui_widgets::MenuItem>,
    With<bevy::ui_widgets::MenuPopup>,
    With<crate::feathers::context_menu::PointerContextMenuItem>,
    With<crate::feathers::context_menu::PointerContextMenuSurface>,
)>;

fn track_active_code_editor(
    focus: Res<InputFocus>,
    editors: Query<(), With<CodeEditor>>,
    menus: Query<(), MenuMarker>,
    scopes: Query<(), With<crate::history::HistoryScope>>,
    parents: Query<&ChildOf>,
    mut active: ResMut<ActiveCodeEditor>,
) {
    if !focus.is_changed() {
        return;
    }
    let Some(target) = focus.get() else {
        return; // focus momentarily cleared (e.g. a rebuild): keep the last active editor
    };
    let mut entity = Some(target);
    while let Some(current) = entity {
        if editors.contains(current) {
            active.0 = Some(current);
            return;
        }
        // A menu (checked before the scope, since a context menu is spawned inside a scoped pane)
        // keeps the active editor, so its own items still act on it.
        if menus.contains(current) {
            return;
        }
        if scopes.contains(current) {
            active.0 = None; // focus entered another editing panel
            return;
        }
        entity = parents.get(current).ok().map(ChildOf::parent);
    }
    // The toolbar and other unscoped UI keep the active editor.
}

// ---- context menu ---------------------------------------------------------------------------

/// Marks the anchor of an open code-editor context menu (despawned to close it).
#[derive(Component)]
struct CodeEditorContextAnchor;

/// Marks the menu surface, so a click outside it dismisses the menu.
#[derive(Component)]
struct CodeEditorContextMenu;

/// The edit command a context-menu item runs on its editor.
#[derive(Component, Clone, Copy)]
struct CodeEditorMenuAction {
    editor: Entity,
    command: CodeEditorCommand,
}

#[derive(Clone, Copy, PartialEq)]
enum CodeEditorCommand {
    Cut,
    Copy,
    Paste,
    SelectAll,
    Undo,
    Redo,
}

/// Opens a right-click edit menu over the editor under the pointer. Uses the mouse button plus
/// `cursor_over` (not a pick event, which does not fall through the text) and is spawned as a child
/// of the editor's scroll viewport so [`render_code_editors`] rebuilding the editor does not drop it.
fn open_code_editor_context_menu(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    editors: Query<(Entity, &RelativeCursorPosition, &ChildOf), With<CodeEditor>>,
    hosts: Query<(&ComputedNode, &UiGlobalTransform)>,
    anchors: Query<Entity, With<CodeEditorContextAnchor>>,
    mut focus: ResMut<InputFocus>,
    mut commands: Commands,
) {
    if !mouse.just_pressed(MouseButton::Right) {
        return;
    }
    for anchor in &anchors {
        commands.entity(anchor).despawn();
    }
    let Some((editor, _, child_of)) = editors.iter().find(|(_, cursor, _)| cursor.cursor_over)
    else {
        return;
    };
    let host = child_of.parent();
    let (Ok((node, transform)), Ok(window)) = (hosts.get(host), window.single()) else {
        return;
    };
    let Some(cursor) = window.physical_cursor_position() else {
        return;
    };
    *focus = InputFocus::from_entity(editor);
    let position = pointer_position_in_node(cursor, node, transform) * node.inverse_scale_factor();
    commands.entity(host).with_children(|host| {
        spawn_pointer_context_menu(
            host,
            position,
            CodeEditorContextAnchor,
            CodeEditorContextMenu,
            |menu| {
                for (label, command) in [
                    ("Cut", CodeEditorCommand::Cut),
                    ("Copy", CodeEditorCommand::Copy),
                    ("Paste", CodeEditorCommand::Paste),
                    ("Select All", CodeEditorCommand::SelectAll),
                    ("Undo", CodeEditorCommand::Undo),
                    ("Redo", CodeEditorCommand::Redo),
                ] {
                    spawn_pointer_context_menu_item(
                        menu,
                        label,
                        CodeEditorMenuAction { editor, command },
                    );
                }
            },
        );
    });
}

/// Runs a context-menu command on its editor and closes the menu.
fn activate_code_editor_menu(
    event: On<Activate>,
    actions: Query<&CodeEditorMenuAction>,
    anchors: Query<Entity, With<CodeEditorContextAnchor>>,
    mut editors: Query<&mut CodeEditor>,
    mut clipboard: ResMut<Clipboard>,
    mut commands: Commands,
) {
    let Ok(action) = actions.get(event.entity) else {
        return;
    };
    if let Ok(mut editor) = editors.get_mut(action.editor) {
        let mut changed = false;
        match action.command {
            CodeEditorCommand::Copy => {
                if let Some(text) = editor.selected_text() {
                    let _ = clipboard.set_text(text);
                }
            }
            CodeEditorCommand::Cut => {
                if let Some(text) = editor.selected_text() {
                    let _ = clipboard.set_text(text);
                    editor.record(EditKind::Other);
                    changed = editor.delete_selection();
                }
            }
            CodeEditorCommand::Paste => {
                if let Some(Ok(text)) = clipboard.fetch_text().poll_result() {
                    let text = text.replace("\r\n", "\n").replace('\r', "\n");
                    if !text.is_empty() {
                        editor.record(EditKind::Other);
                        editor.insert(&text);
                        changed = true;
                    }
                }
            }
            CodeEditorCommand::SelectAll => {
                editor.anchor = 0;
                editor.cursor = editor.text.chars().count();
            }
            CodeEditorCommand::Undo => changed = editor.undo(),
            CodeEditorCommand::Redo => changed = editor.redo(),
        }
        if changed {
            commands.trigger(CodeEditorChanged(action.editor));
        }
    }
    for anchor in &anchors {
        commands.entity(anchor).despawn();
    }
}

/// Closes the context menu on Escape or a left-click outside it.
fn dismiss_code_editor_context_menu(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    surfaces: Query<&RelativeCursorPosition, With<CodeEditorContextMenu>>,
    anchors: Query<Entity, With<CodeEditorContextAnchor>>,
    mut commands: Commands,
) {
    if anchors.is_empty() {
        return;
    }
    let dismiss = should_dismiss_pointer_context_menu(
        true,
        buttons.just_pressed(MouseButton::Left),
        keys.just_pressed(KeyCode::Escape),
        surfaces.iter().any(RelativeCursorPosition::cursor_over),
    );
    if dismiss {
        for anchor in &anchors {
            commands.entity(anchor).despawn();
        }
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

    #[test]
    fn typing_coalesces_into_a_single_undo_step() {
        let mut editor = CodeEditor::new(String::new());
        for character in ["h", "i"] {
            editor.record(EditKind::Type);
            editor.insert(character);
        }
        assert_eq!(editor.text, "hi");
        // One undo reverts the whole typing run back to the empty start.
        assert!(editor.undo());
        assert_eq!(editor.text, "");
        assert!(!editor.undo());
    }

    #[test]
    fn redo_restores_an_undone_edit() {
        let mut editor = CodeEditor::new("a".into());
        editor.record(EditKind::Other);
        editor.insert("b");
        assert_eq!(editor.text, "ab");
        assert!(editor.undo());
        assert_eq!(editor.text, "a");
        assert!(editor.redo());
        assert_eq!(editor.text, "ab");
        // A fresh edit clears the redo stack.
        editor.record(EditKind::Other);
        editor.insert("c");
        assert!(!editor.redo());
    }

    #[test]
    fn double_click_selects_the_word_under_the_cursor() {
        let mut editor = CodeEditor::new("let value = 3;".into());
        editor.select_word_at(6); // inside "value"
        assert_eq!(editor.selected_text().as_deref(), Some("value"));
        editor.select_word_at(11); // on the space between "=" and "3"
        assert_eq!(editor.selected_text(), None);
    }
}
