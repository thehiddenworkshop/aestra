use aestra_compiler::EffectCompiler;
use aestra_core::{
    EffectAsset, EffectPlaybackMode, Emitter, RENDERER_TRAIL, RendererProperties, RendererTypeId,
};
use aestra_gpu::GpuEffectArtifact;
use aestra_runtime::{EffectInstance, RendererCapability};
use std::sync::Arc;

fn fixture() -> EffectAsset {
    let mut effect = EffectAsset::new("Trail contract", 2.0);
    let mut emitter = Emitter::basic_sprite("Trail", 2.0);
    emitter.max_particles = 8;
    emitter.renderers[0].renderer_type = RendererTypeId(RENDERER_TRAIL.into());
    emitter.renderers[0].properties = RendererProperties::Trail {
        width: 1.0,
        sample_interval: 0.025,
        lifetime: 0.5,
        max_points: 32,
    };
    effect.emitters.push(emitter);
    effect
}

#[test]
fn validates_bounded_history_and_keeps_normal_particle_capacity_separate() {
    let effect = fixture();
    let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
    assert!(
        compiled
            .requirements
            .renderers
            .contains(&RendererCapability::RibbonParticles)
    );
    let instance = EffectInstance::new(compiled);
    let gpu = GpuEffectArtifact::from_instance(&instance).unwrap();
    assert_eq!(gpu.particles.len(), 8 + 1 + 8 * 32);
    assert_eq!(gpu.total_slots, 8);
    for invalid in [
        RendererProperties::Trail {
            width: f32::NAN,
            sample_interval: 0.1,
            lifetime: 1.0,
            max_points: 4,
        },
        RendererProperties::Trail {
            width: 1.0,
            sample_interval: 0.0,
            lifetime: 1.0,
            max_points: 4,
        },
        RendererProperties::Trail {
            width: 1.0,
            sample_interval: 0.1,
            lifetime: -1.0,
            max_points: 4,
        },
        RendererProperties::Trail {
            width: 1.0,
            sample_interval: 0.1,
            lifetime: 1.0,
            max_points: 65,
        },
    ] {
        let mut effect = effect.clone();
        effect.emitters[0].renderers[0].properties = invalid;
        assert!(EffectCompiler::default().compile(&effect).is_err());
    }
    let mut oversized = effect.clone();
    oversized.emitters[0].max_particles = 257;
    assert!(EffectCompiler::default().compile(&oversized).is_err());
    let mut duplicate = effect;
    let mut renderer = duplicate.emitters[0].renderers[0].clone();
    renderer.id = aestra_core::RendererId::new();
    duplicate.emitters[0].renderers.push(renderer);
    assert!(EffectCompiler::default().compile(&duplicate).is_err());
}

#[test]
fn history_epoch_distinguishes_clock_updates_from_explicit_seeks_and_restarts() {
    let mut effect = fixture();
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;
    let mut instance = EffectInstance::new(Arc::new(
        EffectCompiler::default().compile(&effect).unwrap(),
    ));
    let initial = instance.history_epoch();
    instance.set_playback_time(1.5);
    instance.advance(1.0);
    instance.advance_with_choreography_events(2.0, &mut Vec::new());
    assert_eq!(
        instance.history_epoch(),
        initial,
        "continuous wraps preserve history"
    );
    instance.seek(4.75);
    assert_ne!(instance.history_epoch(), initial);
    let after_seek = instance.history_epoch();
    instance.set_seed(0);
    assert_eq!(instance.history_epoch(), after_seek);
    instance.set_seed(1);
    assert_ne!(instance.history_epoch(), after_seek);
    let seeded = instance.history_epoch();
    instance.restart();
    assert_ne!(instance.history_epoch(), seeded);
    effect.playback_mode = EffectPlaybackMode::LoopRestart;
    let mut instance = EffectInstance::new(Arc::new(
        EffectCompiler::default().compile(&effect).unwrap(),
    ));
    let initial = instance.history_epoch();
    instance.advance_with_choreography_events(4.0, &mut Vec::new());
    assert_ne!(
        instance.history_epoch(),
        initial,
        "whole-cycle advances must also reset"
    );
}
