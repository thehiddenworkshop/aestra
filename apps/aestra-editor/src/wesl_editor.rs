//! WESL source code editor pane (Milestone 7).
//!
//! A single-mode, syntax-coloured, editable code view for an open
//! [`crate::wesl_document::WeslDocuments`] buffer. Text is rendered as coloured token spans (via
//! [`crate::wesl_syntax`]) with a caret; keyboard input edits the buffer in place through a custom
//! editor (Bevy's built-in text field is single-line only). Ctrl+S or the header Save button write
//! the buffer to the `.wesl` file, and the footer shows live compiler diagnostics.

use crate::wesl_document::{WeslCompileState, WeslDiagnostics, WeslDocuments, WeslSourceId};
use crate::wesl_syntax::{WeslTokenKind, tokenize};
use crate::*;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::{FocusedInput, InputFocus};
use bevy::text::{LineHeight, TextSpan};
use bevy::ui::{ComputedNode, RelativeCursorPosition};
use std::collections::HashMap;
use std::path::Path;

/// The editable code surface for a WESL document. Carries the id so clicks (focus) and keyboard
/// events route to the right buffer, and so the render can be refreshed in place.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct WeslEditorSurface(pub(crate) WeslSourceId);

/// Fired to write a WESL buffer back to its module file.
#[derive(Event, Debug, Clone, Copy)]
pub(crate) struct SaveWeslSource(pub(crate) WeslSourceId);

/// Per-document caret position, as a character index into the buffer. Kept in a resource so it
/// survives the dock rebuilds that reconstruct the pane.
#[derive(Resource, Debug, Default)]
pub(crate) struct WeslEditorCursors(HashMap<WeslSourceId, usize>);

impl WeslEditorCursors {
    fn get(&self, id: WeslSourceId) -> usize {
        self.0.get(&id).copied().unwrap_or(0)
    }

    fn set(&mut self, id: WeslSourceId, position: usize) {
        self.0.insert(id, position);
    }
}

const CARET_COLOR: Color = Color::srgb(0.85, 0.85, 0.95);
const ERROR_COLOR: Color = Color::srgb(1.0, 0.38, 0.32);
const EDITOR_FONT_SIZE: f32 = 13.0;
// Fira Mono metrics at the editor font size, for positioning the caret/underline overlays and
// mapping clicks to a character. Advance width is ~0.6em for this monospace face.
const CHAR_WIDTH: f32 = EDITOR_FONT_SIZE * 0.6;
const LINE_HEIGHT: f32 = 19.0;

/// The display colour for a token class, tuned for the editor's dark panels.
fn token_color(kind: WeslTokenKind) -> Color {
    match kind {
        WeslTokenKind::Comment => Color::srgb(0.45, 0.52, 0.45),
        WeslTokenKind::Keyword => Color::srgb(0.78, 0.55, 1.0),
        WeslTokenKind::Type => Color::srgb(0.40, 0.82, 0.86),
        WeslTokenKind::Number => Color::srgb(1.0, 0.72, 0.42),
        WeslTokenKind::Attribute => Color::srgb(1.0, 0.80, 0.36),
        WeslTokenKind::String => Color::srgb(0.55, 0.86, 0.58),
        WeslTokenKind::Ident | WeslTokenKind::Whitespace => theme::TEXT,
        WeslTokenKind::Punctuation => theme::TEXT_MUTED,
    }
}

/// The coloured token runs for `source`. Concatenating them reproduces `source`.
fn editor_runs(source: &str) -> Vec<(String, Color)> {
    let mut runs: Vec<(String, Color)> = tokenize(source)
        .into_iter()
        .map(|token| (token.text, token_color(token.kind)))
        .collect();
    if runs.is_empty() {
        runs.push((String::new(), theme::TEXT));
    }
    runs
}

/// The (line, column) of a character index in `source`, both zero-based.
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

/// The character index at zero-based (line, column), clamping the column to the line's length.
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
                return index; // end of the target line
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

/// The visible character length of zero-based `line` in `source` (excluding the newline).
fn line_length(source: &str, line: usize) -> usize {
    source
        .split('\n')
        .nth(line)
        .map_or(0, |text| text.chars().count())
}

/// The inline error line (1-based) for a document, if its last compile failed with a located error.
fn error_line_of(diagnostics: &WeslDiagnostics, id: WeslSourceId) -> Option<usize> {
    match diagnostics.state(id) {
        Some(WeslCompileState::Error { line, .. }) => *line,
        _ => None,
    }
}

fn editor_text_font() -> TextFont {
    TextFont {
        font_size: FontSize::Px(EDITOR_FONT_SIZE),
        ..default()
    }
}

/// (Re)builds the surface children: the coloured text, the error underline, and the caret overlay.
fn spawn_surface_children(
    surface: &mut ChildSpawnerCommands,
    source: &str,
    caret: usize,
    focused: bool,
    error_line: Option<usize>,
) {
    surface
        .spawn((
            Text::new(""),
            editor_text_font(),
            LineHeight::Px(LINE_HEIGHT),
            TextColor(theme::TEXT),
            Node {
                width: Val::Percent(100.0),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|text| {
            for (run, color) in editor_runs(source) {
                text.spawn((
                    TextSpan::new(run),
                    editor_text_font(),
                    LineHeight::Px(LINE_HEIGHT),
                    TextColor(color),
                ));
            }
        });

    if let Some(line) = error_line {
        let row = line.saturating_sub(1);
        let width = (line_length(source, row).max(1) as f32) * CHAR_WIDTH;
        surface.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(row as f32 * LINE_HEIGHT + LINE_HEIGHT - 2.0),
                width: Val::Px(width),
                height: Val::Px(2.0),
                ..default()
            },
            BackgroundColor(ERROR_COLOR),
            Pickable::IGNORE,
        ));
    }

    if focused {
        let (line, column) = line_col(source, caret);
        surface.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(column as f32 * CHAR_WIDTH),
                top: Val::Px(line as f32 * LINE_HEIGHT),
                width: Val::Px(2.0),
                height: Val::Px(LINE_HEIGHT),
                ..default()
            },
            BackgroundColor(CARET_COLOR),
            Pickable::IGNORE,
        ));
    }
}

pub(crate) fn spawn_wesl_editor_view(
    parent: &mut ChildSpawnerCommands,
    id: WeslSourceId,
    documents: &WeslDocuments,
    cursors: &WeslEditorCursors,
    diagnostics: &WeslDiagnostics,
    focus: Option<Entity>,
    localizer: &Localizer,
) {
    let name = documents
        .relative_path(id)
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("{id}"));
    let dirty = documents.is_dirty(id);

    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|panel| {
            panel
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Px(30.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(12.0)),
                        column_gap: Val::Px(8.0),
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL_LIGHT),
                    BorderColor::all(theme::BORDER),
                ))
                .with_children(|header| {
                    header.spawn((
                        Text::new(name),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme::TEXT),
                    ));
                    if dirty {
                        header.spawn((
                            Node {
                                width: Val::Px(6.0),
                                height: Val::Px(6.0),
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(theme::ACCENT),
                        ));
                    }
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    header
                        .spawn((
                            Button,
                            EditorNativeControl,
                            Node {
                                height: Val::Px(22.0),
                                padding: UiRect::horizontal(Val::Px(10.0)),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                border_radius: BorderRadius::all(Val::Px(3.0)),
                                ..default()
                            },
                            BackgroundColor(if dirty {
                                theme::ACCENT_DIM
                            } else {
                                theme::BUTTON
                            }),
                        ))
                        .observe(
                            move |mut click: On<Pointer<Click>>, mut commands: Commands| {
                                if click.button == PointerButton::Primary {
                                    click.propagate(false);
                                    commands.trigger(SaveWeslSource(id));
                                }
                            },
                        )
                        .with_child((
                            Text::new(localizer.text("wesl-editor-save")),
                            TextFont {
                                font_size: FontSize::Px(10.0),
                                ..default()
                            },
                            TextColor(theme::TEXT),
                            Pickable::IGNORE,
                        ));
                });

            let Some(source) = documents.text(id).map(str::to_owned) else {
                panel.spawn((
                    Text::new(localizer.text("wesl-editor-unavailable")),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_MUTED),
                    Node {
                        margin: UiRect::all(Val::Px(20.0)),
                        ..default()
                    },
                ));
                return;
            };

            spawn_vertical_scroll_area(
                panel,
                ScrollMemoryKey::WeslSource,
                Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(10.0)),
                    ..default()
                },
                |body| {
                    let mut surface = body.spawn((
                        WeslEditorSurface(id),
                        Node {
                            width: Val::Percent(100.0),
                            min_height: Val::Percent(100.0),
                            position_type: PositionType::Relative,
                            ..default()
                        },
                        RelativeCursorPosition::default(),
                    ));
                    let focused = focus == Some(surface.id());
                    let error_line = error_line_of(diagnostics, id);
                    surface
                        .observe(focus_wesl_editor)
                        .observe(edit_wesl_source)
                        .with_children(|surface| {
                            spawn_surface_children(
                                surface,
                                &source,
                                cursors.get(id),
                                focused,
                                error_line,
                            );
                        });
                },
            );

            spawn_diagnostics_footer(panel, id, diagnostics.state(id), localizer);
        });
}

/// Clicking the code surface focuses it and moves the caret to the clicked character.
fn focus_wesl_editor(
    mut click: On<Pointer<Click>>,
    surfaces: Query<(
        Entity,
        &WeslEditorSurface,
        &RelativeCursorPosition,
        &ComputedNode,
    )>,
    documents: Res<WeslDocuments>,
    mut cursors: ResMut<WeslEditorCursors>,
    mut focus: ResMut<InputFocus>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok((entity, surface, relative, node)) = surfaces.get(click.event_target()) else {
        return;
    };
    click.propagate(false);
    *focus = InputFocus::from_entity(entity);

    // Map the click to a caret position using the monospace metrics.
    if let (Some(normalized), Some(source)) = (relative.normalized, documents.text(surface.0)) {
        let size = node.size() * node.inverse_scale_factor;
        let x = normalized.x * size.x;
        let y = normalized.y * size.y;
        let line = (y / LINE_HEIGHT).floor().max(0.0) as usize;
        let column = (x / CHAR_WIDTH).round().max(0.0) as usize;
        cursors.set(surface.0, char_index(source, line, column));
    }
}

/// Applies a keystroke to the focused WESL surface: edits the buffer text and/or moves the caret.
fn edit_wesl_source(
    key: On<FocusedInput<KeyboardInput>>,
    surfaces: Query<&WeslEditorSurface>,
    mut documents: ResMut<WeslDocuments>,
    mut cursors: ResMut<WeslEditorCursors>,
) {
    let Ok(surface) = surfaces.get(key.event_target()) else {
        return;
    };
    if key.input.state != ButtonState::Pressed {
        return;
    }
    let id = surface.0;
    let Some(text) = documents.text(id) else {
        return;
    };
    let mut chars: Vec<char> = text.chars().collect();
    let mut caret = cursors.get(id).min(chars.len());
    let mut text_changed = false;

    match key.input.key_code {
        KeyCode::Backspace => {
            if caret > 0 {
                chars.remove(caret - 1);
                caret -= 1;
                text_changed = true;
            }
        }
        KeyCode::Delete => {
            if caret < chars.len() {
                chars.remove(caret);
                text_changed = true;
            }
        }
        KeyCode::Enter | KeyCode::NumpadEnter => {
            chars.insert(caret, '\n');
            caret += 1;
            text_changed = true;
        }
        KeyCode::Tab => {
            for _ in 0..4 {
                chars.insert(caret, ' ');
                caret += 1;
            }
            text_changed = true;
        }
        KeyCode::ArrowLeft => caret = caret.saturating_sub(1),
        KeyCode::ArrowRight => caret = (caret + 1).min(chars.len()),
        KeyCode::Home => caret = line_start(&chars, caret),
        KeyCode::End => caret = line_end(&chars, caret),
        KeyCode::ArrowUp => caret = vertical(&chars, caret, false),
        KeyCode::ArrowDown => caret = vertical(&chars, caret, true),
        _ => {
            if let Key::Character(input) = &key.input.logical_key {
                for character in input.chars() {
                    chars.insert(caret, character);
                    caret += 1;
                }
                text_changed = true;
            } else if key.input.logical_key == Key::Space {
                chars.insert(caret, ' ');
                caret += 1;
                text_changed = true;
            } else {
                return;
            }
        }
    }

    if text_changed {
        documents.set_text(id, chars.into_iter().collect());
    }
    cursors.set(id, caret);
}

fn line_start(chars: &[char], caret: usize) -> usize {
    chars[..caret]
        .iter()
        .rposition(|&c| c == '\n')
        .map_or(0, |index| index + 1)
}

fn line_end(chars: &[char], caret: usize) -> usize {
    chars[caret..]
        .iter()
        .position(|&c| c == '\n')
        .map_or(chars.len(), |index| caret + index)
}

/// Moves the caret up (`down == false`) or down one visual line, keeping the column where possible.
fn vertical(chars: &[char], caret: usize, down: bool) -> usize {
    let start = line_start(chars, caret);
    let column = caret - start;
    if down {
        let end = line_end(chars, caret);
        if end >= chars.len() {
            return caret;
        }
        let next_start = end + 1;
        let next_end = line_end(chars, next_start);
        (next_start + column).min(next_end)
    } else {
        if start == 0 {
            return caret;
        }
        let prev_end = start - 1;
        let prev_start = line_start(chars, prev_end);
        (prev_start + column).min(prev_end)
    }
}

/// Re-renders the coloured runs (and caret) of each open code surface when its buffer, caret, or
/// focus changes — in place, so editing does not rebuild (and refocus) the pane.
pub(crate) fn refresh_wesl_editor_surfaces(
    documents: Res<WeslDocuments>,
    cursors: Res<WeslEditorCursors>,
    diagnostics: Res<WeslDiagnostics>,
    focus: Res<InputFocus>,
    surfaces: Query<(Entity, &WeslEditorSurface, Option<&Children>)>,
    mut commands: Commands,
) {
    if !documents.is_changed()
        && !cursors.is_changed()
        && !diagnostics.is_changed()
        && !focus.is_changed()
    {
        return;
    }
    for (entity, surface, children) in &surfaces {
        let Some(source) = documents.text(surface.0).map(str::to_owned) else {
            continue;
        };
        if let Some(children) = children {
            for &child in children {
                commands.entity(child).despawn();
            }
        }
        let focused = focus.get() == Some(entity);
        let caret = cursors.get(surface.0);
        let error_line = error_line_of(&diagnostics, surface.0);
        commands.entity(entity).with_children(|surface| {
            spawn_surface_children(surface, &source, caret, focused, error_line);
        });
    }
}

/// Writes a WESL buffer back to its `.wesl` file and clears its dirty state.
pub(crate) fn save_wesl_source(
    event: On<SaveWeslSource>,
    mut session: ResMut<EditorSession>,
    catalog: Res<ProjectEffectCatalog>,
    localizer: Res<Localizer>,
    mut documents: ResMut<WeslDocuments>,
) {
    let id = event.0;
    let (Some(relative), Some(text)) = (
        documents.relative_path(id).map(Path::to_path_buf),
        documents.text(id).map(str::to_owned),
    ) else {
        return;
    };
    let absolute = catalog.root().join(&relative);
    match std::fs::write(&absolute, text) {
        Ok(()) => {
            documents.mark_saved(id);
            session.status = localizer.text("wesl-editor-saved");
            session.ui_revision += 1;
        }
        Err(error) => {
            session.status = format!("Cannot save WESL source: {error}");
        }
    }
}

/// Marks a WESL pane's diagnostics footer so the compile result can be refreshed in place.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct WeslDiagnosticsFooter(pub(crate) WeslSourceId);

fn diagnostics_message(
    state: Option<&WeslCompileState>,
    localizer: &Localizer,
) -> Option<(String, Color)> {
    match state {
        Some(WeslCompileState::Error { message, .. }) => Some((message.clone(), ERROR_COLOR)),
        Some(WeslCompileState::Ok) => Some((
            localizer.text("wesl-editor-no-errors"),
            Color::srgb(0.35, 0.88, 0.57),
        )),
        None => None,
    }
}

fn spawn_diagnostics_footer(
    panel: &mut ChildSpawnerCommands,
    id: WeslSourceId,
    state: Option<&WeslCompileState>,
    localizer: &Localizer,
) {
    let message = diagnostics_message(state, localizer);
    panel
        .spawn((
            WeslDiagnosticsFooter(id),
            if message.is_some() {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            },
            Node {
                width: Val::Percent(100.0),
                max_height: Val::Px(96.0),
                padding: UiRect::all(Val::Px(8.0)),
                border: UiRect::top(Val::Px(1.0)),
                overflow: Overflow::scroll_y(),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(theme::PANEL_DARK),
            BorderColor::all(theme::BORDER),
        ))
        .with_child((
            Text::new(
                message
                    .as_ref()
                    .map(|(text, _)| text.clone())
                    .unwrap_or_default(),
            ),
            TextFont {
                font_size: FontSize::Px(10.0),
                ..default()
            },
            TextColor(message.map(|(_, color)| color).unwrap_or(theme::TEXT_MUTED)),
        ));
}

/// Refreshes each WESL pane's diagnostics footer in place when compile results change.
pub(crate) fn refresh_wesl_diagnostics(
    diagnostics: Res<WeslDiagnostics>,
    localizer: Res<Localizer>,
    mut footers: Query<(&WeslDiagnosticsFooter, &Children, &mut Visibility)>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
) {
    if !diagnostics.is_changed() {
        return;
    }
    for (footer, children, mut visibility) in &mut footers {
        let message = diagnostics_message(diagnostics.state(footer.0), &localizer);
        *visibility = if message.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let Some(&child) = children.first() else {
            continue;
        };
        if let Ok((mut text, mut color)) = texts.get_mut(child) {
            let (message, tint) = message.unwrap_or_else(|| (String::new(), theme::TEXT_MUTED));
            text.0 = message;
            color.0 = tint;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_runs_reproduce_the_source() {
        let source = "fn f() -> f32 { return 1.0; }";
        let rebuilt: String = editor_runs(source)
            .into_iter()
            .map(|(run, _)| run)
            .collect();
        assert_eq!(rebuilt, source);
    }

    #[test]
    fn caret_positions_round_trip_through_line_column_and_back() {
        let source = "abc\ndefg\nhi";
        // Column 2 of the middle line is character index 6.
        assert_eq!(line_col(source, 6), (1, 2));
        assert_eq!(char_index(source, 1, 2), 6);
        // A column past the line clamps to the line end.
        assert_eq!(char_index(source, 1, 99), 8); // end of "defg"
        assert_eq!(line_length(source, 1), 4);
    }

    #[test]
    fn vertical_navigation_keeps_the_column() {
        let chars: Vec<char> = "abc\ndefg\nhi".chars().collect();
        // Caret at column 2 of the middle line ("de|fg"), index 6.
        let up = vertical(&chars, 6, false);
        assert_eq!(chars[..up].iter().collect::<String>(), "ab"); // clamped to "abc" column 2
        let down = vertical(&chars, 6, true);
        assert_eq!(chars[..down].iter().collect::<String>(), "abc\ndefg\nhi"); // clamped to end "hi"
    }
}
