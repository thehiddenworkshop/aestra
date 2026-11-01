//! Ordinary saved editor effects validate shared flow; demo choreography stays host-owned.
use aestra_core::{spatial_flow::*, *};
use std::path::{Path, PathBuf};
const BASE: u128 = 0xa3574a00_0000_4000_8000_000000ab2000;
fn root(folder: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(folder)
}
fn flow() -> SpatialFlow {
    SpatialFlow {
        space: FlowSpace::World,
        scale: 0.13,
        strength: 2.0,
        evolution: [0.22, 0.02, 0.05],
        ..Default::default()
    }
}
fn add_flow(effect: &mut EffectAsset, active: bool) {
    for (i, emitter) in effect.emitters.iter_mut().enumerate() {
        if !emitter.name.to_ascii_lowercase().contains("smoke") {
            continue;
        }
        // Match both studies' solver/legacy forces. Existing assets are not rewritten.
        for module in &mut emitter.modules {
            if let ModuleParameters::Motion { turbulence, .. } = &mut module.parameters {
                *turbulence = 0.0;
            }
        }
        let mut module = ModuleInstance::spatial_flow(SpatialFlow {
            strength: if active { flow().strength } else { 0.0 },
            ..flow()
        });
        module.id = ModuleId::from_u128(BASE + 0x100 + i as u128);
        emitter.modules.push(module);
    }
}
fn assets() -> Result<[(PathBuf, EffectAsset); 3], String> {
    let mut wake =
        EffectAsset::load_ron(root("sample-project").join("effects/smoke_wake_lab.aestra.ron"))
            .map_err(|e| e.to_string())?;
    let appearance =
        EffectAsset::load_ron(root("sample-project").join("effects/noise_smoke_lab.aestra.ron"))
            .map_err(|e| e.to_string())?;
    wake.id = EffectId::from_u128(BASE);
    wake.name = "Spatial Flow Wake Lab".into();
    wake.material_instances = appearance.material_instances;
    wake.parameters = appearance.parameters;
    add_flow(&mut wake, true);
    let mut control =
        EffectAsset::load_ron(root("assets/test").join("effects/fireworks_noise_smoke.aestra.ron"))
            .map_err(|e| e.to_string())?;
    let mut candidate = control.clone();
    control.id = EffectId::from_u128(BASE + 1);
    control.name = "Fireworks — Spatial Flow Control".into();
    add_flow(&mut control, false);
    candidate.id = EffectId::from_u128(BASE + 2);
    candidate.name = "Fireworks — Spatial Flow Study".into();
    add_flow(&mut candidate, true);
    Ok([
        (
            root("sample-project").join("effects/spatial_flow_wake_lab.aestra.ron"),
            wake,
        ),
        (
            root("assets/test").join("effects/fireworks_spatial_flow_control.aestra.ron"),
            control,
        ),
        (
            root("assets/test").join("effects/fireworks_spatial_flow.aestra.ron"),
            candidate,
        ),
    ])
}
fn main() -> Result<(), String> {
    for (path, effect) in assets()? {
        std::fs::write(path, effect.to_pretty_ron().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_studies_preserve_launches_events_and_allocations_and_compile_all_tiers() {
        let assets = assets().unwrap();
        let control = &assets[1].1;
        let candidate = &assets[2].1;
        assert_eq!(control.events, candidate.events);
        assert_eq!(control.point_lights, candidate.point_lights);
        assert_eq!(control.particle_outputs, candidate.particle_outputs);
        for (a, b) in control.emitters.iter().zip(&candidate.emitters) {
            assert_eq!(a.max_particles, b.max_particles);
            assert_eq!(a.renderers, b.renderers);
            if a.name.to_ascii_lowercase().contains("smoke") {
                assert_eq!(
                    &a.modules[..a.modules.len() - 1],
                    &b.modules[..b.modules.len() - 1]
                );
            } else {
                assert_eq!(a, b);
            }
        }
        for (path, effect) in assets {
            assert_eq!(EffectAsset::load_ron(&path).unwrap(), effect);
            let index =
                aestra_project::ProjectAssetIndex::scan(path.parent().unwrap().parent().unwrap());
            let resolved = index.resolve_effect_project(&effect).unwrap();
            for tier in ["high", "medium", "low"] {
                aestra_compiler::EffectCompiler::default()
                    .with_tier(aestra_runtime::QualityTier::preset(tier).unwrap())
                    .compile_resolved_project(&resolved)
                    .unwrap();
            }
        }
    }
}
