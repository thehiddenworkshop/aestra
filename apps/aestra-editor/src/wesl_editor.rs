//! WESL source editor pane (Milestone 7).
//!
//! Hosts the generic [`crate::feathers::code_editor`] widget for an open
//! [`crate::wesl_document::WeslDocuments`] buffer: the pane supplies the WESL syntax highlighter and
//! an error-underline marker, and syncs the widget's text back into the document store (which drives
//! dirty state, save, and diagnostics). The header carries the file name, dirty dot, and Save; the
//! footer shows the live compiler message.

use crate::feathers::code_editor::{
    CodeEditor, CodeEditorError, CodeEditorHighlighter, CodeEditorMarkers, spawn_code_editor,
    spawn_code_gutter,
};
use crate::wesl_document::{WeslCompileState, WeslDiagnostics, WeslDocuments, WeslSourceId};
use crate::wesl_syntax::{WeslTokenKind, tokenize};
use crate::*;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Marks the code-editor widget of a WESL pane so its text syncs to the right document (`source`)
/// and its caret to the right *view* (`view`). Two views of one module share the buffer but keep
/// independent carets, so the pair carries both ids.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct WeslEditorSurface {
    pub(crate) source: WeslSourceId,
    pub(crate) view: crate::docking::EditorViewId,
}

/// Fired to write a WESL buffer back to its module file.
#[derive(Event, Debug, Clone, Copy)]
pub(crate) struct SaveWeslSource(pub(crate) WeslSourceId);

/// Fired (by the diagnostics panel) to jump to a WESL compile error in a specific view: place the
/// caret at the failing character and scroll it into view.
#[derive(Event, Debug, Clone, Copy)]
pub(crate) struct RevealWeslError {
    pub(crate) view: crate::docking::EditorViewId,
    pub(crate) id: WeslSourceId,
    pub(crate) char: usize,
}

const ERROR_COLOR: Color = Color::srgb(1.0, 0.38, 0.32);

/// Per-view caret + selection anchor, kept so they survive the dock rebuilds that reconstruct the
/// pane (and its widget). Keyed by view, not document, so two views of one module scroll and place
/// their carets independently while sharing the buffer.
#[derive(Resource, Debug, Default)]
pub(crate) struct WeslEditorCursors(HashMap<crate::docking::EditorViewId, (usize, usize)>);

impl WeslEditorCursors {
    fn get(&self, view: crate::docking::EditorViewId) -> (usize, usize) {
        self.0.get(&view).copied().unwrap_or((0, 0))
    }

    fn set(&mut self, view: crate::docking::EditorViewId, cursor: usize, anchor: usize) {
        self.0.insert(view, (cursor, anchor));
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

/// The inline error marker for a document, from its last compile: the exact failing char range and
/// a short message shown after it.
fn error_marker(diagnostics: &WeslDiagnostics, id: WeslSourceId) -> CodeEditorMarkers {
    let error = match diagnostics.state(id) {
        Some(WeslCompileState::Error {
            message,
            span: Some(span),
            ..
        }) => Some(CodeEditorError {
            span: *span,
            message: concise_wesl_error(message),
        }),
        _ => None,
    };
    CodeEditorMarkers {
        error,
        error_color: ERROR_COLOR,
    }
}

/// Trims a verbose WESL compiler message down to the description shown inline in the editor: the
/// text after the `chars A..B:` location, first line only.
fn concise_wesl_error(message: &str) -> String {
    let description = message
        .rsplit_once("chars ")
        .and_then(|(_, rest)| rest.split_once(": "))
        .map(|(_, description)| description)
        .unwrap_or(message);
    description
        .lines()
        .next()
        .unwrap_or(description)
        .trim()
        .to_owned()
}

pub(crate) fn spawn_wesl_editor_view(
    parent: &mut ChildSpawnerCommands,
    id: WeslSourceId,
    view: crate::docking::EditorViewId,
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
                    // Saving is via Ctrl+S / File ▸ Save All / the Changes panel, so the editor
                    // header carries no Save button of its own.
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

            let (cursor, anchor) = cursors.get(view);
            let editor = CodeEditor::new(source).with_selection(cursor, anchor);
            // Code does not wrap, so it scrolls on both axes: a vertical scrollbar on the right and
            // a horizontal one below for long lines. The line-number gutter is a sibling of the
            // editor in the same scroll viewport, spawned after it so it draws on top.
            spawn_scroll_area_xy(
                panel,
                ScrollMemoryKey::WeslSource(view),
                Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                |body| {
                    let code = spawn_code_editor(
                        body,
                        editor,
                        wesl_highlighter(),
                        error_marker(diagnostics, id),
                        WeslEditorSurface {
                            source: id,
                            view,
                        },
                    );
                    spawn_code_gutter(body, code);
                },
            );
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
        documents.set_text(surface.source, editor.text.clone());
        cursors.set(surface.view, editor.cursor, editor.anchor);
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
        let next = error_marker(&diagnostics, surface.source);
        markers.set_if_neq(next);
    }
}

/// Jumps to a WESL compile error: sets the persisted caret (applied when the editor respawns from
/// the tab reveal) and, if the editor is already open, moves its live caret and scrolls the failing
/// line into view.
pub(crate) fn reveal_wesl_error(
    event: On<RevealWeslError>,
    mut cursors: ResMut<WeslEditorCursors>,
    mut editors: Query<(&WeslEditorSurface, &mut CodeEditor, &ChildOf)>,
    mut viewports: Query<(&ComputedNode, &mut bevy::ui::ScrollPosition), Without<CodeEditor>>,
) {
    let RevealWeslError { view, id, char } = *event;
    cursors.set(view, char, char);
    for (surface, mut editor, child_of) in &mut editors {
        if surface.view != view || surface.source != id {
            continue;
        }
        editor.set_caret(char);
        if let Ok((node, mut scroll)) = viewports.get_mut(child_of.parent()) {
            let viewport = node.size().y * node.inverse_scale_factor();
            let line = editor.line_of(char) as f32 * crate::feathers::code_editor::CODE_LINE_HEIGHT;
            // Keep the failing line a little below the top of the viewport.
            scroll.0.y = (line - viewport * 0.4).max(0.0);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docking::EditorViewId;

    #[test]
    fn carets_are_tracked_per_view_not_per_document() {
        // Two views of the same WESL module keep independent carets: writing one leaves the other's
        // untouched, and an unseen view falls back to the origin.
        let mut cursors = WeslEditorCursors::default();
        let left = EditorViewId(1);
        let right = EditorViewId(2);
        cursors.set(left, 40, 12);
        cursors.set(right, 3, 3);
        assert_eq!(cursors.get(left), (40, 12));
        assert_eq!(cursors.get(right), (3, 3));
        assert_eq!(cursors.get(EditorViewId(99)), (0, 0));
    }
}
