//! Pending transaction review, semantic navigation, and Changes workspace actions.

use crate::*;
use aestra_authoring::{ChangeKind, SemanticTarget};
use aestra_core::DiagnosticSeverity;
use bevy::ui_widgets::Activate;
use fluent_bundle::FluentArgs;

pub(crate) struct EditorChangesPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ChangesSet {
    Actions,
}

impl Plugin for EditorChangesPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(queue_changes_action_activation)
            .add_systems(Update, handle_changes_actions.in_set(ChangesSet::Actions));
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum ChangesAction {
    Apply,
    Discard,
    Navigate(SemanticTarget),
}

fn queue_changes_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<ChangesAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands
            .entity(activate.entity)
            .insert((PendingFeathersActivation, Interaction::Pressed));
    }
}

#[allow(clippy::type_complexity)]
fn handle_changes_actions(
    mut commands: Commands,
    mut actions: Query<
        (
            Entity,
            &Interaction,
            &ChangesAction,
            Option<&FeathersActionButton>,
            Option<&PendingFeathersActivation>,
            &mut BackgroundColor,
        ),
        (
            Changed<Interaction>,
            Or<(With<Button>, With<FeathersActionButton>)>,
        ),
    >,
    mut session: ResMut<EditorSession>,
    mut layout: ResMut<WorkspaceLayout>,
    localizer: Res<Localizer>,
) {
    for (entity, interaction, action, feathers, pending, mut background) in &mut actions {
        match *interaction {
            Interaction::Hovered if feathers.is_none() => background.0 = theme::BUTTON_HOVER,
            Interaction::None if feathers.is_none() => background.0 = theme::PANEL,
            Interaction::Pressed => {
                if feathers.is_some() {
                    if pending.is_none() {
                        continue;
                    }
                    commands
                        .entity(entity)
                        .remove::<PendingFeathersActivation>()
                        .insert(Interaction::None);
                } else {
                    background.0 = theme::ACCENT_DIM;
                }
                match *action {
                    ChangesAction::Apply => {
                        session.apply_pending_change();
                    }
                    ChangesAction::Discard => {
                        session.discard_pending_change();
                    }
                    ChangesAction::Navigate(target) => {
                        if select_change_target(&mut session, target, &localizer) {
                            reveal_dock_panel(&mut layout, &mut session, DockPanel::Properties);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn select_change_target(
    session: &mut EditorSession,
    target: SemanticTarget,
    localizer: &Localizer,
) -> bool {
    let mut selection = session.selection;
    selection.primary = target;
    selection.repair(&session.effect);
    if selection.primary != target {
        session.status = localizer.text("changes-target-preview-only");
        return false;
    }
    if session.selection.primary != target {
        session.selection.primary = target;
        session.ui_revision += 1;
    }
    let mut args = FluentArgs::new();
    args.set("target", target.to_string());
    session.status = localizer.text_with("changes-selected-target", &args);
    true
}

pub(crate) fn spawn_changes_workspace(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
) {
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
                        height: Val::Px(38.0),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(Val::Px(14.0)),
                        column_gap: Val::Px(8.0),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL_LIGHT),
                ))
                .with_children(|header| {
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    let summary = session.pending_change.as_ref().map_or_else(
                        || localizer.text("changes-none-pending"),
                        |pending| {
                            let mut args = FluentArgs::new();
                            args.set(
                                "transaction",
                                pending.preview.transaction().label.to_uppercase(),
                            );
                            args.set("count", pending.preview.diff().changes.len());
                            localizer.text_with("changes-summary", &args)
                        },
                    );
                    header.spawn((
                        Text::new(summary),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(theme::TEXT_FAINT),
                    ));
                });

            let Some(pending) = &session.pending_change else {
                panel.spawn((
                    Text::new(localizer.text("changes-empty-description")),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_MUTED),
                    Node {
                        margin: UiRect::all(Val::Px(28.0)),
                        ..default()
                    },
                ));
                return;
            };

            panel
                .spawn(Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Row,
                    ..default()
                })
                .with_children(|body| {
                    body.spawn((
                        Node {
                            width: Val::Percent(66.0),
                            height: Val::Percent(100.0),
                            min_width: Val::Px(0.0),
                            border: UiRect::right(Val::Px(1.0)),
                            ..default()
                        },
                        BackgroundColor(theme::PANEL_DARK),
                        BorderColor::all(theme::BORDER),
                    ))
                    .with_children(|column| {
                        spawn_vertical_scroll_area(
                            column,
                            ScrollMemoryKey::ChangesList,
                            Node {
                                flex_grow: 1.0,
                                min_width: Val::Px(0.0),
                                min_height: Val::Px(0.0),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(Val::Px(8.0)),
                                row_gap: Val::Px(4.0),
                                ..default()
                            },
                            |changes| {
                                for change in &pending.preview.diff().changes {
                                    let (kind, color) = change_kind_style(change.kind, localizer);
                                    let values = match (&change.before, &change.after) {
                                        (Some(before), Some(after)) => {
                                            format!("{before}  →  {after}")
                                        }
                                        (Some(before), None) => before.clone(),
                                        (None, Some(after)) => after.clone(),
                                        (None, None) => String::new(),
                                    };
                                    changes
                                        .spawn((
                                            Button,
                                            ChangesAction::Navigate(change.target),
                                            AccessibleLabel(change.path.clone()),
                                            Node {
                                                width: Val::Percent(100.0),
                                                min_height: Val::Px(30.0),
                                                align_items: AlignItems::Center,
                                                padding: UiRect::horizontal(Val::Px(8.0)),
                                                column_gap: Val::Px(8.0),
                                                border_radius: BorderRadius::all(Val::Px(3.0)),
                                                ..default()
                                            },
                                            BackgroundColor(theme::PANEL),
                                        ))
                                        .with_children(|row| {
                                            row.spawn((
                                                Text::new(kind),
                                                TextFont {
                                                    font_size: FontSize::Px(9.0),
                                                    ..default()
                                                },
                                                TextColor(color),
                                                Node {
                                                    width: Val::Px(58.0),
                                                    ..default()
                                                },
                                                Pickable::IGNORE,
                                            ));
                                            row.spawn((
                                                Text::new(change.path.clone()),
                                                TextFont {
                                                    font_size: FontSize::Px(10.0),
                                                    ..default()
                                                },
                                                TextColor(theme::TEXT),
                                                Node {
                                                    width: Val::Percent(42.0),
                                                    ..default()
                                                },
                                                Pickable::IGNORE,
                                            ));
                                            row.spawn((
                                                Text::new(values),
                                                TextFont {
                                                    font_size: FontSize::Px(9.0),
                                                    ..default()
                                                },
                                                TextColor(theme::TEXT_MUTED),
                                                Pickable::IGNORE,
                                            ));
                                        });
                                }
                            },
                        );
                    });
                    body.spawn(Node {
                        flex_grow: 1.0,
                        height: Val::Percent(100.0),
                        min_width: Val::Px(0.0),
                        ..default()
                    })
                    .with_children(|column| {
                        spawn_vertical_scroll_area(
                            column,
                            ScrollMemoryKey::ChangesReview,
                            Node {
                                flex_grow: 1.0,
                                min_width: Val::Px(0.0),
                                min_height: Val::Px(0.0),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(Val::Px(10.0)),
                                row_gap: Val::Px(6.0),
                                ..default()
                            },
                            |review| {
                                let errors = pending
                                    .diagnostics
                                    .diagnostics
                                    .iter()
                                    .filter(|item| item.severity == DiagnosticSeverity::Error)
                                    .count();
                                review.spawn((
                                    Text::new(if pending.can_apply {
                                        localizer.text("changes-ready")
                                    } else {
                                        let mut args = FluentArgs::new();
                                        args.set("count", errors);
                                        localizer.text_with("changes-blocked", &args)
                                    }),
                                    TextFont {
                                        font_size: FontSize::Px(10.0),
                                        ..default()
                                    },
                                    TextColor(if pending.can_apply {
                                        Color::srgb(0.35, 0.88, 0.57)
                                    } else {
                                        Color::srgb(1.0, 0.38, 0.32)
                                    }),
                                ));
                                for diagnostic in &pending.diagnostics.diagnostics {
                                    review.spawn((
                                        Text::new(format!(
                                            "{:?} · {}\n{}",
                                            diagnostic.code, diagnostic.path, diagnostic.message
                                        )),
                                        TextFont {
                                            font_size: FontSize::Px(9.0),
                                            ..default()
                                        },
                                        TextColor(match diagnostic.severity {
                                            DiagnosticSeverity::Error => {
                                                Color::srgb(1.0, 0.38, 0.32)
                                            }
                                            DiagnosticSeverity::Warning => {
                                                Color::srgb(1.0, 0.74, 0.30)
                                            }
                                            DiagnosticSeverity::Info => theme::TEXT_MUTED,
                                        }),
                                    ));
                                }
                                review.spawn(Node {
                                    flex_grow: 1.0,
                                    ..default()
                                });
                                review
                                    .spawn(Node {
                                        width: Val::Percent(100.0),
                                        min_height: Val::Px(32.0),
                                        justify_content: JustifyContent::FlexEnd,
                                        column_gap: Val::Px(8.0),
                                        ..default()
                                    })
                                    .with_children(|actions| {
                                        properties_action_button(
                                            actions,
                                            &localizer.text("changes-discard"),
                                            ChangesAction::Discard,
                                            None,
                                        );
                                        properties_action_button(
                                            actions,
                                            &localizer.text(if pending.can_apply {
                                                "changes-apply"
                                            } else {
                                                "changes-apply-blocked"
                                            }),
                                            ChangesAction::Apply,
                                            None,
                                        );
                                    });
                            },
                        );
                    });
                });
        });
}

fn change_kind_style(kind: ChangeKind, localizer: &Localizer) -> (String, Color) {
    match kind {
        ChangeKind::Added => (
            localizer.text("changes-kind-added"),
            Color::srgb(0.35, 0.88, 0.57),
        ),
        ChangeKind::Removed => (
            localizer.text("changes-kind-removed"),
            Color::srgb(1.0, 0.38, 0.32),
        ),
        ChangeKind::Modified => (
            localizer.text("changes-kind-modified"),
            Color::srgb(0.45, 0.70, 1.0),
        ),
        ChangeKind::Moved => (
            localizer.text("changes-kind-moved"),
            Color::srgb(1.0, 0.74, 0.30),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;

    fn app_with_changes_action(action: ChangesAction) -> App {
        let mut app = App::new();
        app.insert_resource(test_support::session_with_timing_slack())
            .insert_resource(Localizer::new("en-US").unwrap())
            .init_resource::<WorkspaceLayout>()
            .add_systems(Update, handle_changes_actions);
        app.world_mut().spawn((
            Button,
            Interaction::Pressed,
            action,
            BackgroundColor(theme::PANEL),
        ));
        app
    }

    #[test]
    fn discard_action_clears_a_pending_transaction() {
        let mut app = app_with_changes_action(ChangesAction::Discard);
        app.world_mut()
            .resource_mut::<EditorSession>()
            .preview_transaction(EffectTransaction::new(
                "Rename effect",
                vec![EffectCommand::SetEffectName {
                    name: "Preview name".into(),
                }],
            ));

        app.update();

        assert!(
            app.world()
                .resource::<EditorSession>()
                .pending_change
                .is_none()
        );
        assert_ne!(
            app.world().resource::<EditorSession>().effect.name,
            "Preview name"
        );
    }

    #[test]
    fn apply_action_commits_a_pending_transaction() {
        let mut app = app_with_changes_action(ChangesAction::Apply);
        app.world_mut()
            .resource_mut::<EditorSession>()
            .preview_transaction(EffectTransaction::new(
                "Rename effect",
                vec![EffectCommand::SetEffectName {
                    name: "Applied name".into(),
                }],
            ));

        app.update();

        let session = app.world().resource::<EditorSession>();
        assert!(session.pending_change.is_none());
        assert_eq!(session.effect.name, "Applied name");
    }

    #[test]
    fn navigate_action_selects_a_live_semantic_target_and_opens_properties() {
        let emitter = test_support::session_with_timing_slack().effect.emitters[1].id;
        let mut app =
            app_with_changes_action(ChangesAction::Navigate(SemanticTarget::Emitter(emitter)));

        app.update();

        assert_eq!(
            app.world().resource::<EditorSession>().selection.primary,
            SemanticTarget::Emitter(emitter)
        );
    }
}
