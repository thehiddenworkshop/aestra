use aestra_core::{EffectAsset, EventAggregation, ValueType};
use aestra_runtime::{
    CompiledEventRoute, CompiledParameter, CompiledPointLightBinding, ParameterSlot,
};
use std::collections::BTreeMap;

pub(crate) fn compile_particle_outputs(
    asset: &EffectAsset,
    emitter: &aestra_core::Emitter,
    slots: &BTreeMap<aestra_core::ParameterId, ParameterSlot>,
    tier: &str,
) -> Vec<aestra_runtime::SceneOutputPlan> {
    use aestra_core::{ParticleLightColorSource, SceneOutputProperties, Value};
    use aestra_runtime::{
        CompiledCurve, CompiledGradient, ParticleLightColorPlan, ParticlePointLightPlan,
        SceneOutputPlan, SceneOutputPlanKind,
    };
    emitter
        .scene_outputs
        .iter()
        .filter(|output| emitter.enabled && output.enabled)
        .map(|output| {
            let SceneOutputProperties::ParticlePointLight(light) = &output.properties;
            let color = match light.color_source {
                ParticleLightColorSource::ParticleColor => ParticleLightColorPlan::ParticleColor,
                ParticleLightColorSource::Constant(rgb) => ParticleLightColorPlan::Constant(rgb),
                ParticleLightColorSource::GradientParameter(id) => {
                    if let Some(slot) = slots.get(&id) {
                        ParticleLightColorPlan::GradientParameter(*slot)
                    } else {
                        let Value::Gradient(gradient) = &asset
                            .parameters
                            .iter()
                            .find(|p| p.id == id)
                            .expect("validated scene-output parameter exists")
                            .default
                        else {
                            unreachable!("validated scene-output gradient type");
                        };
                        ParticleLightColorPlan::Gradient(CompiledGradient::compile(gradient))
                    }
                }
            };
            SceneOutputPlan {
                source: output.id,
                kind: SceneOutputPlanKind::ParticlePointLight(ParticlePointLightPlan {
                    color,
                    intensity: CompiledCurve::compile(&light.intensity_curve),
                    range: CompiledCurve::compile(&light.range_curve),
                    radius: light.radius,
                    selection_policy: light.selection_policy,
                    max_lights: light.max_lights(tier),
                    priority: light.priority,
                }),
            }
        })
        .collect()
}

/// Source validation has checked stable references and representative semantics.
pub(crate) fn compile(
    asset: &EffectAsset,
    routes: &[CompiledEventRoute],
    emitters: &[aestra_runtime::CompiledEmitter],
    slots: &BTreeMap<aestra_core::ParameterId, ParameterSlot>,
    parameters: &[CompiledParameter],
) -> Vec<CompiledPointLightBinding> {
    asset.point_lights.iter().filter_map(|binding| {
        let authored = asset.particle_outputs.iter().find(|r| r.id == binding.route)?;
        let output = asset.event_outputs.iter().find(|o| o.id == authored.output)?;
        let route = routes.iter().position(|route| matches!(route,
            CompiledEventRoute::ParticleOutput(r)
                if r.output == output.name && emitters[r.source].source == authored.source
                    && r.trigger == authored.trigger && r.aggregation == EventAggregation::FirstPerTick))?;
        let mut pulse = binding.pulse.clone();
        let color_parameter = binding.color_parameter.as_ref().and_then(|color| {
            if let Some(slot) = slots.get(&color.parameter).copied() {
                debug_assert_eq!(parameters[slot.0].value_type, ValueType::Gradient);
                Some((slot, color.normalized_age))
            } else {
                // Non-exposed parameters are constants, as in module/material lowering.
                let parameter = asset.parameters.iter().find(|p| p.id == color.parameter)
                    .expect("validated color parameter exists");
                let aestra_core::Value::Gradient(gradient) = &parameter.default else {
                    unreachable!("validated light color is a gradient");
                };
                let [r, g, b, _] = gradient.sample(color.normalized_age);
                pulse.linear_color = [r, g, b];
                None
            }
        });
        Some(CompiledPointLightBinding {
            source: binding.id, route, pulse, color_parameter,
        })
    }).collect()
}
