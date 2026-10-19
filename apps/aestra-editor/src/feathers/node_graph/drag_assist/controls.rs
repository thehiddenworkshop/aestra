//! Actual graph toolbar snapping controls; no candidate-only toggle or styling.
use crate::feathers::node_graph::drag_assist::State;
use crate::feathers::{
    action_activation::FeathersActionButton, node_graph::spawn_graph_tool_button,
    tooltip::EditorTooltip,
};
use bevy::{feathers::controls::ButtonVariant, prelude::*, ui::Selected, ui_widgets::Activate};
#[derive(Component, Clone, Copy)]
pub(crate) enum Toggle {
    Grid,
    Alignment,
}
/// Existing shared icon-button chrome; sync_controls supplies localized labels.
pub(crate) fn spawn_controls(parent: &mut ChildSpawnerCommands, assets: &AssetServer) {
    for (action, icon, label) in [
        (
            Toggle::Grid,
            "icons/grid.svg",
            "Grid snapping (hold Alt for free movement)",
        ),
        (
            Toggle::Alignment,
            "icons/graph-align.svg",
            "Alignment snapping and guides (hold Alt for free movement)",
        ),
    ] {
        spawn_graph_tool_button(parent, assets, icon, label.into(), action);
    }
}

pub(crate) fn toggle(event: On<Activate>, buttons: Query<&Toggle>, mut state: ResMut<State>) {
    if let Ok(action) = buttons.get(event.entity) {
        match action {
            Toggle::Grid => state.grid = !state.grid,
            Toggle::Alignment => state.alignment = !state.alignment,
        }
    }
}

pub(crate) fn sync_controls(
    mut commands: Commands,
    state: Res<State>,
    localizer: Option<Res<crate::Localizer>>,
    buttons: Query<(Entity, Ref<Toggle>, Has<Selected>), With<FeathersActionButton>>,
) {
    for (entity, action, selected) in &buttons {
        let (enabled, label) = match *action {
            Toggle::Grid => (state.grid, "graph-snap-grid"),
            Toggle::Alignment => (state.alignment, "graph-snap-alignment"),
        };
        if enabled != selected {
            if enabled {
                commands
                    .entity(entity)
                    .insert((Selected, ButtonVariant::Primary));
            } else {
                commands
                    .entity(entity)
                    .remove::<Selected>()
                    .insert(ButtonVariant::Plain);
            }
        }
        if let Some(localizer) = &localizer {
            // Only queue changed values; do not repeatedly invalidate hover tooltips.
            if action.is_added() || localizer.is_changed() || enabled != selected {
                let text = localizer.text(label);
                commands.entity(entity).insert((
                    AccessibleLabel(text.clone()),
                    EditorTooltip::description(text),
                ));
            }
        }
    }
}
