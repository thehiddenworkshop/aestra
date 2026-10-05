//! Explicit host lighting policy, independent of effect compilation and playback history.
use crate::{
    AestraParticleLightSettings, MAX_TRANSIENT_LIGHTS, ParticleLightGpuSettings,
    ParticleLightReadbackSettings, ParticleLightRealizationSettings, TransientLightSettings,
};
use bevy::prelude::World;

/// One independently configurable family of shadowless lights.
/// Counts are admission ceilings, not promises of live lights or total renderer memory.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightQualityBudget {
    pub enabled: bool,
    pub max_lights: u32,
    pub max_lumens: f32,
    pub max_range: f32,
}

/// Global host-controlled representative and selected-particle lighting quality.
/// Applying a policy does not compile effects, change playback/history, install plugins,
/// change realization mode, or enable shadows. Authored per-output limits still apply.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightingQualityPolicy {
    pub representative: LightQualityBudget,
    pub particle: LightQualityBudget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LightingQualityError {
    #[error("representative light count exceeds the adapter ceiling")]
    RepresentativeCount,
    #[error("light clamps must be finite, with positive range and lumens")]
    InvalidClamps,
}

impl LightingQualityPolicy {
    /// Example-host presets, not device-independent performance guarantees.
    /// Clamps remain identical across presets to preserve authored pulse appearance;
    /// hosts may independently lower either family's range or intensity.
    pub fn preset(name: &str) -> Option<Self> {
        let (representative, particle) = match name {
            "high" => (8, 96),
            "medium" => (4, 48),
            "low" => (2, 24),
            _ => return None,
        };
        let budget = |max_lights| LightQualityBudget {
            enabled: true,
            max_lights,
            max_lumens: 1_000_000.0,
            max_range: 200.0,
        };
        Some(Self {
            representative: budget(representative),
            particle: budget(particle),
        })
    }

    /// Validate before any resource is created or changed (including disabled families).
    pub fn validate(&self) -> Result<(), LightingQualityError> {
        if self.representative.max_lights as usize > MAX_TRANSIENT_LIGHTS {
            return Err(LightingQualityError::RepresentativeCount);
        }
        for budget in [self.representative, self.particle] {
            if !budget.max_lumens.is_finite()
                || budget.max_lumens <= 0.0
                || !budget.max_range.is_finite()
                || budget.max_range <= 0.0
            {
                return Err(LightingQualityError::InvalidClamps);
            }
        }
        Ok(())
    }

    /// Apply once during setup or explicitly when the host changes quality, before
    /// the next update/extraction. Both GPU and portable-async paths receive the same
    /// particle cap; disabling particles zeros selection, transport and realization.
    /// Existing byte budgets, age/lag limits, request caps and authored binding mode
    /// are preserved. Missing settings are initialized, but plugins remain opt-in.
    /// Renderer pipelines/in-flight work converge through normal updates; this call
    /// does not synchronously wait for the GPU or revoke an already submitted frame.
    ///
    /// ```rust,no_run
    /// use aestra_bevy::LightingQualityPolicy;
    /// use bevy::prelude::*;
    /// let mut app = App::new();
    /// // Install AestraPlugin and the desired opt-in lighting plugins separately.
    /// let mut policy = LightingQualityPolicy::preset("medium").unwrap();
    /// policy.particle.max_range = 80.0;
    /// policy.apply(app.world_mut()).unwrap();
    /// // Later, representative flashes may remain on while particle lighting is off.
    /// policy.particle.enabled = false;
    /// policy.apply(app.world_mut()).unwrap();
    /// ```
    pub fn apply(&self, world: &mut World) -> Result<(), LightingQualityError> {
        self.validate()?;
        world.init_resource::<TransientLightSettings>();
        world.init_resource::<AestraParticleLightSettings>();
        world.init_resource::<ParticleLightGpuSettings>();
        world.init_resource::<ParticleLightReadbackSettings>();
        world.init_resource::<ParticleLightRealizationSettings>();
        {
            let mut settings = world.resource_mut::<TransientLightSettings>();
            settings.enabled = self.representative.enabled;
            settings.max_lights = self.representative.max_lights as usize;
            settings.max_lumens = self.representative.max_lumens;
            settings.max_range = self.representative.max_range;
        }
        let cap = if self.particle.enabled {
            self.particle.max_lights
        } else {
            0
        };
        world
            .resource_mut::<AestraParticleLightSettings>()
            .max_lights = cap;
        world.resource_mut::<ParticleLightGpuSettings>().max_lights = cap;
        world
            .resource_mut::<ParticleLightReadbackSettings>()
            .max_lights = cap;
        let mut settings = world.resource_mut::<ParticleLightRealizationSettings>();
        settings.max_lumens = self.particle.max_lumens;
        settings.max_range = self.particle.max_range;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ParticleLightMode;

    #[test]
    fn presets_update_both_paths_and_preserve_independent_safety_budgets() {
        let mut world = World::new();
        world.insert_resource(AestraParticleLightSettings {
            max_scratch_bytes: 1234,
            ..Default::default()
        });
        world.insert_resource(ParticleLightGpuSettings {
            max_buffer_bytes: 2345,
            max_manifest_bytes: 3456,
            ..Default::default()
        });
        world.insert_resource(ParticleLightReadbackSettings {
            max_staging_bytes: 4567,
            max_in_flight: 2,
            ..Default::default()
        });
        world.insert_resource(TransientLightSettings {
            max_requests_per_frame: 7,
            authored_bindings: false,
            ..Default::default()
        });
        world.insert_resource(ParticleLightMode::PortableAsync);
        for (name, representative, particle) in [
            ("high", 8, 96),
            ("low", 2, 24),
            ("medium", 4, 48),
            ("high", 8, 96),
        ] {
            LightingQualityPolicy::preset(name)
                .unwrap()
                .apply(&mut world)
                .unwrap();
            assert_eq!(
                world.resource::<TransientLightSettings>().max_lights,
                representative
            );
            assert_eq!(
                world.resource::<AestraParticleLightSettings>().max_lights,
                particle
            );
            assert_eq!(
                world.resource::<ParticleLightGpuSettings>().max_lights,
                particle
            );
            assert_eq!(
                world.resource::<ParticleLightReadbackSettings>().max_lights,
                particle
            );
        }
        assert_eq!(
            world
                .resource::<AestraParticleLightSettings>()
                .max_scratch_bytes,
            1234
        );
        assert_eq!(
            world
                .resource::<ParticleLightGpuSettings>()
                .max_buffer_bytes,
            2345
        );
        assert_eq!(
            world
                .resource::<ParticleLightGpuSettings>()
                .max_manifest_bytes,
            3456
        );
        assert_eq!(
            world
                .resource::<ParticleLightReadbackSettings>()
                .max_staging_bytes,
            4567
        );
        assert_eq!(
            world
                .resource::<ParticleLightReadbackSettings>()
                .max_in_flight,
            2
        );
        assert_eq!(
            world
                .resource::<TransientLightSettings>()
                .max_requests_per_frame,
            7
        );
        assert!(!world.resource::<TransientLightSettings>().authored_bindings);
        assert_eq!(
            *world.resource::<ParticleLightMode>(),
            ParticleLightMode::PortableAsync
        );
    }

    #[test]
    fn independent_disable_and_clamps_are_explicit_not_quality_name_magic() {
        assert!(LightingQualityPolicy::preset("custom").is_none());
        let mut world = World::new();
        let mut policy = LightingQualityPolicy::preset("low").unwrap();
        policy.particle.enabled = false;
        policy.particle.max_range = 12.0;
        policy.particle.max_lumens = 1000.0;
        policy.apply(&mut world).unwrap();
        assert!(world.resource::<TransientLightSettings>().enabled);
        assert_eq!(
            world.resource::<AestraParticleLightSettings>().max_lights,
            0
        );
        assert_eq!(world.resource::<ParticleLightGpuSettings>().max_lights, 0);
        assert_eq!(
            world.resource::<ParticleLightReadbackSettings>().max_lights,
            0
        );
        assert_eq!(
            world
                .resource::<ParticleLightRealizationSettings>()
                .max_range,
            12.0
        );
        assert_eq!(
            world
                .resource::<ParticleLightRealizationSettings>()
                .max_lumens,
            1000.0
        );
        policy.representative.enabled = false;
        policy.particle.enabled = true;
        policy.apply(&mut world).unwrap();
        assert!(!world.resource::<TransientLightSettings>().enabled);
        assert_eq!(
            world.resource::<AestraParticleLightSettings>().max_lights,
            24
        );
    }

    #[test]
    fn invalid_policy_is_atomic_and_does_not_initialize_settings() {
        let mut world = World::new();
        let original = LightingQualityPolicy::preset("high").unwrap();
        let mut bad = original;
        bad.particle.max_range = f32::NAN;
        assert_eq!(
            bad.apply(&mut world),
            Err(LightingQualityError::InvalidClamps)
        );
        assert!(!world.contains_resource::<TransientLightSettings>());
        original.apply(&mut world).unwrap();
        for value in [f32::INFINITY, f32::NEG_INFINITY, 0.0, -1.0, f32::NAN] {
            let mut bad = original;
            bad.representative.max_lights = 1;
            bad.particle.max_lumens = value;
            assert!(bad.apply(&mut world).is_err());
            assert_eq!(world.resource::<TransientLightSettings>().max_lights, 8);
            assert_eq!(
                world.resource::<AestraParticleLightSettings>().max_lights,
                96
            );
        }
        let mut bad = original;
        bad.representative.max_lights = MAX_TRANSIENT_LIGHTS as u32 + 1;
        assert_eq!(
            bad.apply(&mut world),
            Err(LightingQualityError::RepresentativeCount)
        );
    }
}
