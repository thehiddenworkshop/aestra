//! Existing renderer regressions compiled unchanged on both engine graphs.
use super::super::{
    draw_instance::GpuRenderMode, particle_statistics::GpuParticleStatistics, storage_buffers,
};
use super::*;
use aestra_compiler::EffectCompiler;
use aestra_core::{EffectAsset, Emitter};
use aestra_runtime::{EffectInstance, PlaybackHistoryPolicy, SeekQuality};
use bevy::{camera::primitives::Aabb, render::storage::ShaderBuffer};
use std::sync::Arc;

#[test]
fn sprite_sampling_culling_override_is_opt_in_and_reversible() {
    let mut app = App::new();
    app.init_resource::<crate::sampling::SpriteSampling>()
        .add_systems(Update, sync_host_motion_replay_culling);
    let parent = app.world_mut().spawn_empty().id();
    let mut draws = Vec::new();
    for (kind, blend) in [
        (0, GpuBlend::Additive),
        (0, GpuBlend::Alpha),
        (1, GpuBlend::Additive),
        (2, GpuBlend::Additive),
        (3, GpuBlend::Additive),
        (4, GpuBlend::Additive),
    ] {
        draws.push(
            app.world_mut()
                .spawn((
                    ChildOf(parent),
                    GpuDrawInstance {
                        sort_range: UVec2::ZERO,
                        renderer_kind: kind,
                        owner: parent,
                        mesh: None,
                        wireframe_geometry: None,
                        renderers: default(),
                        particles: default(),
                        alive: default(),
                        aux: default(),
                        indirect: default(),
                        render_globals: default(),
                        render_params: default(),
                        texture: default(),
                        fallback_texture: default(),
                        renderer_order: 0,
                        emitter_index: 0,
                        indirect_offset: 0,
                        blend,
                        material: aestra_core::MaterialId::new(),
                        semantic_material: None,
                        render_mode: GpuRenderMode::Rendered,
                        mesh_center: Vec3::ZERO,
                        sampled_sprite_cull: None,
                        trail_owners: 0,
                        trail_instances: None,
                    },
                ))
                .id(),
        );
    }
    for minimum in [0.0, 2.0, 0.0, f32::NAN] {
        app.world_mut()
            .resource_mut::<crate::sampling::SpriteSampling>()
            .minimum_pixels = minimum;
        app.update();
        for (index, entity) in draws.iter().enumerate() {
            assert_eq!(
                app.world()
                    .entity(*entity)
                    .contains::<visibility::NoFrustumCulling>(),
                index == 0 && minimum == 2.0
            );
        }
    }
}

#[test]
fn rendering_and_ribbon_culling_receive_the_same_propagated_transform() {
    let mut effect = EffectAsset::new("Bounds upload", 2.0);
    effect.emitters.push(Emitter::basic_sprite("Emitter", 2.0));
    let compiled = Arc::new(EffectCompiler::default().compile(&effect).unwrap());
    let mut app = App::new();
    app.add_plugins(bevy::transform::TransformPlugin)
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<ShaderBuffer>>();
    install(&mut app);
    let handle = app
        .world_mut()
        .resource_mut::<Assets<ShaderBuffer>>()
        .add(ShaderBuffer::default());
    let parent = app.world_mut().spawn(Transform::IDENTITY).id();
    let player = app
        .world_mut()
        .spawn((
            ChildOf(parent),
            Transform::IDENTITY,
            GpuParticleStatistics::new(&EffectInstance::new(compiled.clone())),
            PresentedEffect::new(compiled),
            GpuEffectBuffers {
                emitters: default(),
                renderers: default(),
                particles: default(),
                alive: default(),
                dead: default(),
                counters: default(),
                indirect: default(),
                globals: default(),
                aux: default(),
                render_globals: handle.clone(),
                workgroups: 1,
                has_ribbons: true,
                has_trails: false,
                ribbon_workgroups: 1,
                trail_workgroups: 1,
                trail_plan: default(),
                total_slots: 1,
                simulation_time: 0.0,
                seek_quality: SeekQuality::Exact,
                history_policy: PlaybackHistoryPolicy::default(),
                history_epoch: 0,
                statistics_token: 0,
                checkpoint_context: default(),
                trail_roots: vec![],
                simulation_state: default(),
                stateful_dispatch: Vec::new(),
                stateful_only: false,
                event_links: Vec::new(),
                routed: false,
                particle_outputs: Vec::new(),
                output_suppress_through: 0,
                host_events: Default::default(),
                physics: aestra_gpu::pack_physics_scene(&Default::default()).into(),
            },
        ))
        .id();
    let model = aestra_gpu::ribbon_bounds::RibbonParticleBounds {
        position_half_extents: [1.0, 5.0, 2.0].into(),
        maximum_half_width: 3.0,
    };
    let draw = app
        .world_mut()
        .spawn((
            ChildOf(player),
            Transform::IDENTITY,
            Aabb::default(),
            ribbon_bounds::RibbonBoundsSource(Some(model)),
            HostMotionDraw,
            visibility::NoFrustumCulling,
        ))
        .id();
    for transform in [
        Transform::IDENTITY,
        Transform::from_xyz(123.0, -42.0, 70.0)
            .with_rotation(Quat::from_rotation_y(0.7))
            .with_scale(Vec3::new(-0.1, 3.0, 2.0)),
        Transform::from_xyz(-500.0, 20.0, 0.0),
    ] {
        *app.world_mut().get_mut::<Transform>(parent).unwrap() = transform;
        app.update();
        let expected = app.world().get::<GlobalTransform>(player).unwrap().affine();
        let buffers = app.world().resource::<Assets<ShaderBuffer>>();
        let bytes = storage_buffers::bytes(buffers.get(&handle).unwrap()).unwrap();
        let actual: [f32; 16] = std::array::from_fn(|i| {
            f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
        });
        assert_eq!(
            Mat4::from_cols_array(&actual),
            Mat4::from(expected),
            "same-frame render upload"
        );
        assert_eq!(
            app.world().get::<GlobalTransform>(draw).unwrap().affine(),
            expected
        );
        let aabb = app.world().get::<Aabb>(draw).unwrap();
        assert_eq!(
            Vec3::from(aabb.half_extents),
            Vec3::from_array(
                model
                    .half_extents_from_columns(Mat3::from(expected.matrix3).to_cols_array())
                    .unwrap()
            )
        );
        assert!(
            !app.world()
                .entity(draw)
                .contains::<visibility::NoFrustumCulling>()
        );
    }
    let track = aestra_runtime::CompiledHostTransformTrack::new(
        aestra_core::HostTransformTrack::from_pose_keys(
            vec![
                aestra_core::HostTransformKey {
                    time: 0.0,
                    transform: default(),
                },
                aestra_core::HostTransformKey {
                    time: 1.0,
                    transform: aestra_core::EmitterTransform {
                        translation: [20.0, 30.0, 10.0],
                        rotation: Quat::from_rotation_y(0.8).to_array(),
                        scale: [0.5, 2.0, 1.5],
                    },
                },
            ],
            false,
        ),
    )
    .unwrap();
    app.world_mut()
        .get_mut::<PresentedEffect>(player)
        .unwrap()
        .instance
        .set_host_transform_track(Some(Arc::new(track.clone())));
    let clip = aestra_core::EmitterTransform {
        rotation: Quat::from_rotation_z(0.7).to_array(),
        scale: [2.0, 0.5, 1.0],
        ..default()
    };
    app.world_mut()
        .get_mut::<PresentedEffect>(player)
        .unwrap()
        .instance
        .set_inherited_host_transform(Arc::new(
            aestra_runtime::InheritedHostTransform::default().for_child(
                Some(Arc::new(track.clone())),
                clip,
                0.25,
            ),
        ));
    for time in [0.0, 0.5, 1.0, 0.25, 0.25] {
        app.world_mut()
            .get_mut::<PresentedEffect>(player)
            .unwrap()
            .instance
            .seek(time);
        app.update();
        let placement = Mat4::from(app.world().get::<GlobalTransform>(player).unwrap().affine());
        let pose = app
            .world()
            .get::<PresentedEffect>(player)
            .unwrap()
            .instance
            .host_transform_at(time);
        let expected = placement
            * crate::host_transform::matrix(track.sample(time + 0.25))
            * crate::host_transform::matrix(clip)
            * crate::host_transform::matrix(pose);
        let actual = Mat4::from(app.world().get::<GlobalTransform>(draw).unwrap().affine());
        assert!(actual.abs_diff_eq(expected, 1e-5));
        let buffer = app
            .world()
            .resource::<Assets<ShaderBuffer>>()
            .get(&handle)
            .unwrap();
        let bytes = storage_buffers::bytes(buffer).unwrap();
        let uploaded = Mat4::from_cols_array(&std::array::from_fn(|i| {
            f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
        }));
        assert!(
            uploaded.abs_diff_eq(actual, 1e-5),
            "GPU and Bevy must use the same frame's pose"
        );
    }
}
