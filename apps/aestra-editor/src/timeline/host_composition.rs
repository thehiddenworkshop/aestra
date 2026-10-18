//! Production host-motion header, pose lane and action bridge.
use super::{
    control_input,
    gestures::stop_timeline_control_press,
    interaction_state::HostMotionControl,
    motion_gestures::{KeyControl, begin_drag, click_key, drag_key, end_drag},
    motion_state::Action,
    navigation_cursor::EntityCursor,
    timeline_pointer::TimelineButton as Button,
};
use crate::feathers::{
    action_activation::{EditorNativeControl, FeathersActionButton, PendingFeathersActivation},
    button::spawn_tool_button as mini_button,
    tooltip::EditorTooltip,
};
use crate::{session::EditorSession, theme};
use bevy::{
    prelude::*, ui::RelativeCursorPosition, ui_widgets::Activate, window::SystemCursorIcon,
};

pub(super) fn activate(
    event: On<Activate>,
    controls: Query<(), (With<Action>, With<FeathersActionButton>)>,
    mut commands: Commands,
) {
    if controls.contains(event.entity) {
        control_input::queue_pressed(&mut commands, event.entity);
    }
}

pub(super) fn buttons(
    mut commands: Commands,
    controls: Query<(Entity, &Action), With<PendingFeathersActivation>>,
) {
    for (entity, action) in &controls {
        let mut control = commands.entity(entity);
        control.remove::<PendingFeathersActivation>();
        control_input::clear_pressed(&mut commands, entity);
        commands.trigger(*action);
    }
}

pub(super) fn text_action(
    parent: &mut ChildSpawnerCommands,
    label: &str,
    action: Action,
) -> Entity {
    let entity = mini_button(parent, label, action);
    parent.commands().entity(entity).insert(Node {
        width: Val::Auto,
        height: Val::Px(24.0),
        flex_shrink: 0.0,
        padding: UiRect::horizontal(Val::Px(8.0)),
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        ..default()
    });
    entity
}

pub(super) fn spawn_header(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn(Node {
            height: Val::Px(28.0),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(Val::Px(8.0)),
            column_gap: Val::Px(5.0),
            ..default()
        })
        .with_children(|row| {
            let inspect = text_action(row, "Host Motion", Action::Inspect);
            let add = mini_button(row, "+", Action::Add);
            for entity in [inspect, add] {
                row.commands()
                    .entity(entity)
                    .insert((HostMotionControl, RelativeCursorPosition::default()));
            }
            row.commands()
                .entity(add)
                .insert(EditorTooltip::description(
                    "Add a pose key at the playhead; existing keys are selected for editing",
                ));
        });
}

pub(super) fn spawn_lane(parent: &mut ChildSpawnerCommands, session: &EditorSession) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(53.0),
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                height: Val::Px(28.0),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(theme::BORDER),
            BackgroundColor(theme::PANEL_DARK),
        ))
        .with_children(|lane| {
            for (index, key) in session
                .effect
                .host_transform_track
                .iter()
                .flat_map(|track| track.keys().into_iter().enumerate())
            {
                lane.spawn((
                Button,
                EditorNativeControl,
                KeyControl { effect: session.effect.id, index },
                EntityCursor::System(if index == 0 {
                    SystemCursorIcon::Pointer
                } else {
                    SystemCursorIcon::EwResize
                }),
                AccessibleLabel(format!("Host pose key at {:.3} seconds", key.time)),
                EditorTooltip::description(format!(
                    "Pose at {:.3}s. Select to edit; drag to retime. The first key stays at zero.",
                    key.time,
                )),
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(4.0),
                    width: Val::Px(18.0),
                    height: Val::Px(20.0),
                    margin: UiRect::left(Val::Px(-9.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(theme::ACCENT_DIM),
                Text::new("◆"),
                TextFont { font_size: FontSize::Px(12.0), ..default() },
                TextColor(theme::TEXT),
            ))
                .insert((HostMotionControl, RelativeCursorPosition::default()))
                .observe(click_key)
                .observe(stop_timeline_control_press)
                .observe(begin_drag)
                .observe(drag_key)
                .observe(end_drag);
            }
        });
}

#[cfg(test)]
#[path = "host_composition/tests.rs"]
mod tests;
