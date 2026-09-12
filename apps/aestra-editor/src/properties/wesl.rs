//! Contextual Properties for the active WESL source document (Milestone 10).
//!
//! When the active editor tab is a WESL module, the Properties panel shows the source's file, its
//! derived module name, live compile status, and declared functions instead of the effect/material
//! inspector. The content re-renders in place (via [`refresh_wesl_properties`]) as the source
//! compiles, without a dock rebuild.

use crate::document::{DocumentKey, DocumentManager};
use crate::editor_view::ActiveEditorContext;
use crate::feathers::field_row::{FieldRowProps, spawn_field_row};
use crate::feathers::panel::spawn_panel_heading;
use crate::feathers::scroll::spawn_vertical_scroll_area;
use crate::wesl_document::{
    WeslCompileState, WeslDiagnostics, WeslDocuments, WeslSourceId, module_name_for,
};
use crate::wesl_syntax::declared_functions;
use crate::{Localizer, ScrollMemoryKey, theme};
use bevy::prelude::*;
use bevy::text::FontSize;

/// Marks the WESL Properties content wrapper (with its source id) so it can re-render in place.
#[derive(Component)]
pub(crate) struct WeslPropertiesContent(WeslSourceId);

/// Renders WESL source properties when the active editor document is a WESL module, returning
/// whether it rendered so the Properties panel can skip its effect/material view.
pub(super) fn spawn(
    parent: &mut ChildSpawnerCommands,
    active: &ActiveEditorContext,
    documents: &DocumentManager,
    wesl_documents: &WeslDocuments,
    wesl_diagnostics: &WeslDiagnostics,
    localizer: &Localizer,
) -> bool {
    let Some(id) = active_wesl(active, documents) else {
        return false;
    };
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_width: Val::Px(0.0),
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            WeslPropertiesContent(id),
        ))
        .with_children(|content| {
            render_content(content, id, wesl_documents, wesl_diagnostics, localizer);
        });
    true
}

/// Re-renders each WESL Properties panel in place when its source or diagnostics change, so status
/// and the function list stay live as the file is edited (editing does not rebuild the dock).
pub(crate) fn refresh_wesl_properties(
    wesl_documents: Res<WeslDocuments>,
    wesl_diagnostics: Res<WeslDiagnostics>,
    localizer: Res<Localizer>,
    panels: Query<(Entity, &WeslPropertiesContent, Option<&Children>)>,
    mut commands: Commands,
) {
    if panels.is_empty()
        || !(wesl_documents.is_changed() || wesl_diagnostics.is_changed() || localizer.is_changed())
    {
        return;
    }
    for (entity, content, children) in &panels {
        if let Some(children) = children {
            for &child in children {
                commands.entity(child).despawn();
            }
        }
        let id = content.0;
        commands.entity(entity).with_children(|content| {
            render_content(content, id, &wesl_documents, &wesl_diagnostics, &localizer);
        });
    }
}

fn render_content(
    parent: &mut ChildSpawnerCommands,
    id: WeslSourceId,
    wesl_documents: &WeslDocuments,
    wesl_diagnostics: &WeslDiagnostics,
    localizer: &Localizer,
) {
    let relative = wesl_documents.relative_path(id);
    let name = relative
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("{id}"));
    let path = relative
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let module = relative.map(module_name_for).unwrap_or_default();
    let functions = wesl_documents
        .text(id)
        .map(declared_functions)
        .unwrap_or_default();

    spawn_panel_heading(parent, &localizer.text("wesl-properties"), &name);
    spawn_vertical_scroll_area(
        parent,
        ScrollMemoryKey::Properties,
        Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        },
        |body| {
            body.spawn(Node {
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0,
                row_gap: Val::Px(2.0),
                padding: UiRect::all(Val::Px(8.0)),
                ..default()
            })
            .with_children(|body| {
                info_row(
                    body,
                    &localizer.text("wesl-properties-file"),
                    &path,
                    theme::TEXT,
                );
                info_row(
                    body,
                    &localizer.text("wesl-properties-module"),
                    &module,
                    theme::TEXT,
                );
                let (status, color) = compile_status(wesl_diagnostics.state(id), localizer);
                info_row(
                    body,
                    &localizer.text("wesl-properties-status"),
                    &status,
                    color,
                );

                section_heading(
                    body,
                    &format!(
                        "{}  ({})",
                        localizer.text("wesl-properties-functions"),
                        functions.len()
                    ),
                );
                if functions.is_empty() {
                    section_note(body, &localizer.text("wesl-properties-functions-none"));
                } else {
                    for function in functions {
                        function_row(body, &function);
                    }
                }
            });
        },
    );
}

fn active_wesl(active: &ActiveEditorContext, documents: &DocumentManager) -> Option<WeslSourceId> {
    match documents.document(active.active_document?)?.key {
        DocumentKey::WeslSource(id) => Some(id),
        _ => None,
    }
}

fn compile_status(state: Option<&WeslCompileState>, localizer: &Localizer) -> (String, Color) {
    match state {
        Some(WeslCompileState::Ok) => (
            localizer.text("wesl-properties-status-ok"),
            Color::srgb(0.35, 0.88, 0.57),
        ),
        Some(WeslCompileState::Error { .. }) => (
            localizer.text("wesl-properties-status-error"),
            Color::srgb(1.0, 0.38, 0.32),
        ),
        None => (
            localizer.text("wesl-properties-status-none"),
            theme::TEXT_MUTED,
        ),
    }
}

fn info_row(parent: &mut ChildSpawnerCommands, label: &str, value: &str, color: Color) {
    let value = value.to_owned();
    spawn_field_row(parent, FieldRowProps::new(label), (), move |controls| {
        controls.spawn((
            Text::new(value),
            TextFont {
                font_size: FontSize::Px(11.0),
                ..default()
            },
            TextColor(color),
            Pickable::IGNORE,
        ));
    });
}

fn section_heading(parent: &mut ChildSpawnerCommands, title: &str) {
    parent.spawn((
        Text::new(title.to_owned()),
        TextFont {
            font_size: FontSize::Px(9.0),
            ..default()
        },
        TextColor(theme::TEXT_FAINT),
        Node {
            margin: UiRect::new(Val::Px(0.0), Val::Px(0.0), Val::Px(10.0), Val::Px(4.0)),
            ..default()
        },
    ));
}

fn section_note(parent: &mut ChildSpawnerCommands, note: &str) {
    parent.spawn((
        Text::new(note.to_owned()),
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
        TextColor(theme::TEXT_MUTED),
        Node {
            padding: UiRect::left(Val::Px(4.0)),
            ..default()
        },
    ));
}

fn function_row(parent: &mut ChildSpawnerCommands, name: &str) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(20.0),
            align_items: AlignItems::Center,
            padding: UiRect::left(Val::Px(4.0)),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(name.to_owned()),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(Color::srgb(0.40, 0.82, 0.86)),
                Pickable::IGNORE,
            ));
        });
}
