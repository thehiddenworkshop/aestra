//! Shared, session-only drag assistance. Geometry is captured from the dragged view;
//! stale/rebuilt targets disable alignment for the remainder of that gesture.
#[cfg(test)]
use super::geometry::*;
use super::*;
use bevy::{feathers::controls::ButtonVariant, ui::Selected};

pub(super) mod model;

#[derive(Component, Clone, Copy)]
pub(super) enum Toggle {
    Grid,
    Alignment,
}

pub(super) use super::drag_context::{Context, State};
/// Uses existing shared icon-button chrome. Labels are localized by sync_controls,
/// including function graph hosts that do not currently pass a Localizer to spawn.
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

pub(super) fn toggle(event: On<Activate>, buttons: Query<&Toggle>, mut state: ResMut<State>) {
    if let Ok(action) = buttons.get(event.entity) {
        match action {
            Toggle::Grid => state.grid = !state.grid,
            Toggle::Alignment => state.alignment = !state.alignment,
        }
    }
}

pub(super) fn sync_controls(
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

#[derive(Component)]
pub(super) struct GuideLine;

pub(super) fn sync_guides(
    mut commands: Commands,
    mut state: ResMut<State>,
    nodes: Query<&FeathersGraphNode>,
    views: Query<&FeathersGraphViewport>,
    lines: Query<Entity, With<GuideLine>>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    // At most two passive, clipped lines in the originating view. No hit-test interception.
    for entity in &lines {
        commands.entity(entity).despawn();
    }
    state.gestures.retain(|entity, gesture| {
        nodes.get(*entity).is_ok_and(|node| node.dragging) && views.contains(gesture.view)
    });
    if keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight) {
        return;
    }
    for gesture in state.gestures.values() {
        let Ok(view) = views.get(gesture.view) else {
            continue;
        };
        for guide in &gesture.guides {
            let axis = guide.axis;
            let mut start = Vec2::ZERO;
            start[axis] = guide.coordinate;
            start[1 - axis] = guide.start;
            let position = view.project_graph_point(start);
            let length = ((guide.end - guide.start) * view.zoom).max(1.0);
            commands.spawn((
                GuideLine,
                ChildOf(gesture.view),
                Pickable::IGNORE,
                ZIndex(30),
                BackgroundColor(theme::ACCENT),
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(position.x),
                    top: Val::Px(position.y),
                    width: Val::Px(if axis == 0 { 1.0 } else { length }),
                    height: Val::Px(if axis == 1 { 1.0 } else { length }),
                    ..default()
                },
            ));
        }
    }
}

#[cfg(test)]
mod tests;
