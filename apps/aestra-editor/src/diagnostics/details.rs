//! Bounded summaries with lossless, on-demand operation/diagnostic details.

use super::*;
use bevy::ui_widgets::ScrollArea;

#[derive(Component)]
pub(crate) enum DetailsAction {
    Message(String),
    LatestStatus,
    Back,
}

/// Bound presentation only: the original message remains available in Details.
pub(crate) fn summary(message: &str, limit: usize) -> String {
    if limit == 0 {
        return String::new();
    }
    let normalized = message.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() <= limit {
        return normalized;
    }
    let mut result: String = normalized.chars().take(limit.saturating_sub(1)).collect();
    result.push('…');
    result
}

pub(crate) fn wrapped_text_node() -> Node {
    Node {
        width: Val::Percent(100.0),
        min_width: Val::Px(0.0),
        flex_shrink: 0.0,
        ..default()
    }
}

pub(crate) fn spawn_button(
    parent: &mut ChildSpawnerCommands,
    action: DetailsAction,
    localizer: &Localizer,
) {
    let label = localizer.text(if matches!(action, DetailsAction::Back) {
        "diagnostics-back"
    } else {
        "diagnostics-details"
    });
    parent
        .spawn_empty()
        .apply_scene(ui_shell::feathers_button())
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(label.clone()),
            Node {
                height: Val::Px(22.0),
                padding: UiRect::horizontal(Val::Px(8.0)),
                align_items: AlignItems::Center,
                align_self: AlignSelf::Start,
                flex_shrink: 0.0,
                ..default()
            },
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(label),
                TextLayout::no_wrap(),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(theme::TEXT),
                ThemedText,
                Pickable::IGNORE,
            ));
        });
}

pub(crate) fn spawn_summary(
    parent: &mut ChildSpawnerCommands,
    message: &str,
    localizer: &Localizer,
) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(4.0),
            margin: UiRect::vertical(Val::Px(6.0)),
            ..wrapped_text_node()
        })
        .with_children(|body| {
            body.spawn((
                Text::new(summary(message, 160)),
                TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(theme::TEXT),
                wrapped_text_node(),
            ));
            spawn_button(body, DetailsAction::Message(message.to_owned()), localizer);
        });
}

pub(super) fn activate_details(
    event: On<Activate>,
    actions: Query<&DetailsAction>,
    mut state: ResMut<DiagnosticsPanelState>,
    mut session: ResMut<EditorSession>,
    mut layout: ResMut<WorkspaceLayout>,
) {
    let Ok(action) = actions.get(event.entity) else {
        return;
    };
    let next = match action {
        DetailsAction::Message(message) => Some(message.clone()),
        DetailsAction::LatestStatus if !session.status.is_empty() => Some(session.status.clone()),
        DetailsAction::LatestStatus => return,
        DetailsAction::Back => None,
    };
    if state.details != next {
        state.details = next;
        session.ui_revision += 1;
    }
    reveal_dock_panel(&mut layout, &mut session, DockPanel::Diagnostics);
}

pub(super) fn sync_status_details(
    session: Res<EditorSession>,
    mut buttons: Query<(&DetailsAction, &mut Node)>,
) {
    for (action, mut node) in &mut buttons {
        if matches!(action, DetailsAction::LatestStatus) {
            let display = if session.status.is_empty() {
                Display::None
            } else {
                Display::Flex
            };
            if node.display != display {
                node.display = display;
            }
        }
    }
}

pub(super) fn spawn_workspace(
    parent: &mut ChildSpawnerCommands,
    message: &str,
    localizer: &Localizer,
) {
    parent
        .spawn(Node {
            height: Val::Percent(100.0),
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            overflow: Overflow::clip(),
            ..wrapped_text_node()
        })
        .with_children(|panel| {
            spawn_button(panel, DetailsAction::Back, localizer);
            panel
                .spawn(Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    min_height: Val::Px(0.0),
                    ..default()
                })
                .with_children(|body| {
                    let target = body
                        .spawn((
                            Node {
                                flex_basis: Val::Px(0.0),
                                flex_grow: 1.0,
                                min_width: Val::Px(0.0),
                                min_height: Val::Px(0.0),
                                overflow: Overflow::scroll_y(),
                                flex_direction: FlexDirection::Column,
                                scrollbar_width: 0.0,
                                padding: UiRect::all(Val::Px(12.0)),
                                ..default()
                            },
                            ScrollArea,
                        ))
                        .with_children(|viewport| {
                            viewport.spawn((
                                Text::new(message),
                                TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter),
                                TextFont {
                                    font_size: FontSize::Px(12.0),
                                    ..default()
                                },
                                TextColor(theme::TEXT),
                                wrapped_text_node(),
                            ));
                        })
                        .id();
                    crate::feathers::scroll::spawn_vertical_scrollbar(body, target);
                });
        });
}

#[cfg(test)]
mod tests;
