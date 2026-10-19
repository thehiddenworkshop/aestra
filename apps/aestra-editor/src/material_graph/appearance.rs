//! Actual material node background policy, independent of native interaction representation.
use crate::{
    material_graph::{MaterialGraphSelectionState, appearance_input},
    theme,
};
use bevy::prelude::*;
#[derive(Clone, Copy, Debug)]
pub(super) enum Phase {
    None,
    Hovered,
    Pressed,
}
pub(super) fn handle_material_graph_actions(
    mut actions: appearance_input::Controls,
    selection: Res<MaterialGraphSelectionState>,
) {
    for (interaction, action, scope, mut background) in actions.iter() {
        match interaction {
            Phase::Hovered => background.0 = theme::BUTTON_HOVER,
            Phase::None => {
                let scope = scope.map(|scope| scope.0).unwrap_or(None);
                background.0 =
                    if selection.is_expression_selected(scope, action.program, action.expression) {
                        theme::SELECTION
                    } else {
                        theme::PANEL
                    };
            }
            Phase::Pressed => {
                background.0 = theme::ACCENT_DIM;
            }
        }
    }
}
#[cfg(test)]
#[path = "appearance/tests.rs"]
mod tests;
