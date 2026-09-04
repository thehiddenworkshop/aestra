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
