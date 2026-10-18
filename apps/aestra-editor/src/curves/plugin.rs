//! Production Curves observer, resource and schedule registration on both engine graphs.
use super::controls::{handle_curves_actions, queue_curves_action_activation};
use super::keyboard::curves_keyboard_input;
use super::{host_motion, state::CurvesState};
use bevy::prelude::*;

pub(crate) struct EditorCurvesPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CurvesSet {
    Actions,
}

impl Plugin for EditorCurvesPlugin {
    fn build(&self, app: &mut App) {
        host_motion::install(app);
        app.init_resource::<CurvesState>()
            .add_observer(queue_curves_action_activation)
            .add_systems(
                Update,
                (curves_keyboard_input, handle_curves_actions)
                    .chain()
                    .in_set(CurvesSet::Actions),
            );
    }
}

#[cfg(test)]
#[path = "plugin/tests.rs"]
mod tests;
