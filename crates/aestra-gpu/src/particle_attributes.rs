//! Live presentation requirements without changing particle storage/readback layout.
use aestra_core::material::MaterialInput;

use crate::{GpuEmitter, GpuRenderer, material::MaterialReflection};

/// Bit assignments shared with the portable simulation and sprite shaders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuParticleAttributes(pub u32);

impl GpuParticleAttributes {
    pub const POSITION: u32 = 1;
    pub const SIZE: u32 = 2;
    pub const ROTATION: u32 = 4;
    pub const COLOR: u32 = 8;
    pub const OPACITY: u32 = 16;
    pub const NORMALIZED_AGE: u32 = 32;
    pub const ALL: Self = Self(63);

    pub fn count(self) -> u32 {
        (self.0 & Self::ALL.0).count_ones()
    }

    /// Geometry is always live. Material reflection must come from optimized IR.
    /// `None` conservatively selects the legacy renderer (also used on fallback).
    pub fn for_renderer(
        renderer: &GpuRenderer,
        material: Option<&MaterialReflection>,
        wireframe: bool,
    ) -> Self {
        Self::for_inputs(
            renderer,
            material.map(|m| {
                (
                    m.required_particle_inputs.as_slice(),
                    m.required_vertex_inputs.contains(&MaterialInput::Uv0),
                )
            }),
            wireframe,
        )
    }

    fn for_inputs(
        renderer: &GpuRenderer,
        material: Option<(&[MaterialInput], bool)>,
        wireframe: bool,
    ) -> Self {
        // History may outlive its current material binding. Preserve recorded attributes.
        if renderer.renderer_kind == 4 {
            return Self::ALL;
        }
        let mut required = Self::POSITION | Self::SIZE | Self::ROTATION;
        let needs_uv = if wireframe {
            if renderer.particle_color != 0 {
                required |= Self::COLOR;
            }
            false
        } else if let Some((particle_inputs, uv)) = material {
            for input in particle_inputs {
                required |= match input {
                    MaterialInput::ParticleColor => Self::COLOR | Self::OPACITY,
                    MaterialInput::ParticleOpacity => Self::OPACITY,
                    MaterialInput::ParticleNormalizedAge => Self::NORMALIZED_AGE,
                    // Future inputs must explicitly declare their storage dependencies.
                    _ => Self::ALL.0,
                };
            }
            uv
        } else {
            if renderer.particle_color != 0 {
                required |= Self::COLOR | Self::OPACITY;
            }
            renderer.textured != 0
        };
        if needs_uv
            && renderer.renderer_kind == 1
            && renderer.frame_count > 1
            && renderer.flipbook_flags & 1 == 0
        {
            required |= Self::NORMALIZED_AGE;
        }
        if renderer.particle_color == 0 {
            required &= !(Self::COLOR | Self::OPACITY);
        }
        Self(required)
    }
}

/// Static estimate for the Compiler Inspector. Runtime bindings/modes may change it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuParticleAttributeSummary {
    pub live: u32,
    pub omitted: u32,
}

pub fn estimate_particle_attributes(
    instance: &aestra_runtime::EffectInstance,
) -> Result<GpuParticleAttributeSummary, crate::GpuArtifactError> {
    use aestra_compiler::{MaterialCompiler, reflect_material_inputs};
    let mut dynamics = crate::GpuEffectArtifact::dynamics_from_instance(instance)?;
    let inputs = instance
        .effect()
        .material_programs
        .iter()
        .filter_map(|program| {
            MaterialCompiler
                .compile(program)
                .ok()
                .map(|ir| (program.id, reflect_material_inputs(&ir)))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let requirements = dynamics
        .renderers
        .iter()
        .zip(
            instance
                .effect()
                .emitters
                .iter()
                .filter(|emitter| emitter.enabled)
                .flat_map(|emitter| &emitter.renderers),
        )
        .map(|(renderer, plan)| {
            let material = instance
                .effect()
                .material_instance(plan.material)
                .and_then(|material| inputs.get(&material.program.id()));
            GpuParticleAttributes::for_inputs(
                renderer,
                material.map(|m| {
                    (
                        m.particle.as_slice(),
                        m.vertex.contains(&MaterialInput::Uv0),
                    )
                }),
                false,
            )
        })
        .collect::<Vec<_>>();
    prune_particle_attributes(
        &mut dynamics.emitters,
        &mut dynamics.renderers,
        &requirements,
    );
    // Static authoring estimates include authored scene-output consumers. The
    // live adapter can omit these again when the host disables particle lights.
    retain_particle_light_attributes(&mut dynamics.emitters, instance.effect(), true);
    let omitted = dynamics
        .emitters
        .iter()
        .map(|emitter| GpuParticleAttributes(emitter.omitted_attributes).count())
        .sum();
    Ok(GpuParticleAttributeSummary {
        live: dynamics.emitters.len() as u32 * GpuParticleAttributes::ALL.count() - omitted,
        omitted,
    })
}

/// Union all consumers before pruning simulation. Re-run after binding/mode changes.
/// A zero omission mask preserves the historical full CPU-reference readback contract.
pub fn prune_particle_attributes(
    emitters: &mut [GpuEmitter],
    renderers: &mut [GpuRenderer],
    requirements: &[GpuParticleAttributes],
) {
    assert_eq!(renderers.len(), requirements.len());
    for emitter in emitters.iter_mut() {
        emitter.omitted_attributes = GpuParticleAttributes::ALL.0;
    }
    for (renderer, required) in renderers.iter_mut().zip(requirements) {
        renderer.attribute_flags.x = GpuParticleAttributes::ALL.0 & !required.0;
        emitters[renderer.emitter_index as usize].omitted_attributes &= !required.0;
    }
}

/// Scene outputs are presentation consumers too, even without a material or
/// renderer. Keep only their actual inputs; disabled host policy adds none.
pub fn retain_particle_light_attributes(
    emitters: &mut [GpuEmitter],
    effect: &aestra_runtime::CompiledEffect,
    enabled: bool,
) {
    if !enabled {
        return;
    }
    for (gpu, emitter) in emitters.iter_mut().zip(&effect.emitters) {
        if !emitter.enabled {
            continue;
        }
        for output in &emitter.scene_outputs {
            let aestra_runtime::SceneOutputPlanKind::ParticlePointLight(plan) = &output.kind;
            if plan.max_lights == 0 {
                continue;
            }
            let mut required =
                GpuParticleAttributes::POSITION | GpuParticleAttributes::NORMALIZED_AGE;
            if matches!(
                plan.color,
                aestra_runtime::ParticleLightColorPlan::ParticleColor
            ) {
                required |= GpuParticleAttributes::COLOR;
            }
            gpu.omitted_attributes &= !required;
        }
    }
}

#[cfg(test)]
mod light_tests {
    use super::*;
    use aestra_core::*;
    use aestra_runtime::EffectInstance;
    use std::sync::Arc;

    #[test]
    fn light_only_consumers_preserve_age_and_color_without_sprite_attributes() {
        let mut source = EffectAsset::new("lights", 3.0);
        let mut emitter = Emitter::basic_sprite("light only", 3.0);
        emitter.renderers.clear();
        emitter
            .scene_outputs
            .push(SceneOutputInstance::particle_point_light(
                ParticlePointLightProperties::new(10.0, 2.0),
            ));
        source.emitters.push(emitter);
        let compiled = Arc::new(
            aestra_compiler::EffectCompiler::default()
                .compile(&source)
                .unwrap(),
        );
        let mut gpu = crate::GpuEffectArtifact::dynamics_from_instance(&EffectInstance::new(
            compiled.clone(),
        ))
        .unwrap();
        prune_particle_attributes(&mut gpu.emitters, &mut gpu.renderers, &[]);
        let all_omitted = gpu.emitters[0].omitted_attributes;
        retain_particle_light_attributes(&mut gpu.emitters, &compiled, false);
        assert_eq!(gpu.emitters[0].omitted_attributes, all_omitted);
        retain_particle_light_attributes(&mut gpu.emitters, &compiled, true);
        let required = GpuParticleAttributes::POSITION
            | GpuParticleAttributes::NORMALIZED_AGE
            | GpuParticleAttributes::COLOR;
        assert_eq!(
            gpu.emitters[0].omitted_attributes,
            GpuParticleAttributes::ALL.0 & !required
        );
        let mut constant = (*compiled).clone();
        let aestra_runtime::SceneOutputPlanKind::ParticlePointLight(plan) =
            &mut constant.emitters[0].scene_outputs[0].kind;
        plan.color = aestra_runtime::ParticleLightColorPlan::Constant([1.0; 3]);
        gpu.emitters[0].omitted_attributes = all_omitted;
        retain_particle_light_attributes(&mut gpu.emitters, &constant, true);
        assert_ne!(
            gpu.emitters[0].omitted_attributes & GpuParticleAttributes::COLOR,
            0
        );
        assert_eq!(
            gpu.emitters[0].omitted_attributes & GpuParticleAttributes::NORMALIZED_AGE,
            0
        );
        constant.emitters[0].enabled = false;
        gpu.emitters[0].omitted_attributes = all_omitted;
        retain_particle_light_attributes(&mut gpu.emitters, &constant, true);
        assert_eq!(gpu.emitters[0].omitted_attributes, all_omitted);
    }
}
