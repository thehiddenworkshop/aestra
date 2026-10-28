//! Add deposited smoke to the calibrated shells through saved event links only.
use aestra_core::{
    material::{MaterialParameterValue, MaterialValue},
    *,
};
use std::path::{Path, PathBuf};

const BASE: u128 = 0xa3574a00_0000_4000_8000_000000f88000;
const SHOW_SEED: u64 = 0xf1e0_0000_0000_0001;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/test")
}

fn read(root: &Path, name: &str) -> Result<EffectAsset, String> {
    EffectAsset::from_ron(
        &std::fs::read_to_string(root.join("effects").join(name)).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn author(root: &Path) -> Result<Vec<(String, EffectAsset)>, String> {
    let template = read(root, "fireworks_multi_break_wind_haze_smoke.aestra.ron")?;
    let mut variants = Vec::new();
    let mut original_ids = Vec::new();
    for (index, family) in ["point_star", "gold_tail"].iter().enumerate() {
        let mut asset = read(root, &format!("fireworks_{family}_candidate.aestra.ron"))?;
        original_ids.push(asset.id);
        let base = BASE + (index as u128 + 1) * 256;
        asset.id = EffectId::from_u128(base);
        asset.name = format!(
            "Fireworks {} with Deposited Smoke",
            if index == 0 {
                "Point Stars"
            } else {
                "Dense Gold Tails"
            }
        );
        asset.metadata.insert(
            "status".into(),
            "Calibrated shell with deposited smoke; opt-in review candidate".into(),
        );
        asset.metadata.insert("notes".into(), "Calibrated luminous emitters, cues, flashes and ember links retained. Main stars deposit zero-inheritance smoke with no independent emission; sampler agrees with any existing distance link. Separate budgets and native art/overlap review; not default-show adoption.".into());
        let stars = asset
            .emitters
            .iter()
            .find(|e| e.name == "Main stars")
            .unwrap()
            .clone();
        let trigger = asset
            .events
            .iter()
            .find(|e| e.source == stars.id && e.trigger.distance_settings().is_some())
            .map(|e| e.trigger)
            .unwrap_or(EventTrigger::OnDistance {
                spacing: 0.8,
                max_per_tick: 2,
            });
        let mut smoke = template
            .emitters
            .iter()
            .find(|e| e.name == "Star path smoke")
            .unwrap()
            .clone();
        smoke.id = EmitterId::from_u128(base + 1);
        // Conservative pools: parent lifetime/speed are longer than in C4V.
        let per_star = if index == 0 { 128 } else { 512 };
        smoke.max_particles = (stars.max_particles * per_star).next_power_of_two();
        let density = if index == 0 { 0.5 } else { 0.25 };
        for (i, module) in smoke.modules.iter_mut().enumerate() {
            module.id = ModuleId::from_u128(base + 16 + i as u128);
            module.bindings.clear();
            if let ModuleParameters::Appearance {
                size,
                opacity,
                color,
            } = &mut module.parameters
            {
                size.id = CurveId::from_u128(base + 32);
                size.keys = vec![
                    CurveKey::new(0.0, 1.2),
                    CurveKey::new(0.25, 1.8),
                    CurveKey::new(0.65, 2.5),
                    CurveKey::new(1.0, 3.2),
                ];
                opacity.id = CurveId::from_u128(base + 33);
                for key in &mut opacity.keys {
                    key.value *= density;
                }
                color.id = GradientId::from_u128(base + 34);
            }
        }
        let old_material = smoke.renderers[0].material;
        let mut material = template
            .material_instances
            .iter()
            .find(|m| m.id == old_material)
            .unwrap()
            .clone();
        material.id = MaterialId::from_u128(base + 48);
        material.values.insert(
            MaterialParameterId::from_u128(0xa3574a00_0000_4000_8000_000000f85701),
            MaterialParameterValue::Constant(MaterialValue::Float(0.08)),
        );
        for (i, renderer) in smoke.renderers.iter_mut().enumerate() {
            renderer.id = RendererId::from_u128(base + 40 + i as u128);
            renderer.material = material.id;
        }
        for profile in asset.particle_budgets.values_mut() {
            let source = profile
                .emitter_capacity
                .get(&stars.id)
                .copied()
                .unwrap_or(stars.max_particles);
            profile
                .emitter_capacity
                .insert(smoke.id, (source * per_star).next_power_of_two());
        }
        let mut link = EventLink::new(stars.id, trigger, smoke.id);
        link.id = EventId::from_u128(base + 64);
        asset.material_instances.push(material);
        asset.emitters.push(smoke);
        asset.events.push(link);
        variants.push((format!("fireworks_{family}_smoke.aestra.ron"), asset));
    }
    let mut show = EffectAsset::new("Fireworks Calibrated Shell Smoke Pair", 24.0);
    show.id = EffectId::from_u128(BASE);
    show.playback_mode = EffectPlaybackMode::Once;
    for (index, (source, start, position)) in [
        (variants[1].1.id, 0.0, [-10.0, 0.0, 0.0]),
        (variants[0].1.id, 5.5, [6.0, 5.0, 0.0]),
    ]
    .into_iter()
    .enumerate()
    {
        let mut clip = EffectClip::new(source, start, 18.0);
        clip.id = EffectClipId::from_u128(BASE + 0xa0 + index as u128);
        clip.seed = EffectClipSeed::Fixed(SHOW_SEED);
        clip.transform.translation = position;
        if index == 1 {
            let parameter = variants[0]
                .1
                .parameters
                .iter()
                .find(|p| p.name == "Main stars color")
                .unwrap()
                .id;
            clip.parameter_overrides.insert(
                parameter,
                Value::Gradient(Gradient {
                    id: GradientId::from_u128(BASE + 0xb0),
                    keys: vec![
                        ColorKey::new(0.0, [0.25, 0.6, 1.0, 1.0]),
                        ColorKey::new(0.65, [0.12, 0.4, 1.0, 1.0]),
                        ColorKey::new(1.0, [0.04, 0.08, 0.25, 1.0]),
                    ],
                }),
            );
        }
        show.effect_clips.push(clip);
    }
    show.metadata.insert("notes".into(), "Opt-in two-shell smoke integration: dense gold at 0s, blue point stars at 5.5s, fixed source seeds. Later representative flash illuminates older deposited smoke. Existing 13-clip shows unchanged; selected-star lighting off; native image/cost approval separate.".into());
    let mut control = show.clone();
    control.id = EffectId::from_u128(BASE + 0x300);
    control.name = "Fireworks Calibrated Shell Pair without Path Smoke".into();
    control.effect_clips[0].source.id = original_ids[1];
    control.effect_clips[1].source.id = original_ids[0];
    control.metadata.insert("notes".into(), "Same two calibrated luminous shells, seeds, transforms, timing, color overrides and representative flashes; source effects have no deposited path smoke. Existing launch/burst haze remains.".into());
    variants.push(("fireworks_shell_smoke_control.aestra.ron".into(), control));
    variants.push(("fireworks_shell_smoke_candidate.aestra.ron".into(), show));
    Ok(variants)
}

fn main() -> Result<(), String> {
    for (name, asset) in author(&root())? {
        std::fs::write(
            root().join("effects").join(&name),
            asset.to_pretty_ron().map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        println!("Authored {name}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_compiler::{EffectCompiler, ExtensionRegistry};
    use aestra_project::ProjectAssetIndex;
    use aestra_runtime::QualityTier;

    #[test]
    fn smoke_preserves_calibrated_shells_and_compiles_shared_sampler_at_all_tiers() {
        let root = root();
        let index = ProjectAssetIndex::scan(&root);
        for (name, expected) in author(&root).unwrap() {
            let saved = read(&root, &name).unwrap();
            assert_eq!(saved, expected, "regenerate {name}");
            if name.ends_with("_smoke.aestra.ron") {
                let original = read(
                    &root,
                    &name.replace("_smoke.aestra.ron", "_candidate.aestra.ron"),
                )
                .unwrap();
                for emitter in &original.emitters {
                    assert_eq!(
                        saved.emitters.iter().find(|e| e.id == emitter.id).unwrap(),
                        emitter
                    );
                }
                assert_eq!(saved.particle_outputs, original.particle_outputs);
                assert_eq!(saved.point_lights, original.point_lights);
                assert_eq!(&saved.events[..original.events.len()], &original.events);
                let smoke = saved
                    .emitters
                    .iter()
                    .find(|e| e.name == "Star path smoke")
                    .unwrap();
                let link = saved.events.iter().find(|e| e.target == smoke.id).unwrap();
                assert_eq!(link.inherit_velocity, 0.0);
                assert_eq!(link.count, 1);
                assert!(smoke.modules.iter().all(|m| !matches!(m.parameters, ModuleParameters::Emission { spawn_rate, burst_count } if spawn_rate != 0.0 || burst_count != 0)));
                for route in saved
                    .events
                    .iter()
                    .filter(|e| e.source == link.source && e.trigger.distance_settings().is_some())
                {
                    assert_eq!(route.trigger, link.trigger);
                }
            }
            for tier in ["high", "medium", "low"] {
                let resolved = index.resolve_effect_project(&saved).unwrap();
                EffectCompiler::with_extensions(ExtensionRegistry::linked())
                    .with_tier(QualityTier::preset(tier).unwrap())
                    .compile_resolved_project(&resolved)
                    .unwrap();
            }
        }
    }
}
