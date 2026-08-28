//! Preview transport controls, shortcuts, playback advancement, and icon synchronization.

use crate::*;
use bevy::ui_widgets::Activate;

pub(crate) struct EditorTransportPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TransportSet {
    Input,
    Actions,
    Playback,
    Sync,
}

impl Plugin for EditorTransportPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(queue_transport_action_activation)
            .add_observer(execute_transport_action)
            .add_systems(
                Update,
                (
                    transport_keyboard_input.in_set(TransportSet::Input),
                    (handle_transport_buttons, audit_transport_controls)
                        .chain()
                        .in_set(TransportSet::Actions),
                    advance_playback.in_set(TransportSet::Playback),
                    update_transport_icons.in_set(TransportSet::Sync),
                ),
            );
    }
}

#[derive(Component, Event, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransportAction {
    TogglePlayback,
    Stop,
    Restart,
    StepFrame(i8),
    AdjustPreviewSeed(i8),
}

#[derive(Component)]
struct PlaybackPlayIcon;

#[derive(Component)]
struct PlaybackPauseIcon;

fn queue_transport_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<TransportAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands
            .entity(activate.entity)
            .insert((PendingFeathersActivation, Interaction::Pressed));
    }
}

#[allow(clippy::type_complexity)]
fn handle_transport_buttons(
    mut commands: Commands,
    mut buttons: Query<
        (
            Entity,
            &Interaction,
            &TransportAction,
            Option<&FeathersActionButton>,
            Option<&PendingFeathersActivation>,
            Option<&InteractionDisabled>,
            &mut BackgroundColor,
        ),
        (
            Changed<Interaction>,
            Or<(With<Button>, With<FeathersActionButton>)>,
        ),
    >,
    mut menu: ResMut<MenuState>,
    mut session: ResMut<EditorSession>,
) {
    for (entity, interaction, action, feathers, pending, disabled, mut background) in &mut buttons {
        if disabled.is_some() {
            if feathers.is_none() {
                background.0 = theme::PANEL_DARK;
            }
            continue;
        }
        match *interaction {
            Interaction::Hovered if feathers.is_none() => background.0 = theme::BUTTON_HOVER,
            Interaction::None if feathers.is_none() => background.0 = theme::BUTTON,
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
                menu.open = None;
                menu.panels_open = false;
                if menu.tab_context.take().is_some() {
                    session.ui_revision += 1;
                }
                commands.trigger(*action);
            }
            _ => {}
        }
    }
}

fn execute_transport_action(action: On<TransportAction>, mut session: ResMut<EditorSession>) {
    match *action {
        TransportAction::TogglePlayback => session.playing = !session.playing,
        TransportAction::Stop => session.stop(),
        TransportAction::Restart => session.restart(),
        TransportAction::StepFrame(direction) => session.step_frame(direction),
        TransportAction::AdjustPreviewSeed(direction) => {
            session.adjust_preview_seed(direction);
        }
    }
}

fn transport_keyboard_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    palette: Res<ModulePaletteState>,
) {
    if palette.open {
        return;
    }
    if keys.just_pressed(KeyCode::Space) {
        commands.trigger(TransportAction::TogglePlayback);
    }
    if keys.just_pressed(KeyCode::KeyR) {
        commands.trigger(TransportAction::Restart);
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        commands.trigger(TransportAction::StepFrame(-1));
    }
    if keys.just_pressed(KeyCode::ArrowRight) {
        commands.trigger(TransportAction::StepFrame(1));
    }
}

fn advance_playback(time: Res<Time>, mut session: ResMut<EditorSession>) {
    session.advance_playback(time.delta_secs());
}

fn update_transport_icons(
    session: Res<EditorSession>,
    mut play_icons: Query<&mut Node, (With<PlaybackPlayIcon>, Without<PlaybackPauseIcon>)>,
    mut pause_icons: Query<&mut Node, (With<PlaybackPauseIcon>, Without<PlaybackPlayIcon>)>,
) {
    if !session.is_changed() {
        return;
    }
    for mut node in &mut play_icons {
        node.display = if session.playing {
            Display::None
        } else {
            Display::Flex
        };
    }
    for mut node in &mut pause_icons {
        node.display = if session.playing {
            Display::Flex
        } else {
            Display::None
        };
    }
}

type UnclassifiedTransportControl = (
    Added<TransportAction>,
    With<Button>,
    Without<FeathersActionButton>,
    Without<EditorNativeControl>,
);

fn audit_transport_controls(controls: Query<Entity, UnclassifiedTransportControl>) {
    #[cfg(debug_assertions)]
    if let Some(entity) = controls.iter().next() {
        panic!(
            "transport control {entity:?} must use FeathersActionButton or be explicitly marked \
             EditorNativeControl"
        );
    }
}

pub(crate) fn spawn_transport_controls(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
) {
    parent
        .spawn((
            Node {
                height: Val::Px(34.0),
                padding: UiRect::all(Val::Px(2.0)),
                column_gap: Val::Px(2.0),
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(5.0)),
                ..default()
            },
            ThemeBackgroundColor(tokens::PANE_HEADER_BG),
            ThemeBorderColor(tokens::PANE_HEADER_BORDER),
        ))
        .with_children(|transport| {
            transport_button(
                transport,
                "toolbar-play",
                TransportAction::TogglePlayback,
                localizer,
            )
            .with_children(|button| {
                spawn_play_icon(button, !session.playing);
                spawn_pause_icon(button, session.playing);
            });
            transport_button(transport, "toolbar-stop", TransportAction::Stop, localizer)
                .with_child((
                    Node {
                        width: Val::Px(10.0),
                        height: Val::Px(10.0),
                        border_radius: BorderRadius::all(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(theme::TEXT),
                    Pickable::IGNORE,
                ));
        });
}

fn transport_button<'a>(
    parent: &'a mut ChildSpawnerCommands,
    message_id: &'static str,
    action: TransportAction,
    localizer: &Localizer,
) -> EntityCommands<'a> {
    let mut button = parent.spawn_empty();
    button
        .apply_scene(ui_shell::feathers_tool_button())
        .insert((
            action,
            FeathersActionButton,
            AccessibleLabel(localizer.text(message_id)),
            Node {
                width: Val::Px(28.0),
                height: Val::Px(28.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
        ));
    button
}

fn spawn_play_icon(parent: &mut ChildSpawnerCommands, visible: bool) {
    parent
        .spawn((
            PlaybackPlayIcon,
            Node {
                display: if visible {
                    Display::Flex
                } else {
                    Display::None
                },
                width: Val::Px(8.0),
                height: Val::Px(14.0),
                position_type: PositionType::Relative,
                overflow: Overflow::clip(),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_child((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(-5.0),
                top: Val::Px(2.0),
                width: Val::Px(10.0),
                height: Val::Px(10.0),
                border_radius: BorderRadius::all(Val::Px(1.0)),
                ..default()
            },
            UiTransform::from_rotation(Rot2::radians(std::f32::consts::FRAC_PI_4)),
            BackgroundColor(theme::TEXT),
            Pickable::IGNORE,
        ));
}

fn spawn_pause_icon(parent: &mut ChildSpawnerCommands, visible: bool) {
    parent
        .spawn((
            PlaybackPauseIcon,
            Node {
                display: if visible {
                    Display::Flex
                } else {
                    Display::None
                },
                width: Val::Px(11.0),
                height: Val::Px(13.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                column_gap: Val::Px(3.0),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|icon| {
            for _ in 0..2 {
                icon.spawn((
                    Node {
                        width: Val::Px(3.0),
                        height: Val::Px(12.0),
                        border_radius: BorderRadius::all(Val::Px(0.5)),
                        ..default()
                    },
                    BackgroundColor(theme::TEXT),
                    Pickable::IGNORE,
                ));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_actions_own_playback_mutations() {
        let mut app = App::new();
        let session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        assert!(session.playing);
        app.insert_resource(session)
            .add_observer(execute_transport_action);

        app.world_mut().trigger(TransportAction::TogglePlayback);
        app.update();
        assert!(!app.world().resource::<EditorSession>().playing);

        app.world_mut().trigger(TransportAction::Restart);
        app.update();
        let session = app.world().resource::<EditorSession>();
        assert!(session.playing);
        assert_eq!(session.frame(), 0);
    }

    #[test]
    fn feathers_activation_queues_one_transport_action() {
        let mut app = App::new();
        app.add_observer(queue_transport_action_activation);
        let action = app
            .world_mut()
            .spawn((
                TransportAction::Stop,
                FeathersActionButton,
                Interaction::None,
            ))
            .id();

        app.world_mut().trigger(Activate { entity: action });
        app.update();

        let action = app.world().entity(action);
        assert!(action.contains::<PendingFeathersActivation>());
        assert_eq!(action.get::<Interaction>(), Some(&Interaction::Pressed));
    }

    #[test]
    fn keyboard_input_routes_through_the_transport_action_contract() {
        let mut app = App::new();
        app.insert_resource(EditorSession::from_embedded_sample(
            EFFECT_SOURCE,
            EFFECT_PATH,
        ))
        .insert_resource(ButtonInput::<KeyCode>::default())
        .init_resource::<ModulePaletteState>()
        .add_observer(execute_transport_action)
        .add_systems(Update, transport_keyboard_input);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);

        app.update();

        assert!(!app.world().resource::<EditorSession>().playing);
    }
}
