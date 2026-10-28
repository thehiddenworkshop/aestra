//! Offline gold-tail shell authoring; all motion/births use saved runtime modules.
use aestra_core::{material::MaterialParameterValue, *};
use std::path::{Path, PathBuf};

const BASE: u128 = 0xa3574a00_0000_4000_8000_000000f87000;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/test")
}

fn read(root: &Path, name: &str) -> Result<EffectAsset, String> {
    EffectAsset::from_ron(
        &std::fs::read_to_string(root.join("effects").join(name)).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn author(root: &Path) -> Result<[(String, EffectAsset); 2], String> {
    let mut control = read(root, "fireworks_point_star_control.aestra.ron")?;
    control.id = EffectId::from_u128(BASE);
    control.name = "Fireworks Gold-Tail Calibration Control".into();
    control.metadata.insert(
        "status".into(),
        "Isolated gold-shell control; not default adoption".into(),
    );
    control.metadata.insert("notes".into(), "Retained isolated shell dynamics/density/trails with a shared warm-gold palette. No secondary breaks or deposited star smoke. Control for the separate gold-tail candidate.".into());
    let palette = vec![
        ColorKey::new(0.0, [1.0, 0.91, 0.55, 1.0]),
        ColorKey::new(0.25, [1.0, 0.64, 0.18, 1.0]),
        ColorKey::new(0.75, [0.72, 0.25, 0.035, 1.0]),
        ColorKey::new(1.0, [0.12, 0.012, 0.001, 1.0]),
    ];
    if let Value::Gradient(gradient) = &mut control
        .parameters
        .iter_mut()
        .find(|p| p.name == "Main stars color")
        .unwrap()
        .default
    {
        gradient.keys = palette;
    }
    let mut candidate = control.clone();
    candidate.id = EffectId::from_u128(BASE + 0x100);
    candidate.name = "Fireworks Gold-Tail Calibration Candidate".into();
    candidate.metadata.insert(
        "status".into(),
        "Gold-tail art candidate; not default adoption".into(),
    );
    candidate.metadata.insert("notes".into(), "Video-reference gold-tail candidate: varied long-lived ballistic stars shed zero-independent-emission embers along OnDistance paths. Thin cooling history, small drifting/fading sparks, explicit tier pools. No deposited smoke or selected lights; separate show/performance approval.".into());
    let stars = candidate
        .emitters
        .iter_mut()
        .find(|e| e.name == "Main stars")
        .unwrap();
    let star_id = stars.id;
    for module in &mut stars.modules {
        match &mut module.parameters {
            ModuleParameters::Initialize {
                lifetime, speed, ..
            } => {
                *lifetime = ScalarRange::new(2.8, 4.0);
                *speed = ScalarRange::new(24.0, 30.0);
            }
            ModuleParameters::Motion { drag, .. } => *drag = 0.4,
            ModuleParameters::Appearance { size, opacity, .. } => {
                size.keys = vec![
                    CurveKey::new(0.0, 0.6),
                    CurveKey::new(0.55, 0.45),
                    CurveKey::new(1.0, 0.08),
                ];
                opacity.keys = vec![
                    CurveKey::new(0.0, 1.0),
                    CurveKey::new(0.6, 0.85),
                    CurveKey::new(0.85, 0.45),
                    CurveKey::new(1.0, 0.0),
                ];
            }
            _ => {}
        }
    }
    for (index, renderer) in stars.renderers.iter_mut().enumerate() {
        let trail = matches!(renderer.properties, RendererProperties::Trail { .. });
        let mut material = candidate
            .material_instances
            .iter()
            .find(|m| m.id == renderer.material)
            .unwrap()
            .clone();
        material.id = MaterialId::from_u128(BASE + 0x180 + index as u128);
        let id = ParameterId::from_u128(BASE + 0x190 + index as u128);
        for value in material.values.values_mut() {
            *value = MaterialParameterValue::EffectParameter(id);
        }
        candidate.parameters.push(EffectParameter {
            id,
            name: if trail {
                "Gold cooling tail radiance"
            } else {
                "Gold head radiance"
            }
            .into(),
            default: Value::Scalar(if trail { 4.0 } else { 24.0 }),
            exposed: true,
        });
        renderer.material = material.id;
        candidate.material_instances.push(material);
        if let RendererProperties::Trail {
            width, lifetime, ..
        } = &mut renderer.properties
        {
            *width = 0.12;
            *lifetime = 0.65;
        }
    }
    let original = read(root, "fireworks_multi_break_wind_haze_smoke.aestra.ron")?;
    let mut embers = original
        .emitters
        .iter()
        .find(|e| e.name == "Secondary sparks")
        .unwrap()
        .clone();
    embers.id = EmitterId::from_u128(BASE + 0x120);
    embers.name = "Gold tail embers".into();
    embers.duration = 18.0;
    embers.max_particles = 32768;
    for (index, module) in embers.modules.iter_mut().enumerate() {
        module.id = ModuleId::from_u128(BASE + 0x130 + index as u128);
        module.bindings.clear();
        match &mut module.parameters {
            ModuleParameters::Emission {
                spawn_rate,
                burst_count,
            } => {
                *spawn_rate = 0.0;
                *burst_count = 0;
            }
            ModuleParameters::Shape { shape } => *shape = EmitterShape::Point,
            ModuleParameters::Initialize {
                lifetime, speed, ..
            } => {
                *lifetime = ScalarRange::new(0.45, 1.05);
                *speed = ScalarRange::new(0.6, 2.8);
            }
            ModuleParameters::Motion {
                gravity,
                drag,
                turbulence,
            } => {
                *gravity = [0.25, -3.5, 0.05];
                *drag = 1.1;
                *turbulence = 0.9;
            }
            ModuleParameters::Appearance {
                size,
                opacity,
                color,
            } => {
                size.id = CurveId::from_u128(BASE + 0x140);
                size.keys = vec![
                    CurveKey::new(0.0, 0.35),
                    CurveKey::new(0.6, 0.18),
                    CurveKey::new(1.0, 0.035),
                ];
                opacity.id = CurveId::from_u128(BASE + 0x141);
                opacity.keys = vec![
                    CurveKey::new(0.0, 0.9),
                    CurveKey::new(0.5, 0.6),
                    CurveKey::new(1.0, 0.0),
                ];
                color.id = GradientId::from_u128(BASE + 0x142);
                color.keys = vec![
                    ColorKey::new(0.0, [1.0, 0.72, 0.24, 1.0]),
                    ColorKey::new(0.5, [0.9, 0.3, 0.035, 1.0]),
                    ColorKey::new(1.0, [0.12, 0.015, 0.001, 1.0]),
                ];
            }
            _ => {}
        }
    }
    // Short-lived sprite embers break up the thin ribbon without allocating a
    // second ribbon history for every shed spark.
    embers
        .renderers
        .retain(|r| matches!(r.properties, RendererProperties::Sprite));
    let renderer = &mut embers.renderers[0];
    renderer.id = RendererId::from_u128(BASE + 0x150);
    let mut material = candidate
        .material_instances
        .iter()
        .find(|m| m.id == renderer.material)
        .unwrap()
        .clone();
    material.id = MaterialId::from_u128(BASE + 0x160);
    let id = ParameterId::from_u128(BASE + 0x1a0);
    for value in material.values.values_mut() {
        *value = MaterialParameterValue::EffectParameter(id);
    }
    candidate.parameters.push(EffectParameter {
        id,
        name: "Gold ember radiance".into(),
        default: Value::Scalar(24.0),
        exposed: true,
    });
    renderer.material = material.id;
    candidate.material_instances.push(material);
    let mut link = EventLink::new(
        star_id,
        EventTrigger::OnDistance {
            spacing: 0.4,
            max_per_tick: 2,
        },
        embers.id,
    );
    link.id = EventId::from_u128(BASE + 0x170);
    link.count = 6;
    link.inherit_velocity = 0.06;
    for (tier, capacity) in [("medium", 16384), ("low", 8192)] {
        candidate
            .particle_budgets
            .get_mut(tier)
            .unwrap()
            .emitter_capacity
            .insert(embers.id, capacity);
    }
    candidate.emitters.push(embers);
    candidate.events.push(link);
    Ok([
        ("fireworks_gold_tail_control.aestra.ron".into(), control),
        ("fireworks_gold_tail_candidate.aestra.ron".into(), candidate),
    ])
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
    fn gold_assets_compile_at_all_tiers_with_bounded_distance_births_and_no_independent_embers() {
        let root = root();
        let index = ProjectAssetIndex::scan(&root);
        for (name, expected) in author(&root).unwrap() {
            let saved = read(&root, &name).unwrap();
            assert_eq!(saved, expected, "regenerate {name}");
            for tier in ["high", "medium", "low"] {
                let resolved = index.resolve_effect_project(&saved).unwrap();
                EffectCompiler::with_extensions(ExtensionRegistry::linked())
                    .with_tier(QualityTier::preset(tier).unwrap())
                    .compile_resolved_project(&resolved)
                    .unwrap();
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
                    if let RendererProperties::Trail {
                        max_trails,
                        max_points,
                        lifetime,
                        sample_interval,
                        ..
                    } = renderer.properties
                    {
                        assert_eq!(max_trails, stars.max_particles);
                        assert!(lifetime / sample_interval + 2.0 <= max_points as f32);
                    }
                }
                if let Some(embers) = effective
                    .emitters
                    .iter()
                    .find(|e| e.name == "Gold tail embers")
                {
                    assert!(
                        embers
                            .renderers
                            .iter()
                            .all(|r| matches!(r.properties, RendererProperties::Sprite))
                    );
                    assert!(embers.modules.iter().all(|m| !matches!(m.parameters, ModuleParameters::Emission { spawn_rate, burst_count } if spawn_rate != 0.0 || burst_count != 0)));
                    let link = effective
                        .events
                        .iter()
                        .find(|e| e.target == embers.id)
                        .unwrap();
                    assert_eq!(link.source, stars.id);
                    assert_eq!(link.count, 6);
                    assert!(stars.max_particles * link.trigger.samples_per_tick() <= 1024);
                }
            }
        }
    }
}
