//! Actual production installer and drained 0.20 storage uploads; no GPU required.
use crate::{effect_inputs::GpuEffectBuffers, particle_statistics::GpuParticleStatistics, *};
use bevy::{
    prelude::*,
    render::{
        render_asset::RenderAsset,
        storage::{GpuShaderBuffer, ShaderBuffer},
    },
};
use std::sync::Arc;

fn bytes(app: &App, handle: &Handle<ShaderBuffer>) -> Vec<u8> {
    storage_buffers::bytes(
        app.world()
            .resource::<Assets<ShaderBuffer>>()
            .get(handle)
            .unwrap(),
    )
    .unwrap()
    .to_vec()
}
fn word(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}
fn matrix(data: &[u8], offset: usize) -> Mat4 {
    Mat4::from_cols_array(&std::array::from_fn(|i| {
        f32::from_bits(word(data, offset + i * 4))
    }))
}

#[test]
fn production_sync_repopulates_drained_uploads_and_reuses_assets_across_context_edits() {
    let mut asset = EffectAsset::new("Transform synchronization", 6.);
    asset.emitters.push(Emitter::basic_sprite("Particles", 6.));
    let compiled = Arc::new(EffectCompiler::default().compile(&asset).unwrap());
    let mut player =
        PresentedEffect::new(compiled).with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
    player.instance.set_playback_time(1.25);
    player.instance.set_seed(0x1000_0000_0301);
    let track =
        aestra_runtime::CompiledHostTransformTrack::new(HostTransformTrack::from_pose_keys(
            vec![HostTransformKey {
                time: 0.,
                transform: EmitterTransform {
                    translation: [2., 3., 4.],
                    ..default()
                },
            }],
            false,
        ))
        .unwrap();
    player
        .instance
        .set_host_transform_track(Some(Arc::new(track)));
    let mut app = App::new();
    app.add_plugins(bevy::transform::TransformPlugin)
        .init_resource::<Assets<ShaderBuffer>>();
    transform_sync::install(&mut app);
    let gpu =
        simulation_native::inputs(app.world_mut(), &player.instance, player.simulation_time());
    let globals = gpu.globals.clone();
    let render_globals = gpu.render_globals.clone();
    let particles = gpu.particles.clone();
    let statistics = GpuParticleStatistics::new(&player.instance);
    let parent = app.world_mut().spawn(Transform::from_xyz(5., 0., 0.)).id();
    let owner = app
        .world_mut()
        .spawn((
            ChildOf(parent),
            Transform::from_xyz(0., 7., 0.),
            player,
            statistics,
            gpu,
        ))
        .id();
    let asset_count = app.world().resource::<Assets<ShaderBuffer>>().len();
    app.update();
    let original_token = app
        .world()
        .get::<GpuEffectBuffers>(owner)
        .unwrap()
        .statistics_token;
    assert_ne!(original_token, 0);
    for change in 0..6 {
        // Model Bevy 0.20's real extraction drain before the next host frame.
        for handle in [&globals, &render_globals] {
            let mut buffers = app.world_mut().resource_mut::<Assets<ShaderBuffer>>();
            let mut buffer = buffers.get_mut(handle).unwrap();
            GpuShaderBuffer::take_gpu_data(&mut buffer, None).unwrap();
            assert!(storage_buffers::bytes(&buffer).is_none());
        }
        {
            let mut p = app.world_mut().get_mut::<PresentedEffect>(owner).unwrap();
            match change {
                0 => p.instance.set_playback_time(2.),
                1 => p.instance.set_seed(997),
                2 => {
                    p.instance.seek(0.5);
                    p.set_seek_quality(aestra_runtime::SeekQuality::Preview);
                }
                3 => {
                    p.instance
                        .send_event(aestra_runtime::INPUT_STOP_EMITTING, vec![])
                        .unwrap();
                }
                4 => {
                    p.instance
                        .send_event(aestra_runtime::INPUT_KILL, vec![])
                        .unwrap();
                }
                _ => {
                    p.instance.restart();
                    p.set_history_policy(PlaybackHistoryPolicy::default());
                }
            }
        }
        app.world_mut()
            .get_mut::<Transform>(parent)
            .unwrap()
            .translation
            .x = 5. + change as f32;
        // Multiple host updates before extraction must replace, never append bytes.
        app.update();
        app.update();
        let p = app.world().get::<PresentedEffect>(owner).unwrap();
        let gpu = app.world().get::<GpuEffectBuffers>(owner).unwrap();
        assert_eq!(gpu.particles, particles);
        assert_eq!(gpu.globals, globals);
        assert_eq!(gpu.render_globals, render_globals);
        assert_eq!(
            app.world().resource::<Assets<ShaderBuffer>>().len(),
            asset_count
        );
        assert_eq!(gpu.simulation_time, p.simulation_time());
        assert_eq!(gpu.history_epoch, p.instance.history_epoch());
        assert_eq!(gpu.seek_quality, p.seek_quality());
        assert_eq!(gpu.history_policy, p.history_policy());
        assert_eq!(
            gpu.output_suppress_through,
            aestra_runtime::trace_tick(p.instance.history_epoch_start_time())
        );
        let sim = bytes(&app, &globals);
        let draw = bytes(&app, &render_globals);
        assert_eq!(sim.len(), 112);
        assert_eq!(draw.len(), 80);
        assert_eq!(word(&sim, 0), p.simulation_time().to_bits());
        assert_eq!(word(&sim, 8), aestra_gpu::fold_seed(p.instance.seed()));
        assert_eq!(word(&sim, 24), p.instance.history_epoch());
        assert_eq!(word(&sim, 28), gpu.statistics_token);
        assert_eq!(
            word(&sim, 96),
            p.instance.emission_cutoffs().emission_end().to_bits()
        );
        assert_eq!(
            word(&sim, 100),
            p.instance.emission_cutoffs().kill_time().to_bits()
        );
        assert_eq!(word(&draw, 64), p.simulation_time().to_bits());
        assert_eq!(word(&draw, 68), aestra_gpu::fold_seed(p.instance.seed()));
        let expected = Mat4::from_translation(Vec3::new(7. + change as f32, 10., 4.));
        assert_eq!(matrix(&sim, 32), expected);
        assert_eq!(matrix(&draw, 0), expected);
        if change == 0 {
            assert_eq!(gpu.statistics_token, original_token);
        } else {
            assert_ne!(gpu.statistics_token, original_token);
        }
    }
}

#[test]
fn spawn_placement_normalizes_rotation_and_preserves_translation_and_scale() {
    for rotation in [[0.; 4], [0., 0., 2., 2.]] {
        let placement = transform_sync::spawn_placement(EmitterTransform {
            translation: [1., 2., 3.],
            rotation,
            scale: [2., -3., 4.],
        });
        assert_eq!(placement.translation, [1., 2., 3.]);
        assert_eq!(placement.scale, [2., -3., 4.]);
        assert!((Quat::from_array(placement.rotation).length() - 1.).abs() < 1e-6);
    }
}
