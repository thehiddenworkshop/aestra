//! Actual Bevy phase/scissor oracle; separate from the isolated wgpu prototype.
use super::overflow::{MESH, sheet_project};
use super::*;
use aestra_bevy::{ExperimentalOitPartitionStatus, ExperimentalOitPartitions};
use bevy::render::render_resource::Extent3d;

#[test]
#[ignore = "native absent-target extraction, OIT admission and partition recovery; run alone"]
fn native_target_preparation_recovers_without_changing_pixels() {
    let root = root("target-admission");
    let mut cases = Vec::new();
    for partitioned in [false, true] {
        let (mut app, player, target) = headless_project("medium", true, true, sheet(false));
        let camera = enable(&mut app);
        if partitioned {
            app.world_mut()
                .entity_mut(camera)
                .insert(ExperimentalOitPartitions {
                    grid: UVec2::splat(4),
                });
        }
        let _ = capture(&mut app, &target);
        app.world_mut()
            .get_mut::<EffectPlayer>(player)
            .unwrap()
            .seek_frame(30);
        let reference = capture(&mut app, &target);
        assert!(reference.pixels().any(|p| p[2] > 64));
        let missing = app.world().resource::<Assets<Image>>().reserve_handle();
        assert!(
            app.world()
                .resource::<Assets<Image>>()
                .get(&missing)
                .is_none()
        );
        // Reproduce an unresolved zero-size startup target without allocating an
        // invalid zero-size GPU texture. Bevy retains cached camera values for
        // an unavailable image, so explicitly seed that startup cache here.
        app.world_mut()
            .get_mut::<Camera>(camera)
            .unwrap()
            .computed
            .target_info = Some(bevy::camera::RenderTargetInfo {
            physical_size: UVec2::ZERO,
            scale_factor: 1.0,
        });
        app.world_mut()
            .entity_mut(camera)
            .insert(RenderTarget::Image(missing.clone().into()));
        for _ in 0..8 {
            app.update();
        }
        let status = app.world().get::<ExperimentalOitStatus>(camera).unwrap();
        assert!(status.is_preparing(), "{status:?}");
        assert!(
            app.world()
                .get::<bevy::core_pipeline::oit::OrderIndependentTransparencySettings>(camera)
                .is_none()
        );
        // Asset arrival triggers Bevy's real camera target/projection update.
        let image = app
            .world()
            .resource::<Assets<Image>>()
            .get(&target)
            .unwrap()
            .clone();
        app.world_mut()
            .resource_mut::<Assets<Image>>()
            .insert(missing.id(), image)
            .unwrap();
        let restored = capture(&mut app, &missing);
        let status = app.world().get::<ExperimentalOitStatus>(camera).unwrap();
        assert!(!status.is_preparing() && status.rejection.is_none());
        assert!(status.estimated_storage_bytes > 0);
        assert_eq!(delta(&reference, &restored).1, 0);
        if partitioned {
            healthy(&app, UVec2::splat(4));
        }
        restored
            .save(root.join(format!("restored-{partitioned}.png")))
            .unwrap();
        cases.push(
            json!({"partitioned":partitioned,"unavailable_target_updates":8,"seeded_zero_extent":true,
            "max_byte_error":0,"restored_estimated_bytes":status.estimated_storage_bytes}),
        );
    }
    write_report(
        &root,
        &json!({"passed":true,"cases":cases,
        "scope":"native unavailable image with explicitly seeded zero-size camera cache and recovery; no zero-size GPU texture; CPU tests also cover missing/one-zero-axis targets"}),
    );
}

#[test]
#[ignore = "native per-view alpha permutation retirement and particle identity; run alone"]
fn partition_capture_retires_sprite_sort_and_restores_other_views() {
    use crate::volume_light_images::outputs::cross_draw::sheet_program;
    use aestra_bevy::gpu::GpuAlphaSortStatistics;
    let root = root("partition-alpha-sort");
    // Random color exposes particle identity errors when alive-index order changes.
    let material = MaterialProgram::from_ron(r#"(
        id:"a3574a00-0000-4000-8000-000000f81b91",schema_version:1,
        name:"Partition particle identity",domain:Sprite,
        render_state_policy:(default:(blend:Alpha,depth_test:LessEqual,depth_write:false,cull_mode:r#None),
        allowed:[(blend:Alpha,depth_test:LessEqual,depth_write:false,cull_mode:r#None)]),
        parameters:[],expressions:[
        (id:"a3574a00-0000-4000-8000-000000f81b92",kind:Input(ParticleRandom)),
        (id:"a3574a00-0000-4000-8000-000000f81b93",kind:Constant(Vec3((0.8,0.3,0.1)))),
        (id:"a3574a00-0000-4000-8000-000000f81b94",kind:Multiply("a3574a00-0000-4000-8000-000000f81b92","a3574a00-0000-4000-8000-000000f81b93")),
        (id:"a3574a00-0000-4000-8000-000000f81b95",kind:Input(ParticleOpacity))],
        outputs:(color:"a3574a00-0000-4000-8000-000000f81b94",alpha:"a3574a00-0000-4000-8000-000000f81b95"))"#).unwrap();
    let project = sheet_program(
        8,
        EmitterShape::Box {
            half_extents: [2.0, 2.0, 4.0],
        },
        0.0,
        material,
    );
    let (mut app, first, target) = headless_project("medium", true, true, project.clone());
    app.world_mut().despawn(first);
    let a = spawn(&mut app, project.clone(), 1, 0.25);
    let b = spawn(&mut app, project, 2, 0.0);
    let camera = enable(&mut app);
    super::hdr::camera_hdr(&mut app, camera, 0.0);
    app.world_mut()
        .entity_mut(camera)
        .insert(ExperimentalOitSettings {
            fragments_per_pixel_average: 16.0,
            max_storage_bytes: 128 * 1024 * 1024,
            ..default()
        });
    let _ = capture(&mut app, &target);
    forward(&mut app, a, b, 30);
    let mut cases = Vec::new();
    for opposite in [false, true] {
        *app.world_mut().get_mut::<Transform>(camera).unwrap() =
            Transform::from_xyz(0.0, 0.0, if opposite { -16.0 } else { 16.0 })
                .looking_at(Vec3::ZERO, Vec3::Y);
        app.world_mut()
            .entity_mut(camera)
            .remove::<ExperimentalOitPartitions>();
        let reference = capture(&mut app, &target);
        let sorted = app.world().resource::<GpuAlphaSortStatistics>().snapshot();
        assert_eq!(sorted.pairs, 2);
        assert!(sorted.dispatched && sorted.owned_buffer_bytes > 0);
        for side in [1, 2, 4] {
            let grid = UVec2::splat(side);
            app.world_mut()
                .entity_mut(camera)
                .insert(ExperimentalOitPartitions { grid });
            let actual = capture(&mut app, &target);
            let health = healthy(&app, grid);
            let unsorted = app.world().resource::<GpuAlphaSortStatistics>().snapshot();
            assert_eq!(unsorted.pairs, 0);
            assert_eq!(unsorted.owned_buffer_bytes, 0);
            assert!(!unsorted.dispatched);
            assert_eq!(
                delta(&reference, &actual).1,
                0,
                "particle identity/order changed"
            );
            actual
                .save(root.join(format!("sprite-{opposite}-{side}.png")))
                .unwrap();
            cases.push(json!({"opposite":opposite,"grid":grid.to_array(),"max_byte_error":0,
                "reference_sort_bytes":sorted.owned_buffer_bytes,"partition_sort_bytes":unsorted.owned_buffer_bytes,
                "attempted":health.attempted_fragments,"dropped":health.dropped_fragments}));
        }
        // A second ordinary view of the same pools still needs two permutations.
        // It must neither steal that view's sorting nor leave stale allocations.
        let other_target =
            app.world_mut()
                .resource_mut::<Assets<Image>>()
                .add(Image::new_target_texture(
                    480,
                    360,
                    TextureFormat::Rgba8UnormSrgb,
                    None,
                ));
        let other_transform = *app.world().get::<Transform>(camera).unwrap();
        let other = app
            .world_mut()
            .spawn((
                Camera3d::default(),
                Camera::default(),
                RenderTarget::Image(other_target.clone().into()),
                Msaa::Off,
                other_transform,
            ))
            .id();
        let ordinary = capture(&mut app, &other_target);
        let multi = app.world().resource::<GpuAlphaSortStatistics>().snapshot();
        assert_eq!(multi.pairs, 2);
        assert!(multi.dispatched && multi.owned_buffer_bytes == sorted.owned_buffer_bytes);
        app.world_mut()
            .entity_mut(camera)
            .remove::<ExperimentalOitPartitions>();
        let restored = capture(&mut app, &other_target);
        assert_eq!(delta(&ordinary, &restored).1, 0);
        assert_eq!(
            app.world()
                .resource::<GpuAlphaSortStatistics>()
                .snapshot()
                .pairs,
            4
        );
        app.world_mut().despawn(other);
        let restored = capture(&mut app, &target);
        assert_eq!(delta(&reference, &restored).1, 0);
        assert_eq!(
            app.world()
                .resource::<GpuAlphaSortStatistics>()
                .snapshot()
                .pairs,
            2
        );
    }
    write_report(
        &root,
        &json!({"passed":true,"cases":cases,"ordinary_view_preserved":true,
        "retired_and_restored_per_view_buffers":true,"scope":"native sprite ParticleRandom identity, exact SDR captures of HDR/Reinhard rendering; second ordinary camera has its own target"}),
    );
}

fn healthy(app: &App, grid: UVec2) -> aestra_bevy::ExperimentalOitPartitionSample {
    let status = app
        .world()
        .resource::<ExperimentalOitPartitionStatus>()
        .snapshot()
        .expect("native phase status");
    assert!(status.ready && status.rejection.is_none(), "{status:?}");
    assert_eq!(status.grid, grid);
    let sample = status
        .sample
        .expect("missing observation is not acceptance");
    assert_eq!(sample.regions.len(), (grid.x * grid.y) as usize);
    assert!(!sample.empty_phase);
    assert!(sample.attempted_fragments > 0);
    assert_eq!((sample.dropped_fragments, sample.invalid_links), (0, 0));
    sample
}

#[test]
#[ignore = "native partition phase readiness, exact images, resize, foreign draw rejection and restore; run alone"]
fn native_partition_phase_matches_full_lists_and_rejects_unsupported_draws() {
    let root = root("native-partition-phase");
    let mut cases = Vec::new();
    for opposite in [false, true] {
        let a = sheet_project(20, [0.8, 0.0, 0.0], 0.125, false, true);
        let b = sheet_project(20, [0.0, 0.0, 1.0], 0.125, false, true);
        let (mut app, first, target) = headless_project("medium", true, true, a.clone());
        app.world_mut().despawn(first);
        let red = spawn(&mut app, a.clone(), 1, 0.25);
        let blue = spawn(&mut app, b.clone(), 2, 0.0);
        let light = sheet_project(1, [0.0, 8.0, 0.0], 0.5, true, false);
        let emission = spawn(&mut app, light.clone(), 3, 0.1);
        let mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Rectangle::new(1.0, 1.0));
        for (entity, project) in [(red, a), (blue, b), (emission, light)] {
            app.world_mut().entity_mut(entity).insert(
                PresentedEffect::new(project.root.clone())
                    .with_mesh_overrides(BTreeMap::from([(MESH, mesh.clone())])),
            );
            app.world_mut().get_mut::<Transform>(entity).unwrap().scale = Vec3::splat(0.25);
        }
        let camera = enable(&mut app);
        let wall_mesh = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(Cuboid::new(5.0, 20.0, 0.2));
        let wall_material = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                base_color: Color::linear_rgb(0.1, 0.2, 0.1),
                unlit: true,
                ..default()
            });
        app.world_mut().spawn((
            Mesh3d(wall_mesh),
            MeshMaterial3d(wall_material),
            Transform::from_xyz(-2.5, 0.0, 0.5),
        ));
        let full_settings = ExperimentalOitSettings {
            fragments_per_pixel_average: 16.0,
            max_storage_bytes: 128 * 1024 * 1024,
            ..default()
        };
        app.world_mut().entity_mut(camera).insert(full_settings);
        app.world()
            .resource::<aestra_bevy::ExperimentalOitDiagnostics>()
            .set_enabled(true);
        *app.world_mut().get_mut::<Transform>(camera).unwrap() =
            Transform::from_xyz(0.0, 0.0, if opposite { -16.0 } else { 16.0 })
                .looking_at(Vec3::ZERO, Vec3::Y);
        super::hdr::camera_hdr(&mut app, camera, 0.0);
        let _ = capture(&mut app, &target);
        forward(&mut app, red, blue, 30);
        app.world_mut()
            .get_mut::<EffectPlayer>(emission)
            .unwrap()
            .seek(0.5);
        for size in [UVec2::new(480, 360), UVec2::new(257, 193)] {
            app.world_mut()
                .resource_mut::<Assets<Image>>()
                .get_mut(&target)
                .unwrap()
                .resize(Extent3d {
                    width: size.x,
                    height: size.y,
                    depth_or_array_layers: 1,
                });
            app.world_mut()
                .entity_mut(camera)
                .remove::<ExperimentalOitPartitions>();
            let reference = capture(&mut app, &target);
            let full = app
                .world()
                .resource::<aestra_bevy::ExperimentalOitDiagnostics>()
                .snapshot()
                .unwrap();
            assert_eq!(
                (full.dropped_fragments, full.invalid_links),
                (0, 0),
                "reference itself lost fragments"
            );
            reference
                .save(root.join(format!("reference-{opposite}-{}.png", size.x)))
                .unwrap();
            let bytes = app
                .world()
                .resource::<ExperimentalOitStatistics>()
                .snapshot()
                .storage_bytes;
            for side in [1, 2, 4] {
                let grid = UVec2::splat(side);
                app.world_mut()
                    .entity_mut(camera)
                    .insert(ExperimentalOitPartitions { grid });
                let actual = capture(&mut app, &target);
                let sample = healthy(&app, grid);
                assert_eq!(
                    sample.attempted_fragments,
                    u64::from(full.attempted_fragments),
                    "partition changed capture demand"
                );
                assert_eq!(
                    sample.regions.iter().map(|row| row[1]).sum::<u32>(),
                    full.over_sort_pixels,
                    "regional health scan changed long-list pixel coverage"
                );
                assert_eq!(
                    sample.regions.iter().map(|row| row[3]).max().unwrap(),
                    full.capped_max_depth,
                    "regional health scan changed capped depth"
                );
                assert_eq!(
                    delta(&reference, &actual).1,
                    0,
                    "seam/order/color changed at {size:?}, {grid:?}"
                );
                assert_eq!(
                    app.world()
                        .resource::<ExperimentalOitStatistics>()
                        .snapshot()
                        .storage_bytes,
                    bytes
                );
                actual
                    .save(root.join(format!("partition-{opposite}-{}-{side}.png", size.x)))
                    .unwrap();
                cases.push(json!({"opposite":opposite,"size":size.to_array(),"grid":grid.to_array(),"whole_image_max_byte_error":0,
                    "attempted":sample.attempted_fragments,"dropped":sample.dropped_fragments,"invalid":sample.invalid_links,"regions":sample.regions,"node_capacity":sample.node_capacity,"node_head_bytes":bytes}));
            }
        }
        // A deliberately too-small logical pool remains a failed run, rather
        // than hiding the loss behind a smaller/cleaner-looking image.
        app.world_mut()
            .entity_mut(camera)
            .insert(ExperimentalOitSettings {
                fragments_per_pixel_average: 0.05,
                ..full_settings
            });
        let _ = capture(&mut app, &target);
        let negative = app
            .world()
            .resource::<ExperimentalOitPartitionStatus>()
            .snapshot()
            .unwrap();
        assert!(negative.ready && negative.sample.unwrap().dropped_fragments > 0);
        // An empty reset audit must not erase earlier observed loss in the same
        // configuration, even though its own GPU counters are genuinely zero.
        for entity in [red, blue, emission] {
            app.world_mut()
                .entity_mut(entity)
                .insert(Visibility::Hidden);
        }
        let _ = capture(&mut app, &target);
        let reset = app
            .world()
            .resource::<ExperimentalOitPartitionStatus>()
            .snapshot()
            .unwrap();
        assert!(reset.ready && reset.observed_loss);
        let reset = reset.sample.unwrap();
        assert!(reset.empty_phase && reset.regions == vec![[0; 4]]);
        assert_eq!(
            (
                reset.attempted_fragments,
                reset.dropped_fragments,
                reset.invalid_links
            ),
            (0, 0, 0)
        );
        for entity in [red, blue, emission] {
            app.world_mut()
                .entity_mut(entity)
                .insert(Visibility::Visible);
        }
        app.world_mut().entity_mut(camera).insert(full_settings);
        let _ = capture(&mut app, &target);
        healthy(&app, UVec2::splat(4));
        // A real foreign transparent command must reject the phase, never be
        // silently omitted or repeated. Use the exact same view/renderer state.
        let material = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                base_color: Color::srgba(0.0, 1.0, 0.0, 0.5),
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                ..default()
            });
        let foreign = app
            .world_mut()
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                Transform::from_scale(Vec3::splat(8.0)),
            ))
            .id();
        let rejected_image = capture(&mut app, &target);
        let rejected = app
            .world()
            .resource::<ExperimentalOitPartitionStatus>()
            .snapshot()
            .unwrap();
        assert!(
            !rejected.ready && rejected.rejection.is_some() && rejected.sample.is_none(),
            "{rejected:?}"
        );
        app.world_mut().despawn(foreign);
        let restored = capture(&mut app, &target);
        assert!(
            delta(&restored, &rejected_image).1 > 32,
            "foreign rejection silently rendered a fallback"
        );
        healthy(&app, UVec2::splat(4));
        for control in [0, 1] {
            if control == 0 {
                app.world_mut()
                    .entity_mut(camera)
                    .insert(ExperimentalOitPartitions { grid: UVec2::ZERO });
            } else {
                app.world_mut().get_mut::<Camera>(camera).unwrap().viewport =
                    Some(bevy::camera::Viewport {
                        physical_position: UVec2::ZERO,
                        physical_size: UVec2::new(200, 100),
                        ..default()
                    });
            }
            let _ = capture(&mut app, &target);
            let rejected = app
                .world()
                .resource::<ExperimentalOitPartitionStatus>()
                .snapshot()
                .unwrap();
            assert!(!rejected.ready && rejected.rejection.is_some() && rejected.sample.is_none());
            app.world_mut().get_mut::<Camera>(camera).unwrap().viewport = None;
            app.world_mut()
                .entity_mut(camera)
                .insert(ExperimentalOitPartitions {
                    grid: UVec2::splat(4),
                });
            let recovered = capture(&mut app, &target);
            healthy(&app, UVec2::splat(4));
            assert_eq!(delta(&restored, &recovered).1, 0);
        }
        for entity in [red, blue, emission] {
            app.world_mut()
                .entity_mut(entity)
                .insert(Visibility::Hidden);
        }
        let mut previous_empty = 0;
        let mut empty_images = Vec::new();
        for _ in 0..3 {
            empty_images.push(capture(&mut app, &target));
            let empty = app
                .world()
                .resource::<ExperimentalOitPartitionStatus>()
                .snapshot()
                .unwrap();
            assert!(empty.ready && !empty.observed_loss && empty.phase_draws == 0);
            let empty = empty.sample.unwrap();
            assert!(empty.empty_phase && empty.regions == vec![[0; 4]]);
            assert!(empty.sequence > previous_empty);
            assert_eq!(
                (
                    empty.attempted_fragments,
                    empty.dropped_fragments,
                    empty.invalid_links
                ),
                (0, 0, 0)
            );
            previous_empty = empty.sequence;
        }
        app.world_mut()
            .entity_mut(camera)
            .remove::<ExperimentalOitPartitions>();
        let empty_control = capture(&mut app, &target);
        for image in empty_images {
            assert_eq!(
                delta(&image, &empty_control).1,
                0,
                "empty path changed opaque/background pixels"
            );
        }
        app.world_mut()
            .entity_mut(camera)
            .insert(ExperimentalOitPartitions {
                grid: UVec2::splat(4),
            });
        for entity in [red, blue, emission] {
            app.world_mut()
                .entity_mut(entity)
                .insert(Visibility::Visible);
        }
        let visible = capture(&mut app, &target);
        healthy(&app, UVec2::splat(4));
        assert_eq!(
            delta(&restored, &visible).1,
            0,
            "retained visibility did not recover"
        );
        app.world_mut()
            .entity_mut(camera)
            .remove::<ExperimentalOitPartitions>();
        let control = capture(&mut app, &target);
        assert_eq!(delta(&restored, &control).1, 0);
        assert!(
            app.world()
                .resource::<ExperimentalOitPartitionStatus>()
                .snapshot()
                .is_none()
        );
    }
    write_report(
        &root,
        &json!({"cases":cases,"passed":true,"regional_diagnostics_match_full_target":true,"empty_reset_audit":true,"repeated_empty_frames":6,"sticky_loss_survives_empty_audit":true,"empty_matches_ordinary_background_and_opaque":true,"scope":"native phase SDR captures of HDR/Reinhard target, exact byte comparison; isolated raw HDR oracle remains separate"}),
    );
}
