//! Author editable repertoire assets; the host supplies only launch choreography.
use aestra_core::*;
use std::path::{Path, PathBuf};

const BASE: u128 = 0xa3574a00_0000_4000_8000_000000fa0000;
const KINDS: [&str; 6] = ["ring", "palm", "willow", "crossette", "crackle", "strobe"];
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/test")
}
fn read(root: &Path, name: &str) -> Result<EffectAsset, String> {
    EffectAsset::from_ron(
        &std::fs::read_to_string(root.join(format!("effects/fireworks_{name}.aestra.ron")))
            .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn author(root: &Path, kind: &str, index: usize) -> Result<EffectAsset, String> {
    let reference = read(root, "point_star_reference_smoke")?;
    let source = match kind {
        "ring" => "point_star_reference_smoke".into(),
        "palm" | "willow" => "gold_tail_reference_smoke".into(),
        _ => format!("{kind}_star_path_smoke"),
    };
    let mut asset = read(root, &source)?;
    asset.id = EffectId::from_u128(BASE + index as u128 * 256);
    asset.name = format!(
        "Fireworks — {}",
        match kind {
            "ring" => "Cyan Ring",
            "palm" => "Amber Palm",
            "willow" => "Golden Willow",
            "crossette" => "Ruby Crossette",
            "crackle" => "Silver Crackle",
            _ => "Violet Strobe",
        }
    );
    asset.metadata.insert("repertoire".into(), format!("Editable {kind} shell for the host variety sequence. Shape, cooling, secondary breaks and deposited smoke are authored here; the host owns launch position, velocity and fuse. Separate art draft, not frozen showcase adoption."));
    // Share the current smoke kernel and its exposed controls with the editor.
    for parameter in &reference.parameters {
        if !matches!(
            parameter.name.as_str(),
            "Smoke pattern velocity"
                | "Smoke edge feather"
                | "Smoke billow breakup"
                | "Smoke depth variation"
        ) {
            continue;
        }
        if !asset.parameters.iter().any(|p| p.id == parameter.id) {
            asset.parameters.push(parameter.clone());
        }
    }
    for emitter in &mut asset.emitters {
        if let Some(template) = reference
            .emitters
            .iter()
            .find(|e| e.name == emitter.name && e.name.to_lowercase().contains("smoke"))
        {
            let material = reference
                .material_instances
                .iter()
                .find(|m| m.id == template.renderers[0].material)
                .unwrap();
            if let Some(existing) = asset
                .material_instances
                .iter_mut()
                .find(|m| m.id == material.id)
            {
                *existing = material.clone();
            } else {
                asset.material_instances.push(material.clone());
            }
            for renderer in &mut emitter.renderers {
                renderer.material = material.id;
            }
            for module in &mut emitter.modules {
                if let ModuleParameters::Appearance { size, opacity, .. } = &mut module.parameters {
                    let look = template
                        .modules
                        .iter()
                        .find_map(|m| match &m.parameters {
                            ModuleParameters::Appearance { size, opacity, .. } => {
                                Some((size, opacity))
                            }
                            _ => None,
                        })
                        .unwrap();
                    size.keys = look.0.keys.clone();
                    opacity.keys = look.1.keys.clone();
                }
            }
        }
        if emitter.name != "Main stars" {
            continue;
        }
        for module in &mut emitter.modules {
            match &mut module.parameters {
                ModuleParameters::Initialize {
                    lifetime,
                    speed,
                    direction,
                    spread_degrees,
                    velocity_distribution,
                    ..
                } => match kind {
                    "ring" => {
                        *lifetime = ScalarRange::new(2.6, 2.9);
                        *speed = ScalarRange::new(22.0, 22.0);
                        *direction = [0.0, 0.0, 1.0];
                        *velocity_distribution = VelocityDistribution::Ring;
                    }
                    "palm" => {
                        *lifetime = ScalarRange::new(3.6, 4.2);
                        *speed = ScalarRange::new(19.0, 25.0);
                        *direction = [0.0, 1.0, 0.0];
                        *spread_degrees = 125.0;
                        *velocity_distribution = VelocityDistribution::Cone;
                    }
                    "willow" => {
                        *lifetime = ScalarRange::new(6.0, 7.0);
                        *speed = ScalarRange::new(13.0, 18.0);
                        *velocity_distribution = VelocityDistribution::Sphere;
                    }
                    _ => {}
                },
                ModuleParameters::Motion { drag, gravity, .. }
                    if matches!(kind, "palm" | "willow") =>
                {
                    *drag = if kind == "willow" { 0.28 } else { 0.25 };
                    *gravity = [0.0, -9.81, 0.0];
                }
                ModuleParameters::Appearance { opacity, .. } if kind == "willow" => {
                    opacity.keys = vec![
                        CurveKey::new(0.0, 1.0),
                        CurveKey::new(0.55, 0.85),
                        CurveKey::new(0.82, 0.4),
                        CurveKey::new(1.0, 0.0),
                    ];
                }
                _ => {}
            }
        }
    }
    if matches!(kind, "ring" | "palm" | "willow") {
        let stars = asset
            .emitters
            .iter()
            .find(|e| e.name == "Main stars")
            .unwrap()
            .id;
        let launch = asset
            .emitters
            .iter()
            .find(|e| e.name == "Launch shell")
            .unwrap()
            .id;
        let counts = match kind {
            "ring" => [128, 96, 64],
            "palm" => [16, 12, 8],
            _ => [48, 32, 24],
        };
        let event = asset
            .events
            .iter_mut()
            .find(|e| e.source == launch && e.target == stars && e.trigger == EventTrigger::OnDeath)
            .unwrap();
        event.count = counts[0];
        for (tier, count) in ["medium", "low"].into_iter().zip([counts[1], counts[2]]) {
            asset
                .particle_budgets
                .entry(tier.into())
                .or_default()
                .event_count
                .insert(event.id, count);
        }
    }
    let tint = match kind {
        "ring" => [0.15, 0.9, 1.0],
        "palm" => [1.0, 0.65, 0.18],
        "willow" => [1.0, 0.72, 0.28],
        "crossette" => [1.0, 0.12, 0.18],
        "crackle" => [0.85, 0.93, 1.0],
        _ => [0.6, 0.25, 1.0],
    };
    for parameter in &mut asset.parameters {
        if (parameter.name == "Main stars color"
            || parameter.name == "Crossette arms color"
            || parameter.name == "Crackle sparks color")
            && let Value::Gradient(g) = &mut parameter.default
        {
            for key in &mut g.keys {
                key.color[..3].copy_from_slice(&tint);
            }
        }
    }
    // A standalone saved shell also starts high enough for its long fall. Host
    // flight is independent and supplies its own pose/fuse to the detached burst.
    if kind == "willow" {
        let shell = asset
            .emitters
            .iter_mut()
            .find(|e| e.name == "Launch shell")
            .unwrap();
        for m in &mut shell.modules {
            if let ModuleParameters::Initialize {
                lifetime, speed, ..
            } = &mut m.parameters
            {
                *lifetime = ScalarRange::new(1.8, 1.8);
                *speed = ScalarRange::new(55.0, 55.0);
            }
        }
    }
    Ok(asset)
}

fn main() -> Result<(), String> {
    for (index, kind) in KINDS.into_iter().enumerate() {
        let asset = author(&root(), kind, index)?;
        let path = root().join(format!("effects/fireworks_variety_{kind}.aestra.ron"));
        std::fs::write(&path, asset.to_pretty_ron().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        println!("Authored {}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_repertoire_resolves_compiles_and_retains_secondary_breaks_at_every_tier() {
        let root = root();
        let index = aestra_project::ProjectAssetIndex::scan(&root);
        for (i, kind) in KINDS.into_iter().enumerate() {
            let asset = author(&root, kind, i).unwrap();
            assert_eq!(asset, read(&root, &format!("variety_{kind}")).unwrap());
            for tier in ["high", "medium", "low"] {
                let resolved = index.resolve_effect_project(&asset).unwrap();
                let effect = aestra_compiler::EffectCompiler::default()
                    .with_tier(aestra_runtime::QualityTier::preset(tier).unwrap())
                    .compile_resolved_project(&resolved)
                    .unwrap()
                    .root;
                let presented = aestra_bevy::PresentedEffect::new(effect.clone());
                for e in &effect.emitters {
                    for r in &e.renderers {
                        assert!(
                            presented
                                .material_binding_for_emitter(r.material, e.source)
                                .is_some(),
                            "{kind}/{tier}"
                        );
                    }
                }
            }
            let stars = asset
                .emitters
                .iter()
                .find(|e| e.name == "Main stars")
                .unwrap()
                .id;
            if matches!(kind, "crossette" | "crackle") {
                assert!(
                    asset
                        .events
                        .iter()
                        .any(|e| e.source == stars && e.trigger == EventTrigger::OnDeath)
                );
            }
        }
    }
}
