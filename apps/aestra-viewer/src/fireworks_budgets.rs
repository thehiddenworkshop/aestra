//! F5F ordinary authored tier profiles and a bounded shared-pool volley.
use aestra_bevy::EffectAsset;

pub fn volley_effect() -> EffectAsset {
    EffectAsset::from_ron(include_str!(
        "../../../assets/test/effects/fireworks_secondary_volley.aestra.ron"
    ))
    .expect("checked-in secondary volley must parse")
}

#[cfg(test)]
pub(crate) fn author_profiles(effect: &mut EffectAsset) {
    use aestra_bevy::{ParticleBudgetProfile, RendererProperties};
    effect.particle_budgets.clear();
    for (tier, divisor, multi_fanout, crackle_fanout) in [("medium", 2, 6, 8), ("low", 4, 4, 4)] {
        let mut profile = ParticleBudgetProfile::default();
        let main = effect
            .emitters
            .iter()
            .find(|e| e.name == "Main stars")
            .unwrap();
        let main_capacity = main.max_particles / divisor;
        for emitter in &effect.emitters {
            let capacity = match emitter.name.as_str() {
                "Main stars" | "Burning carriers" | "Crossette NE" | "Crossette NW"
                | "Crossette SW" | "Crossette SE" => main_capacity,
                "Secondary sparks" => main_capacity * multi_fanout,
                "Crackle sparks" => main_capacity * crackle_fanout,
                "Burst smoke" => emitter.max_particles / divisor,
                _ => emitter.max_particles,
            };
            if capacity == emitter.max_particles {
                continue;
            }
            profile.emitter_capacity.insert(emitter.id, capacity);
            for renderer in &emitter.renderers {
                if let RendererProperties::Trail { max_trails, .. } = renderer.properties {
                    let authored = if max_trails == 0 {
                        emitter.max_particles
                    } else {
                        max_trails
                    };
                    profile.trail_capacity.insert(
                        renderer.id,
                        (u64::from(authored) * u64::from(capacity))
                            .div_ceil(u64::from(emitter.max_particles))
                            as u32,
                    );
                }
            }
        }
        for link in &effect.events {
            let target = effect
                .emitters
                .iter()
                .find(|e| e.id == link.target)
                .unwrap();
            let count = match target.name.as_str() {
                "Main stars" | "Burst smoke" => link.count / divisor,
                "Secondary sparks" => multi_fanout,
                "Crackle sparks" => crackle_fanout,
                _ => link.count, // Keep flash, carrier and each of the four arms.
            };
            if count != link.count {
                profile.event_count.insert(link.id, count);
            }
        }
        effect.particle_budgets.insert(tier.into(), profile);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_bevy::{EffectId, ModuleParameters, QualityTier, RendererProperties, ScalarRange};

    fn build_volley() -> EffectAsset {
        let mut effect = super::super::fireworks_f5::effect();
        effect.id = EffectId::from_u128(0xa3574a00_0000_4000_8000_0000000fc000);
        effect.name = "Fireworks Secondary Volley".into();
        effect.duration = 9.0;
        for emitter in &mut effect.emitters {
            if emitter.name != "Launch smoke" {
                emitter.duration = effect.duration;
            }
            emitter.max_particles *= 4;
            for renderer in &mut emitter.renderers {
                if let RendererProperties::Trail { max_trails, .. } = &mut renderer.properties
                    && *max_trails != 0
                {
                    *max_trails *= 4;
                }
            }
        }
        // Four real rockets share one source and each target pool. Seeded rocket
        // lifetimes stagger the first breaks; no host-timed child spawning.
        effect.emitters[0].max_particles = 4;
        if let ModuleParameters::Emission {
            spawn_rate,
            burst_count,
        } = &mut effect.emitters[0].modules[0].parameters
        {
            *spawn_rate = 0.0;
            *burst_count = 4;
        }
        if let ModuleParameters::Initialize { lifetime, .. } =
            &mut effect.emitters[0].modules[2].parameters
        {
            *lifetime = ScalarRange::new(0.8, 1.4);
        }
        author_profiles(&mut effect);
        effect.metadata.insert(
            "status".into(),
            "F5F bounded four-rocket secondary volley; not finale or artistic certification".into(),
        );
        effect.metadata.insert("notes".into(), "Shared source/target pools; four real rocket deaths feed overlapping main and secondary cohorts. Explicit medium/low budgets reduce primary count and secondary fan-out together; histories reserve all births including retired owners. No replay history or host spawn timer.".into());
        effect
    }

    #[test]
    #[ignore = "prints fixture source for a reviewed apply_patch update"]
    fn export_volley_fixture() {
        println!(
            "F5_VOLLEY={}",
            serde_json::to_string(&build_volley().to_pretty_ron().unwrap()).unwrap()
        );
    }

    #[test]
    fn profiles_compile_all_shells_without_changing_high_or_star_behavior() {
        let index = aestra_project::ProjectAssetIndex::scan(super::super::viewer_asset_root(None));
        for (source, capacities) in [
            (super::super::fireworks_f5::effect(), [690, 314, 158]),
            (
                super::super::fireworks_f5::crackle_effect(),
                [1458, 570, 222],
            ),
            (
                super::super::fireworks_f5::crossette_effect(),
                [274, 170, 118],
            ),
            (super::super::fireworks_strobe::effect(), [306, 186, 126]),
            (volley_effect(), [2760, 1256, 632]),
        ] {
            assert_eq!(source.particle_budgets.len(), 2);
            let original = source.clone();
            let project = index.resolve_effect_project(&source).unwrap();
            let mut previous = usize::MAX;
            for (tier, capacity) in QualityTier::presets().into_iter().zip(capacities) {
                let compiled = aestra_bevy::EffectCompiler::default()
                    .with_tier(tier.clone())
                    .compile_resolved_project(&project)
                    .unwrap();
                let budgeted = source.with_particle_budget_profile(&tier.name);
                assert_eq!(compiled.root.max_particles, capacity);
                assert_eq!(compiled.root.requirements.max_particles, capacity);
                assert!(compiled.root.max_particles < previous);
                previous = compiled.root.max_particles;
                for (authored, effective) in source.emitters.iter().zip(&budgeted.emitters) {
                    assert_eq!(authored.modules, effective.modules);
                    assert_eq!(authored.transform, effective.transform);
                }
                assert_eq!(source.material_instances, budgeted.material_instances);
                assert_eq!(source.particle_outputs, budgeted.particle_outputs);
                for (link, authored) in compiled.root.event_links.iter().zip(&source.events) {
                    if authored.count == 1 {
                        assert_eq!(link.count, 1);
                    }
                }
                let instance = aestra_bevy::EffectInstance::with_seed(
                    compiled.root,
                    super::super::fireworks_f0::SEED,
                )
                .with_history_policy(aestra_bevy::PlaybackHistoryPolicy::PlaybackOnly);
                aestra_gpu::GpuEffectArtifact::from_instance(&instance).unwrap();
            }
            assert_eq!(source, original);
            assert_eq!(source.with_particle_budget_profile("high"), source);
        }
    }

    #[test]
    fn volley_matches_builder_and_cli_keeps_live_and_bench_clocks_distinct() {
        assert_eq!(
            volley_effect().to_pretty_ron().unwrap(),
            build_volley().to_pretty_ron().unwrap()
        );
        for tier in ["high", "medium", "low"] {
            let config = super::super::ViewerConfig::from_iter(
                [
                    "--fireworks-f0",
                    "--fireworks-f0-probe",
                    "f5-secondary-volley",
                    "--semantic-materials",
                    "--backend",
                    "gpu",
                    "--history",
                    "playback-only",
                    "--tier",
                    tier,
                ]
                .into_iter()
                .map(str::to_owned),
            )
            .unwrap();
            assert_eq!(
                super::super::prepare_viewer(&config)
                    .unwrap_or_else(|e| panic!("{}", e.message))
                    .compiled
                    .name,
                "Fireworks Secondary Volley"
            );
            assert!(config.probe_bench_step().is_none());
        }
    }
}
