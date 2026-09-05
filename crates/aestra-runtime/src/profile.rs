use crate::{CompiledEffect, ParticleAttribute, ParticleSample};
use aestra_core::EmitterId;
use std::time::Duration;

/// Describes whether a profiler value was observed, inferred, or cannot be collected.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProfileValue<T> {
    Measured(T),
    Estimated(T),
    Unavailable,
}

impl<T: Copy> ProfileValue<T> {
    pub fn value(self) -> Option<T> {
        match self {
            Self::Measured(value) | Self::Estimated(value) => Some(value),
            Self::Unavailable => None,
        }
    }

    pub const fn source(self) -> ProfileValueSource {
        match self {
            Self::Measured(_) => ProfileValueSource::Measured,
            Self::Estimated(_) => ProfileValueSource::Estimated,
            Self::Unavailable => ProfileValueSource::Unavailable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileValueSource {
    Measured,
    Estimated,
    Unavailable,
}

/// Per-emitter values collected without changing simulation state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmitterProfile {
    pub source: EmitterId,
    pub name: String,
    pub alive_particles: u32,
    pub peak_particles: u32,
    pub particle_capacity: u32,
}

/// Machine-readable runtime and compiler cost snapshot for one effect instance.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectProfile {
    pub cpu_time_ns: ProfileValue<u64>,
    pub gpu_time_ns: ProfileValue<u64>,
    pub alive_particles: ProfileValue<u32>,
    pub submitted_instances: ProfileValue<u32>,
    pub peak_particles: ProfileValue<u32>,
    pub particle_capacity: ProfileValue<u32>,
    pub emitter_count: ProfileValue<u32>,
    pub draw_calls: ProfileValue<u32>,
    pub dispatch_count: ProfileValue<u32>,
    pub estimated_overdraw: ProfileValue<f32>,
    pub texture_sample_count: ProfileValue<u32>,
    pub buffer_memory_bytes: ProfileValue<u64>,
    pub texture_memory_bytes: ProfileValue<u64>,
    pub collision_time_ns: ProfileValue<u64>,
    pub emitters: Vec<EmitterProfile>,
    pub platform_warnings: Vec<String>,
}

impl EffectProfile {
    pub fn from_compiled(effect: &CompiledEffect) -> Self {
        let emitter_count = effect.emitters.len().min(u32::MAX as usize) as u32;
        let draw_calls = effect
            .emitters
            .iter()
            .filter(|emitter| emitter.enabled)
            .flat_map(|emitter| emitter.renderers.iter())
            .count()
            .min(u32::MAX as usize) as u32;
        let texture_sample_count = effect
            .emitters
            .iter()
            .filter(|emitter| emitter.enabled)
            .flat_map(|emitter| emitter.renderers.iter())
            .filter(|renderer| {
                effect
                    .material(renderer.material)
                    .is_some_and(|material| material.texture.is_some())
            })
            .count()
            .min(u32::MAX as usize) as u32;
        let has_ribbons = effect.emitters.iter().filter(|e| e.enabled).any(|e| {
            e.renderers.iter().any(|r| {
                matches!(
                    r.kind,
                    crate::RendererPlanKind::Ribbon { .. } | crate::RendererPlanKind::Trail { .. }
                )
            })
        });
        let has_trails = effect.emitters.iter().filter(|e| e.enabled).any(|e| {
            e.renderers
                .iter()
                .any(|r| matches!(r.kind, crate::RendererPlanKind::Trail { .. }))
        });
        let dispatch_count = u32::from(effect.max_particles > 0)
            * (2 + u32::from(has_ribbons) + u32::from(has_trails));
        Self {
            cpu_time_ns: ProfileValue::Unavailable,
            gpu_time_ns: ProfileValue::Unavailable,
            alive_particles: ProfileValue::Unavailable,
            submitted_instances: ProfileValue::Unavailable,
            peak_particles: ProfileValue::Unavailable,
            particle_capacity: ProfileValue::Measured(
                effect.max_particles.min(u32::MAX as usize) as u32
            ),
            emitter_count: ProfileValue::Measured(emitter_count),
            draw_calls: ProfileValue::Estimated(draw_calls),
            dispatch_count: ProfileValue::Estimated(dispatch_count),
            estimated_overdraw: ProfileValue::Unavailable,
            texture_sample_count: ProfileValue::Estimated(texture_sample_count),
            buffer_memory_bytes: ProfileValue::Estimated(estimated_buffer_memory(effect)),
            texture_memory_bytes: if texture_sample_count == 0 {
                ProfileValue::Estimated(0)
            } else {
                ProfileValue::Unavailable
            },
            collision_time_ns: ProfileValue::Unavailable,
            emitters: effect
                .emitters
                .iter()
                .map(|emitter| EmitterProfile {
                    source: emitter.source,
                    name: emitter.name.clone(),
                    alive_particles: 0,
                    peak_particles: 0,
                    particle_capacity: emitter.max_particles,
                })
                .collect(),
            platform_warnings: Vec::new(),
        }
    }

    pub fn matches_compiled(&self, effect: &CompiledEffect) -> bool {
        self.emitters.len() == effect.emitters.len()
            && self
                .emitters
                .iter()
                .zip(&effect.emitters)
                .all(|(profile, emitter)| {
                    profile.source == emitter.source
                        && profile.particle_capacity == emitter.max_particles
                })
    }

    pub fn record_cpu_frame(&mut self, elapsed: Duration, samples: &[ParticleSample]) {
        self.cpu_time_ns =
            ProfileValue::Measured(elapsed.as_nanos().min(u128::from(u64::MAX)) as u64);
        self.record_particle_frame(samples);
    }

    pub fn record_particle_frame(&mut self, samples: &[ParticleSample]) {
        for emitter in &mut self.emitters {
            emitter.alive_particles = 0;
        }
        for sample in samples {
            if let Some(emitter) = self.emitters.get_mut(sample.emitter_index) {
                emitter.alive_particles = emitter.alive_particles.saturating_add(1);
            }
        }
        for emitter in &mut self.emitters {
            emitter.peak_particles = emitter.peak_particles.max(emitter.alive_particles);
        }
        let alive = samples.len().min(u32::MAX as usize) as u32;
        let previous_peak = self.peak_particles.value().unwrap_or_default();
        self.alive_particles = ProfileValue::Measured(alive);
        self.peak_particles = ProfileValue::Measured(previous_peak.max(alive));
    }

    /// Records the number of particle instances presented after emitter/renderer filtering.
    pub fn record_submitted_frame(&mut self, effect: &CompiledEffect, samples: &[ParticleSample]) {
        let submitted = samples.iter().fold(0_u32, |total, sample| {
            let renderer_count = effect
                .emitters
                .get(sample.emitter_index)
                .filter(|emitter| emitter.enabled)
                .map_or(0, |emitter| {
                    emitter.renderers.len().min(u32::MAX as usize) as u32
                });
            total.saturating_add(renderer_count)
        });
        self.submitted_instances = ProfileValue::Measured(submitted);
    }

    pub fn reset_peaks(&mut self) {
        self.peak_particles = self.alive_particles;
        for emitter in &mut self.emitters {
            emitter.peak_particles = emitter.alive_particles;
        }
    }
}

fn estimated_buffer_memory(effect: &CompiledEffect) -> u64 {
    let attribute_bytes = effect
        .particle_layout
        .attributes
        .iter()
        .chain(&effect.particle_layout.transient_attributes)
        .copied()
        .map(particle_attribute_bytes)
        .sum::<u64>();
    let particle_count = effect.max_particles as u64;
    let particle_storage = particle_count.saturating_mul(attribute_bytes);
    // Native history uses the fixed 64-byte particle ABI, independently of live attributes.
    let history_storage = effect
        .emitters
        .iter()
        .filter(|e| e.enabled)
        .flat_map(|e| {
            e.renderers.iter().filter_map(move |r| match r.kind {
                crate::RendererPlanKind::Trail { max_points, .. } => {
                    Some((1 + u64::from(e.max_particles) * u64::from(max_points)) * 64)
                }
                _ => None,
            })
        })
        .sum::<u64>();
    let alive_and_dead_indices = particle_count.saturating_mul(2 * size_of::<u32>() as u64);
    let counters = 2 * size_of::<u32>() as u64;
    let indirect_commands = effect.emitters.len() as u64 * 4 * size_of::<u32>() as u64;
    particle_storage
        .saturating_add(history_storage)
        .saturating_add(alive_and_dead_indices)
        .saturating_add(counters)
        .saturating_add(indirect_commands)
}

const fn particle_attribute_bytes(attribute: ParticleAttribute) -> u64 {
    match attribute {
        ParticleAttribute::Position | ParticleAttribute::Velocity => 2 * size_of::<f32>() as u64,
        ParticleAttribute::Color => 4 * size_of::<f32>() as u64,
        ParticleAttribute::Age
        | ParticleAttribute::Lifetime
        | ParticleAttribute::NormalizedAge
        | ParticleAttribute::Rotation
        | ParticleAttribute::AngularVelocity
        | ParticleAttribute::Size => size_of::<f32>() as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_values_keep_measurement_provenance() {
        assert_eq!(
            ProfileValue::Measured(12_u32).source(),
            ProfileValueSource::Measured
        );
        assert_eq!(
            ProfileValue::Estimated(12_u32).source(),
            ProfileValueSource::Estimated
        );
        assert_eq!(
            ProfileValue::<u32>::Unavailable.source(),
            ProfileValueSource::Unavailable
        );
    }
}
