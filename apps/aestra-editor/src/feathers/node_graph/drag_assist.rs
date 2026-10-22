//! Shared, session-only drag assistance. Geometry is captured from the dragged view;
//! stale/rebuilt targets disable alignment for the remainder of that gesture.
#[cfg(test)]
use super::geometry::*;
use super::*;

#[path = "drag_assist/model.rs"]
pub(super) mod model;

#[path = "drag_assist/controls.rs"]
pub(super) mod controls;
#[cfg(test)]
pub(super) use controls::Toggle;
pub(crate) use controls::spawn_controls;
pub(super) use controls::{sync_controls, toggle};

pub(super) use super::drag_context::{Context, State};

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
#[path = "drag_assist/tests.rs"]
mod tests;
