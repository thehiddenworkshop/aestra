//! Offline, opt-in single-shell calibration. Playback stays in the public host.
use aestra_core::{material::MaterialParameterValue, *};
use std::path::{Path, PathBuf};

const BASE: u128 = 0xa3574a00_0000_4000_8000_000000f86000;
const CONTROL: &str = "fireworks_point_star_control.aestra.ron";
const CANDIDATE: &str = "fireworks_point_star_candidate.aestra.ron";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/test")
}

fn author(root: &Path) -> Result<[(String, EffectAsset); 2], String> {
    let source = EffectAsset::from_ron(
        &std::fs::read_to_string(
            root.join("effects/fireworks_multi_break_wind_haze_smoke.aestra.ron"),
        )
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let mut control = source.clone();
    control.id = EffectId::from_u128(BASE);
    control.name = "Fireworks Point-Star Calibration Control".into();
    // Remove secondary breaks and deposited smoke to assess the luminous shell
    // first. Launch/burst haze and the actual launch/death cue/light routes stay.
    control
        .emitters
        .retain(|e| !matches!(e.name.as_str(), "Secondary sparks" | "Star path smoke"));
    control.events.retain(|event| {
        control.emitters.iter().any(|e| e.id == event.source)
            && control.emitters.iter().any(|e| e.id == event.target)
    });
    control.particle_outputs.retain(|route| {
        route.trigger != EventTrigger::OnDeath
            || control
                .emitters
                .iter()
                .find(|e| e.id == route.source)
                .unwrap()
                .name
                != "Main stars"
    });
    let output_ids: Vec<_> = control
        .particle_outputs
        .iter()
        .map(|route| route.output)
        .collect();
    control
        .event_outputs
        .retain(|output| output_ids.contains(&output.id));
    control.prune_particle_budget_targets(&source);
    // Same white-to-warm palette in both controls; do not confuse a hue change
    // with improved head/tail separation. Keep its existing exposed binding.
    if let Value::Gradient(gradient) = &mut control
        .parameters
        .iter_mut()
        .find(|p| p.name == "Main stars color")
        .unwrap()
        .default
    {
        gradient.keys = vec![
            ColorKey::new(0.0, [1.0, 0.97, 0.9, 1.0]),
            ColorKey::new(0.65, [1.0, 0.97, 0.9, 1.0]),
            ColorKey::new(0.85, [0.9, 0.75, 0.5, 1.0]),
            ColorKey::new(1.0, [0.3, 0.08, 0.01, 1.0]),
        ];
    }
    control.metadata.insert("notes".into(), "Video-reference single-shell control: retained Main-star dynamics/density/trails, shared white palette, no secondary breaks or deposited star smoke. Original launch/haze/cues/light retained. Not a show replacement.".into());
    let mut candidate = control.clone();
    candidate.id = EffectId::from_u128(BASE + 0x100);
    candidate.name = "Fireworks Point-Star Calibration Candidate".into();
    candidate.metadata.insert("notes".into(), "Video-reference point-star candidate: longer varied burn, more small bright heads and shorter thinner cooling tails. Uses existing authoring APIs; no deposited star smoke/secondary breaks. Separate art and performance approval; not default adoption.".into());
    let stars = candidate
        .emitters
        .iter_mut()
        .find(|e| e.name == "Main stars")
        .unwrap();
    let star_id = stars.id;
    stars.max_particles = 128;
    for module in &mut stars.modules {
        match &mut module.parameters {
            ModuleParameters::Initialize {
                lifetime, speed, ..
            } => {
                *lifetime = ScalarRange::new(2.4, 3.2);
                *speed = ScalarRange::new(27.0, 31.0);
            }
            ModuleParameters::Motion { drag, .. } => *drag = 0.45,
            ModuleParameters::Appearance { size, opacity, .. } => {
                size.keys = vec![
                    CurveKey::new(0.0, 0.7),
                    CurveKey::new(0.65, 0.65),
                    CurveKey::new(0.9, 0.5),
                    CurveKey::new(1.0, 0.12),
                ];
                opacity.keys = vec![
                    CurveKey::new(0.0, 1.0),
                    CurveKey::new(0.6, 0.95),
                    CurveKey::new(0.85, 0.65),
                    CurveKey::new(1.0, 0.0),
                ];
            }
            _ => {}
        }
    }
    let trail_id = stars
        .renderers
        .iter()
        .find(|r| matches!(r.properties, RendererProperties::Trail { .. }))
        .unwrap()
        .id;
    for (index, renderer) in stars.renderers.iter_mut().enumerate() {
        let mut material = candidate
            .material_instances
            .iter()
            .find(|m| m.id == renderer.material)
            .unwrap()
            .clone();
        material.id = MaterialId::from_u128(BASE + 0x110 + index as u128);
        let parameter = ParameterId::from_u128(BASE + 0x120 + index as u128);
        let trail = matches!(renderer.properties, RendererProperties::Trail { .. });
        // Separate Main-star instances: raising head gain must not inflate the
        // launch shell or burst flash that share the original sprite material.
        for value in material.values.values_mut() {
            *value = MaterialParameterValue::EffectParameter(parameter);
        }
        candidate.parameters.push(EffectParameter {
            id: parameter,
            name: if trail {
                "Main cooling tail radiance"
            } else {
                "Main head radiance"
            }
            .into(),
            default: Value::Scalar(if trail { 1.2 } else { 48.0 }),
            exposed: true,
        });
        renderer.material = material.id;
        candidate.material_instances.push(material);
        if let RendererProperties::Trail {
            width,
            lifetime,
            max_trails,
            ..
        } = &mut renderer.properties
        {
            *width = 0.09;
            *lifetime = 0.22;
            *max_trails = 128;
        }
    }
    let burst = candidate
        .events
        .iter_mut()
        .find(|e| e.target == star_id)
        .unwrap();
    burst.count = 128;
    let burst_id = burst.id;
    for (tier, count) in [("medium", 64), ("low", 32)] {
        let budget = candidate.particle_budgets.get_mut(tier).unwrap();
        budget.emitter_capacity.insert(star_id, count);
        budget.event_count.insert(burst_id, count);
        budget.trail_capacity.insert(trail_id, count);
    }
    Ok([(CONTROL.into(), control), (CANDIDATE.into(), candidate)])
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
    fn saved_calibration_compiles_with_matching_birth_and_trail_budgets_at_every_tier() {
        let root = root();
        let index = ProjectAssetIndex::scan(&root);
        for (name, expected) in author(&root).unwrap() {
            let saved = EffectAsset::from_ron(
                &std::fs::read_to_string(root.join("effects").join(&name)).unwrap(),
            )
            .unwrap();
            assert_eq!(saved, expected, "regenerate {name}");
            assert_eq!(saved.emitters.len(), 5);
            assert_eq!(saved.particle_outputs.len(), 2);
            assert_eq!(saved.point_lights.len(), 1);
            for tier in ["high", "medium", "low"] {
                let resolved = index.resolve_effect_project(&saved).unwrap();
                let compiled = EffectCompiler::with_extensions(ExtensionRegistry::linked())
                    .with_tier(QualityTier::preset(tier).unwrap())
                    .compile_resolved_project(&resolved)
                    .unwrap();
                assert!(compiled.dependencies.is_empty());
                let effective = saved.with_particle_budget_profile(tier);
                let stars = effective
                    .emitters
                    .iter()
                    .find(|e| e.name == "Main stars")
                    .unwrap();
                assert_eq!(
                    effective
                        .events
                        .iter()
                        .find(|e| e.target == stars.id)
                        .unwrap()
                        .count,
                    stars.max_particles
                );
                for renderer in &stars.renderers {
                    if let RendererProperties::Trail { max_trails, .. } = renderer.properties {
                        assert_eq!(max_trails, stars.max_particles);
                    }
                }
            }
        }
    }
}
