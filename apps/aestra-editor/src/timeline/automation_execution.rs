//! Production automation action execution and semantic command construction.
use super::automation_data::{
    automation_lane_keys, automation_lane_value, bound_automation_parameter,
};
use super::{AutomationLaneId, ChoreographyAction, TimelineAutomationKeySelection, TimelineState};
use crate::{CurvesState, EditorSession, localization::Localizer};
use aestra_authoring::EffectCommand;
use aestra_core::{ColorKey, CurveKey, EffectAsset, Value};
use bevy::prelude::*;

pub(super) fn set_automation_lane_value_command(
    effect: &EffectAsset,
    lane: &AutomationLaneId,
    value: Value,
) -> Option<EffectCommand> {
    let emitter = effect
        .emitters
        .iter()
        .find(|emitter| emitter.id == lane.emitter)?;
    let module = emitter
        .modules
        .iter()
        .find(|module| module.id == lane.module)?;
    if let Some(mut parameter) =
        bound_automation_parameter(effect, module, &lane.parameter).cloned()
    {
        parameter.default = value;
        return Some(EffectCommand::SetParameter {
            id: parameter.id,
            parameter,
        });
    }
    if let Some(source) = module.property_source(&lane.parameter)
        && source != aestra_core::PropertySource::Constant
        && module
            .property_source_values
            .get(&lane.parameter)
            .is_some_and(|values| values.iter().any(|candidate| candidate.source == source))
    {
        return Some(EffectCommand::SetModulePropertySourceValue {
            emitter: lane.emitter,
            module: lane.module,
            parameter: lane.parameter.clone(),
            source,
            value,
        });
    }
    Some(EffectCommand::SetModuleParameter {
        emitter: lane.emitter,
        module: lane.module,
        parameter: lane.parameter.clone(),
        value,
    })
}

pub(super) fn curve_stored_sample(curve: &aestra_core::Curve, time: f32) -> f32 {
    let sampled = curve.sample(time);
    let Some(range) = curve.output_range else {
        return sampled;
    };
    let span = range.max - range.min;
    if span.abs() <= f32::EPSILON {
        return curve.keys.first().map_or(0.0, |key| key.value);
    }
    ((sampled - range.min) / span).clamp(0.0, 1.0)
}

pub(super) fn handle_automation_action(
    action: &ChoreographyAction,
    commands: &mut Commands,
    session: &mut EditorSession,
    curves: &mut CurvesState,
    state: &mut TimelineState,
    localizer: &Localizer,
) -> bool {
    match action.clone() {
        ChoreographyAction::ToggleEmitterAutomation(emitter) => {
            state.context_emitter = None;
            state.color_picker_emitter = None;
            state.context_effect_clip = None;
            state.expanded_automation_emitters.insert(emitter);
            state.automation_menu_emitter =
                (state.automation_menu_emitter != Some(emitter)).then_some(emitter);
            session.ui_revision += 1;
        }
        ChoreographyAction::SetEmitterAutomationVisibility {
            emitter,
            lanes,
            visible,
        } => {
            state.expanded_automation_emitters.insert(emitter);
            if visible {
                state.visible_automation_lanes.extend(lanes);
            } else {
                for lane in lanes {
                    state.visible_automation_lanes.remove(&lane);
                }
                state.selected_automation_key = state
                    .selected_automation_key
                    .take()
                    .filter(|selection| selection.lane.emitter != emitter);
            }
            state.automation_menu_emitter = None;
            session.ui_revision += 1;
        }
        ChoreographyAction::SetAutomationLaneVisibility { lane, visible } => {
            state.expanded_automation_emitters.insert(lane.emitter);
            if visible {
                state.visible_automation_lanes.insert(lane);
            } else {
                state.visible_automation_lanes.remove(&lane);
                state.selected_automation_key = state
                    .selected_automation_key
                    .take()
                    .filter(|selection| selection.lane != lane);
            }
            session.ui_revision += 1;
        }
        ChoreographyAction::SelectAutomationKey(selection) => {
            if automation_lane_keys(&session.effect, &selection.lane)
                .is_some_and(|keys| selection.key < keys.len())
            {
                let selected_region = state
                    .selected_emitter_region
                    .filter(|(emitter, _)| *emitter == selection.lane.emitter);
                state.select_only_emitter(selection.lane.emitter);
                if let Some((emitter, region)) = selected_region {
                    state.select_only_emitter_region(emitter, region);
                    session.select_emitter_region(emitter, region);
                } else {
                    session.select_emitter(selection.lane.emitter);
                }
                curves.select_key_channel(
                    selection.lane.module,
                    selection.lane.input,
                    selection.key,
                    selection.lane.channel,
                );
                state.selected_automation_key = Some(selection);
                state.inspected_child = None;
                session.status = localizer.text("timeline-selected-automation-key");
                session.ui_revision += 1;
            }
        }
        ChoreographyAction::AddAutomationKey(lane) => {
            let Some(emitter) = session
                .effect
                .emitters
                .iter()
                .find(|emitter| emitter.id == lane.emitter)
            else {
                return true;
            };
            let Some(region) = state
                .selected_emitter_region
                .filter(|(selected_emitter, _)| *selected_emitter == emitter.id)
                .and_then(|(_, region)| emitter.timeline_region(region))
                .or_else(|| emitter.timeline_regions().into_iter().next())
            else {
                return true;
            };
            let source_start = region.start_time - region.source_offset;
            let duration = emitter.duration.max(0.001);
            let normalized_time = ((session.time() - source_start) / duration).clamp(0.0, 1.0);
            commands.trigger(ChoreographyAction::AddAutomationKeyAt {
                lane,
                normalized_time_bits: normalized_time.to_bits(),
                value_bits: None,
            });
        }
        ChoreographyAction::AddAutomationKeyAt {
            lane,
            normalized_time_bits,
            value_bits,
        } => {
            let normalized_time = f32::from_bits(normalized_time_bits).clamp(0.0, 1.0);
            let Some(keys) = automation_lane_keys(&session.effect, &lane) else {
                return true;
            };
            if let Some(index) = keys
                .times()
                .position(|time| (time - normalized_time).abs() <= 0.0005)
            {
                commands.trigger(ChoreographyAction::SelectAutomationKey(
                    TimelineAutomationKeySelection { lane, key: index },
                ));
                return true;
            }
            let index = keys
                .times()
                .position(|time| time > normalized_time)
                .unwrap_or_else(|| keys.len());
            let command = match automation_lane_value(&session.effect, &lane) {
                Some(Value::Curve(mut curve)) => {
                    let value = value_bits.map_or_else(
                        || curve_stored_sample(&curve, normalized_time),
                        f32::from_bits,
                    );
                    curve
                        .keys
                        .insert(index, CurveKey::new(normalized_time, value));
                    let Some(command) = set_automation_lane_value_command(
                        &session.effect,
                        &lane,
                        Value::Curve(curve),
                    ) else {
                        return true;
                    };
                    command
                }
                Some(Value::Vec3Curve(mut curves)) => {
                    let Some(curve) = lane
                        .channel
                        .and_then(|channel| curves.curves.get_mut(channel as usize))
                    else {
                        return true;
                    };
                    let value = value_bits.map_or_else(
                        || curve_stored_sample(curve, normalized_time),
                        f32::from_bits,
                    );
                    curve
                        .keys
                        .insert(index, CurveKey::new(normalized_time, value));
                    let Some(command) = set_automation_lane_value_command(
                        &session.effect,
                        &lane,
                        Value::Vec3Curve(curves),
                    ) else {
                        return true;
                    };
                    command
                }
                Some(Value::Gradient(mut gradient)) => {
                    let color = gradient.sample(normalized_time);
                    gradient
                        .keys
                        .insert(index, ColorKey::new(normalized_time, color));
                    let Some(command) = set_automation_lane_value_command(
                        &session.effect,
                        &lane,
                        Value::Gradient(gradient),
                    ) else {
                        return true;
                    };
                    command
                }
                _ => return true,
            };
            if session.execute(
                localizer.text("timeline-add-automation-key-command"),
                command,
                true,
            ) {
                state.select_only_emitter(lane.emitter);
                session.select_emitter(lane.emitter);
                curves.select_key_channel(lane.module, lane.input, index, lane.channel);
                state.selected_automation_key =
                    Some(TimelineAutomationKeySelection { lane, key: index });
            }
        }
        ChoreographyAction::DeleteAutomationKey(selection) => {
            let Some(keys) = automation_lane_keys(&session.effect, &selection.lane) else {
                return true;
            };
            if keys.len() <= 2 || selection.key >= keys.len() {
                session.status = localizer.text("timeline-automation-keep-two-keys");
                return true;
            }
            let command = match automation_lane_value(&session.effect, &selection.lane) {
                Some(Value::Curve(mut curve)) => {
                    curve.keys.remove(selection.key);
                    let Some(command) = set_automation_lane_value_command(
                        &session.effect,
                        &selection.lane,
                        Value::Curve(curve),
                    ) else {
                        return true;
                    };
                    command
                }
                Some(Value::Vec3Curve(mut curves)) => {
                    let Some(curve) = selection
                        .lane
                        .channel
                        .and_then(|channel| curves.curves.get_mut(channel as usize))
                    else {
                        return true;
                    };
                    curve.keys.remove(selection.key);
                    let Some(command) = set_automation_lane_value_command(
                        &session.effect,
                        &selection.lane,
                        Value::Vec3Curve(curves),
                    ) else {
                        return true;
                    };
                    command
                }
                Some(Value::Gradient(mut gradient)) => {
                    gradient.keys.remove(selection.key);
                    let Some(command) = set_automation_lane_value_command(
                        &session.effect,
                        &selection.lane,
                        Value::Gradient(gradient),
                    ) else {
                        return true;
                    };
                    command
                }
                _ => return true,
            };
            if session.execute(
                localizer.text("timeline-delete-automation-key-command"),
                command,
                true,
            ) {
                let next = selection.key.saturating_sub(1).min(keys.len() - 2);
                curves.select_key_channel(
                    selection.lane.module,
                    selection.lane.input,
                    next,
                    selection.lane.channel,
                );
                state.selected_automation_key = Some(TimelineAutomationKeySelection {
                    lane: selection.lane,
                    key: next,
                });
            }
        }
        _ => return false,
    }
    true
}
