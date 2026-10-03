//! Read-only particle scene-output evaluation. No event queue or simulation state.
use crate::{
    CompiledCurve, CompiledEmitter, CompiledGradient, ParameterSlot, ParticleSample, RuntimeValue,
};
use aestra_core::{EmitterId, EmitterRegionId, ParticleLightSelectionPolicy, SceneOutputId};

#[derive(Debug, Clone, PartialEq)]
pub enum ParticleLightColorPlan {
    ParticleColor,
    Constant([f32; 3]),
    Gradient(CompiledGradient),
    GradientParameter(ParameterSlot),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParticlePointLightPlan {
    pub color: ParticleLightColorPlan,
    pub intensity: CompiledCurve,
    pub range: CompiledCurve,
    pub radius: f32,
    pub selection_policy: ParticleLightSelectionPolicy,
    /// Resolved named compile-time quality limit; host/global caps still apply.
    pub max_lights: u32,
    pub priority: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SceneOutputPlanKind {
    ParticlePointLight(ParticlePointLightPlan),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneOutputPlan {
    pub source: SceneOutputId,
    pub kind: SceneOutputPlanKind,
}

/// Stable within one effect presentation occurrence. Hosts add the root instance,
/// epoch and child clip/occurrence path before combining candidates across instances.
/// Never use input vector order as a particle identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ParticleLightIdentity {
    pub emitter: EmitterId,
    pub region: EmitterRegionId,
    pub output: SceneOutputId,
    pub particle_index: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParticleLightCandidate {
    pub identity: ParticleLightIdentity,
    /// Same coordinate space as the supplied presentation sample. Adapters apply
    /// the effect/host transform exactly once before realizing world-space lights.
    pub position: [f32; 3],
    pub light: aestra_core::PointLightSample,
    pub priority: u32,
}

impl SceneOutputPlan {
    /// Evaluate an alive presentation sample. Dead particles must not be supplied:
    /// `ParticleSample` has no alive flag. Expired/invalid ages and invalid live color
    /// overrides fail closed. Particle opacity and size do not scale lumens/range.
    pub fn particle_light_candidate<'a>(
        &self,
        emitter: &CompiledEmitter,
        particle: &ParticleSample,
        mut parameter: impl FnMut(ParameterSlot) -> Option<&'a RuntimeValue>,
    ) -> Option<ParticleLightCandidate> {
        let SceneOutputPlanKind::ParticlePointLight(plan) = &self.kind;
        let age = particle.normalized_age;
        if !emitter.enabled
            || plan.max_lights == 0
            || !age.is_finite()
            || !(0.0..1.0).contains(&age)
            || !particle.position.iter().all(|v| v.is_finite())
        {
            return None;
        }
        let rgb = match &plan.color {
            ParticleLightColorPlan::ParticleColor => particle.color[..3].try_into().unwrap(),
            ParticleLightColorPlan::Constant(rgb) => *rgb,
            ParticleLightColorPlan::Gradient(gradient) => {
                gradient.first()?;
                gradient.sample(age)[..3].try_into().unwrap()
            }
            ParticleLightColorPlan::GradientParameter(slot) => {
                let RuntimeValue::Gradient(gradient) = parameter(*slot)? else {
                    return None;
                };
                gradient.first()?;
                gradient.sample(age)[..3].try_into().unwrap()
            }
        };
        let intensity = plan.intensity.sample(age);
        let range = plan.range.sample(age);
        if !aestra_core::valid_light_rgb(rgb)
            || !intensity.is_finite()
            || intensity <= 0.0
            || !range.is_finite()
            || range <= 0.0
            || !plan.radius.is_finite()
            || plan.radius < 0.0
        {
            return None;
        }
        Some(ParticleLightCandidate {
            identity: ParticleLightIdentity {
                emitter: emitter.source,
                region: emitter.region,
                output: self.source,
                particle_index: particle.particle_index,
            },
            position: particle.position,
            light: aestra_core::PointLightSample {
                linear_color: rgb,
                intensity_lumens: intensity,
                range,
                radius: plan.radius,
            },
            priority: plan.priority,
        })
    }
}

impl crate::CompiledEffect {
    /// Allocation-free reference candidate iterator, not a host event stream or
    /// selected-light list. Admission/compaction/top-K belongs to presentation.
    pub fn particle_light_candidates<'a>(
        &'a self,
        particle: &'a ParticleSample,
        parameters: &'a [RuntimeValue],
    ) -> impl Iterator<Item = ParticleLightCandidate> + 'a {
        self.emitters
            .get(particle.emitter_index)
            .into_iter()
            .flat_map(move |emitter| {
                emitter.scene_outputs.iter().filter_map(move |output| {
                    output
                        .particle_light_candidate(emitter, particle, |slot| parameters.get(slot.0))
                })
            })
    }
}
