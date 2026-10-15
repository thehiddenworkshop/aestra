//! Production automation value resolution and pointer-to-key projection.
use super::{AutomationLaneId, ChoreographyAction};
use crate::feathers::automation_curve::{
    AutomationCurveData, AutomationCurvePoint, AutomationGradientPoint,
};
use aestra_core::{ColorKey, CurveKey, EffectAsset, EffectParameter, Value};
use bevy::prelude::*;

#[derive(Clone)]
pub(super) enum AutomationLaneKeys {
    Curve(Vec<CurveKey>, aestra_core::CurveInterpolation),
    Gradient(Vec<ColorKey>),
}

impl AutomationLaneKeys {
    pub(super) fn times(&self) -> impl Iterator<Item = f32> + '_ {
        match self {
            Self::Curve(keys, _) => EitherAutomationTimes::Curve(keys.iter()),
            Self::Gradient(keys) => EitherAutomationTimes::Gradient(keys.iter()),
        }
    }

    pub(super) fn len(&self) -> usize {
        match self {
            Self::Curve(keys, _) => keys.len(),
            Self::Gradient(keys) => keys.len(),
        }
    }

    pub(super) fn curve_value(&self, key: usize) -> Option<f32> {
        match self {
            Self::Curve(keys, _) => keys.get(key).map(|key| key.value),
            Self::Gradient(_) => None,
        }
    }

    pub(super) fn graph_data(&self) -> AutomationCurveData {
        match self {
            Self::Curve(keys, interpolation) => AutomationCurveData::Curve {
                interpolation: *interpolation,
                points: keys
                    .iter()
                    .map(|key| AutomationCurvePoint {
                        time: key.time,
                        value: key.value,
                    })
                    .collect(),
                value_bounds: None,
            },
            Self::Gradient(keys) => AutomationCurveData::Gradient(
                keys.iter()
                    .map(|key| AutomationGradientPoint {
                        time: key.time,
                        color: key.color,
                    })
                    .collect(),
            ),
        }
    }
}

enum EitherAutomationTimes<'a> {
    Curve(std::slice::Iter<'a, CurveKey>),
    Gradient(std::slice::Iter<'a, ColorKey>),
}

impl Iterator for EitherAutomationTimes<'_> {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Curve(keys) => keys.next().map(|key| key.time),
            Self::Gradient(keys) => keys.next().map(|key| key.time),
        }
    }
}

pub(super) fn automation_lane_value(
    effect: &EffectAsset,
    lane: &AutomationLaneId,
) -> Option<Value> {
    let emitter = effect
        .emitters
        .iter()
        .find(|emitter| emitter.id == lane.emitter)?;
    let module = emitter
        .modules
        .iter()
        .find(|module| module.id == lane.module)?;
    if let Some(parameter) = bound_automation_parameter(effect, module, &lane.parameter) {
        return Some(parameter.default.clone());
    }
    module.active_parameter_value(&lane.parameter)
}

pub(super) fn bound_automation_parameter<'a>(
    effect: &'a EffectAsset,
    module: &aestra_core::ModuleInstance,
    input: &str,
) -> Option<&'a EffectParameter> {
    let parameter_id = module.bindings.get(input)?;
    effect
        .parameters
        .iter()
        .find(|parameter| parameter.id == *parameter_id)
}

pub(super) fn automation_lane_keys(
    effect: &EffectAsset,
    lane: &AutomationLaneId,
) -> Option<AutomationLaneKeys> {
    match automation_lane_value(effect, lane)? {
        Value::Curve(curve) => Some(AutomationLaneKeys::Curve(curve.keys, curve.interpolation)),
        Value::Vec3Curve(curves) => lane
            .channel
            .and_then(|channel| curves.curves.get(channel as usize))
            .map(|curve| AutomationLaneKeys::Curve(curve.keys.clone(), curve.interpolation)),
        Value::Gradient(gradient) => Some(AutomationLaneKeys::Gradient(gradient.keys)),
        _ => None,
    }
}

pub(super) fn add_automation_key_at_pointer_action(
    effect: &EffectAsset,
    lane: &AutomationLaneId,
    position: Vec2,
) -> Option<ChoreographyAction> {
    let normalized_time = (position.x + 0.5).clamp(0.0, 1.0);
    let value_bits = automation_lane_keys(effect, lane)?
        .graph_data()
        .value_for_top_percent((position.y + 0.5) * 100.0)
        .map(f32::to_bits);
    Some(ChoreographyAction::AddAutomationKeyAt {
        lane: lane.clone(),
        normalized_time_bits: normalized_time.to_bits(),
        value_bits,
    })
}
