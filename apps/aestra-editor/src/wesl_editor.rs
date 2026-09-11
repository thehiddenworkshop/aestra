//! WESL source editor pane (Milestone 7-2).
//!
//! Renders an open [`crate::wesl_document::WeslDocuments`] buffer as an editable, scrollable text
//! pane docked as its own editor tab. Edits sync into the buffer per keystroke; Ctrl+S or the
//! header Save button write it back to the `.wesl` file. Tokenizer-based coloration and compiler
//! diagnostics arrive in a later Milestone 7 slice.

use crate::wesl_document::{WeslDocuments, WeslSourceId};
use crate::*;
use bevy::text::{EditableText, TextEditChange};
use std::path::Path;

/// Marks the editable text field of a WESL pane so edits route to the right buffer.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct WeslSourceEditorField(pub(crate) WeslSourceId);

/// Fired to write a WESL buffer back to its module file.
#[derive(Event, Debug, Clone, Copy)]
pub(crate) struct SaveWeslSource(pub(crate) WeslSourceId);

pub(crate) fn spawn_wesl_editor_view(
    parent: &mut ChildSpawnerCommands,
    id: WeslSourceId,
    documents: &WeslDocuments,
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

            let Some(source) = documents.text(id) else {
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
                    body.spawn((
                        Text::new(source.to_owned()),
                        EditableText::new(source),
                        WeslSourceEditorField(id),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(theme::TEXT),
                    ));
                },
            );
        });
}

/// Per-keystroke sync of an editable WESL field into its buffer. Deliberately does not bump the UI
/// revision, so typing does not rebuild (and refocus) the pane on every character.
pub(crate) fn sync_wesl_source_edit(
    change: On<TextEditChange>,
    fields: Query<(&WeslSourceEditorField, &EditableText)>,
    mut documents: ResMut<WeslDocuments>,
) {
    if let Ok((field, text)) = fields.get(change.event_target()) {
        documents.set_text(field.0, text.value().to_string());
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

    #[test]
    fn editing_then_saving_writes_the_file_and_clears_dirty() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("noise.wesl");
        std::fs::write(&path, "fn noise() -> f32 { return 0.0; }").unwrap();

        let mut app = App::new();
        app.insert_resource(crate::test_support::session_with_timing_slack())
            .insert_resource(ProjectEffectCatalog::scan(root.path()))
            .insert_resource(Localizer::new("en-US").unwrap())
            .init_resource::<WeslDocuments>()
            .add_observer(save_wesl_source);

        let id = app.world_mut().resource_mut::<WeslDocuments>().open(
            std::path::PathBuf::from("noise.wesl"),
            "fn noise() -> f32 { return 0.0; }".into(),
        );

        // Simulate a keystroke edit into the buffer.
        assert!(
            app.world_mut()
                .resource_mut::<WeslDocuments>()
                .set_text(id, "fn noise() -> f32 { return 1.0; }".into())
        );
        assert!(app.world().resource::<WeslDocuments>().is_dirty(id));

        app.world_mut().trigger(SaveWeslSource(id));
        app.world_mut().flush();

        // The file on disk now holds the edit, and the buffer is clean.
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "fn noise() -> f32 { return 1.0; }"
        );
        assert!(!app.world().resource::<WeslDocuments>().is_dirty(id));
    }
}
