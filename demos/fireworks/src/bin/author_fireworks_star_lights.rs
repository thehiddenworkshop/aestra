//! Opt-in, material-free selected-star outputs on the calibrated smoke pair.
use aestra_core::*;
use std::path::{Path, PathBuf};

const BASE: u128 = 0xa3574a00_0000_4000_8000_000000f8a000;
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
    let mut assets = Vec::new();
    let mut ids = Vec::new();
    for (family, name) in ["point_star", "gold_tail"].into_iter().enumerate() {
        let mut asset = read(root, &format!("fireworks_{name}_smoke.aestra.ron"))?;
        let previous = asset.id;
        let base = BASE + (family as u128 + 1) * 256;
        asset.id = EffectId::from_u128(base);
        asset.name.push_str(" — Selected Star Lighting");
        let stars = asset
            .emitters
            .iter_mut()
            .find(|e| e.name == "Main stars")
            .ok_or("Main stars missing")?;
        let mut light = ParticlePointLightProperties::new(6000.0, 12.0);
        let color = asset
            .parameters
            .iter()
            .find(|p| p.name == "Main stars color")
            .ok_or("Main stars color missing")?
            .id;
        light.color_source = ParticleLightColorSource::GradientParameter(color);
        light.intensity_curve = Curve::new(vec![
            CurveKey::new(0.0, 6000.0),
            CurveKey::new(0.65, 3000.0),
            CurveKey::new(1.0, 0.0),
        ]);
        light.intensity_curve.id = CurveId::from_u128(base + 2);
        light.range_curve.id = CurveId::from_u128(base + 3);
        // Low deliberately admits 24 of the sparse stars, matching the earlier
        // qualified low-tier fixture rather than its rejected eight-light draft.
        light.max_lights_by_quality = [
            ("high".into(), 32),
            ("medium".into(), 16),
            ("low".into(), 24),
        ]
        .into();
        let mut output = SceneOutputInstance::particle_point_light(light);
        output.id = SceneOutputId::from_u128(base + 1);
        stars.scene_outputs.push(output);
        asset.metadata.insert("lighting_review".into(), "Opt-in bounded Main-star output only; no smoke/ember/flash particle lights. Host enablement is independent. Art and production cost approval remain separate.".into());
        ids.push((previous, asset.id));
        assets.push((format!("fireworks_{name}_lit_smoke.aestra.ron"), asset));
    }
    let mut pair = read(root, "fireworks_shell_smoke_candidate.aestra.ron")?;
    pair.id = EffectId::from_u128(BASE);
    pair.name = "Fireworks Smoke Pair — Selected Star Lighting".into();
    for clip in &mut pair.effect_clips {
        clip.source.id = ids
            .iter()
            .find(|(old, _)| *old == clip.source.id)
            .ok_or("Unexpected shell in pair")?
            .1;
    }
    pair.metadata.insert("notes".into(), "Same calibrated smoke pair, seeds, placements and blue override; only Main-star scene outputs added. Use --star-lights to enable host selection independently of representative flashes.".into());
    assets.push(("fireworks_shell_star_lights.aestra.ron".into(), pair));
    Ok(assets)
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
    use aestra_compiler::EffectCompiler;
    use aestra_project::ProjectAssetIndex;
    use aestra_runtime::QualityTier;

    #[test]
    fn saved_outputs_preserve_simulation_art_and_compile_all_tiers() {
        let root = root();
        let index = ProjectAssetIndex::scan(&root);
        for (name, expected) in author(&root).unwrap() {
            let asset = read(&root, &name).unwrap();
            assert_eq!(asset, expected, "regenerate {name}");
            if !name.contains("shell") {
                let source = read(&root, &name.replace("lit_smoke", "smoke")).unwrap();
                let mut emitters = asset.emitters.clone();
                for e in &mut emitters {
                    e.scene_outputs.clear();
                }
                assert_eq!(emitters, source.emitters);
                assert_eq!(asset.events, source.events);
                assert_eq!(asset.particle_outputs, source.particle_outputs);
                assert_eq!(asset.point_lights, source.point_lights);
                assert_eq!(asset.material_instances, source.material_instances);
                assert_eq!(asset.parameters, source.parameters);
                assert_eq!(asset.particle_budgets, source.particle_budgets);
            }
            for (tier, cap) in [("high", 32), ("medium", 16), ("low", 24)] {
                let resolved = index.resolve_effect_project(&asset).unwrap();
                let project = EffectCompiler::default()
                    .with_tier(QualityTier::preset(tier).unwrap())
                    .compile_resolved_project(&resolved)
                    .unwrap();
                for effect in std::iter::once(&project.root).chain(project.dependencies.values()) {
                    for emitter in &effect.emitters {
                        assert_eq!(
                            emitter.scene_outputs.len(),
                            usize::from(emitter.name == "Main stars")
                        );
                        for output in &emitter.scene_outputs {
                            let aestra_runtime::SceneOutputPlanKind::ParticlePointLight(light) =
                                &output.kind;
                            assert_eq!(light.max_lights, cap);
                        }
                    }
                }
            }
        }
    }
}
