//! Real extension-stage/volume installers. Readbacks and waits are test-only.
use crate::{extension_stages::*, stage_runtimes::StageRuntimes, volume::*};
use bevy::{
    camera::{RenderTarget, ShadowLodOrigin},
    core_pipeline::prepass::DepthPrepass,
    prelude::*,
    render::{
        RenderApp, RenderPlugin,
        render_asset::RenderAssets,
        render_resource::{TextureFormat, TextureUsages},
        renderer::{RenderAdapterInfo, RenderDevice, RenderQueue},
        texture::GpuImage,
    },
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn effect(resolution: u32, ambient: f32) -> Arc<aestra_runtime::CompiledEffect> {
    use aestra_core::{ModuleParameters, Value};
    let registry = aestra_compiler::ExtensionRegistry::linked();
    let mut source = aestra_fluid::smoke_effect(&registry);
    source.emitters.clear(); // Exercise volume-only readiness, without a seeded particle owner.
    source.playback_mode = aestra_core::EffectPlaybackMode::Once;
    for (module, values) in [
        (
            aestra_fluid::MODULE_GRID,
            vec![
                ("resolution", Value::U32(resolution)),
                ("cell_size", Value::Scalar(6.)),
                ("center", Value::Vec3([0.; 3])),
            ],
        ),
        (
            aestra_fluid::MODULE_DENSITY_SOURCE,
            vec![("position", Value::Vec3([0.; 3]))],
        ),
        (
            aestra_fluid::MODULE_VOLUME_LOOK,
            vec![("ambient", Value::Scalar(ambient))],
        ),
    ] {
        let found = source.simulation_stages[0]
            .modules
            .iter_mut()
            .find(|candidate| candidate.module_type.0 == module)
            .unwrap();
        let ModuleParameters::Custom(payload) = &mut found.parameters else {
            panic!()
        };
        for (key, value) in values {
            payload.insert(key.into(), value);
        }
    }
    Arc::new(
        aestra_compiler::EffectCompiler::with_extensions(registry)
            .compile(&source)
            .unwrap(),
    )
}

fn app() -> App {
    aestra_fluid::link();
    let mut app = App::new();
    let plugins = DefaultPlugins
        .set(bevy::window::WindowPlugin {
            primary_window: None,
            exit_condition: bevy::window::ExitCondition::DontExit,
            ..default()
        })
        .set(RenderPlugin {
            synchronous_pipeline_compilation: true,
            render_creation: bevy::render::settings::RenderCreation::Automatic(Box::new(
                bevy::render::settings::WgpuSettings {
                    backends: Some(wgpu::Backends::VULKAN),
                    features: wgpu::Features::TIMESTAMP_QUERY,
                    ..default()
                },
            )),
            ..default()
        });
    #[cfg(feature = "async-qualification")]
    let plugins = plugins.disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>();
    app.add_plugins(plugins);
    crate::extraction::install(&mut app);
    crate::outer_graph_native::install_with_stage_installer(&mut app);
    crate::extension_stages::install(&mut app);
    crate::volume::install(&mut app);
    app.sub_app_mut(RenderApp)
        .init_resource::<crate::stateful_simulation::StatefulStates>();
    app.init_resource::<crate::AestraRenderSettings>()
        .insert_resource(crate::catchup_pacing::AestraCatchupPacing { paced: false })
        .insert_resource(AestraDebugViews { field_slices: true })
        .insert_resource(ClearColor(Color::BLACK));
    app.finish();
    app.cleanup();
    let info = app
        .sub_app(RenderApp)
        .world()
        .resource::<RenderAdapterInfo>();
    assert_ne!(info.device_type, wgpu::DeviceType::Cpu, "hardware required");
    println!(
        "Native production volumes: {} / {:?} / {}",
        info.name, info.backend, info.driver_info
    );
    app
}

fn camera(app: &mut App) -> (Entity, Handle<Image>) {
    let mut image = Image::new_target_texture(64, 64, TextureFormat::Bgra8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let image = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    let entity = app
        .world_mut()
        .spawn((
            Camera3d::default(),
            DepthPrepass,
            ShadowLodOrigin,
            Msaa::Off,
            RenderTarget::Image(image.clone().into()),
            Transform::from_xyz(0., 0., 130.).looking_at(Vec3::ZERO, Vec3::Y),
        ))
        .id();
    (entity, image)
}

fn settle(app: &mut App, owner: Entity, tick: u32) {
    let deadline = Instant::now() + Duration::from_secs(45);
    while Instant::now() < deadline {
        app.update();
        let ready = app
            .world()
            .get::<GpuStageProgress>(owner)
            .is_some_and(|ready| {
                ready.is_ready(
                    &app.world()
                        .get::<crate::PresentedEffect>(owner)
                        .unwrap()
                        .instance,
                )
            });
        let render = app.sub_app(RenderApp).world();
        let runtimes = render.resource::<StageRuntimes>();
        if ready
            && runtimes.0.values().any(|runtime| {
                runtime
                    .timelines
                    .iter()
                    .flatten()
                    .any(|timeline| timeline.last_tick() == tick)
            })
        {
            // Flush ordinary asset preparation and the next submitted material frame.
            for _ in 0..3 {
                app.update();
            }
            return;
        }
        std::thread::yield_now();
    }
    panic!("volume stage did not reach tick {tick}");
}

fn views(app: &mut App) -> Vec<(Entity, Handle<VolumeMaterial>)> {
    app.world_mut()
        .query::<(Entity, &MeshMaterial3d<VolumeMaterial>)>()
        .iter(app.world())
        .map(|(entity, material)| (entity, material.0.clone()))
        .collect()
}

fn texture(app: &App, image: AssetId<Image>, bpp: u32) -> Vec<u32> {
    let world = app.sub_app(RenderApp).world();
    let image = world
        .resource::<RenderAssets<GpuImage>>()
        .get(image)
        .expect("prepared image");
    let size = image.texture.size();
    let row = (size.width * bpp).div_ceil(256) * 256;
    let device = world.resource::<RenderDevice>();
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test-only volume texture copy"),
        size: u64::from(row * size.height * size.depth_or_array_layers),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&default());
    encoder.copy_texture_to_buffer(
        image.texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(size.height),
            },
        },
        size,
    );
    world.resource::<RenderQueue>().submit([encoder.finish()]);
    let padded = crate::trail_native::read(
        app,
        &buffer,
        (row * size.height * size.depth_or_array_layers / 4) as usize,
    );
    padded
        .chunks_exact((row / 4) as usize)
        .flat_map(|row| row[..(size.width * bpp / 4) as usize].iter().copied())
        .collect()
}

fn half(bits: u16) -> f32 {
    let sign = if bits & 0x8000 == 0 { 1. } else { -1. };
    let exponent = ((bits >> 10) & 31) as i32;
    let fraction = f32::from(bits & 1023);
    if exponent == 0 {
        sign * fraction * 2f32.powi(-24)
    } else if exponent == 31 {
        panic!("nonfinite field texel")
    } else {
        sign * (1. + fraction / 1024.) * 2f32.powi(exponent - 15)
    }
}

fn field_matches_texture(app: &App, owner: Entity) -> Vec<u32> {
    let targets = app.world().get::<VolumeViews>(owner).unwrap().targets();
    let [target] = targets.as_slice() else {
        panic!("one density texture")
    };
    assert!(target.table.is_none(), "dense field gate");
    let render = app.sub_app(RenderApp).world();
    let stages = render.resource::<StageRuntimes>();
    let timeline = stages.0.values().next().unwrap().timelines[0]
        .as_ref()
        .unwrap();
    assert_eq!(timeline.checkpoint_bytes(), 0, "playback-only");
    let cells = target.layout.dims.iter().product::<u32>() as usize;
    let field = timeline
        .executor()
        .field_buffers(&target.layout)
        .unwrap()
        .field;
    let density = crate::trail_native::read(app, field, cells);
    let pixels = texture(app, target.image, 8);
    assert!(density.iter().any(|word| f32::from_bits(*word) > 0.01));
    for (pixel, word) in pixels.as_chunks::<2>().0.iter().zip(&density) {
        let expected = f32::from_bits(*word);
        let actual = half(pixel[0] as u16);
        assert!(
            (actual - expected).abs() <= (expected.abs() * 0.002).max(0.001),
            "volume texel {actual} != live density {expected}"
        );
    }
    pixels
}

// Add readback permission only to test-owned observations, never to production image defaults.
fn permit_copy(images: &mut Assets<Image>, targets: &[crate::stage_inputs::VolumeFieldTarget]) {
    for target in targets {
        images
            .get_mut(target.image)
            .unwrap()
            .texture_descriptor
            .usage |= TextureUsages::COPY_SRC;
    }
}

#[test]
#[ignore = "Requires a native Vulkan GPU; run explicitly and serially"]
fn native_020_production_volume_installer_copies_live_fields_draws_and_retires() {
    let mut app = app();
    let device = app
        .sub_app(RenderApp)
        .world()
        .resource::<RenderDevice>()
        .clone();
    let error_scope = device
        .wgpu_device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    let (view, target) = camera(&mut app);
    let mut player = crate::PresentedEffect::new(effect(16, 1.))
        .with_history_policy(aestra_runtime::PlaybackHistoryPolicy::PlaybackOnly);
    player.instance.set_playback_time(0.5001);
    let owner = app.world_mut().spawn((player, Transform::default())).id();
    // Main-world synchronization is real. Observe the created volume, then allow test readback.
    app.update();
    let original_views = views(&mut app);
    assert_eq!(original_views.len(), 1);
    let targets = app.world().get::<VolumeViews>(owner).unwrap().targets();
    permit_copy(
        &mut app.world_mut().resource_mut::<Assets<Image>>(),
        &targets,
    );
    settle(&mut app, owner, 30);
    assert!(
        app.world()
            .get::<crate::stage_inputs::ExtractedStages>(owner)
            .unwrap()
            .view
            .is_none(),
        "volumes suppress automatic debug slices"
    );
    let first = field_matches_texture(&app, owner);
    assert_eq!(
        first,
        field_matches_texture(&app, owner),
        "exact paused texture"
    );
    let pixels = texture(&app, target.id(), 4);
    assert!(
        pixels.iter().any(|pixel| pixel & 0x00ffffff != 0),
        "actual volume material must render against black"
    );

    app.world_mut()
        .get_mut::<crate::PresentedEffect>(owner)
        .unwrap()
        .instance
        .set_playback_time(0.7501);
    settle(&mut app, owner, 45);
    assert_ne!(first, field_matches_texture(&app, owner));
    assert_eq!(
        views(&mut app),
        original_views,
        "paused/forward frames reuse material and mesh"
    );
    // The bounded timestamp mailbox keeps only the latest completed batch;
    // pause frames can supersede an earlier measurement. Observe one actual
    // advance before submitting another paused frame. This wait is test-only.
    app.world_mut()
        .get_mut::<crate::PresentedEffect>(owner)
        .unwrap()
        .instance
        .set_playback_time(0.7668);
    app.update();
    device
        .wgpu_device()
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(45)),
        })
        .unwrap();
    app.update();
    assert!(
        matches!(app.world().get::<GpuStageTiming>(owner).unwrap()
        .time_ns(&app.world().get::<crate::PresentedEffect>(owner).unwrap().instance),
        aestra_runtime::ProfileValue::Measured(ns) if ns > 0),
        "real stage timestamp callback"
    );
    let old_image = targets[0].image;
    // Same-layout look edit must retain the material/texture while updating uniform values.
    *app.world_mut()
        .get_mut::<crate::PresentedEffect>(owner)
        .unwrap() = crate::PresentedEffect::new(effect(16, 0.1))
        .with_history_policy(aestra_runtime::PlaybackHistoryPolicy::PlaybackOnly);
    app.world_mut()
        .get_mut::<crate::PresentedEffect>(owner)
        .unwrap()
        .instance
        .set_playback_time(0.5001);
    settle(&mut app, owner, 30);
    assert_eq!(views(&mut app), original_views);
    assert_eq!(
        app.world().get::<VolumeViews>(owner).unwrap().targets()[0].image,
        old_image
    );
    field_matches_texture(&app, owner);
    let dim_pixels = texture(&app, target.id(), 4);
    assert_ne!(
        pixels, dim_pixels,
        "live look edit reaches actual material pixels"
    );

    // A changed layout must replace geometry and targets, not keep a stale texture binding.
    *app.world_mut()
        .get_mut::<crate::PresentedEffect>(owner)
        .unwrap() = crate::PresentedEffect::new(effect(24, 0.1))
        .with_history_policy(aestra_runtime::PlaybackHistoryPolicy::PlaybackOnly);
    app.world_mut()
        .get_mut::<crate::PresentedEffect>(owner)
        .unwrap()
        .instance
        .set_playback_time(0.5001);
    app.update();
    let targets = app.world().get::<VolumeViews>(owner).unwrap().targets();
    assert_eq!(targets[0].layout.dims, [24; 3]);
    assert_ne!(targets[0].image, old_image);
    assert!(app.world().get_entity(original_views[0].0).is_err());
    permit_copy(
        &mut app.world_mut().resource_mut::<Assets<Image>>(),
        &targets,
    );
    settle(&mut app, owner, 30);
    field_matches_texture(&app, owner);

    // Remove camera, retire volume boxes, then recreate and rebind fresh targets.
    app.world_mut().despawn(view);
    for _ in 0..3 {
        app.update();
    }
    assert!(views(&mut app).is_empty());
    assert!(
        app.world()
            .get::<VolumeViews>(owner)
            .unwrap()
            .targets()
            .is_empty()
    );
    let extracted = app
        .world()
        .get::<crate::stage_inputs::ExtractedStages>(owner)
        .unwrap();
    let slice = extracted
        .view
        .clone()
        .expect("automatic debug slice without a volume camera");
    app.world_mut()
        .resource_mut::<Assets<Image>>()
        .get_mut(slice.image)
        .unwrap()
        .texture_descriptor
        .usage |= TextureUsages::COPY_SRC;
    for _ in 0..3 {
        app.update();
    }
    assert!(
        texture(&app, slice.image, 4)
            .iter()
            .any(|word| word >> 24 != 0),
        "actual field-slice copy remains available without a 3D camera"
    );
    let (_view, _) = camera(&mut app);
    app.update();
    let targets = app.world().get::<VolumeViews>(owner).unwrap().targets();
    assert_ne!(targets[0].image, old_image);
    permit_copy(
        &mut app.world_mut().resource_mut::<Assets<Image>>(),
        &targets,
    );
    settle(&mut app, owner, 30);
    field_matches_texture(&app, owner);
    assert!(
        app.world()
            .get::<crate::stage_inputs::ExtractedStages>(owner)
            .unwrap()
            .view
            .is_none()
    );
    app.world_mut().despawn(owner);
    for _ in 0..4 {
        app.update();
    }
    assert!(views(&mut app).is_empty());
    assert!(
        app.sub_app(RenderApp)
            .world()
            .resource::<StageRuntimes>()
            .0
            .is_empty()
    );
    assert!(
        pollster::block_on(error_scope.pop()).is_none(),
        "GPU validation error"
    );
    println!(
        "production_stage_volume_installers=true actual_material_pixels=true live_density_texture=true exact_pause=true forward_refresh=true look_edit_reuse=true layout_rebuild=true stage_timestamp_callback=true debug_slice_fallback=true camera_retirement_reentry=true owner_teardown=true playback_only=true test_only_readback=true coupled_volume_qualified=false sparse_volume_qualified=false"
    );
}
