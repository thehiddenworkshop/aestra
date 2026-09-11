//! WESL source editor pane (Milestone 7).
//!
//! Hosts the generic [`crate::feathers::code_editor`] widget for an open
//! [`crate::wesl_document::WeslDocuments`] buffer: the pane supplies the WESL syntax highlighter and
//! an error-underline marker, and syncs the widget's text back into the document store (which drives
//! dirty state, save, and diagnostics). The header carries the file name, dirty dot, and Save; the
//! footer shows the live compiler message.

use crate::feathers::code_editor::{
    CodeEditor, CodeEditorHighlighter, CodeEditorMarkers, spawn_code_editor,
};
use crate::wesl_document::{WeslCompileState, WeslDiagnostics, WeslDocuments, WeslSourceId};
use crate::wesl_syntax::{WeslTokenKind, tokenize};
use crate::*;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Marks the code-editor widget of a WESL pane so its text/caret sync to the right document.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct WeslEditorSurface(pub(crate) WeslSourceId);

/// Fired to write a WESL buffer back to its module file.
#[derive(Event, Debug, Clone, Copy)]
pub(crate) struct SaveWeslSource(pub(crate) WeslSourceId);

const ERROR_COLOR: Color = Color::srgb(1.0, 0.38, 0.32);

/// Per-document caret + selection anchor, kept so they survive the dock rebuilds that reconstruct
/// the pane (and its widget).
#[derive(Resource, Debug, Default)]
pub(crate) struct WeslEditorCursors(HashMap<WeslSourceId, (usize, usize)>);

impl WeslEditorCursors {
    fn get(&self, id: WeslSourceId) -> (usize, usize) {
        self.0.get(&id).copied().unwrap_or((0, 0))
    }

    fn set(&mut self, id: WeslSourceId, cursor: usize, anchor: usize) {
        self.0.insert(id, (cursor, anchor));
    }
}

/// The display colour for a WESL token class, tuned for the editor's dark panels.
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

/// The WESL syntax highlighter passed to the code editor widget.
fn wesl_highlighter() -> CodeEditorHighlighter {
    CodeEditorHighlighter(Arc::new(|source: &str| {
        tokenize(source)
            .into_iter()
            .map(|token| (token.text, token_color(token.kind)))
            .collect()
    }))
}

/// The zero-based error line marker for a document, from its last compile.
fn error_marker(diagnostics: &WeslDiagnostics, id: WeslSourceId) -> CodeEditorMarkers {
    let error_line = match diagnostics.state(id) {
        Some(WeslCompileState::Error { line, .. }) => line.map(|line| line.saturating_sub(1)),
        _ => None,
    };
    CodeEditorMarkers {
        error_line,
        error_color: ERROR_COLOR,
    }
}

pub(crate) fn spawn_wesl_editor_view(
    parent: &mut ChildSpawnerCommands,
    id: WeslSourceId,
    documents: &WeslDocuments,
    cursors: &WeslEditorCursors,
    diagnostics: &WeslDiagnostics,
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

            let (cursor, anchor) = cursors.get(id);
            let editor = CodeEditor::new(source).with_selection(cursor, anchor);
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
                    spawn_code_editor(
                        body,
                        editor,
                        wesl_highlighter(),
                        error_marker(diagnostics, id),
                        WeslEditorSurface(id),
                    );
                },
            );

            spawn_diagnostics_footer(panel, id, diagnostics.state(id), localizer);
        });
}

/// Syncs each WESL code-editor widget's edited text and caret back into the document store, so dirty
/// state, save, diagnostics, and the persisted caret follow the widget.
pub(crate) fn sync_wesl_editors(
    editors: Query<(&WeslEditorSurface, Ref<CodeEditor>)>,
    mut documents: ResMut<WeslDocuments>,
    mut cursors: ResMut<WeslEditorCursors>,
) {
    for (surface, editor) in &editors {
        if !editor.is_changed() {
            continue;
        }
        documents.set_text(surface.0, editor.text.clone());
        cursors.set(surface.0, editor.cursor, editor.anchor);
    }
}

/// Keeps each WESL widget's error-underline marker in step with its compile diagnostics.
pub(crate) fn sync_wesl_editor_markers(
    diagnostics: Res<WeslDiagnostics>,
    mut editors: Query<(&WeslEditorSurface, &mut CodeEditorMarkers)>,
) {
    if !diagnostics.is_changed() {
        return;
    }
    for (surface, mut markers) in &mut editors {
        let next = error_marker(&diagnostics, surface.0);
        if markers.error_line != next.error_line {
            markers.error_line = next.error_line;
        }
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
