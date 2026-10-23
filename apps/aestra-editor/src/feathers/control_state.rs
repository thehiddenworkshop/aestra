//! Transitional read-only state for application consumers not yet using native marker queries.
//! Picking and widgets own input/activation. This bridge never emits events or semantic actions.
use crate::feathers::action_activation::{FeathersActionButton, PendingFeathersActivation};
use bevy::{
    picking::hover::Hovered,
    prelude::*,
    ui::Pressed,
    ui_widgets::{Button, MenuButton, MenuItem},
};

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ControlInteraction {
    Pressed,
    Hovered,
    #[default]
    None,
}

pub(super) fn register(app: &mut App) {
    app.register_required_components::<Button, ControlInteraction>()
        .register_required_components::<MenuButton, ControlInteraction>()
        .register_required_components::<MenuItem, ControlInteraction>()
        .register_required_components::<FeathersActionButton, ControlInteraction>()
        .add_systems(Update, sync.in_set(super::AestraFeathersSet::Input));
}

fn sync(
    mut controls: Query<(
        Option<&Pressed>,
        Option<&Hovered>,
        Has<PendingFeathersActivation>,
        &mut ControlInteraction,
    )>,
) {
    for (pressed, hovered, pending, mut state) in &mut controls {
        // Activate may arrive after release (including keyboard activation). Keep the pending
        // action visible until its existing consumer takes it, rather than losing it to hover.
        let next = if pending || pressed.is_some() {
            ControlInteraction::Pressed
        } else if hovered.is_some_and(|hovered| hovered.0) {
            ControlInteraction::Hovered
        } else {
            ControlInteraction::None
        };
        if *state != next {
            *state = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_markers_and_removal_drive_existing_state_without_idle_rewrites() {
        let mut app = App::new();
        register(&mut app);
        let entity = app.world_mut().spawn(Button).id();
        app.update();
        assert_eq!(
            app.world().get::<ControlInteraction>(entity),
            Some(&ControlInteraction::None)
        );
        app.world_mut().entity_mut(entity).insert(Hovered(true));
        app.update();
        assert_eq!(
            app.world().get::<ControlInteraction>(entity),
            Some(&ControlInteraction::Hovered)
        );
        app.world_mut().entity_mut(entity).insert(Pressed);
        app.update();
        assert_eq!(
            app.world().get::<ControlInteraction>(entity),
            Some(&ControlInteraction::Pressed)
        );
        app.world_mut()
            .entity_mut(entity)
            .remove::<(Pressed, Hovered)>();
        app.update();
        assert_eq!(
            app.world().get::<ControlInteraction>(entity),
            Some(&ControlInteraction::None)
        );
        let changed = app
            .world()
            .entity(entity)
            .get_ref::<ControlInteraction>()
            .unwrap()
            .last_changed();
        app.update();
        assert_eq!(
            app.world()
                .entity(entity)
                .get_ref::<ControlInteraction>()
                .unwrap()
                .last_changed(),
            changed
        );
    }

    #[test]
    fn released_keyboard_activation_stays_pressed_until_consumed() {
        let mut app = App::new();
        register(&mut app);
        let entity = app
            .world_mut()
            .spawn((Button, PendingFeathersActivation))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<ControlInteraction>(entity),
            Some(&ControlInteraction::Pressed)
        );
        app.world_mut()
            .entity_mut(entity)
            .remove::<PendingFeathersActivation>();
        app.update();
        assert_eq!(
            app.world().get::<ControlInteraction>(entity),
            Some(&ControlInteraction::None)
        );
    }
}
