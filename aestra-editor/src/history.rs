//! Undo/redo actions, shortcuts, menu activation, and availability synchronization.

use crate::{
    EditorNativeControl, FeathersActionButton, ModulePaletteState, PendingFeathersActivation,
    menus::{MenuState, RedoMenuItem, UndoMenuItem},
    session::EditorSession,
    theme,
};
use bevy::{prelude::*, ui::InteractionDisabled, ui_widgets::Activate};

pub(crate) struct EditorHistoryPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum HistorySet {
    Input,
    Actions,
    Sync,
}

impl Plugin for EditorHistoryPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(queue_history_action_activation)
            .add_observer(execute_history_action)
            .add_systems(
                Update,
                (
                    history_keyboard_input.in_set(HistorySet::Input),
                    (handle_history_buttons, audit_history_controls)
                        .chain()
                        .in_set(HistorySet::Actions),
                    update_history_availability.in_set(HistorySet::Sync),
                ),
            );
    }
}

#[derive(Component, Event, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistoryAction {
    Undo,
    Redo,
}

fn queue_history_action_activation(
    activate: On<Activate>,
    actions: Query<(), (With<HistoryAction>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if actions.contains(activate.entity) {
        commands
            .entity(activate.entity)
            .insert((PendingFeathersActivation, Interaction::Pressed));
    }
}

#[allow(clippy::type_complexity)]
fn handle_history_buttons(
    mut commands: Commands,
    mut buttons: Query<
        (
            Entity,
            &Interaction,
            &HistoryAction,
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

fn execute_history_action(action: On<HistoryAction>, mut session: ResMut<EditorSession>) {
    match *action {
        HistoryAction::Undo => session.undo(),
        HistoryAction::Redo => session.redo(),
    }
}

fn history_keyboard_input(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    palette: Res<ModulePaletteState>,
) {
    if palette.open {
        return;
    }
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if control && keys.just_pressed(KeyCode::KeyZ) {
        commands.trigger(HistoryAction::Undo);
    }
    if control && keys.just_pressed(KeyCode::KeyY) {
        commands.trigger(HistoryAction::Redo);
    }
}

fn update_history_availability(
    session: Res<EditorSession>,
    mut commands: Commands,
    items: Query<
        (Entity, Has<UndoMenuItem>, Has<RedoMenuItem>),
        Or<(With<UndoMenuItem>, With<RedoMenuItem>)>,
    >,
) {
    if !session.is_changed() {
        return;
    }
    for (entity, undo, redo) in &items {
        let enabled = (undo && session.can_undo()) || (redo && session.can_redo());
        if enabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
}

type UnclassifiedHistoryControl = (
    Added<HistoryAction>,
    With<Button>,
    Without<FeathersActionButton>,
    Without<EditorNativeControl>,
);

fn audit_history_controls(controls: Query<Entity, UnclassifiedHistoryControl>) {
    #[cfg(debug_assertions)]
    if let Some(entity) = controls.iter().next() {
        panic!(
            "history control {entity:?} must use FeathersActionButton or be explicitly marked \
             EditorNativeControl"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EFFECT_PATH, EFFECT_SOURCE};

    fn edited_session() -> EditorSession {
        let mut session = EditorSession::from_embedded_sample(EFFECT_SOURCE, EFFECT_PATH);
        session.adjust_effect_duration(0.25);
        assert!(session.can_undo());
        session
    }

    #[test]
    fn history_actions_own_undo_and_redo() {
        let session = edited_session();
        let changed_duration = session.effect.duration;
        let mut app = App::new();
        app.insert_resource(session)
            .add_observer(execute_history_action);

        app.world_mut().trigger(HistoryAction::Undo);
        app.update();
        assert_ne!(
            app.world().resource::<EditorSession>().effect.duration,
            changed_duration
        );

        app.world_mut().trigger(HistoryAction::Redo);
        app.update();
        assert_eq!(
            app.world().resource::<EditorSession>().effect.duration,
            changed_duration
        );
    }

    #[test]
    fn keyboard_input_routes_through_the_history_action_contract() {
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::KeyZ);
        let changed_duration = edited_session().effect.duration;
        let mut app = App::new();
        app.insert_resource(edited_session())
            .insert_resource(keys)
            .init_resource::<ModulePaletteState>()
            .add_observer(execute_history_action)
            .add_systems(Update, history_keyboard_input);

        app.update();

        assert_ne!(
            app.world().resource::<EditorSession>().effect.duration,
            changed_duration
        );
    }

    #[test]
    fn availability_sync_does_not_disable_unrelated_ui() {
        let mut app = App::new();
        app.insert_resource(EditorSession::from_embedded_sample(
            EFFECT_SOURCE,
            EFFECT_PATH,
        ))
        .add_systems(Update, update_history_availability);
        let particle_color = Color::srgba(0.8, 0.4, 1.0, 0.75);
        let particle = app.world_mut().spawn(BackgroundColor(particle_color)).id();
        let undo = app
            .world_mut()
            .spawn((UndoMenuItem, BackgroundColor(theme::PANEL)))
            .id();

        app.update();

        let world = app.world();
        assert_eq!(
            world.get::<BackgroundColor>(particle).unwrap().0,
            particle_color
        );
        assert!(!world.entity(particle).contains::<InteractionDisabled>());
        assert!(world.entity(undo).contains::<InteractionDisabled>());
    }

    #[test]
    fn feathers_activation_queues_one_history_action() {
        let mut app = App::new();
        app.add_observer(queue_history_action_activation);
        let action = app
            .world_mut()
            .spawn((HistoryAction::Undo, FeathersActionButton, Interaction::None))
            .id();

        app.world_mut().trigger(Activate { entity: action });
        app.update();

        let action = app.world().entity(action);
        assert!(action.contains::<PendingFeathersActivation>());
        assert_eq!(action.get::<Interaction>(), Some(&Interaction::Pressed));
    }
}
