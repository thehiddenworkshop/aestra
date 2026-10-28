//! Separate smoke silhouette/light tuning; the calibrated control remains intact.
use aestra_core::{material::*, *};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

const BASE: u128 = 0xa3574a00_0000_4000_8000_000000f8b000;
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/test")
}
fn read(root: &Path, file: &str) -> Result<String, String> {
    std::fs::read_to_string(root.join(file)).map_err(|e| e.to_string())
}
fn effect(root: &Path, name: &str) -> Result<EffectAsset, String> {
    EffectAsset::from_ron(&read(root, &format!("effects/{name}.aestra.ron"))?)
        .map_err(|e| e.to_string())
}

fn material(root: &Path) -> Result<(MaterialFunction, MaterialProgram), String> {
    let text = read(
        root,
        "materials/wind_smoke_wisp.aestra.material-function.ron",
    )?
    .replace("000000f856", "000000f8b6");
    let mut function = MaterialFunction::from_ron(&text).map_err(|e| e.to_string())?;
    function.name = "Eroding Fireworks Smoke Wisps".into();
    let source = &mut function
        .custom_wesl
        .as_mut()
        .ok_or("custom smoke mask missing")?
        .source;
    for (old, new) in [
        ("uv * 1.8", "uv * 2.8"),
        ("0.80, 0.17, 0.03", "0.62, 0.28, 0.10"),
        ("vec2<f32>(0.75, 1.25)", "vec2<f32>(0.60, 1.60)"),
        ("* 0.28;", "* 0.45;"),
        ("exp(-2.4 *", "exp(-1.6 *"),
        (
            "0.75 + 0.25 * smoothstep(mix(0.10, 0.30, age), 0.85, n)",
            "smoothstep(mix(0.20, 0.44, age), 0.82, n)",
        ),
        (
            "return edge * erosion * (0.65 + 0.35 * seed);",
            "let tile = 1.0 - smoothstep(0.34, 0.50, max(abs(centered.x), abs(centered.y)));\n    return edge * erosion * tile * (0.65 + 0.35 * seed);",
        ),
    ] {
        if !source.contains(old) {
            return Err(format!("Smoke template changed: {old}"));
        }
        *source = source.replace(old, new);
    }
    let text = read(
        root,
        "materials/fireworks_wind_haze_smoke.aestra.material.ron",
    )?
    .replace("000000f857", "000000f8b7")
    .replace("000000f856", "000000f8b6");
    let mut program = MaterialProgram::from_ron(&text).map_err(|e| e.to_string())?;
    program.name = "Fireworks Eroding Lit Smoke".into();
    for parameter in &mut program.parameters {
        if parameter.name == "Scene light gain" {
            parameter.default = Some(MaterialValue::Float(0.035));
        }
    }
    Ok((function, program))
}

fn author(root: &Path) -> Result<Vec<(String, EffectAsset)>, String> {
    let mut assets = Vec::new();
    let mut remap = Vec::new();
    for (family, name) in ["point_star", "gold_tail"].into_iter().enumerate() {
        let mut asset = effect(root, &format!("fireworks_{name}_lit_smoke"))?;
        remap.push((
            asset.id,
            EffectId::from_u128(BASE + (family as u128 + 1) * 256),
        ));
        asset.id = remap.last().unwrap().1;
        asset.name.push_str(" — Eroding Smoke Look");
        let mut smoke_materials = BTreeSet::new();
        for emitter in &mut asset.emitters {
            if emitter.name.to_ascii_lowercase().contains("smoke") {
                smoke_materials.extend(emitter.renderers.iter().map(|r| r.material));
                for module in &mut emitter.modules {
                    if let ModuleParameters::Appearance { size, opacity, .. } =
                        &mut module.parameters
                    {
                        // Thin deposition first; expansion belongs to older smoke.
                        size.keys = if emitter.name == "Star path smoke" {
                            vec![
                                CurveKey::new(0.0, 0.8),
                                CurveKey::new(0.25, 1.6),
                                CurveKey::new(0.65, 3.1),
                                CurveKey::new(1.0, 4.0),
                            ]
                        } else if emitter.name == "Burst smoke" {
                            vec![
                                CurveKey::new(0.0, 0.4),
                                CurveKey::new(0.12, 1.6),
                                CurveKey::new(0.45, 4.0),
                                CurveKey::new(1.0, 6.0),
                            ]
                        } else {
                            vec![
                                CurveKey::new(0.0, 1.2),
                                CurveKey::new(0.08, 1.6),
                                CurveKey::new(0.4, 2.8),
                                CurveKey::new(1.0, 5.0),
                            ]
                        };
                        // Fade in in tens of milliseconds, while the head/flash is
                        // still present. The old launch peak arrived after its fuse.
                        opacity.keys = if emitter.name == "Launch smoke" {
                            vec![
                                CurveKey::new(0.0, 0.0),
                                CurveKey::new(0.012, 0.42),
                                CurveKey::new(0.12, 0.24),
                                CurveKey::new(0.55, 0.06),
                                CurveKey::new(0.85, 0.01),
                                CurveKey::new(1.0, 0.0),
                            ]
                        } else if emitter.name == "Burst smoke" {
                            vec![
                                CurveKey::new(0.0, 0.0),
                                CurveKey::new(0.015, 0.085),
                                CurveKey::new(0.12, 0.07),
                                CurveKey::new(0.55, 0.025),
                                CurveKey::new(0.85, 0.0045),
                                CurveKey::new(1.0, 0.0),
                            ]
                        } else {
                            let density = if family == 0 { 1.0 } else { 0.5 };
                            [
                                (0.0, 0.0),
                                (0.012, 0.075),
                                (0.2, 0.048),
                                (0.55, 0.012),
                                (0.85, 0.0015),
                                (1.0, 0.0),
                            ]
                            .into_iter()
                            .map(|(t, v)| CurveKey::new(t, v * density))
                            .collect()
                        };
                    }
                }
            }
            if family == 1 && emitter.name == "Main stars" {
                for output in &mut emitter.scene_outputs {
                    let SceneOutputProperties::ParticlePointLight(light) = &mut output.properties;
                    for key in &mut light.intensity_curve.keys {
                        key.value *= 2.0;
                    }
                    for key in &mut light.range_curve.keys {
                        key.value = 16.0;
                    }
                }
            }
        }
        for instance in &mut asset.material_instances {
            if !smoke_materials.contains(&instance.id) {
                continue;
            }
            let old = match instance.program {
                MaterialProgramRef::Project(id)
                    if id
                        == MaterialProgramId::from_u128(0xa3574a00_0000_4000_8000_000000f85700) =>
                {
                    0xa3574a00_0000_4000_8000_000000f85700
                }
                MaterialProgramRef::Project(id)
                    if id
                        == MaterialProgramId::from_u128(0xa3574a00_0000_4000_8000_000000f81700) =>
                {
                    0xa3574a00_0000_4000_8000_000000f81700
                }
                _ => return Err("Unexpected smoke material program".into()),
            };
            instance.program =
                MaterialProgramRef::Project(MaterialProgramId::from_u128(BASE + 0x700));
            for index in 1..=3 {
                if let Some(value) = instance
                    .values
                    .remove(&MaterialParameterId::from_u128(old + index))
                {
                    instance
                        .values
                        .insert(MaterialParameterId::from_u128(BASE + 0x700 + index), value);
                }
            }
            let gain = MaterialParameterId::from_u128(BASE + 0x701);
            if instance.values.contains_key(&gain) {
                instance.values.insert(
                    gain,
                    MaterialParameterValue::Constant(MaterialValue::Float(0.24)),
                );
            }
        }
        asset.metadata.insert("smoke_look".into(), "Opt-in eroding mask with prompt deposited-wake visibility, audience-readable star filaments, gradual opacity dilution and local scene-light response; gold selected-light intensity/range tuning. Births, lifetimes, motion, luminous art, event routes and all capacity ceilings retained.".into());
        assets.push((format!("fireworks_{name}_tuned_smoke.aestra.ron"), asset));
    }
    let mut pair = effect(root, "fireworks_shell_star_lights")?;
    pair.id = EffectId::from_u128(BASE);
    pair.name = "Fireworks Eroding Smoke Look Pair".into();
    for clip in &mut pair.effect_clips {
        clip.source.id = remap
            .iter()
            .find(|(old, _)| *old == clip.source.id)
            .ok_or("Unexpected source")?
            .1;
    }
    assets.push(("fireworks_shell_tuned_smoke.aestra.ron".into(), pair));
    Ok(assets)
}
fn main() -> Result<(), String> {
    let root = root();
    let (function, program) = material(&root)?;
    std::fs::write(
        root.join("materials/eroding_smoke_wisp.aestra.material-function.ron"),
        function.to_pretty_ron().map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(
        root.join("materials/fireworks_eroding_smoke.aestra.material.ron"),
        program.to_pretty_ron().map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    for (name, asset) in author(&root)? {
        std::fs::write(
            root.join("effects").join(&name),
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
    fn launch_wake_is_visible_before_the_fuse_and_smoke_dilutes_before_retirement() {
        for (name, asset) in author(&root()).unwrap() {
            if name.contains("shell") {
                continue;
            }
            for emitter in &asset.emitters {
                if !emitter.name.to_lowercase().contains("smoke") {
                    continue;
                }
                let lifetime = emitter
                    .modules
                    .iter()
                    .find_map(|m| match m.parameters {
                        ModuleParameters::Initialize { lifetime, .. } => Some(lifetime),
                        _ => None,
                    })
                    .unwrap();
                let (size, opacity) = emitter
                    .modules
                    .iter()
                    .find_map(|m| match &m.parameters {
                        ModuleParameters::Appearance { size, opacity, .. } => Some((size, opacity)),
                        _ => None,
                    })
                    .unwrap();
                assert_eq!(opacity.sample(0.0), 0.0);
                assert_eq!(opacity.sample(1.0), 0.0);
                assert!(opacity.sample(0.85) < opacity.sample(0.2) * 0.1);
                assert!(size.sample(0.6) > size.sample(0.05));
                if emitter.name == "Launch smoke" {
                    // Longest-lived wake must already register 100ms after birth,
                    // rather than appearing only after the 1.3s host detonation.
                    assert!(opacity.sample(0.1 / lifetime.max) > 0.1);
                    assert!(size.sample(0.1 / lifetime.max) >= 1.0);
                }
            }
        }
    }
    #[test]
    fn saved_look_retains_workload_and_compiles_at_all_tiers() {
        let root = root();
        let index = ProjectAssetIndex::scan(&root);
        let (function, program) = material(&root).unwrap();
        assert_eq!(
            function,
            MaterialFunction::from_ron(
                &read(
                    &root,
                    "materials/eroding_smoke_wisp.aestra.material-function.ron"
                )
                .unwrap()
            )
            .unwrap()
        );
        assert_eq!(
            program,
            MaterialProgram::from_ron(
                &read(
                    &root,
                    "materials/fireworks_eroding_smoke.aestra.material.ron"
                )
                .unwrap()
            )
            .unwrap()
        );
        for (name, expected) in author(&root).unwrap() {
            let saved =
                EffectAsset::from_ron(&read(&root, &format!("effects/{name}")).unwrap()).unwrap();
            assert_eq!(saved, expected);
            if !name.contains("shell") {
                let source = effect(
                    &root,
                    &name
                        .trim_end_matches(".aestra.ron")
                        .replace("tuned_smoke", "lit_smoke"),
                )
                .unwrap();
                assert_eq!(saved.events, source.events);
                assert_eq!(saved.particle_outputs, source.particle_outputs);
                assert_eq!(saved.point_lights, source.point_lights);
                assert_eq!(saved.particle_budgets, source.particle_budgets);
                for (a, b) in saved.emitters.iter().zip(&source.emitters) {
                    assert_eq!(a.max_particles, b.max_particles);
                    assert_eq!(a.renderers, b.renderers);
                    if !a.name.to_lowercase().contains("smoke") {
                        assert_eq!(a.modules, b.modules);
                    } else {
                        for (am, bm) in a.modules.iter().zip(&b.modules) {
                            if !matches!(am.parameters, ModuleParameters::Appearance { .. }) {
                                assert_eq!(am, bm);
                            }
                        }
                    }
                }
            }
            for tier in ["high", "medium", "low"] {
                let resolved = index.resolve_effect_project(&saved).unwrap();
                EffectCompiler::default()
                    .with_tier(QualityTier::preset(tier).unwrap())
                    .compile_resolved_project(&resolved)
                    .unwrap();
            }
        }
    }
}
