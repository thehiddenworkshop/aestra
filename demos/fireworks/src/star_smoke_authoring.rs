//! Offline authoring of saved star-path candidates; no host-side particle simulation.
use aestra_core::material::MaterialProgramRef;
use aestra_core::*;
use std::{collections::BTreeMap, path::Path};

const BASE: u128 = 0xa3574a00_0000_4000_8000_000000f84000;
pub const FAMILIES: [&str; 4] = ["multi_break", "crackle", "crossette", "strobe"];

pub fn author(root: &Path) -> Result<Vec<(String, EffectAsset)>, String> {
    let read = |name: &str| -> Result<EffectAsset, String> {
        EffectAsset::from_ron(
            &std::fs::read_to_string(root.join("effects").join(name)).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    };
    let mut remap = BTreeMap::new();
    let mut output = Vec::new();
    for (index, family) in FAMILIES.iter().enumerate() {
        let mut asset = read(&format!("fireworks_{family}_wispy_smoke.aestra.ron"))?;
        let base = BASE + (index as u128 + 1) * 256;
        let id = EffectId::from_u128(base);
        remap.insert(asset.id, id);
        asset.id = id;
        asset.metadata.insert("notes".into(), "F8.1C4S opt-in star-path candidate: main stars deposit zero-inheritance smoke via distance crossings; subdued shell haze. Original star, rocket, cue and light sources remain unchanged. Art and paced cost acceptance remain open.".into());
        asset.metadata.insert(
            "status".into(),
            "Star-path smoke art candidate; not default adoption".into(),
        );
        asset.name = asset.name.replace("Wispy Smoke Art", "Star-Path Smoke Art");
        let stars = asset
            .emitters
            .iter()
            .find(|e| e.name == "Main stars")
            .unwrap()
            .clone();
        let mut smoke = asset
            .emitters
            .iter()
            .find(|e| e.name == "Launch smoke")
            .unwrap()
            .clone();
        smoke.id = EmitterId::from_u128(base + 1);
        smoke.name = "Star path smoke".into();
        smoke.max_particles = (stars.max_particles * 64).next_power_of_two();
        smoke.duration = 18.0;
        for (i, module) in smoke.modules.iter_mut().enumerate() {
            module.id = ModuleId::from_u128(base + 16 + i as u128);
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
                    lifetime,
                    speed,
                    angular_velocity,
                    ..
                } => {
                    *lifetime = ScalarRange::new(7.0, 9.0);
                    *speed = ScalarRange::new(0.0, 0.08);
                    *angular_velocity = ScalarRange::new(-0.12, 0.12);
                }
                ModuleParameters::Motion {
                    gravity,
                    drag,
                    turbulence,
                } => {
                    *gravity = [0.27, 0.035, 0.04];
                    *drag = 0.45;
                    *turbulence = 0.35;
                }
                ModuleParameters::Appearance {
                    size,
                    opacity,
                    color,
                } => {
                    size.id = CurveId::from_u128(base + 32);
                    size.keys = vec![
                        CurveKey::new(0.0, 2.0),
                        CurveKey::new(0.3, 2.3),
                        CurveKey::new(1.0, 2.7),
                    ];
                    opacity.id = CurveId::from_u128(base + 33);
                    opacity.keys = vec![
                        CurveKey::new(0.0, 0.0),
                        CurveKey::new(0.035, 0.09),
                        CurveKey::new(0.55, 0.028),
                        CurveKey::new(1.0, 0.0),
                    ];
                    color.id = GradientId::from_u128(base + 34);
                }
                _ => {}
            }
        }
        let old_material = smoke.renderers[0].material;
        let mut material = asset
            .material_instances
            .iter()
            .find(|m| m.id == old_material)
            .unwrap()
            .clone();
        material.id = MaterialId::from_u128(base + 48);
        material.program = MaterialProgramRef::Project(MaterialProgramId::from_u128(
            0xa3574a00_0000_4000_8000_000000f84700,
        ));
        for (i, renderer) in smoke.renderers.iter_mut().enumerate() {
            renderer.id = RendererId::from_u128(base + 40 + i as u128);
            renderer.material = material.id;
        }
        asset.material_instances.push(material);
        let mut link = EventLink::new(
            stars.id,
            EventTrigger::OnDistance {
                spacing: 0.8,
                max_per_tick: 2,
            },
            smoke.id,
        );
        link.id = EventId::from_u128(base + 64);
        for profile in asset.particle_budgets.values_mut() {
            let source_capacity = profile
                .emitter_capacity
                .get(&stars.id)
                .copied()
                .unwrap_or(stars.max_particles);
            profile
                .emitter_capacity
                .insert(smoke.id, (source_capacity * 64).next_power_of_two());
        }
        asset.emitters.push(smoke);
        asset.events.push(link);
        // Keep the old shell-born haze as a subdued background layer rather than
        // letting its large puffs obscure the deposited radial filaments.
        for module in &mut asset
            .emitters
            .iter_mut()
            .find(|e| e.name == "Burst smoke")
            .unwrap()
            .modules
        {
            if let ModuleParameters::Appearance { size, opacity, .. } = &mut module.parameters {
                for key in &mut size.keys {
                    key.value *= 0.6;
                }
                for key in &mut opacity.keys {
                    key.value *= 0.35;
                }
            }
        }
        output.push((
            format!("fireworks_{family}_star_path_smoke.aestra.ron"),
            asset,
        ));
    }
    let mut show = read("fireworks_show_wispy_smoke.aestra.ron")?;
    show.id = EffectId::from_u128(BASE);
    show.name = "Fireworks Star-Path Smoke Art Candidate".into();
    show.metadata.insert("notes".into(), "F8.1C4S saved 13-clip star-path smoke candidate. Choreography and cue bindings retained; new dependency IDs select authored path deposition. Art/performance review is separate from default adoption.".into());
    for clip in &mut show.effect_clips {
        clip.source.id = remap[&clip.source.id];
    }
    output.push(("fireworks_show_star_path_smoke.aestra.ron".into(), show));
    Ok(output)
}
