//! Read-only WESL source viewer pane (Milestone 7-1).
//!
//! Renders an open [`crate::wesl_document::WeslDocuments`] buffer as a scrollable, monochrome text
//! pane docked as its own editor tab. Editing input and tokenizer-based coloration arrive in later
//! Milestone 7 slices.

use crate::wesl_document::{WeslDocuments, WeslSourceId};
use crate::*;

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
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    header.spawn((
                        Text::new(localizer.text("wesl-editor-read-only")),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_FAINT),
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
