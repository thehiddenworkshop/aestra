//! Shared production property-curve/gradient resolution, editing and preview math.
use super::state::{ComplexSelection, CurvesState};
use crate::feathers::automation_curve::{
    AutomationCurveData, AutomationCurvePoint, AutomationGradientPoint,
};
use crate::{EditorModuleRegistry, EditorSession, module_parameter};
use aestra_compiler::{InputControl, InputMetadata, ModuleRegistry};
use aestra_core::{ColorKey, CurveKey, ModuleId, ModuleInstance, Value};
use bevy::prelude::*;

pub(super) fn resolve_complex_input<'a>(
    session: &'a EditorSession,
    registry: &'a EditorModuleRegistry,
    selection: ComplexSelection,
) -> Option<(&'a ModuleInstance, &'a InputMetadata, Value)> {
    let selected_layer = session.selected_layer()?;
    let module = selected_layer
        .modules
        .iter()
        .find(|module| module.id == selection.module)?;
    let input = registry
        .0
        .get(&module.module_type)?
        .inputs
        .get(selection.input as usize)?;
    let value = curve_module_parameter(session, module, input.name)?;
    Some((module, input, value))
}

pub(super) fn curve_module_parameter(
    session: &EditorSession,
    module: &ModuleInstance,
    parameter: &str,
) -> Option<Value> {
    if let Some(parameter_id) = module.bindings.get(parameter) {
        return session
            .effect
            .parameters
            .iter()
            .find(|candidate| candidate.id == *parameter_id)
            .map(|parameter| parameter.default.clone());
    }
    module_parameter(module, parameter)
}

pub(super) fn editable_curve(
    session: &EditorSession,
    module: ModuleId,
    parameter: &str,
    vector_channel: Option<u8>,
) -> Option<aestra_core::Curve> {
    let module = session
        .selected_layer()?
        .modules
        .iter()
        .find(|candidate| candidate.id == module)?;
    match (
        curve_module_parameter(session, module, parameter)?,
        vector_channel,
    ) {
        (Value::Curve(curve), None) => Some(curve),
        (Value::Vec3Curve(curves), Some(channel)) => {
            curves.curves.get(channel.min(2) as usize).cloned()
        }
        _ => None,
    }
}

pub(super) fn update_vector_curve(
    session: &mut EditorSession,
    module: ModuleId,
    parameter: &str,
    channel: u8,
    label: &str,
    edit: impl FnOnce(&mut aestra_core::Curve) -> bool,
) -> bool {
    let Some(selected_layer) = session.selected_layer() else {
        return false;
    };
    let Some(module_instance) = selected_layer
        .modules
        .iter()
        .find(|candidate| candidate.id == module)
    else {
        return false;
    };
    let Some(Value::Vec3Curve(mut curves)) =
        curve_module_parameter(session, module_instance, parameter)
    else {
        return false;
    };
    let Some(curve) = curves.curves.get_mut(channel.min(2) as usize) else {
        return false;
    };
    if !edit(curve) {
        return false;
    }
    session.set_active_module_property_value(module, parameter, Value::Vec3Curve(curves), label)
}

pub(super) fn set_editable_curve_key(
    session: &mut EditorSession,
    module: ModuleId,
    parameter: &str,
    vector_channel: Option<u8>,
    index: usize,
    key: CurveKey,
) {
    if let Some(channel) = vector_channel {
        update_vector_curve(
            session,
            module,
            parameter,
            channel,
            &format!("Changed {parameter} {channel} curve key"),
            |curve| {
                let Some(previous) = curve.keys.get_mut(index) else {
                    return false;
                };
                *previous = key;
                true
            },
        );
    } else {
        session.set_curve_key(module, parameter, index, key);
    }
}

pub(super) fn insert_vector_curve_key(
    session: &mut EditorSession,
    module: ModuleId,
    parameter: &str,
    channel: u8,
    index: usize,
    key: CurveKey,
) {
    update_vector_curve(
        session,
        module,
        parameter,
        channel,
        &format!("Added {parameter} {channel} curve key"),
        |curve| {
            if index > curve.keys.len() {
                return false;
            }
            curve.keys.insert(index, key);
            true
        },
    );
}

pub(super) fn remove_vector_curve_key(
    session: &mut EditorSession,
    module: ModuleId,
    parameter: &str,
    channel: u8,
    index: usize,
) {
    update_vector_curve(
        session,
        module,
        parameter,
        channel,
        &format!("Removed {parameter} {channel} curve key"),
        |curve| {
            if index >= curve.keys.len() {
                return false;
            }
            curve.keys.remove(index);
            true
        },
    );
}

pub(super) fn curve_graph_data(curve: &aestra_core::Curve) -> AutomationCurveData {
    let output_range = curve.output_range();
    let value_bounds = if curve.output_range.is_some() {
        Some((0.0, 1.0))
    } else {
        Some((output_range.min, output_range.max))
    };
    AutomationCurveData::Curve {
        interpolation: curve.interpolation,
        points: curve
            .keys
            .iter()
            .map(|key| AutomationCurvePoint {
                time: key.time,
                value: key.value,
            })
            .collect(),
        value_bounds,
    }
}

pub(super) fn formatted_curve_output_value(value: f32) -> String {
    crate::feathers::number_input::formatted(value, 3)
}

pub(super) fn curve_key_output_value(curve: &aestra_core::Curve, key: CurveKey) -> f32 {
    curve.output_value(key.value)
}

pub(super) fn gradient_graph_data(gradient: &aestra_core::Gradient) -> AutomationCurveData {
    AutomationCurveData::Gradient(
        gradient
            .keys
            .iter()
            .map(|key| AutomationGradientPoint {
                time: key.time,
                color: key.color,
            })
            .collect(),
    )
}

pub(super) fn curve_drag_preview(
    curve: &aestra_core::Curve,
    key_index: usize,
    distance: Vec2,
    graph_size: Vec2,
    min: f32,
    max: f32,
) -> Option<(CurveKey, AutomationCurveData)> {
    let graph_data = curve_graph_data(curve);
    let mut key = curve.keys.get(key_index).copied()?;
    let previous = key_index
        .checked_sub(1)
        .and_then(|index| curve.keys.get(index))
        .map_or(0.0, |key| key.time + 0.001);
    let next = curve
        .keys
        .get(key_index + 1)
        .map_or(1.0, |key| key.time - 0.001);
    key.time = (key.time + distance.x / graph_size.x.max(1.0)).clamp(previous, next);
    let original_top = graph_data.key_top_percent(key_index);
    let target_top = original_top + distance.y / graph_size.y.max(1.0) * 100.0;
    key.value = graph_data
        .value_for_top_percent(target_top)
        .unwrap_or(key.value)
        .clamp(min, max);
    let mut preview = graph_data;
    let AutomationCurveData::Curve { points, .. } = &mut preview else {
        return None;
    };
    let point = points.get_mut(key_index)?;
    point.time = key.time;
    point.value = key.value;
    Some((key, preview))
}

pub(super) fn gradient_drag_preview(
    gradient: &aestra_core::Gradient,
    key_index: usize,
    distance_x: f32,
    graph_width: f32,
) -> Option<(ColorKey, AutomationCurveData)> {
    let mut key = gradient.keys.get(key_index).copied()?;
    let previous = key_index
        .checked_sub(1)
        .and_then(|index| gradient.keys.get(index))
        .map_or(0.0, |key| key.time + 0.001);
    let next = gradient
        .keys
        .get(key_index + 1)
        .map_or(1.0, |key| key.time - 0.001);
    key.time = (key.time + distance_x / graph_width.max(1.0)).clamp(previous, next);
    let mut preview = gradient_graph_data(gradient);
    let AutomationCurveData::Gradient(points) = &mut preview else {
        return None;
    };
    points.get_mut(key_index)?.time = key.time;
    Some((key, preview))
}

pub(super) fn add_complex_key_at_pointer(
    session: &mut EditorSession,
    registry: &ModuleRegistry,
    workspace: &mut CurvesState,
    cursor: Vec2,
) {
    let Some(selected_layer) = session.selected_layer() else {
        return;
    };
    let Some(selection) = workspace.complex else {
        session.status = "Select a curve or gradient first".into();
        return;
    };
    let Some(module) = selected_layer
        .modules
        .iter()
        .find(|module| module.id == selection.module)
    else {
        session.status = "The selected module no longer exists".into();
        return;
    };
    let Some(input) = registry
        .get(&module.module_type)
        .and_then(|metadata| metadata.inputs.get(selection.input as usize))
    else {
        session.status = "The selected input metadata no longer exists".into();
        return;
    };
    let parameter = input.name;
    let control = input.control;
    let Some(value) = curve_module_parameter(session, module, parameter) else {
        session.status = "The selected authored value no longer exists".into();
        return;
    };
    let time = (cursor.x + 0.5).clamp(0.0, 1.0);
    let top = (cursor.y + 0.5).clamp(0.0, 1.0) * 100.0;

    match value {
        Value::Curve(curve) => {
            if let Some(index) = curve
                .keys
                .iter()
                .position(|key| (key.time - time).abs() <= 0.0005)
            {
                workspace.complex = Some(ComplexSelection {
                    key: index,
                    ..selection
                });
                session.ui_revision += 1;
                return;
            }
            let index = curve
                .keys
                .iter()
                .position(|key| key.time > time)
                .unwrap_or(curve.keys.len());
            let Some((_, min, max)) = curve_value_bounds(&control, &curve) else {
                return;
            };
            let value = curve_graph_data(&curve)
                .value_for_top_percent(top)
                .unwrap_or_else(|| curve.sample(time))
                .clamp(min, max);
            session.add_curve_key(
                selection.module,
                parameter,
                index,
                CurveKey::new(time, value),
            );
            workspace.complex = Some(ComplexSelection {
                key: index,
                ..selection
            });
        }
        Value::Vec3Curve(curves) => {
            let channel = workspace.vector_channel.min(2);
            let curve = &curves.curves[channel as usize];
            if let Some(index) = curve
                .keys
                .iter()
                .position(|key| (key.time - time).abs() <= 0.0005)
            {
                workspace.complex = Some(ComplexSelection {
                    key: index,
                    ..selection
                });
                session.ui_revision += 1;
                return;
            }
            let index = curve
                .keys
                .iter()
                .position(|key| key.time > time)
                .unwrap_or(curve.keys.len());
            let Some((_, min, max)) = curve_value_bounds(&control, curve) else {
                return;
            };
            let value = curve_graph_data(curve)
                .value_for_top_percent(top)
                .unwrap_or_else(|| curve.sample(time))
                .clamp(min, max);
            insert_vector_curve_key(
                session,
                selection.module,
                parameter,
                channel,
                index,
                CurveKey::new(time, value),
            );
            workspace.complex = Some(ComplexSelection {
                key: index,
                ..selection
            });
        }
        Value::Gradient(gradient) => {
            if let Some(index) = gradient
                .keys
                .iter()
                .position(|key| (key.time - time).abs() <= 0.0005)
            {
                workspace.complex = Some(ComplexSelection {
                    key: index,
                    ..selection
                });
                session.ui_revision += 1;
                return;
            }
            let index = gradient
                .keys
                .iter()
                .position(|key| key.time > time)
                .unwrap_or(gradient.keys.len());
            session.add_gradient_key(
                selection.module,
                parameter,
                index,
                ColorKey::new(time, gradient.sample(time)),
            );
            workspace.complex = Some(ComplexSelection {
                key: index,
                ..selection
            });
        }
        _ => session.status = "This property does not contain editable keys".into(),
    }
}

#[derive(Clone, Copy)]
pub(super) enum ComplexKeyEdit {
    Add,
    Delete,
    Time(i8),
    CurveValue(i8),
    GradientChannel(u8, i8),
}

pub(super) fn edit_complex_key(
    session: &mut EditorSession,
    registry: &ModuleRegistry,
    workspace: &mut CurvesState,
    edit: ComplexKeyEdit,
) {
    let Some(selected_layer) = session.selected_layer() else {
        return;
    };
    let Some(selection) = workspace.complex else {
        session.status = "Select a curve or gradient first".into();
        return;
    };
    let Some(module) = selected_layer
        .modules
        .iter()
        .find(|module| module.id == selection.module)
    else {
        session.status = "The selected module no longer exists".into();
        return;
    };
    let Some(input) = registry
        .get(&module.module_type)
        .and_then(|metadata| metadata.inputs.get(selection.input as usize))
    else {
        session.status = "The selected input metadata no longer exists".into();
        return;
    };
    let parameter = input.name;
    let control = input.control;
    let Some(value) = curve_module_parameter(session, module, parameter) else {
        session.status = "The selected authored value no longer exists".into();
        return;
    };
    let vector_channel = workspace.vector_channel.min(2);

    match (value, edit) {
        (Value::Curve(curve), ComplexKeyEdit::Add) => {
            let (index, time) = insertion_time(
                &curve.keys.iter().map(|key| key.time).collect::<Vec<_>>(),
                selection.key,
            );
            let value = curve_stored_sample(&curve, time);
            session.add_curve_key(
                selection.module,
                parameter,
                index,
                CurveKey::new(time, value),
            );
            workspace.complex = Some(ComplexSelection {
                key: index,
                ..selection
            });
        }
        (Value::Vec3Curve(curves), ComplexKeyEdit::Add) => {
            let curve = &curves.curves[vector_channel as usize];
            let (index, time) = insertion_time(
                &curve.keys.iter().map(|key| key.time).collect::<Vec<_>>(),
                selection.key,
            );
            insert_vector_curve_key(
                session,
                selection.module,
                parameter,
                vector_channel,
                index,
                CurveKey::new(time, curve_stored_sample(curve, time)),
            );
            workspace.complex = Some(ComplexSelection {
                key: index,
                ..selection
            });
        }
        (Value::Gradient(gradient), ComplexKeyEdit::Add) => {
            let (index, time) = insertion_time(
                &gradient.keys.iter().map(|key| key.time).collect::<Vec<_>>(),
                selection.key,
            );
            let color = gradient.sample(time);
            session.add_gradient_key(
                selection.module,
                parameter,
                index,
                ColorKey::new(time, color),
            );
            workspace.complex = Some(ComplexSelection {
                key: index,
                ..selection
            });
        }
        (Value::Curve(curve), ComplexKeyEdit::Delete) => {
            if curve.keys.len() <= 2 {
                session.status = "A curve must keep at least two keys".into();
                return;
            }
            let index = selection.key.min(curve.keys.len() - 1);
            session.remove_curve_key(selection.module, parameter, index);
            workspace.complex = Some(ComplexSelection {
                key: index.min(curve.keys.len() - 2),
                ..selection
            });
        }
        (Value::Vec3Curve(curves), ComplexKeyEdit::Delete) => {
            let curve = &curves.curves[vector_channel as usize];
            if curve.keys.len() <= 2 {
                session.status = "A curve must keep at least two keys".into();
                return;
            }
            let index = selection.key.min(curve.keys.len() - 1);
            remove_vector_curve_key(session, selection.module, parameter, vector_channel, index);
            workspace.complex = Some(ComplexSelection {
                key: index.min(curve.keys.len() - 2),
                ..selection
            });
        }
        (Value::Gradient(gradient), ComplexKeyEdit::Delete) => {
            if gradient.keys.len() <= 2 {
                session.status = "A gradient must keep at least two keys".into();
                return;
            }
            let index = selection.key.min(gradient.keys.len() - 1);
            session.remove_gradient_key(selection.module, parameter, index);
            workspace.complex = Some(ComplexSelection {
                key: index.min(gradient.keys.len() - 2),
                ..selection
            });
        }
        (Value::Curve(curve), ComplexKeyEdit::Time(direction)) => {
            let Some(mut key) = curve.keys.get(selection.key).copied() else {
                return;
            };
            key.time = bounded_key_time(
                &curve.keys.iter().map(|key| key.time).collect::<Vec<_>>(),
                selection.key,
                key.time + direction as f32 * 0.01,
            );
            session.set_curve_key(selection.module, parameter, selection.key, key);
        }
        (Value::Vec3Curve(curves), ComplexKeyEdit::Time(direction)) => {
            let curve = &curves.curves[vector_channel as usize];
            let Some(mut key) = curve.keys.get(selection.key).copied() else {
                return;
            };
            key.time = bounded_key_time(
                &curve.keys.iter().map(|key| key.time).collect::<Vec<_>>(),
                selection.key,
                key.time + direction as f32 * 0.01,
            );
            set_editable_curve_key(
                session,
                selection.module,
                parameter,
                Some(vector_channel),
                selection.key,
                key,
            );
        }
        (Value::Gradient(gradient), ComplexKeyEdit::Time(direction)) => {
            let Some(mut key) = gradient.keys.get(selection.key).copied() else {
                return;
            };
            key.time = bounded_key_time(
                &gradient.keys.iter().map(|key| key.time).collect::<Vec<_>>(),
                selection.key,
                key.time + direction as f32 * 0.01,
            );
            session.set_gradient_key(selection.module, parameter, selection.key, key);
        }
        (Value::Curve(curve), ComplexKeyEdit::CurveValue(direction)) => {
            let Some(mut key) = curve.keys.get(selection.key).copied() else {
                return;
            };
            let Some((step, min, max)) = curve_value_bounds(&control, &curve) else {
                return;
            };
            key.value = (key.value + direction as f32 * step).clamp(min, max);
            session.set_curve_key(selection.module, parameter, selection.key, key);
        }
        (Value::Vec3Curve(curves), ComplexKeyEdit::CurveValue(direction)) => {
            let curve = &curves.curves[vector_channel as usize];
            let Some(mut key) = curve.keys.get(selection.key).copied() else {
                return;
            };
            let Some((step, min, max)) = curve_value_bounds(&control, curve) else {
                return;
            };
            key.value = (key.value + direction as f32 * step).clamp(min, max);
            set_editable_curve_key(
                session,
                selection.module,
                parameter,
                Some(vector_channel),
                selection.key,
                key,
            );
        }
        (Value::Gradient(gradient), ComplexKeyEdit::GradientChannel(channel, direction)) => {
            let Some(mut key) = gradient.keys.get(selection.key).copied() else {
                return;
            };
            let Some(value) = key.color.get_mut(channel as usize) else {
                return;
            };
            *value = (*value + direction as f32 * 0.05).clamp(0.0, 1.0);
            session.set_gradient_key(selection.module, parameter, selection.key, key);
        }
        _ => session.status = "This edit does not apply to the selected property".into(),
    }
}

pub(super) fn curve_value_bounds(
    control: &InputControl,
    curve: &aestra_core::Curve,
) -> Option<(f32, f32, f32)> {
    if curve.output_range.is_some() {
        return Some((0.01, 0.0, 1.0));
    }
    match control {
        InputControl::Curve { step, min, max } => Some((*step, *min, *max)),
        InputControl::Number { step, min, max } | InputControl::Vector { step, min, max } => {
            let authored_min = curve
                .keys
                .iter()
                .map(|key| key.value)
                .fold(f32::INFINITY, f32::min);
            let authored_max = curve
                .keys
                .iter()
                .map(|key| key.value)
                .fold(f32::NEG_INFINITY, f32::max);
            let minimum = min.unwrap_or(if authored_min.is_finite() {
                authored_min
            } else {
                0.0
            });
            let maximum = max.unwrap_or_else(|| {
                if authored_max.is_finite() {
                    authored_max.max(minimum + *step)
                } else {
                    minimum + *step
                }
            });
            Some((*step, minimum, maximum.max(minimum + f32::EPSILON)))
        }
        _ => None,
    }
}

pub(super) fn curve_stored_sample(curve: &aestra_core::Curve, time: f32) -> f32 {
    let sampled = curve.sample(time);
    let Some(range) = curve.output_range else {
        return sampled;
    };
    let span = range.max - range.min;
    if span.abs() <= f32::EPSILON {
        curve.keys.first().map_or(0.0, |key| key.value)
    } else {
        ((sampled - range.min) / span).clamp(0.0, 1.0)
    }
}

pub(super) fn insertion_time(times: &[f32], selected: usize) -> (usize, f32) {
    if times.is_empty() {
        return (0, 0.5);
    }
    let selected = selected.min(times.len() - 1);
    if let Some(next) = times.get(selected + 1) {
        (selected + 1, (times[selected] + next) * 0.5)
    } else if selected > 0 {
        (selected, (times[selected - 1] + times[selected]) * 0.5)
    } else {
        (1, (times[0] + 1.0) * 0.5)
    }
}

pub(super) fn bounded_key_time(times: &[f32], index: usize, value: f32) -> f32 {
    let previous = index
        .checked_sub(1)
        .and_then(|index| times.get(index))
        .map_or(0.0, |time| time + 0.001);
    let next = times.get(index + 1).map_or(1.0, |time| time - 0.001);
    value.clamp(previous, next)
}

#[cfg(test)]
#[path = "tests/property_editing.rs"]
mod tests;
