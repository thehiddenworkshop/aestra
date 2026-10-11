//! Actual host plugin, selection, async transport and same-frame GPU injection.
//! Only screenshots/selected-record oracles use test readback; production never waits.
use crate::{
    particle_light_gpu::*, particle_light_realization::*, particle_lights_native as source,
};
use bevy::{
    camera::{Exposure, RenderTarget, ShadowLodOrigin, visibility::RenderLayers},
    core_pipeline::tonemapping::{DebandDither, Tonemapping},
    prelude::*,
    render::{
        RenderApp,
        render_resource::TextureFormat,
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
};
use std::time::{Duration, Instant};

#[derive(Resource, Default)]
struct Capture(Option<Image>);
pub(super) fn receiver(app: &mut App) -> (Entity, Handle<Image>) {
    app.init_resource::<Capture>()
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(GlobalAmbientLight {
            brightness: 0.,
            ..default()
        });
    let target = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            128,
            128,
            TextureFormat::Bgra8UnormSrgb,
            None,
        ));
    let camera = app
        .world_mut()
        .spawn((
            Camera3d::default(),
            ShadowLodOrigin,
            Msaa::Off,
            Tonemapping::None,
            DebandDither::Disabled,
            AmbientLight {
                brightness: 0.,
                ..default()
            },
            Exposure { ev100: 0. },
            RenderTarget::Image(target.clone().into()),
            Transform::from_xyz(0., 0., 16.).looking_at(Vec3::ZERO, Vec3::Y),
        ))
        .id();
    let mesh = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Cuboid::new(20., 12., 0.2));
    let material = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 1.,
            ..default()
        });
    app.world_mut().spawn((
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::from_xyz(0., 0., -2.),
    ));
    (camera, target)
}
pub(super) fn until(app: &mut App, condition: impl Fn(&mut App) -> bool) {
    let start = Instant::now();
    loop {
        app.update();
        if condition(app) {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(45),
            "host lights did not settle: {:?}; {:?}",
            app.world().resource::<ParticleLightStatistics>(),
            app.world()
                .resource::<ParticleLightGpuStatistics>()
                .snapshot()
        );
        std::thread::sleep(Duration::from_millis(4));
    }
}
pub(super) fn capture(app: &mut App, target: &Handle<Image>) -> (f64, f64) {
    for _ in 0..4 {
        app.update();
    }
    app.world_mut()
        .spawn(Screenshot::image(target.clone()))
        .observe(
            |event: On<ScreenshotCaptured>, mut result: ResMut<Capture>| {
                result.0 = Some(event.image.clone());
            },
        );
    until(app, |app| app.world().resource::<Capture>().0.is_some());
    let image = app
        .world_mut()
        .resource_mut::<Capture>()
        .0
        .take()
        .unwrap()
        .try_into_dynamic()
        .unwrap()
        .to_rgb8();
    let mut sum = 0.;
    let mut xsum = 0.;
    for (x, _, pixel) in image.enumerate_pixels() {
        let value = pixel.0.into_iter().map(f64::from).sum::<f64>();
        sum += value;
        xsum += f64::from(x) * value;
    }
    (
        sum / f64::from(image.width() * image.height() * 3),
        xsum / sum.max(1.),
    )
}
pub(super) fn lit(app: &mut App, target: &Handle<Image>) -> (f64, f64) {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        let result = capture(app, target);
        if result.0 > 0.5 {
            return result;
        }
        assert!(Instant::now() < deadline, "black receiver: {result:?}");
    }
}
fn proxies(app: &mut App) -> Vec<Entity> {
    let world = app.world_mut();
    let mut query = world.query_filtered::<Entity, With<ParticleLightProxy>>();
    let mut result = query.iter(world).collect::<Vec<_>>();
    result.sort();
    result
}
fn slots(app: &mut App) -> Vec<Entity> {
    let world = app.world_mut();
    let mut query = world.query_filtered::<Entity, With<ParticleLightGpuSlot>>();
    let mut result = query.iter(world).collect::<Vec<_>>();
    result.sort();
    result
}
fn async_ready(app: &mut App, count: usize) {
    until(app, |app| {
        app.world().resource::<ParticleLightStatistics>().active == count
    });
}
fn gpu_ready(app: &mut App, count: u32) {
    until(app, |app| {
        let observation = app
            .world()
            .resource::<ParticleLightGpuStatistics>()
            .snapshot();
        observation.rejection.is_none() && observation.written_capacity == count
    });
}
pub(super) fn configure(app: &mut App, mode: ParticleLightMode) {
    app.insert_resource(AestraParticleLightSettings {
        max_lights: 2,
        ..default()
    })
    .insert_resource(ParticleLightMode::PortableAsync)
    .insert_resource(ParticleLightReadbackSettings {
        max_lights: 2,
        max_in_flight: 2,
        max_staging_bytes: 224,
        max_age: Duration::from_secs(10),
        max_frame_lag: 100,
        ..default()
    })
    .insert_resource(ParticleLightGpuSettings {
        max_lights: 2,
        ..default()
    })
    .insert_resource(ParticleLightRealizationSettings {
        max_lumens: 400.,
        max_range: 8.,
    });
    app.insert_resource(mode);
}

#[test]
#[ignore = "Requires native Vulkan; run explicitly and serially"]
fn native_020_host_lights_realize_async_and_same_frame_and_retire() {
    let mut app = source::host_app();
    configure(&mut app, ParticleLightMode::PortableAsync);
    let (camera, target) = receiver(&mut app);
    let dark = capture(&mut app, &target);
    assert!(dark.0 < 0.1, "unlit baseline: {dark:?}");
    let world = Mat4::from_translation(Vec3::new(-3., 0., 0.));
    let owner = source::spawn(&mut app, true, 71, world);
    app.world_mut()
        .entity_mut(owner)
        .insert((Transform::IDENTITY, Visibility::Visible));
    async_ready(&mut app, 2);
    // A first callback can precede completion of stateful catch-up. Require the
    // settled async packet to match the current paused GPU selection, not merely
    // the first nonempty host pool.
    until(&mut app, |app| {
        let Some((_, records, counts)) = source::snapshot(app) else {
            return false;
        };
        counts[2] == 2
            && proxies(app).iter().all(|entity| {
                let position = app.world().get::<Transform>(*entity).unwrap().translation;
                records.as_chunks::<12>().0.iter().take(2).any(|w| {
                    position.distance(Vec3::new(
                        f32::from_bits(w[0]),
                        f32::from_bits(w[1]),
                        f32::from_bits(w[2]),
                    )) < 1e-5
                })
            })
    });
    let original = proxies(&mut app);
    assert_eq!(original.len(), 2);
    let (_, records, counts) = source::snapshot(&app).unwrap();
    assert_eq!(counts[2], 2);
    for entity in &original {
        let light = app.world().get::<PointLight>(*entity).unwrap();
        assert_eq!((light.intensity, light.range), (400., 8.));
        assert!(!light.shadow_maps_enabled && !light.contact_shadows_enabled);
        let position = app.world().get::<Transform>(*entity).unwrap().translation;
        assert!(records.as_chunks::<12>().0.iter().any(|w| {
            position.distance(Vec3::new(
                f32::from_bits(w[0]),
                f32::from_bits(w[1]),
                f32::from_bits(w[2]),
            )) < 1e-5
        }));
    }
    let first = lit(&mut app, &target);
    app.world_mut()
        .entity_mut(owner)
        .insert(RenderLayers::layer(7));
    until(&mut app, |app| {
        original
            .iter()
            .all(|entity| app.world().get::<RenderLayers>(*entity) == Some(&RenderLayers::layer(7)))
    });
    assert_eq!(proxies(&mut app), original);
    app.world_mut().entity_mut(owner).remove::<RenderLayers>();
    let moved_world = Mat4::from_translation(Vec3::new(3., 0., 0.));
    source::advance(&mut app, owner, 0.7501, moved_world);
    until(&mut app, |app| {
        original
            .iter()
            .all(|entity| app.world().get::<Transform>(*entity).unwrap().translation.x > 2.)
    });
    assert_eq!(proxies(&mut app), original);
    let moved = lit(&mut app, &target);
    assert!(
        moved.1 > first.1 + 3.,
        "receiver must follow live GPU motion: {first:?} -> {moved:?}"
    );
    assert!(
        app.world()
            .resource::<ParticleLightStatistics>()
            .readback
            .completed
            > 0
    );
    app.world_mut()
        .resource_mut::<ParticleLightReadbackSettings>()
        .max_staging_bytes = 0;
    until(&mut app, |app| {
        app.world().resource::<ParticleLightStatistics>().active == 0
            && app
                .world()
                .resource::<ParticleLightStatistics>()
                .readback
                .rejected
                > 0
    });
    let rejected = capture(&mut app, &target);
    assert!(rejected.0 < 0.1, "rejected async receiver: {rejected:?}");
    app.world_mut()
        .resource_mut::<ParticleLightReadbackSettings>()
        .max_staging_bytes = 224;
    async_ready(&mut app, 2);
    assert_eq!(proxies(&mut app), original);

    app.insert_resource(ParticleLightMode::SameFrameGpu);
    gpu_ready(&mut app, 2);
    // Main-world statistics intentionally lag render submission. Sample the
    // shared transport after transition, then require no further copies.
    let submitted = app
        .world()
        .resource::<crate::particle_light_readback::ParticleLightReadback>()
        .take()
        .3
        .submitted;
    assert!(proxies(&mut app).is_empty());
    let native_slots = slots(&mut app);
    assert_eq!(native_slots.len(), 2);
    for &slot in &native_slots {
        assert_eq!(app.world().get::<PointLight>(slot).unwrap().intensity, 0.);
    }
    let same_frame = lit(&mut app, &target);
    assert!(
        (same_frame.1 - moved.1).abs() < 2.,
        "same receiver position in both modes"
    );
    assert_eq!(
        app.world()
            .resource::<ParticleLightStatistics>()
            .readback
            .submitted,
        submitted
    );
    let sequence = app
        .world()
        .resource::<ParticleLightGpuStatistics>()
        .snapshot()
        .sequence;
    source::advance(&mut app, owner, 1.0001, world);
    app.update();
    let selected = app
        .sub_app(RenderApp)
        .world()
        .resource::<crate::particle_lights::GpuSelectedParticleLights>()
        .frame()
        .unwrap();
    let injected = app
        .world()
        .resource::<ParticleLightGpuStatistics>()
        .snapshot();
    assert!(injected.sequence > sequence);
    assert_eq!(
        injected.sequence, selected.sequence,
        "inject consumes the same render selection"
    );
    let returned = lit(&mut app, &target);
    assert!(returned.1 + 3. < same_frame.1);
    assert_eq!(slots(&mut app), native_slots);
    app.world_mut()
        .entity_mut(camera)
        .insert(RenderLayers::layer(2));
    until(&mut app, |app| {
        app.world()
            .resource::<ParticleLightGpuStatistics>()
            .snapshot()
            .rejection
            == Some(ParticleLightGpuRejection::NonDefaultLayers)
    });
    assert_eq!(
        app.world()
            .resource::<ParticleLightGpuStatistics>()
            .snapshot()
            .written_capacity,
        0
    );
    app.world_mut().entity_mut(camera).remove::<RenderLayers>();
    gpu_ready(&mut app, 2);
    app.world_mut()
        .resource_mut::<ParticleLightGpuSettings>()
        .max_manifest_bytes = 0;
    until(&mut app, |app| {
        app.world()
            .resource::<ParticleLightGpuStatistics>()
            .snapshot()
            .rejection
            == Some(ParticleLightGpuRejection::ManifestBudget)
    });
    assert!(capture(&mut app, &target).0 < 0.1);
    app.world_mut()
        .resource_mut::<ParticleLightGpuSettings>()
        .max_manifest_bytes = 1024 * 1024;
    gpu_ready(&mut app, 2);
    lit(&mut app, &target);
    app.world_mut().entity_mut(owner).insert(Visibility::Hidden);
    until(&mut app, |app| {
        native_slots
            .iter()
            .all(|slot| app.world().get::<Visibility>(*slot) == Some(&Visibility::Hidden))
    });
    assert!(capture(&mut app, &target).0 < 0.1);
    app.world_mut()
        .entity_mut(owner)
        .insert(Visibility::Visible);
    gpu_ready(&mut app, 2);
    app.world_mut().despawn(owner);
    until(&mut app, |app| {
        native_slots
            .iter()
            .all(|slot| app.world().get::<Visibility>(*slot) == Some(&Visibility::Hidden))
    });
    assert!(capture(&mut app, &target).0 < 0.1);
    assert_eq!(slots(&mut app), native_slots);
    app.world_mut()
        .resource_mut::<ParticleLightGpuSettings>()
        .max_lights = 0;
    app.update();
    assert!(slots(&mut app).is_empty());
    assert!(
        app.sub_app(RenderApp)
            .world()
            .resource::<crate::stateful_simulation::StatefulStates>()
            .0
            .values()
            .flatten()
            .all(|state| state.checkpoints.is_empty())
    );
}
