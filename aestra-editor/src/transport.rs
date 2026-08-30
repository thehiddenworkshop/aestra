//! Preview transport controls, shortcuts, playback advancement, and icon synchronization.

use crate::feathers::icon::load_svg_icon;
use crate::*;
use bevy::ui_widgets::Activate;
use bevy::{
    feathers::controls::ButtonVariant,
    input::{ButtonState, keyboard::KeyboardInput},
    ui::Selected,
};
use bevy_resvg::prelude::{SvgColor, UiSvg};

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
    ToggleLooping,
    Restart,
    StepFrame(i8),
    AdjustPreviewSeed(i8),
}

#[derive(Component)]
struct PlaybackPlayIcon;

#[derive(Component)]
struct PlaybackPauseIcon;

#[derive(Component)]
struct PlaybackLoopButton;

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
        TransportAction::TogglePlayback => {
            if session.playing {
                session.playing = false;
            } else if session.effect.looping {
                session.playing = true;
            } else {
                session.restart();
            }
        }
        TransportAction::Stop => session.stop(),
        TransportAction::ToggleLooping => {
            let looping = !session.effect.looping;
            session.set_effect_looping(looping);
        }
        TransportAction::Restart => session.restart(),
        TransportAction::StepFrame(direction) => session.step_frame(direction),
        TransportAction::AdjustPreviewSeed(direction) => {
            session.adjust_preview_seed(direction);
        }
    }
}

fn transport_keyboard_input(
    mut commands: Commands,
    mut keyboard_events: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    palette: Res<ModulePaletteState>,
) {
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    let repeated_steps = keyboard_events
        .read()
        .filter_map(|event| repeated_frame_step(event, alt))
        .collect::<Vec<_>>();
    if palette.open {
        return;
    }
    if keys.just_pressed(KeyCode::Space) {
        commands.trigger(TransportAction::TogglePlayback);
    }
    if keys.just_pressed(KeyCode::KeyR) {
        commands.trigger(TransportAction::Restart);
    }
    if !alt && keys.just_pressed(KeyCode::ArrowLeft) {
        commands.trigger(TransportAction::StepFrame(-1));
    }
    if !alt && keys.just_pressed(KeyCode::ArrowRight) {
        commands.trigger(TransportAction::StepFrame(1));
    }
    for direction in repeated_steps {
        commands.trigger(TransportAction::StepFrame(direction));
    }
}

fn repeated_frame_step(event: &KeyboardInput, alt: bool) -> Option<i8> {
    if alt || !event.repeat || event.state != ButtonState::Pressed {
        return None;
    }
    match event.key_code {
        KeyCode::ArrowLeft => Some(-1),
        KeyCode::ArrowRight => Some(1),
        _ => None,
    }
}

fn advance_playback(time: Res<Time>, mut session: ResMut<EditorSession>) {
    session.advance_playback(time.delta_secs());
}

fn update_transport_icons(
    mut commands: Commands,
    session: Res<EditorSession>,
    localizer: Res<Localizer>,
    mut play_icons: Query<&mut Node, (With<PlaybackPlayIcon>, Without<PlaybackPauseIcon>)>,
    mut pause_icons: Query<&mut Node, (With<PlaybackPauseIcon>, Without<PlaybackPlayIcon>)>,
    mut loop_buttons: Query<(Entity, &mut ButtonVariant, Has<Selected>), With<PlaybackLoopButton>>,
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
    let looping = session.effect.looping;
    let loop_label = localizer.text(if looping {
        "toolbar-loop-disable"
    } else {
        "toolbar-loop-enable"
    });
    for (entity, mut variant, selected) in &mut loop_buttons {
        *variant = if looping {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Normal
        };
        if looping != selected {
            if looping {
                commands.entity(entity).insert(Selected);
            } else {
                commands.entity(entity).remove::<Selected>();
            }
        }
        commands.entity(entity).insert((
            AccessibleLabel(loop_label.clone()),
            EditorTooltip::description(loop_label.clone()),
        ));
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
    asset_server: &AssetServer,
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
                spawn_transport_icon(
                    button,
                    asset_server,
                    "icons/play.svg",
                    14.0,
                    !session.playing,
                )
                .insert(PlaybackPlayIcon);
                spawn_transport_icon(
                    button,
                    asset_server,
                    "icons/pause.svg",
                    14.0,
                    session.playing,
                )
                .insert(PlaybackPauseIcon);
            });
            transport_button(transport, "toolbar-stop", TransportAction::Stop, localizer)
                .with_children(|button| {
                    spawn_transport_icon(button, asset_server, "icons/stop.svg", 13.0, true);
                });
            let loop_message = if session.effect.looping {
                "toolbar-loop-disable"
            } else {
                "toolbar-loop-enable"
            };
            let mut loop_button = transport_button(
                transport,
                loop_message,
                TransportAction::ToggleLooping,
                localizer,
            );
            loop_button.insert(PlaybackLoopButton);
            if session.effect.looping {
                loop_button.insert((Selected, ButtonVariant::Primary));
            }
            loop_button.with_children(|button| {
                spawn_transport_icon(button, asset_server, "icons/loop.svg", 19.0, true);
            });
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
            EditorTooltip::description(localizer.text(message_id)),
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

fn spawn_transport_icon<'a>(
    parent: &'a mut ChildSpawnerCommands,
    asset_server: &AssetServer,
    path: &'static str,
    size: f32,
    visible: bool,
) -> EntityCommands<'a> {
    let icon = load_svg_icon(asset_server, path);
    parent.spawn((
        Node {
            display: if visible {
                Display::Flex
            } else {
                Display::None
            },
            width: Val::Px(size),
            height: Val::Px(size),
            ..default()
        },
        UiSvg(icon),
        SvgColor(theme::TEXT),
        Pickable::IGNORE,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{asset::AssetPlugin, scene::ScenePlugin, text::TextPlugin};

    fn spawn_test_transport(
        mut commands: Commands,
        asset_server: Res<AssetServer>,
        session: Res<EditorSession>,
        localizer: Res<Localizer>,
    ) {
        commands.spawn(Node::default()).with_children(|parent| {
            spawn_transport_controls(parent, &session, &localizer, &asset_server);
        });
    }

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

        let original_looping = session.effect.looping;
        app.world_mut().trigger(TransportAction::ToggleLooping);
        app.update();
        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect.looping, !original_looping);
        assert!(session.can_undo());
        app.world_mut().resource_mut::<EditorSession>().undo();
        assert_eq!(
            app.world().resource::<EditorSession>().effect.looping,
            original_looping
        );

        {
            let mut session = app.world_mut().resource_mut::<EditorSession>();
            assert!(session.set_effect_looping(false));
            session.step_frame(1);
            assert_eq!(session.frame(), 1);
            assert!(!session.playing);
        }
        app.world_mut().trigger(TransportAction::TogglePlayback);
        app.update();
        let session = app.world().resource::<EditorSession>();
        assert!(session.playing);
        assert_eq!(session.frame(), 0);
    }

    #[test]
    fn changing_looping_preserves_playback_position_and_running_state() {
        let mut app = App::new();
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        session.advance_playback(0.1);
        let frame = session.frame();
        assert!(frame > 0);
        assert!(session.playing);
        app.insert_resource(session)
            .add_observer(execute_transport_action);

        app.world_mut().trigger(TransportAction::ToggleLooping);
        app.update();

        let session = app.world().resource::<EditorSession>();
        assert!(!session.effect.looping);
        assert!(session.playing);
        assert_eq!(session.frame(), frame);
    }

    #[test]
    fn enabled_loop_control_has_persistent_active_styling_and_metadata() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
            SvgPlugin,
        ))
        .init_asset::<Image>()
        .insert_resource(EditorSession::from_embedded_sample(
            EFFECT_SOURCE,
            EFFECT_PATH,
        ))
        .insert_resource(Localizer::new("en-US").unwrap())
        .add_observer(execute_transport_action)
        .add_systems(Startup, spawn_test_transport)
        .add_systems(Update, update_transport_icons);

        app.update();

        let world = app.world_mut();
        let mut query = world.query::<(
            &TransportAction,
            &ButtonVariant,
            Has<Selected>,
            &AccessibleLabel,
            Has<EditorTooltip>,
        )>();
        let (_, variant, selected, label, tooltip) = query
            .iter(world)
            .find(|(action, _, _, _, _)| **action == TransportAction::ToggleLooping)
            .unwrap();
        assert_eq!(*variant, ButtonVariant::Primary);
        assert!(selected);
        assert_eq!(label.0, "Disable loop playback");
        assert!(tooltip);

        app.world_mut().trigger(TransportAction::ToggleLooping);
        app.update();

        let world = app.world_mut();
        let mut query = world.query::<(
            &TransportAction,
            &ButtonVariant,
            Has<Selected>,
            &AccessibleLabel,
        )>();
        let (_, variant, selected, label) = query
            .iter(world)
            .find(|(action, _, _, _)| **action == TransportAction::ToggleLooping)
            .unwrap();
        assert_eq!(*variant, ButtonVariant::Normal);
        assert!(!selected);
        assert_eq!(label.0, "Enable loop playback");
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
        .add_message::<KeyboardInput>()
        .add_observer(execute_transport_action)
        .add_systems(Update, transport_keyboard_input);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Space);

        app.update();

        assert!(!app.world().resource::<EditorSession>().playing);
    }

    #[test]
    fn held_arrow_repeat_events_continue_stepping_frames() {
        let repeat = KeyboardInput {
            key_code: KeyCode::ArrowRight,
            logical_key: bevy::input::keyboard::Key::ArrowRight,
            state: ButtonState::Pressed,
            text: None,
            repeat: true,
            window: Entity::PLACEHOLDER,
        };

        assert_eq!(repeated_frame_step(&repeat, false), Some(1));
        assert_eq!(repeated_frame_step(&repeat, true), None);

        let mut released = repeat.clone();
        released.state = ButtonState::Released;
        assert_eq!(repeated_frame_step(&released, false), None);

        let mut ordinary = repeat;
        ordinary.repeat = false;
        assert_eq!(repeated_frame_step(&ordinary, false), None);
    }
}
