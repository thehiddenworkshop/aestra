//! Explicit v7 DTOs for material-free particle scene outputs.
use super::*;
use aestra_runtime::{
    ParticleLightColorPlan, ParticlePointLightPlan, SceneOutputPlan, SceneOutputPlanKind,
};

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct SceneOutputV7 {
    source: aestra_core::SceneOutputId,
    kind: SceneOutputKindV7,
}

#[derive(Debug, Serialize, Deserialize)]
enum SceneOutputKindV7 {
    ParticlePointLight {
        color: ParticleColorV7,
        intensity: CurveV1,
        range: CurveV1,
        radius: f32,
        selection_policy: aestra_core::ParticleLightSelectionPolicy,
        max_lights: u32,
        priority: u32,
    },
}

#[derive(Debug, Serialize, Deserialize)]
enum ParticleColorV7 {
    ParticleColor,
    Constant([f32; 3]),
    Gradient(GradientV1),
    GradientParameter(u32),
}

impl SceneOutputV7 {
    pub(super) fn encode(output: &SceneOutputPlan, path: &str) -> Result<Self, ArtifactError> {
        let SceneOutputPlanKind::ParticlePointLight(light) = &output.kind;
        Ok(Self {
            source: output.source,
            kind: SceneOutputKindV7::ParticlePointLight {
                color: match &light.color {
                    ParticleLightColorPlan::ParticleColor => ParticleColorV7::ParticleColor,
                    ParticleLightColorPlan::Constant(rgb) => ParticleColorV7::Constant(*rgb),
                    ParticleLightColorPlan::Gradient(g) => {
                        ParticleColorV7::Gradient(GradientV1::from(g))
                    }
                    ParticleLightColorPlan::GradientParameter(slot) => {
                        ParticleColorV7::GradientParameter(encode_u32(
                            slot.0,
                            format!("{path}.color"),
                        )?)
                    }
                },
                intensity: CurveV1::from(&light.intensity),
                range: CurveV1::from(&light.range),
                radius: light.radius,
                selection_policy: light.selection_policy,
                max_lights: light.max_lights,
                priority: light.priority,
            },
        })
    }

    pub(super) fn decode(
        self,
        parameters: &[CompiledParameter],
        path: &str,
    ) -> Result<SceneOutputPlan, ArtifactError> {
        if self.source.is_nil() {
            return invalid(
                format!("{path}.source"),
                "scene-output identity cannot be nil",
            );
        }
        let SceneOutputKindV7::ParticlePointLight {
            color,
            intensity,
            range,
            radius,
            selection_policy,
            max_lights,
            priority,
        } = self.kind;
        require_finite_non_negative(radius, format!("{path}.radius"))?;
        let intensity = light_curve(intensity, false, &format!("{path}.intensity"))?;
        let range = light_curve(range, true, &format!("{path}.range"))?;
        let color = match color {
            ParticleColorV7::ParticleColor => ParticleLightColorPlan::ParticleColor,
            ParticleColorV7::Constant(rgb) => {
                if !aestra_core::valid_light_rgb(rgb) {
                    return invalid(format!("{path}.color"), "requires finite normalized RGB");
                }
                ParticleLightColorPlan::Constant(rgb)
            }
            ParticleColorV7::Gradient(g) => {
                light_gradient(&g, &format!("{path}.color"))?;
                ParticleLightColorPlan::Gradient(g.decode(&format!("{path}.color"))?)
            }
            ParticleColorV7::GradientParameter(slot) => {
                let slot = slot as usize;
                let Some(CompiledParameter {
                    value_type: ValueType::Gradient,
                    default: RuntimeValue::Gradient(g),
                    ..
                }) = parameters.get(slot)
                else {
                    return invalid(
                        format!("{path}.color"),
                        "requires an existing gradient parameter slot",
                    );
                };
                light_gradient(&GradientV1::from(g), &format!("{path}.color"))?;
                ParticleLightColorPlan::GradientParameter(ParameterSlot(slot))
            }
        };
        Ok(SceneOutputPlan {
            source: self.source,
            kind: SceneOutputPlanKind::ParticlePointLight(ParticlePointLightPlan {
                color,
                intensity,
                range,
                radius,
                selection_policy,
                max_lights,
                priority,
            }),
        })
    }
}

fn light_curve(curve: CurveV1, positive: bool, path: &str) -> Result<CompiledCurve, ArtifactError> {
    if curve.keys.is_empty()
        || curve.keys.len() > aestra_core::MAX_LIGHT_CURVE_KEYS
        || curve.keys.iter().any(|k| {
            if positive {
                k.value <= 0.0
            } else {
                k.value < 0.0
            }
        })
    {
        return invalid(
            path,
            "light curve requires 1..32 keys with nonnegative intensity/positive range",
        );
    }
    curve.decode(path)
}

fn light_gradient(gradient: &GradientV1, path: &str) -> Result<(), ArtifactError> {
    if gradient.keys.is_empty()
        || gradient
            .keys
            .iter()
            .any(|k| !aestra_core::valid_light_rgb([k.color[0], k.color[1], k.color[2]]))
    {
        return invalid(path, "light gradient requires finite normalized RGB keys");
    }
    validate_gradient_keys(&gradient.keys, path)
}
