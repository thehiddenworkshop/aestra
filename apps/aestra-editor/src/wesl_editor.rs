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
use bevy::text::TextSpan;
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
const EDITOR_FONT_SIZE: f32 = 12.0;

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

/// Builds the coloured text runs for `source`, inserting a caret run at `caret` when `focused`.
fn editor_runs(source: &str, caret: usize, focused: bool) -> Vec<(String, Color)> {
    let caret = caret.min(source.chars().count());
    let mut runs = Vec::new();
    let mut offset = 0usize;
    let mut caret_done = !focused;
    for token in tokenize(source) {
        let len = token.text.chars().count();
        if !caret_done && caret >= offset && caret <= offset + len {
            let split = caret - offset;
            let chars: Vec<char> = token.text.chars().collect();
            let before: String = chars[..split].iter().collect();
            let after: String = chars[split..].iter().collect();
            if !before.is_empty() {
                runs.push((before, token_color(token.kind)));
            }
            runs.push(("|".to_owned(), CARET_COLOR));
            if !after.is_empty() {
                runs.push((after, token_color(token.kind)));
            }
            caret_done = true;
        } else {
            runs.push((token.text, token_color(token.kind)));
        }
        offset += len;
    }
    if !caret_done {
        runs.push(("|".to_owned(), CARET_COLOR));
    }
    if runs.is_empty() {
        runs.push((String::new(), theme::TEXT));
    }
    runs
}

fn spawn_runs(text: &mut ChildSpawnerCommands, runs: Vec<(String, Color)>) {
    for (run, color) in runs {
        text.spawn((
            TextSpan::new(run),
            TextFont {
                font_size: FontSize::Px(EDITOR_FONT_SIZE),
                ..default()
            },
            TextColor(color),
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
                        Text::new(""),
                        TextFont {
                            font_size: FontSize::Px(EDITOR_FONT_SIZE),
                            ..default()
                        },
                        TextColor(theme::TEXT),
                        Node {
                            width: Val::Percent(100.0),
                            ..default()
                        },
                    ));
                    let focused = focus == Some(surface.id());
                    surface
                        .observe(focus_wesl_editor)
                        .observe(edit_wesl_source)
                        .with_children(|text| {
                            spawn_runs(text, editor_runs(&source, cursors.get(id), focused));
                        });
                },
            );

            spawn_diagnostics_footer(panel, id, diagnostics.state(id), localizer);
        });
}

/// Clicking the code surface focuses it so keyboard input is routed to this document.
fn focus_wesl_editor(
    mut click: On<Pointer<Click>>,
    surfaces: Query<Entity, With<WeslEditorSurface>>,
    mut focus: ResMut<InputFocus>,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    let Ok(entity) = surfaces.get(click.event_target()) else {
        return;
    };
    click.propagate(false);
    *focus = InputFocus::from_entity(entity);
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
    focus: Res<InputFocus>,
    surfaces: Query<(Entity, &WeslEditorSurface, Option<&Children>)>,
    mut commands: Commands,
) {
    if !documents.is_changed() && !cursors.is_changed() && !focus.is_changed() {
        return;
    }
    for (entity, surface, children) in &surfaces {
        let Some(source) = documents.text(surface.0) else {
            continue;
        };
        if let Some(children) = children {
            for &child in children {
                commands.entity(child).despawn();
            }
        }
        let focused = focus.get() == Some(entity);
        let runs = editor_runs(source, cursors.get(surface.0), focused);
        commands
            .entity(entity)
            .with_children(|text| spawn_runs(text, runs));
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
        Some(WeslCompileState::Error(message)) => {
            Some((message.clone(), Color::srgb(1.0, 0.38, 0.32)))
        }
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
    fn editor_runs_reproduce_the_source_and_place_a_caret() {
        let source = "fn f() -> f32 { return 1.0; }";
        let rebuilt: String = editor_runs(source, 3, true)
            .into_iter()
            .filter(|(run, _)| run != "|")
            .map(|(run, _)| run)
            .collect();
        assert_eq!(rebuilt, source);
        // With focus, exactly one caret run is present; without focus, none.
        assert_eq!(
            editor_runs(source, 3, true)
                .iter()
                .filter(|(run, _)| run == "|")
                .count(),
            1
        );
        assert_eq!(
            editor_runs(source, 3, false)
                .iter()
                .filter(|(run, _)| run == "|")
                .count(),
            0
        );
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
