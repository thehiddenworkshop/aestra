//! Preview transport controls, shortcuts, playback advancement, and icon synchronization.

use crate::{
    feathers::{
        action_activation::{EditorNativeControl, FeathersActionButton, dispatch_action},
        icon::{SvgColor, UiSvg, load_svg_icon},
        scenes as ui_shell,
        tooltip::EditorTooltip,
    },
    localization::Localizer,
    menus::MenuState,
    session::EditorSession,
    theme,
};
use aestra_core::EffectPlaybackMode;
use bevy::ecs::query::QueryFilter;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use bevy::{
    feathers::{
        controls::ButtonVariant,
        theme::{ThemeBackgroundColor, ThemeBorderColor},
        tokens,
    },
    ui::Selected,
};

pub(crate) struct EditorTransportPlugin;

#[path = "transport/playback.rs"]
pub(crate) mod playback;
#[cfg(test)]
use playback::execute_transport_action;
pub(crate) use playback::{TransportAction, TransportSet};

impl Plugin for EditorTransportPlugin {
    fn build(&self, app: &mut App) {
        playback::register(app);
        app.add_observer(queue_transport_action_activation)
            .add_systems(
                Update,
                (
                    (handle_transport_buttons, audit_transport_controls)
                        .chain()
                        .in_set(TransportSet::Actions),
                    update_transport_icons.in_set(TransportSet::Sync),
                ),
            );
    }
}

#[derive(Component)]
struct PlaybackPlayIcon;

#[derive(Component)]
struct PlaybackPauseIcon;

#[derive(Component)]
struct PlaybackRestartLoopButton;

#[derive(Component)]
struct PlaybackContinuousLoopButton;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    None,
    Hovered,
    Pressed,
}

pub(crate) struct Control<'a> {
    pub(crate) entity: Entity,
    pub(crate) phase: Phase,
    pub(crate) action: TransportAction,
    pub(crate) feathers: bool,
    pub(crate) pending: bool,
    pub(crate) disabled: bool,
    pub(crate) background: Mut<'a, BackgroundColor>,
}

fn queue_transport_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<FeathersActionButton>, With<TransportAction>)>,
    commands: Commands,
) {
    crate::feathers::action_activation::queue_activation(activate, actions, commands);
}

fn handle_transport_buttons(
    mut commands: Commands,
    mut buttons: crate::transport_input::Controls,
    mut menu: ResMut<MenuState>,
    mut session: ResMut<EditorSession>,
) {
    for Control {
        entity,
        phase,
        action,
        feathers,
        pending,
        disabled,
        mut background,
    } in buttons.iter()
    {
        if disabled {
            if !feathers {
                background.0 = theme::PANEL_DARK;
            }
            continue;
        }
        // Activate is the action contract; a subsequent visual release must not discard it.
        let phase = if feathers && pending {
            Phase::Pressed
        } else {
            phase
        };
        match phase {
            Phase::Hovered if !feathers => background.0 = theme::BUTTON_HOVER,
            Phase::None if !feathers => background.0 = theme::BUTTON,
            Phase::Pressed => {
                if feathers && !pending {
                    continue;
                }
                if !feathers {
                    background.0 = theme::ACCENT_DIM;
                }
                menu.open = None;
                menu.panels_open = false;
                if menu.tab_context.take().is_some() {
                    session.ui_revision += 1;
                }
                dispatch_action(&mut commands, entity, action, feathers, pending, disabled);
            }
            _ => {}
        }
    }
}

type RestartLoopFilter = (
    With<PlaybackRestartLoopButton>,
    Without<PlaybackContinuousLoopButton>,
);
type ContinuousLoopFilter = (
    With<PlaybackContinuousLoopButton>,
    Without<PlaybackRestartLoopButton>,
);

fn update_transport_icons(
    mut commands: Commands,
    session: Res<EditorSession>,
    localizer: Res<Localizer>,
    mut play_icons: Query<&mut Node, (With<PlaybackPlayIcon>, Without<PlaybackPauseIcon>)>,
    mut pause_icons: Query<&mut Node, (With<PlaybackPauseIcon>, Without<PlaybackPlayIcon>)>,
    mut restart_buttons: Query<(Entity, &mut ButtonVariant, Has<Selected>), RestartLoopFilter>,
    mut continuous_buttons: Query<
        (Entity, &mut ButtonVariant, Has<Selected>),
        ContinuousLoopFilter,
    >,
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
    sync_playback_mode_button(
        &mut commands,
        &localizer,
        &mut restart_buttons,
        session.effect.playback_mode == EffectPlaybackMode::LoopRestart,
        "toolbar-loop-restart",
    );
    sync_playback_mode_button(
        &mut commands,
        &localizer,
        &mut continuous_buttons,
        session.effect.playback_mode == EffectPlaybackMode::LoopContinuous,
        "toolbar-loop-continuous",
    );
}

fn sync_playback_mode_button<F: QueryFilter>(
    commands: &mut Commands,
    localizer: &Localizer,
    buttons: &mut Query<(Entity, &mut ButtonVariant, Has<Selected>), F>,
    active: bool,
    message_id: &'static str,
) {
    let label = localizer.text(message_id);
    for (entity, mut variant, selected) in buttons {
        *variant = if active {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Normal
        };
        if active != selected {
            if active {
                commands.entity(entity).insert(Selected);
            } else {
                commands.entity(entity).remove::<Selected>();
            }
        }
        commands.entity(entity).insert((
            AccessibleLabel(label.clone()),
            EditorTooltip::description(label.clone()),
        ));
    }
}

type UnclassifiedTransportControl = (
    Added<TransportAction>,
    With<crate::transport_input::AuditedButton>,
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
            let mut loop_button = transport_button(
                transport,
                "toolbar-loop-restart",
                TransportAction::TogglePlaybackMode(EffectPlaybackMode::LoopRestart),
                localizer,
            );
            loop_button.insert(PlaybackRestartLoopButton);
            if session.effect.playback_mode == EffectPlaybackMode::LoopRestart {
                loop_button.insert((Selected, ButtonVariant::Primary));
            }
            loop_button.with_children(|button| {
                spawn_transport_icon(button, asset_server, "icons/loop.svg", 19.0, true);
            });
            let mut continuous_button = transport_button(
                transport,
                "toolbar-loop-continuous",
                TransportAction::TogglePlaybackMode(EffectPlaybackMode::LoopContinuous),
                localizer,
            );
            continuous_button.insert(PlaybackContinuousLoopButton);
            if session.effect.playback_mode == EffectPlaybackMode::LoopContinuous {
                continuous_button.insert((Selected, ButtonVariant::Primary));
            }
            continuous_button.with_children(|button| {
                spawn_transport_icon(button, asset_server, "icons/infinite.svg", 19.0, true);
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
    use crate::feathers::action_activation::PendingFeathersActivation;
    use crate::feathers::icon::SvgPlugin;
    use crate::test_support;
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

    fn sample_session_with_playback_mode(mode: EffectPlaybackMode) -> EditorSession {
        test_support::session_with_playback_mode(mode)
    }

    #[test]
    fn playback_mode_controls_are_exclusive_and_toggle_back_to_once() {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
            SvgPlugin,
        ))
        .init_asset::<Image>()
        .insert_resource(sample_session_with_playback_mode(
            EffectPlaybackMode::LoopRestart,
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
            .find(|(action, _, _, _, _)| {
                **action == TransportAction::TogglePlaybackMode(EffectPlaybackMode::LoopRestart)
            })
            .unwrap();
        assert_eq!(*variant, ButtonVariant::Primary);
        assert!(selected);
        assert_eq!(label.0, "Restart the effect at the end");
        assert!(tooltip);

        app.world_mut().trigger(TransportAction::TogglePlaybackMode(
            EffectPlaybackMode::LoopContinuous,
        ));
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
            .find(|(action, _, _, _)| {
                **action == TransportAction::TogglePlaybackMode(EffectPlaybackMode::LoopRestart)
            })
            .unwrap();
        assert_eq!(*variant, ButtonVariant::Normal);
        assert!(!selected);
        assert_eq!(label.0, "Restart the effect at the end");

        let (_, variant, selected, label) = query
            .iter(world)
            .find(|(action, _, _, _)| {
                **action == TransportAction::TogglePlaybackMode(EffectPlaybackMode::LoopContinuous)
            })
            .unwrap();
        assert_eq!(*variant, ButtonVariant::Primary);
        assert!(selected);
        assert_eq!(label.0, "Loop continuously without clearing particles");
    }

    #[test]
    fn feathers_activation_queues_one_transport_action() {
        let mut app = App::new();
        app.add_observer(queue_transport_action_activation);
        let action = app
            .world_mut()
            .spawn((TransportAction::Stop, FeathersActionButton))
            .id();
        crate::transport_input::set_phase(app.world_mut(), action, Phase::None);

        app.world_mut().trigger(Activate { entity: action });
        app.update();

        let action = app.world().entity(action);
        assert!(action.contains::<PendingFeathersActivation>());
        assert!(crate::transport_input::is_pressed(app.world(), action.id()));
    }
}

#[cfg(test)]
#[path = "transport/toolbar_tests.rs"]
mod toolbar_tests;
