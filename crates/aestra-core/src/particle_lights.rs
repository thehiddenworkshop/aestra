//! Material-free, particle-backed presentation sinks. These do not emit events.
use crate::{Curve, CurveKey, ParameterId, SceneOutputId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCENE_OUTPUT_PARTICLE_POINT_LIGHT: &str = "aestra.scene_output.particle_point_light";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SceneOutputTypeId(pub String);

impl SceneOutputTypeId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Color is normalized linear RGB; radiance belongs in the lumens curve.
/// Gradient parameters are sampled at the source particle's normalized age.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ParticleLightColorSource {
    ParticleColor,
    Constant([f32; 3]),
    GradientParameter(ParameterId),
}

/// Presentation admission, never simulation or event ordering. The first built-in
/// policy orders by intensity, then stable particle identity for equal importance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParticleLightSelectionPolicy {
    Brightest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParticlePointLightProperties {
    pub color_source: ParticleLightColorSource,
    /// Normalized particle age -> lumens, independent of sprite opacity/size.
    pub intensity_curve: Curve,
    /// Normalized particle age -> host world units.
    pub range_curve: Curve,
    pub radius: f32,
    pub selection_policy: ParticleLightSelectionPolicy,
    /// Named compile-time tiers; `high` is required and is the custom-tier fallback.
    /// Zero explicitly disables this output. Host/global admission may lower these caps.
    pub max_lights_by_quality: BTreeMap<String, u32>,
    /// Higher priorities are admitted first by presentation selection.
    pub priority: u32,
}

impl ParticlePointLightProperties {
    pub fn new(lumens: f32, range: f32) -> Self {
        Self {
            color_source: ParticleLightColorSource::ParticleColor,
            intensity_curve: Curve::new(vec![CurveKey::new(0.0, lumens), CurveKey::new(1.0, 0.0)]),
            range_curve: Curve::new(vec![CurveKey::new(0.0, range)]),
            radius: 0.1,
            selection_policy: ParticleLightSelectionPolicy::Brightest,
            max_lights_by_quality: BTreeMap::from([
                ("high".into(), 16),
                ("medium".into(), 8),
                ("low".into(), 4),
            ]),
            priority: 0,
        }
    }

    pub fn is_valid(&self) -> bool {
        crate::scene_outputs::valid_curve(&self.intensity_curve, false)
            && crate::scene_outputs::valid_curve(&self.range_curve, true)
            && self.radius.is_finite()
            && self.radius >= 0.0
            && match self.color_source {
                ParticleLightColorSource::Constant(rgb) => valid_light_rgb(rgb),
                _ => true,
            }
            && self.max_lights_by_quality.contains_key("high")
            && self
                .max_lights_by_quality
                .keys()
                .all(|name| !name.is_empty() && name.trim() == name)
    }

    pub fn max_lights(&self, tier: &str) -> u32 {
        self.max_lights_by_quality
            .get(tier)
            .or_else(|| self.max_lights_by_quality.get("high"))
            .copied()
            .unwrap_or(0)
    }
}

pub fn valid_light_rgb(rgb: [f32; 3]) -> bool {
    rgb.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SceneOutputProperties {
    ParticlePointLight(ParticlePointLightProperties),
}

/// Separate from material-backed renderers; no material or host-event route is needed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneOutputInstance {
    pub id: SceneOutputId,
    pub output_type: SceneOutputTypeId,
    pub enabled: bool,
    pub properties: SceneOutputProperties,
}

impl SceneOutputInstance {
    pub fn particle_point_light(properties: ParticlePointLightProperties) -> Self {
        Self {
            id: SceneOutputId::new(),
            output_type: SceneOutputTypeId(SCENE_OUTPUT_PARTICLE_POINT_LIGHT.into()),
            enabled: true,
            properties: SceneOutputProperties::ParticlePointLight(properties),
        }
    }

    pub fn regenerate_ids(&mut self) {
        self.id = SceneOutputId::new();
        let SceneOutputProperties::ParticlePointLight(light) = &mut self.properties;
        light.intensity_curve.id = crate::CurveId::new();
        light.range_curve.id = crate::CurveId::new();
    }
}

pub(crate) fn validate_particle_outputs(
    effect: &crate::EffectAsset,
    report: &mut crate::ValidationReport,
    ids: &mut BTreeMap<u128, String>,
) {
    use crate::{Diagnostic, DiagnosticCode, Value};
    for (emitter_index, emitter) in effect.emitters.iter().enumerate() {
        for (index, output) in emitter.scene_outputs.iter().enumerate() {
            let path = format!("effect.emitters[{emitter_index}].scene_outputs[{index}]");
            crate::model::register_id(
                report,
                ids,
                output.id.as_uuid().as_u128(),
                format!("{path}.id"),
            );
            let SceneOutputProperties::ParticlePointLight(light) = &output.properties;
            for (name, curve) in [
                ("intensity_curve", &light.intensity_curve),
                ("range_curve", &light.range_curve),
            ] {
                crate::model::register_id(
                    report,
                    ids,
                    curve.id.as_uuid().as_u128(),
                    format!("{path}.properties.{name}.id"),
                );
            }
            if output.output_type.0 != SCENE_OUTPUT_PARTICLE_POINT_LIGHT {
                report.push(Diagnostic::error(
                    DiagnosticCode::InvalidValue,
                    format!("{path}.output_type"),
                    "unregistered scene-output type or incompatible properties",
                ));
            }
            if !light.is_valid() {
                report.push(Diagnostic::error(DiagnosticCode::InvalidValue, format!("{path}.properties"), "particle lights require finite normalized RGB, nonnegative radius/intensity, positive range, ordered curves of at most 32 keys, and named limits including high"));
            }
            if let ParticleLightColorSource::GradientParameter(id) = light.color_source {
                let valid = effect.parameters.iter().find(|p| p.id == id).is_some_and(|p| {
                    matches!(&p.default, Value::Gradient(g) if !g.keys.is_empty() && g.keys.iter().all(|k| valid_light_rgb([k.color[0], k.color[1], k.color[2]])))
                });
                if !valid {
                    report.push(Diagnostic::error(DiagnosticCode::InvalidReference, format!("{path}.properties.color_source"), "particle-light color requires an existing gradient parameter with finite normalized RGB defaults"));
                }
            }
        }
    }
}
