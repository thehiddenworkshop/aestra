use std::sync::Arc;

use aestra_compiler::EffectCompiler;
use aestra_core::{EffectAsset, Emitter};
use aestra_gpu::{
    GpuEffectArtifact,
    shader::{GpuShaderPackage, SIMULATION_WESL, SPRITE_RENDER_WESL},
};
use aestra_runtime::EffectInstance;

fn representative_artifact() -> GpuEffectArtifact {
    let mut effect = EffectAsset::new("Shader contract", 2.0);
    effect
        .emitters
        .push(Emitter::basic_sprite("Emitter", effect.duration));
    let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
    GpuEffectArtifact::from_instance(&EffectInstance::new(compiled)).unwrap()
}

fn normalized(source: &str) -> String {
    source.replace("\r\n", "\n").trim_end().to_owned()
}

#[test]
fn representative_artifact_produces_naga_validated_shader_package() {
    let artifact = representative_artifact();
    let package = GpuShaderPackage::for_artifact(&artifact).unwrap();

    assert_eq!(package.layout.emitter_count, 1);
    assert_eq!(package.layout.renderer_count, 1);
    assert_eq!(package.layout.total_particle_slots, artifact.total_slots);
    assert!(package.simulation.wgsl.contains("fn simulate"));
    assert!(package.sprite_render.wgsl.contains("fn vertex"));
}

#[test]
fn generated_wgsl_matches_reviewable_snapshots() {
    let package = GpuShaderPackage::for_artifact(&representative_artifact()).unwrap();

    assert_eq!(
        normalized(&package.simulation.wgsl),
        normalized(include_str!("snapshots/simulation.wgsl"))
    );
    assert_eq!(
        normalized(&package.sprite_render.wgsl),
        normalized(include_str!("snapshots/sprite_render.wgsl"))
    );
}

#[test]
fn portable_wesl_has_no_engine_shader_imports() {
    for source in [SIMULATION_WESL, SPRITE_RENDER_WESL] {
        assert!(!source.contains("#import bevy"));
        assert!(!source.contains("bevy::"));
        assert!(!source.contains("wgpu::"));
    }
}
