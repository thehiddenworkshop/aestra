//! Actual host-motion inspector and controlled numeric initialization.
use super::{
    host_composition::text_action,
    motion_execution::{Field, NumberControl},
    motion_state::{Action, active},
    state::TimelineState,
};
use crate::feathers::{
    field_row::{FieldRowProps, spawn_field_row},
    number_input::{NumberInputValue, ScrubbableNumber, UpdateNumberInput, feathers_scalar_input},
    tooltip::EditorTooltip,
};
use crate::{session::EditorSession, theme};
use aestra_core::EffectId;
use bevy::prelude::*;

fn label(parent: &mut ChildSpawnerCommands, text: impl Into<String>) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
        TextColor(theme::TEXT),
        Node {
            margin: UiRect::vertical(Val::Px(5.0)),
            ..default()
        },
    ));
}

pub(crate) fn spawn_inspector(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    state: &TimelineState,
) -> bool {
    if !active(session, state) {
        return false;
    }
    parent.spawn(Node {
        width: Val::Percent(100.0),
        flex_grow: 1.0,
        min_height: Val::Px(0.0),
        min_width: Val::Px(0.0),
        ..default()
    }).with_children(|body| {
    crate::feathers::scroll::spawn_vertical_scroll_area(
        body,
        crate::ScrollMemoryKey::Properties,
        Node {
            flex_grow: 1.0,
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(12.0)),
            row_gap: Val::Px(4.0),
            ..default()
        },
        |panel| {
            label(panel, "Host Motion");
            crate::spawn_feathers_action_button(panel, "Edit transform curves", crate::curves::CurvesAction::OpenHost(0), false);
            label(panel, "Effect-level pose keys · rotation in degrees (XYZ)");
            panel
                .spawn(Node {
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: Val::Px(5.0),
                    row_gap: Val::Px(4.0),
                    ..default()
                })
                .with_children(|row| {
                    text_action(row, "Key at playhead", Action::Add);
                    text_action(row, "Delete key", Action::Delete);
                    text_action(row, "Clear motion", Action::Clear);
                });
            let Some(track) = &session.effect.host_transform_track else {
                label(
                    panel,
                    "No motion track. Key at playhead creates a time-zero anchor and a pose at the current time.",
                );
                return;
            };
            panel.spawn(Node {
                flex_wrap: FlexWrap::Wrap,
                column_gap: Val::Px(5.0),
                ..default()
            }).with_children(|row| {
                text_action(row, if track.repeat { "Hold" } else { "● Hold" }, Action::Repeat(false));
                text_action(row, if track.repeat { "● Repeat" } else { "Repeat" }, Action::Repeat(true));
                let close = text_action(row, "Close loop", Action::CloseLoop);
                row.commands().entity(close).insert(EditorTooltip::description(
                    "Copy the first pose to the final key and enable Repeat. A single-key track gains an endpoint at the effect duration."
                ));
            });
            label(
                panel,
                format!(
                    "{} keys · End / period {:.3}s",
                    track.keys().len(),
                    track.keys().last().unwrap().time
                ),
            );
            if let Some(error) = &state.host_motion.error {
                label(panel, error.clone());
            }
            let Some(index) = state.host_motion.selected.filter(|i| *i < track.keys().len()) else {
                label(panel, "Select a pose key in the Host Motion lane.");
                return;
            };
            let key = &track.keys()[index];
            if index == 0 {
                label(panel, "Time: 0 s (anchor)");
            } else {
                number(
                    panel,
                    session.effect.id,
                    index,
                    "Time (s)",
                    Field::Time,
                    key.time,
                    0.0,
                    0.01,
                );
            }
            let (x, y, z) = Quat::from_array(key.transform.rotation).to_euler(EulerRot::XYZ);
            for (axis, name) in ["X", "Y", "Z"].into_iter().enumerate() {
                number(
                    panel,
                    session.effect.id,
                    index,
                    &format!("Position {name}"),
                    Field::Position(axis),
                    key.transform.translation[axis],
                    -f32::MAX,
                    0.1,
                );
            }
            for (axis, angle) in [x, y, z].into_iter().enumerate() {
                number(
                    panel,
                    session.effect.id,
                    index,
                    &format!("Rotation {} (°)", ["X", "Y", "Z"][axis]),
                    Field::Rotation(axis),
                    angle.to_degrees(),
                    -360.0,
                    0.5,
                );
            }
            for (axis, name) in ["X", "Y", "Z"].into_iter().enumerate() {
                number(
                    panel,
                    session.effect.id,
                    index,
                    &format!("Scale {name}"),
                    Field::Scale(axis),
                    key.transform.scale[axis],
                    0.001,
                    0.01,
                );
            }
            if track.repeat {
                label(
                    panel,
                    "First and last pose edits remain linked to keep the loop closed.",
                );
            }
        },
    );
    });
    true
}

#[allow(
    clippy::too_many_arguments,
    reason = "Retain the existing explicit numeric field construction contract"
)]
fn number(
    parent: &mut ChildSpawnerCommands,
    effect: EffectId,
    index: usize,
    title: &str,
    field: Field,
    value: f32,
    min: f32,
    step: f32,
) {
    spawn_field_row(parent, FieldRowProps::new(title), (), |row| {
        row.spawn_empty()
            .apply_scene(feathers_scalar_input())
            .insert((
                NumberControl {
                    effect,
                    index,
                    field,
                    initial: value,
                },
                ScrubbableNumber::new(
                    value,
                    min,
                    if matches!(field, Field::Rotation(_)) {
                        360.0
                    } else {
                        f32::MAX
                    },
                    step,
                ),
                AccessibleLabel(title.to_owned()),
            ));
    });
}

pub(super) fn initialize_numbers(
    mut commands: Commands,
    inputs: Query<(Entity, &NumberControl), Added<NumberControl>>,
) {
    for (entity, input) in &inputs {
        commands.trigger(UpdateNumberInput {
            entity,
            value: NumberInputValue::F32(input.initial),
        });
    }
}

#[cfg(test)]
#[path = "inspector_composition/tests.rs"]
mod tests;
