//! Offline art variant of the retained C4S path-deposition fixture.
use aestra_core::{material::MaterialProgramRef, *};
use std::{collections::BTreeMap, path::Path};

pub const FAMILIES: [&str; 4] = ["multi_break", "crackle", "crossette", "strobe"];
const BASE: u128 = 0xa3574a00_0000_4000_8000_000000f85000;

pub fn author(root: &Path) -> Result<Vec<(String, EffectAsset)>, String> {
    let mut output = Vec::new();
    let mut remap = BTreeMap::new();
    for (index, family) in FAMILIES.iter().enumerate() {
        let text = std::fs::read_to_string(
            root.join("effects")
                .join(format!("fireworks_{family}_star_path_smoke.aestra.ron")),
        )
        .map_err(|e| e.to_string())?;
        let mut asset = EffectAsset::from_ron(&text).map_err(|e| e.to_string())?;
        let id = EffectId::from_u128(BASE + (index as u128 + 1) * 256);
        remap.insert(asset.id, id);
        asset.id = id;
        asset.name = asset
            .name
            .replace("Star-Path Smoke Art", "Star-Path Wind Haze");
        asset.metadata.insert("notes".into(), "F8.1C4V opt-in wind-haze art candidate. Path births, stars, cues, budgets and original haze remain unchanged. Only deposited-smoke motion, age appearance and its dedicated material differ; final art/cost approval remains open.".into());
        asset.metadata.insert(
            "status".into(),
            "Wind-haze art candidate; not default adoption".into(),
        );
        let smoke = asset
            .emitters
            .iter_mut()
            .find(|e| e.name == "Star path smoke")
            .unwrap();
        for module in &mut smoke.modules {
            match &mut module.parameters {
                ModuleParameters::Initialize {
                    speed,
                    angular_velocity,
                    ..
                } => {
                    *speed = ScalarRange::new(0.12, 0.55);
                    *angular_velocity = ScalarRange::new(-0.28, 0.28);
                }
                ModuleParameters::Motion {
                    gravity,
                    drag,
                    turbulence,
                } => {
                    *gravity = [0.80, 0.12, 0.16];
                    *drag = 0.25;
                    *turbulence = 3.0;
                }
                ModuleParameters::Appearance { size, opacity, .. } => {
                    size.keys = vec![
                        CurveKey::new(0.0, 1.6),
                        CurveKey::new(0.25, 2.2),
                        CurveKey::new(0.65, 3.1),
                        CurveKey::new(1.0, 4.0),
                    ];
                    opacity.keys = vec![
                        CurveKey::new(0.0, 0.0),
                        CurveKey::new(0.035, 0.065),
                        CurveKey::new(0.45, 0.027),
                        CurveKey::new(0.8, 0.007),
                        CurveKey::new(1.0, 0.0),
                    ];
                }
                _ => {}
            }
        }
        let material = smoke.renderers[0].material;
        asset
            .material_instances
            .iter_mut()
            .find(|m| m.id == material)
            .unwrap()
            .program = MaterialProgramRef::Project(MaterialProgramId::from_u128(BASE + 0x700));
        output.push((
            format!("fireworks_{family}_wind_haze_smoke.aestra.ron"),
            asset,
        ));
    }
    let text =
        std::fs::read_to_string(root.join("effects/fireworks_show_star_path_smoke.aestra.ron"))
            .map_err(|e| e.to_string())?;
    let mut show = EffectAsset::from_ron(&text).map_err(|e| e.to_string())?;
    show.id = EffectId::from_u128(BASE);
    show.name = "Fireworks Star-Path Wind Haze Candidate".into();
    show.metadata.insert("notes".into(), "F8.1C4V saved wind-haze art variant of C4S; same choreography, cue/light routes and path birth counts. Separate native capacity/art/cost review; default and C4S remain controls.".into());
    show.metadata.insert(
        "status".into(),
        "Wind-haze art candidate; not default adoption".into(),
    );
    for clip in &mut show.effect_clips {
        clip.source.id = remap[&clip.source.id];
    }
    output.push(("fireworks_show_wind_haze_smoke.aestra.ron".into(), show));
    Ok(output)
}
