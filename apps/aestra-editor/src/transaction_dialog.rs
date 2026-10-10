//! Confirmation of structural edits without a separate Changes workspace.
use crate::*;
use aestra_authoring::ChangeKind;
use bevy::input_focus::{FocusCause, InputFocus, tab_navigation::TabGroup};
use bevy::ui_widgets::Activate;

pub(crate) struct EditorTransactionDialogPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TransactionDialogSet {
    Actions,
}

impl Plugin for EditorTransactionDialogPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DocumentProtectionState>()
            .add_observer(activate)
            .add_systems(
                Update,
                escape
                    .in_set(TransactionDialogSet::Actions)
                    .before(EditorSet::UiSync),
            )
            .add_systems(Update, sync.in_set(EditorSet::UiSync));
    }
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum Choice {
    Cancel,
    Confirm,
    Diagnostics,
}
#[derive(Component)]
struct Overlay;
#[derive(Component)]
struct Body;
#[derive(Component)]
struct Rendered(u64);

pub(crate) fn spawn(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Overlay,
            TabGroup::modal(),
            GlobalZIndex(315),
            Pickable {
                should_block_lower: true,
                is_hoverable: true,
            },
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.005, 0.007, 0.014, 0.82)),
        ))
        .with_children(|overlay| {
            overlay.spawn((
                Body,
                Node {
                    width: Val::Px(520.0),
                    max_width: Val::Percent(92.0),
                    max_height: Val::Percent(85.0),
                    padding: UiRect::all(Val::Px(20.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(12.0),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(7.0)),
                    ..default()
                },
                BackgroundColor(theme::PANEL),
                BorderColor::all(theme::BORDER_BRIGHT),
            ));
        });
}

fn activate(
    event: On<Activate>,
    choices: Query<(&Choice, Option<&InteractionDisabled>)>,
    mut session: ResMut<EditorSession>,
    mut protection: ResMut<DocumentProtectionState>,
    mut diagnostics: ResMut<DiagnosticsPanelState>,
    mut layout: ResMut<WorkspaceLayout>,
) {
    let Ok((choice, disabled)) = choices.get(event.entity) else {
        return;
    };
    if disabled.is_some() || session.pending_change.is_none() {
        return;
    }
    match choice {
        Choice::Confirm => {
            session.apply_pending_change();
        }
        Choice::Cancel => {
            session.discard_pending_change();
        }
        Choice::Diagnostics => {
            let pending = session.pending_change.as_ref().unwrap();
            let mut message = pending.preview.transaction().label.clone();
            for diagnostic in &pending.diagnostics.diagnostics {
                message.push_str(&format!(
                    "\n\n{:?} · {:?}\n{}\n{}",
                    diagnostic.severity, diagnostic.code, diagnostic.path, diagnostic.message
                ));
            }
            diagnostics.show_details(message);
            session.discard_pending_change();
            reveal_dock_panel(&mut layout, &mut session, ToolPanel::Diagnostics);
        }
    }
    protection.transaction_open = session.pending_change.is_some();
}

fn escape(
    keys: Res<ButtonInput<KeyCode>>,
    mut session: ResMut<EditorSession>,
    mut protection: ResMut<DocumentProtectionState>,
) {
    if protection.transaction_open && keys.just_pressed(KeyCode::Escape) {
        session.discard_pending_change();
        protection.transaction_open = false;
    }
}

#[allow(clippy::too_many_arguments)]
fn sync(
    mut commands: Commands,
    session: Res<EditorSession>,
    localizer: Res<Localizer>,
    mut protection: ResMut<DocumentProtectionState>,
    mut overlays: Query<&mut Node, With<Overlay>>,
    bodies: Query<(Entity, Option<&Rendered>), With<Body>>,
    mut focus: Option<ResMut<InputFocus>>,
    mut return_focus: Local<Option<Entity>>,
    mut was_open: Local<bool>,
) {
    let open = session.pending_change.is_some();
    protection.transaction_open = open;
    if open && !*was_open {
        *return_focus = focus.as_deref().and_then(|focus| focus.get());
    }
    if !open
        && *was_open
        && let Some(focus) = focus.as_deref_mut()
    {
        if let Some(entity) = return_focus
            .take()
            .filter(|entity| commands.get_entity(*entity).is_ok())
        {
            focus.set(entity, FocusCause::Navigated);
        } else {
            focus.clear();
        }
    }
    *was_open = open;
    for mut node in &mut overlays {
        node.display = if open { Display::Flex } else { Display::None };
    }
    if !open {
        for (body, rendered) in &bodies {
            if rendered.is_some() {
                commands
                    .entity(body)
                    .despawn_children()
                    .remove::<Rendered>();
            }
        }
        return;
    }
    let pending = session.pending_change.as_ref().unwrap();
    let deleting = pending
        .preview
        .diff()
        .changes
        .iter()
        .any(|change| change.kind == ChangeKind::Removed);
    for (body, rendered) in &bodies {
        if rendered.is_some_and(|rendered| rendered.0 == session.ui_revision) {
            continue;
        }
        commands
            .entity(body)
            .despawn_children()
            .insert(Rendered(session.ui_revision));
        commands.entity(body).with_children(|dialog| {
            label(dialog, pending.preview.transaction().label.clone(), 16.0);
            label(
                dialog,
                localizer.text(if pending.can_apply {
                    "transaction-confirm-description"
                } else {
                    "transaction-blocked-description"
                }),
                12.0,
            );
            spawn_vertical_scroll_area(
                dialog,
                ScrollMemoryKey::TransactionConfirmation,
                Node {
                    max_height: Val::Px(240.0),
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(4.0),
                    ..default()
                },
                |items| {
                    for change in &pending.preview.diff().changes {
                        if !deleting || change.kind == ChangeKind::Removed {
                            label(
                                items,
                                if deleting {
                                    change.before.clone().unwrap_or_else(|| change.path.clone())
                                } else {
                                    change.path.clone()
                                },
                                11.0,
                            );
                        }
                    }
                },
            );
            dialog
                .spawn(Node {
                    justify_content: JustifyContent::End,
                    column_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|buttons| {
                    let cancel = button(
                        buttons,
                        &localizer.text("transaction-cancel"),
                        Choice::Cancel,
                        false,
                    );
                    if !pending.diagnostics.diagnostics.is_empty() {
                        button(
                            buttons,
                            &localizer.text("transaction-diagnostics"),
                            Choice::Diagnostics,
                            false,
                        );
                    }
                    button(
                        buttons,
                        &localizer.text(if deleting {
                            "transaction-delete"
                        } else {
                            "transaction-confirm"
                        }),
                        Choice::Confirm,
                        !pending.can_apply,
                    );
                    if let Some(focus) = focus.as_mut() {
                        focus.set(cancel, FocusCause::Navigated);
                    }
                });
        });
    }
}

fn label(parent: &mut ChildSpawnerCommands, message: String, size: f32) {
    parent.spawn((
        Text::new(message),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(theme::TEXT),
        Pickable::IGNORE,
        Node {
            width: Val::Percent(100.0),
            flex_shrink: 0.0,
            ..default()
        },
    ));
}

fn button(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    choice: Choice,
    disabled: bool,
) -> Entity {
    let mut button = parent.spawn_empty();
    button.apply_scene(ui_shell::feathers_button()).insert((
        choice,
        TabIndex(0),
        AccessibleLabel(label.to_owned()),
        Node {
            min_height: Val::Px(28.0),
            padding: UiRect::horizontal(Val::Px(12.0)),
            align_items: AlignItems::Center,
            ..default()
        },
    ));
    if disabled {
        button.insert(InteractionDisabled);
    }
    button.with_children(|button| {
        button.spawn((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(12.0),
                ..default()
            },
            TextColor(theme::TEXT),
            ThemedText,
            Pickable::IGNORE,
        ));
    });
    button.id()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::asset::AssetPlugin::default(),
            bevy::scene::ScenePlugin,
            bevy::text::TextPlugin,
        ))
        .init_asset::<crate::feathers::icon::SvgFile>()
        .insert_resource(crate::test_support::session_with_timing_slack())
        .insert_resource(Localizer::new("en-US").unwrap())
        .init_resource::<DiagnosticsPanelState>()
        .init_resource::<WorkspaceLayout>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<InputFocus>()
        .add_plugins(EditorTransactionDialogPlugin);
        app.world_mut()
            .run_system_once(|mut commands: Commands| {
                commands.spawn_empty().with_children(spawn);
            })
            .unwrap();
        app
    }

    fn choose(app: &mut App, choice: Choice) {
        let entity = app
            .world_mut()
            .query::<(Entity, &Choice)>()
            .iter(app.world())
            .find_map(|(entity, actual)| (*actual == choice).then_some(entity))
            .unwrap();
        app.world_mut().trigger(Activate { entity });
        app.world_mut().flush();
        app.update();
    }

    fn delete_emitter(app: &mut App) -> aestra_core::EmitterId {
        let mut session = app.world_mut().resource_mut::<EditorSession>();
        let emitter = session.effect.emitters[1].id;
        assert!(session.preview_transaction(EffectTransaction::single(
            "Delete emitter",
            EffectCommand::RemoveEmitter { id: emitter }
        )));
        emitter
    }

    #[test]
    fn confirmed_deletion_is_undoable_and_does_not_require_a_changes_panel() {
        let mut app = app();
        let original = app.world().resource::<EditorSession>().effect.clone();
        let emitter = delete_emitter(&mut app);
        app.update();
        assert!(app.world().resource::<DocumentProtectionState>().is_open());
        assert_eq!(app.world().resource::<EditorSession>().effect, original);
        choose(&mut app, Choice::Confirm);
        let session = app.world().resource::<EditorSession>();
        assert!(session.pending_change.is_none());
        assert!(
            !session
                .effect
                .emitters
                .iter()
                .any(|item| item.id == emitter)
        );
        assert!(!app.world().resource::<DocumentProtectionState>().is_open());
        app.world_mut().resource_mut::<EditorSession>().undo();
        assert_eq!(app.world().resource::<EditorSession>().effect, original);
    }

    #[test]
    fn cancel_and_escape_preserve_the_document_and_restore_focus() {
        for use_escape in [false, true] {
            let mut app = app();
            let previous = app.world_mut().spawn_empty().id();
            app.world_mut()
                .resource_mut::<InputFocus>()
                .set(previous, FocusCause::Navigated);
            let original = app.world().resource::<EditorSession>().effect.clone();
            delete_emitter(&mut app);
            app.update();
            let focused = app.world().resource::<InputFocus>().get().unwrap();
            assert_eq!(app.world().get::<Choice>(focused), Some(&Choice::Cancel));
            if use_escape {
                app.world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(KeyCode::Escape);
                app.update();
            } else {
                choose(&mut app, Choice::Cancel);
            }
            assert_eq!(app.world().resource::<EditorSession>().effect, original);
            assert!(
                app.world()
                    .resource::<EditorSession>()
                    .pending_change
                    .is_none()
            );
            assert!(!app.world().resource::<DocumentProtectionState>().is_open());
            assert_eq!(app.world().resource::<InputFocus>().get(), Some(previous));
        }
    }

    #[test]
    fn blocked_deletion_cannot_apply_and_diagnostics_survive_dialog_close() {
        let mut app = app();
        let original = app.world().resource::<EditorSession>().effect.clone();
        {
            let mut session = app.world_mut().resource_mut::<EditorSession>();
            let emitter = &session.effect.emitters[0];
            let id = emitter.id;
            let module = emitter
                .module_by_type(aestra_core::MODULE_EMISSION)
                .unwrap()
                .id;
            assert!(session.preview_transaction(EffectTransaction::single(
                "Delete required module",
                EffectCommand::RemoveModule {
                    emitter: id,
                    module
                }
            )));
            assert!(!session.pending_change.as_ref().unwrap().can_apply);
        }
        app.update();
        let confirm = app
            .world_mut()
            .query::<(Entity, &Choice)>()
            .iter(app.world())
            .find_map(|(entity, choice)| (*choice == Choice::Confirm).then_some(entity))
            .unwrap();
        assert!(app.world().get::<InteractionDisabled>(confirm).is_some());
        choose(&mut app, Choice::Confirm);
        assert!(
            app.world()
                .resource::<EditorSession>()
                .pending_change
                .is_some()
        );
        choose(&mut app, Choice::Diagnostics);
        assert_eq!(app.world().resource::<EditorSession>().effect, original);
        assert!(
            app.world()
                .resource::<EditorSession>()
                .pending_change
                .is_none()
        );
        assert!(
            app.world()
                .resource::<WorkspaceLayout>()
                .is_active(ToolPanel::Diagnostics)
        );
        assert!(!app.world().resource::<DocumentProtectionState>().is_open());
        app.world_mut()
            .run_system_once(
                |mut commands: Commands,
                 session: Res<EditorSession>,
                 state: Res<DiagnosticsPanelState>,
                 localizer: Res<Localizer>| {
                    commands.spawn_empty().with_children(|parent| {
                        crate::diagnostics::spawn_diagnostics_workspace(
                            parent,
                            &session,
                            &ProjectEffectCatalog::from_entries(Vec::new()),
                            &state,
                            &crate::wesl_document::WeslDocuments::default(),
                            &crate::wesl_document::WeslDiagnostics::default(),
                            &localizer,
                        );
                    });
                },
            )
            .unwrap();
        assert!(
            app.world_mut()
                .query::<&Text>()
                .iter(app.world())
                .any(|text| text.0.contains("Delete required module") && text.0.contains("Error"))
        );
    }
}
