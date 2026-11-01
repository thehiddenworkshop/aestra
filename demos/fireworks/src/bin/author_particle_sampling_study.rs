//! Matched gold shell and a portable editor lab, using the shared spawn module.
use aestra_core::{particle_sampling::*, *};
use std::path::{Path, PathBuf};
const BASE: u128 = 0xa3574a00_0000_4000_8000_000000ab3000;
fn root(folder: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(folder)
}
fn settings(mode: SamplingMode) -> ParticleSampling {
    ParticleSampling {
        positions: SamplingMode::Legacy,
        directions: mode,
        sequence_count: 64,
        jitter: 0.12,
        seed: 17,
    }
}
fn assets() -> Result<[(PathBuf, EffectAsset); 3], String> {
    let source = EffectAsset::load_ron(
        root("assets/test").join("effects/fireworks_gold_filament_candidate.aestra.ron"),
    )
    .map_err(|e| e.to_string())?;
    let mut control = source.clone();
    control.id = EffectId::from_u128(BASE);
    control.name = "Gold shell — uniform sampling control".into();
    let mut candidate = source.clone();
    candidate.id = EffectId::from_u128(BASE + 1);
    candidate.name = "Gold shell — stratified sampling".into();
    for (effect, mode) in [
        (&mut control, SamplingMode::Uniform),
        (&mut candidate, SamplingMode::Stratified),
    ] {
        let stars = effect
            .emitters
            .iter_mut()
            .find(|e| e.name == "Main stars")
            .ok_or("Main stars missing")?;
        let mut module = ModuleInstance::particle_sampling(settings(mode));
        module.id = ModuleId::from_u128(BASE + 2);
        stars.modules.push(module);
    }
    let mut lab = EffectAsset::new("Particle Sampling Lab", 6.0);
    lab.id = EffectId::from_u128(BASE + 3);
    lab.playback_mode = EffectPlaybackMode::LoopRestart;
    let mut stars = Emitter::basic_sprite("Stratified sphere", 6.0);
    stars.id = EmitterId::from_u128(BASE + 4);
    stars.max_particles = 64;
    stars.transform.translation = [0.0, 32.0, 0.0];
    stars.modules = vec![
        ModuleInstance::emission(0.0, 64),
        ModuleInstance::shape(EmitterShape::Point),
        ModuleInstance::initialize(
            ScalarRange::new(3.5, 4.0),
            ScalarRange::new(18.0, 20.0),
            [0.0, 1.0, 0.0],
            0.0,
            ScalarRange::new(0.0, 0.0),
        ),
        ModuleInstance::motion([0.0, -0.5, 0.0], 0.12, 0.0),
        ModuleInstance::appearance(
            Curve::new(vec![CurveKey::new(0.0, 0.3), CurveKey::new(1.0, 0.1)]),
            Curve::new(vec![
                CurveKey::new(0.0, 1.0),
                CurveKey::new(0.8, 0.8),
                CurveKey::new(1.0, 0.0),
            ]),
            Gradient::new(vec![
                ColorKey::new(0.0, [8.0, 6.8, 3.2, 1.0]),
                ColorKey::new(1.0, [2.0, 0.4, 0.05, 0.0]),
            ]),
        ),
        ModuleInstance::particle_sampling(settings(SamplingMode::Stratified)),
    ];
    for (i, m) in stars.modules.iter_mut().enumerate() {
        m.id = ModuleId::from_u128(BASE + 0x10 + i as u128);
        if let ModuleParameters::Initialize {
            velocity_distribution,
            ..
        } = &mut m.parameters
        {
            *velocity_distribution = VelocityDistribution::Sphere;
        }
    }
    for module in &mut stars.modules {
        if let ModuleParameters::Appearance {
            size,
            opacity,
            color,
        } = &mut module.parameters
        {
            size.id = CurveId::from_u128(BASE + 0x40);
            opacity.id = CurveId::from_u128(BASE + 0x41);
            color.id = GradientId::from_u128(BASE + 0x42);
        }
    }
    stars.renderers[0].id = RendererId::from_u128(BASE + 0x30);
    let template = source
        .emitters
        .iter()
        .find(|e| e.name == "Main stars")
        .unwrap()
        .renderers
        .iter()
        .find(|r| matches!(r.properties, RendererProperties::Trail { .. }))
        .ok_or("Trail missing")?;
    let mut trail = template.clone();
    trail.id = RendererId::from_u128(BASE + 0x31);
    trail.material = DEFAULT_SPRITE_MATERIAL_ID;
    if let RendererProperties::Trail {
        width, lifetime, ..
    } = &mut trail.properties
    {
        *width = 0.7;
        *lifetime = 1.5;
    }
    stars.renderers.push(trail);
    lab.emitters.push(stars);
    // Source v4 groups modules by stage; return the same canonical ordering as saved assets.
    for effect in [&mut lab, &mut control, &mut candidate] {
        *effect = EffectAsset::from_ron(&effect.to_pretty_ron().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    }
    Ok([
        (
            root("sample-project").join("effects/particle_sampling_lab.aestra.ron"),
            lab,
        ),
        (
            root("assets/test").join("effects/fireworks_sampling_control.aestra.ron"),
            control,
        ),
        (
            root("assets/test").join("effects/fireworks_sampling.aestra.ron"),
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
    fn saved_studies_compile_and_preserve_counts_events_motion_materials_and_ranges() {
        let assets = assets().unwrap();
        let control = &assets[1].1;
        let candidate = &assets[2].1;
        assert_eq!(control.events, candidate.events);
        assert_eq!(control.point_lights, candidate.point_lights);
        assert_eq!(control.particle_outputs, candidate.particle_outputs);
        assert_eq!(control.material_instances, candidate.material_instances);
        for (a, b) in control.emitters.iter().zip(&candidate.emitters) {
            assert_eq!(a.max_particles, b.max_particles);
            assert_eq!(a.renderers, b.renderers);
            if a.name == "Main stars" {
                assert_eq!(
                    a.modules
                        .iter()
                        .filter(|m| m.module_type.0 != MODULE_PARTICLE_SAMPLING)
                        .collect::<Vec<_>>(),
                    b.modules
                        .iter()
                        .filter(|m| m.module_type.0 != MODULE_PARTICLE_SAMPLING)
                        .collect::<Vec<_>>()
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
