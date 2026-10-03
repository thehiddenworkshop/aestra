//! Authored representative scene-light bindings and shared pulse contract.
use crate::{Curve, CurveKey};
use serde::{Deserialize, Serialize};

pub const MAX_LIGHT_CURVE_KEYS: usize = 32;

/// One representative point-light pulse, not one light per particle.
/// RGB is normalized linear color (0..1), intensity is lumens; radius/range use
/// the host's world units. Put radiance scale in intensity, not RGB channels.
/// Curves use normalized pulse age. Shadows are an adapter/host policy, off by default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointLightPulse {
    pub linear_color: [f32; 3],
    pub intensity_lumens: Curve,
    pub range: Curve,
    pub radius: f32,
    pub duration_seconds: f32,
}

impl PointLightPulse {
    /// Immediate flash with a rapid fade, independent of particle/trail lifetime.
    pub fn flash(color: [f32; 3], lumens: f32, range: f32, duration: f32) -> Self {
        Self {
            linear_color: color,
            intensity_lumens: Curve::new(vec![
                CurveKey::new(0.0, lumens),
                CurveKey::new(0.2, lumens * 0.25),
                CurveKey::new(1.0, 0.0),
            ]),
            range: Curve::new(vec![CurveKey::new(0.0, range), CurveKey::new(1.0, range)]),
            radius: 0.1,
            duration_seconds: duration,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.linear_color
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            && self.radius.is_finite()
            && self.radius >= 0.0
            && self.duration_seconds.is_finite()
            && self.duration_seconds > 0.0
            && valid_curve(&self.intensity_lumens, false)
            && valid_curve(&self.range, true)
    }

    /// Sample using occurrence time, never delivery-frame wall time. Future and
    /// expired pulses are absent; adapters decide whether to queue future intents.
    pub fn sample(&self, age_seconds: f32) -> Option<PointLightSample> {
        if !self.is_valid()
            || !age_seconds.is_finite()
            || age_seconds < 0.0
            || age_seconds >= self.duration_seconds
        {
            return None;
        }
        let age = age_seconds / self.duration_seconds;
        Some(PointLightSample {
            linear_color: self.linear_color,
            intensity_lumens: self.intensity_lumens.sample(age),
            range: self.range.sample(age),
            radius: self.radius,
        })
    }
}

fn valid_curve(curve: &Curve, positive: bool) -> bool {
    let valid_value =
        |value: f32| value.is_finite() && if positive { value > 0.0 } else { value >= 0.0 };
    !curve.keys.is_empty()
        && curve.keys.len() <= MAX_LIGHT_CURVE_KEYS
        && curve.keys.iter().all(|key| {
            key.time.is_finite()
                && (0.0..=1.0).contains(&key.time)
                && key.value.is_finite()
                && (curve.output_range.is_none() || (0.0..=1.0).contains(&key.value))
                && valid_value(curve.output_value(key.value))
        })
        && curve
            .keys
            .windows(2)
            .all(|pair| pair[0].time < pair[1].time)
        && curve.output_range.is_none_or(|range| {
            valid_value(range.min) && valid_value(range.max) && range.min <= range.max
        })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointLightSample {
    pub linear_color: [f32; 3],
    pub intensity_lumens: f32,
    pub range: f32,
    pub radius: f32,
}

/// A gradient parameter sampled at a fixed normalized age when a light is emitted.
/// This is pulse color selection, not a continuously changing particle-age input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LightColorParameter {
    pub parameter: crate::ParameterId,
    pub normalized_age: f32,
}

/// One representative light bound to a stable FirstPerTick particle output route.
/// Global enable, admission budgets and shadow policy belong to the host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointLightBinding {
    pub id: crate::EventRouteId,
    pub route: crate::EventRouteId,
    pub pulse: PointLightPulse,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_parameter: Option<LightColorParameter>,
}

impl PointLightBinding {
    pub fn new(route: crate::EventRouteId, pulse: PointLightPulse) -> Self {
        Self {
            id: crate::EventRouteId::new(),
            route,
            pulse,
            color_parameter: None,
        }
    }
}

pub(crate) fn validate_bindings(
    effect: &crate::EffectAsset,
    report: &mut crate::ValidationReport,
    ids: &mut std::collections::BTreeMap<u128, String>,
) {
    use crate::{Diagnostic, DiagnosticCode, EventAggregation, Value};
    let mut bound = std::collections::BTreeSet::new();
    for (index, binding) in effect.point_lights.iter().enumerate() {
        let path = format!("effect.point_lights[{index}]");
        crate::model::register_id(
            report,
            ids,
            binding.id.as_uuid().as_u128(),
            format!("{path}.id"),
        );
        for (name, curve) in [
            ("intensity_lumens", &binding.pulse.intensity_lumens),
            ("range", &binding.pulse.range),
        ] {
            crate::model::register_id(
                report,
                ids,
                curve.id.as_uuid().as_u128(),
                format!("{path}.pulse.{name}.id"),
            );
        }
        let route = effect
            .particle_outputs
            .iter()
            .find(|r| r.id == binding.route);
        match route {
            None => report.push(Diagnostic::error(
                DiagnosticCode::InvalidReference,
                format!("{path}.route"),
                "light binding references a missing particle output route",
            )),
            Some(route) => {
                if route.aggregation != EventAggregation::FirstPerTick {
                    report.push(Diagnostic::error(
                        DiagnosticCode::InvalidValue,
                        format!("{path}.route"),
                        "representative lights require FirstPerTick aggregation",
                    ));
                }
                if effect
                    .particle_outputs
                    .iter()
                    .filter(|r| r.source == route.source && r.output == route.output)
                    .count()
                    != 1
                {
                    report.push(Diagnostic::error(
                        DiagnosticCode::InvalidValue,
                        format!("{path}.route"),
                        "light-bound output/emitter pair must identify one particle route",
                    ));
                }
            }
        }
        if !bound.insert(binding.route) {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidValue,
                format!("{path}.route"),
                "only one representative light binding is supported per particle route",
            ));
        }
        if !binding.pulse.is_valid() {
            report.push(Diagnostic::error(DiagnosticCode::InvalidValue,
                format!("{path}.pulse"), "light pulse requires finite normalized RGB, positive duration/range and ordered curves of at most 32 keys"));
        }
        if let Some(color) = &binding.color_parameter {
            if !color.normalized_age.is_finite() || !(0.0..=1.0).contains(&color.normalized_age) {
                report.push(Diagnostic::error(
                    DiagnosticCode::InvalidValue,
                    format!("{path}.color_parameter.normalized_age"),
                    "gradient sample age must be finite and in 0..1",
                ));
            }
            let valid = effect.parameters.iter().find(|p| p.id == color.parameter).is_some_and(|p| {
                matches!(&p.default, Value::Gradient(g)
                    if !g.keys.is_empty() && g.keys.iter().all(|k| k.color[..3].iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))))
            });
            if !valid {
                report.push(Diagnostic::error(
                    DiagnosticCode::InvalidReference,
                    format!("{path}.color_parameter.parameter"),
                    "light color requires a gradient parameter with normalized finite RGB defaults",
                ));
            }
        }
    }
}
