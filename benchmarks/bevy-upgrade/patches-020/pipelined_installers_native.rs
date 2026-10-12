//! Real threaded render app, host project clock and production installers together.
//! Uses production particle/material allocation, live updates, transform synchronization and render installers.
use crate::{
    host_lights_native as receiver, host_stage_native as project,
    particle_light_gpu::*,
    particle_light_realization::*,
    particle_lights::GpuSelectedParticleLights,
    stage_inputs::ExtractedStages,
    stage_output_delivery::{OutputEvents, StageOutputIdentity},
};
use bevy::{
    prelude::*,
    render::{
        Render, RenderApp, RenderPlugin, RenderSystems,
        renderer::{RenderAdapterInfo, RenderDevice},
        sync_world::MainEntity,
    },
    time::TimeUpdateStrategy,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Resource, Default, Clone)]
struct Evidence(Arc<Mutex<Frame>>);
#[derive(Default, Debug)]
struct Frame {
    thread: Option<std::thread::ThreadId>,
    stages: BTreeMap<Entity, (StageOutputIdentity, u32, usize)>,
    sources: Vec<crate::particle_lights::ParticleLightSource>,
    sequence: u64,
    ready: std::collections::BTreeSet<Entity>,
    draws: BTreeMap<Entity, (Entity, Vec3)>,
}
// Read-only render-thread instrumentation; no substitute dispatch or GPU wait.
type ObservationResources<'w> = (
    Res<'w, GpuSelectedParticleLights>,
    Res<'w, crate::presentation_readiness::PresentedOwners>,
    Res<'w, ParticleLightGpuStatistics>,
    Res<'w, ParticleLightMode>,
);
fn observe(
    evidence: Res<Evidence>,
    effects: Query<(Entity, &MainEntity, &ExtractedStages)>,
    draws: Query<(&MainEntity, &crate::draw_instance::GpuDrawInstance)>,
    runtimes: Res<crate::stage_runtimes::StageRuntimes>,
    resources: ObservationResources,
) {
    let (selected, ready, injected, mode) = resources;
    let mut frame = evidence.0.lock().unwrap();
    frame.stages.clear();
    frame.draws = draws
        .iter()
        .map(|(main, draw)| (main.id(), (draw.owner, draw.mesh_center)))
        .collect();
    frame.ready = effects
        .iter()
        .filter(|(entity, _, _)| ready.owners.contains(entity))
        .map(|(_, main, _)| main.id())
        .collect();
    for (entity, main, extracted) in &effects {
        let Some(runtime) = runtimes.0.get(&entity) else {
            continue;
        };
        for stage in runtime.timelines.iter().flatten() {
            assert_eq!(stage.checkpoint_bytes(), 0);
            frame.stages.insert(
                main.id(),
                (
                    StageOutputIdentity::of_extracted(extracted),
                    stage.last_tick(),
                    extracted.volumes.len(),
                ),
            );
        }
    }
    frame.sources.clear();
    frame.sequence = 0;
    if let Some(selected) = selected.frame() {
        for source in &selected.manifest {
            // The manifest also contains unready sources with zero input counts.
            assert!(effects.iter().any(|(_, main, _)| main.id() == source.owner));
        }
        frame.sources.clone_from(&selected.manifest);
        frame.sequence = selected.sequence;
        let injection = injected.snapshot();
        if *mode == ParticleLightMode::SameFrameGpu && injection.written_capacity > 0 {
            assert_eq!(
                injection.sequence, selected.sequence,
                "same-frame injection must consume this render frame's selection"
            );
        }
    }
}
// Ordinary sendable systems can be stolen by any pool worker (even the host
// thread while it awaits extraction). An exclusive system records the actual
// render-app schedule caller without changing the production graph executor.
fn mark_thread(world: &mut World) {
    world.resource::<Evidence>().0.lock().unwrap().thread = Some(std::thread::current().id());
}

// Scene isolation only: receiver screenshots exclude emissive draw children,
// without changing source visibility, light authorization or production dispatch.
#[derive(Resource, Default)]
struct ReceiverCapture(bool);
fn isolate_particle_draws(
    capture: Res<ReceiverCapture>,
    mut draws: Query<&mut Visibility, With<crate::draw_instance::GpuDrawInstance>>,
) {
    for mut visibility in &mut draws {
        *visibility = if capture.0 {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
}
fn capture_receiver(app: &mut App, target: &Handle<Image>, lit: bool) -> (f64, f64) {
    app.world_mut().resource_mut::<ReceiverCapture>().0 = true;
    let result = if lit {
        receiver::lit(app, target)
    } else {
        receiver::capture(app, target)
    };
    app.world_mut().resource_mut::<ReceiverCapture>().0 = false;
    app.update();
    result
}

fn app() -> (App, RenderDevice) {
    app_with(|_| {})
}

// Allow other full-plugin gates to attach read-only observers and controlled
// readiness dependencies before Bevy moves the render app to its real thread.
pub(super) fn app_with(configure: impl FnOnce(&mut App)) -> (App, RenderDevice) {
    aestra_fluid::link();
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(bevy::window::WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .set(RenderPlugin {
                synchronous_pipeline_compilation: false,
                render_creation: bevy::render::settings::RenderCreation::Automatic(Box::new(
                    bevy::render::settings::WgpuSettings {
                        backends: Some(wgpu::Backends::VULKAN),
                        features: wgpu::Features::TIMESTAMP_QUERY,
                        ..default()
                    },
                )),
                ..default()
            }),
    );
    app.add_plugins((crate::AestraPlugin, AestraParticleLightPlugin))
        .init_resource::<ReceiverCapture>()
        .insert_resource(crate::catchup_pacing::AestraCatchupPacing { paced: false })
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO))
        .add_systems(
            Update,
            isolate_particle_draws.after(crate::AestraRenderSet::Prepare),
        );
    app.world_mut()
        .resource_mut::<Time<Virtual>>()
        .set_max_delta(Duration::from_secs(2));
    let evidence = Evidence::default();
    app.insert_resource(evidence.clone());
    app.sub_app_mut(RenderApp)
        .insert_resource(evidence)
        .add_systems(
            Render,
            (observe, mark_thread)
                .chain()
                .in_set(RenderSystems::Cleanup),
        );
    configure(&mut app);
    app.finish();
    let render = app.sub_app(RenderApp).world();
    let device = render.resource::<RenderDevice>().clone();
    let info = render.resource::<RenderAdapterInfo>();
    assert_eq!(info.backend, wgpu::Backend::Vulkan);
    assert_ne!(info.device_type, wgpu::DeviceType::Cpu);
    println!(
        "Combined pipelined installers: {} / {:?} / {}",
        info.name, info.backend, info.driver_info
    );
    app.cleanup();
    assert!(app.get_sub_app(RenderApp).is_none());
    assert!(
        app.world()
            .contains_resource::<bevy::render::pipelined_rendering::RenderAppChannels>()
    );
    (app, device)
}

fn settle(app: &mut App, owners: &[Entity]) {
    receiver::until(app, |app| {
        let evidence = app.world().resource::<Evidence>().0.lock().unwrap();
        let current_sources = evidence.sources.iter().all(|source| {
            app.world()
                .get::<crate::PresentedEffect>(source.owner)
                .is_some_and(|owner| {
                    source.owner_epoch == owner.instance.history_epoch()
                        && source.revision == owner.instance.history_revision()
                        && source.seed == owner.instance.seed()
                        && source.artifact.matches(owner.effect())
                })
                && app
                    .world()
                    .get::<crate::PresentedEffect>(source.root)
                    .is_some_and(|root| source.root_epoch == root.instance.history_epoch())
        });
        current_sources
            && owners.iter().all(|owner| {
                let presented = app.world().get::<crate::PresentedEffect>(*owner).unwrap();
                let tick = aestra_runtime::trace_tick(presented.instance.time()) as u32;
                evidence
                    .stages
                    .get(owner)
                    .is_some_and(|(identity, actual, volumes)| {
                        *identity == StageOutputIdentity::of_presented(presented)
                            && *actual == tick
                            && *volumes > 0
                    })
                    && evidence.ready.contains(owner)
                    && app
                        .world()
                        .get::<crate::GpuStageProgress>(*owner)
                        .is_some_and(|progress| progress.is_ready(&presented.instance))
                    && app
                        .world()
                        .get::<OutputEvents>(*owner)
                        .and_then(|events| events.accepted_tick(0))
                        == Some(u64::from(tick))
            })
    });
}

fn advance(app: &mut App, root: Entity, frames: u32) {
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
        f64::from(frames) / 60.,
    )));
    app.world_mut()
        .get_mut::<crate::EffectPlayer>(root)
        .unwrap()
        .playing = true;
    app.update();
    app.world_mut()
        .get_mut::<crate::EffectPlayer>(root)
        .unwrap()
        .playing = false;
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
}

fn proxy_positions(app: &mut App) -> Vec<Vec3> {
    let world = app.world_mut();
    let mut proxies = world.query_filtered::<&Transform, With<ParticleLightProxy>>();
    let mut positions = proxies
        .iter(world)
        .map(|t| t.translation)
        .collect::<Vec<_>>();
    positions.sort_by(|a, b| a.x.total_cmp(&b.x));
    positions
}

// Observe production-created children and the actual visibility-filtered render
// extraction; never install fixture draw records or force ViewVisibility.
fn settle_draws(app: &mut App, owners: &[Entity]) -> BTreeMap<Entity, (Entity, Vec3)> {
    let world = app.world_mut();
    let mut query = world.query::<(
        Entity,
        &crate::draw_instance::GpuDrawInstance,
        &GlobalTransform,
        &bevy::camera::primitives::Aabb,
        &ViewVisibility,
    )>();
    let expected = query
        .iter(world)
        .filter(|(_, draw, _, _, visible)| owners.contains(&draw.owner) && visible.get())
        .map(|(entity, draw, transform, bounds, _)| {
            (
                entity,
                (
                    draw.owner,
                    crate::draw_instance::gpu_draw_mesh_center(transform, bounds),
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for owner in owners {
        assert!(expected.values().any(|(actual, _)| actual == owner));
    }
    receiver::until(app, |app| {
        let frame = app.world().resource::<Evidence>().0.lock().unwrap();
        expected
            .iter()
            .all(|(entity, value)| frame.draws.get(entity) == Some(value))
    });
    expected
}

#[test]
#[ignore = "Requires native Vulkan; real pipelined render thread; run serially"]
fn native_020_pipelined_installers_share_nested_clock_outputs_and_lights() {
    let (mut app, device) = app();
    let scope = device
        .wgpu_device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    receiver::configure(&mut app, ParticleLightMode::PortableAsync);
    let (_, target) = receiver::receiver(&mut app);
    let compiled = project::project_with_lights(&aestra_compiler::ExtensionRegistry::linked());
    let spawn = |app: &mut App, position| {
        let object = app
            .world_mut()
            .spawn(GlobalTransform::from_translation(position))
            .id();
        let mut player = crate::EffectPlayer::from_project(compiled.clone())
            .with_history_policy(crate::PlaybackHistoryPolicy::PlaybackOnly);
        player.playing = false;
        (
            app.world_mut()
                .spawn((player, crate::AestraBindings::new().bind("Emitter", object)))
                .id(),
            object,
        )
    };
    let (a, source) = spawn(&mut app, Vec3::new(0., 0.5, 0.));
    let (b, _) = spawn(&mut app, Vec3::splat(100.));
    app.update();
    let a_leaf = project::leaf(&mut app, a);
    let b_leaf = project::leaf(&mut app, b);
    settle(&mut app, &[a_leaf, b_leaf]);
    assert!(app.world().resource::<crate::GpuCapabilities>().detected);
    assert_eq!(
        app.world().resource::<crate::AestraRuntimeStatus>().active,
        crate::ActiveBackend::Gpu
    );
    for owner in [a_leaf, b_leaf] {
        assert_eq!(
            app.world()
                .get::<crate::EffectRuntimeStatus>(owner)
                .unwrap()
                .active,
            crate::ActiveBackend::Gpu
        );
    }
    assert!(project::force(&app, a_leaf) > 0.00001);
    assert_eq!(project::force(&app, b_leaf), 0.);
    receiver::until(&mut app, |app| {
        let stats = app.world().resource::<ParticleLightStatistics>();
        stats.active == 2 && stats.readback.completed > 0
    });
    capture_receiver(&mut app, &target, true);
    let buffers_before = [a_leaf, b_leaf].map(|owner| {
        let gpu = app
            .world()
            .get::<crate::effect_inputs::GpuEffectBuffers>(owner)
            .unwrap();
        (
            gpu.particles.clone(),
            gpu.globals.clone(),
            gpu.render_globals.clone(),
        )
    });
    let before_positions = proxy_positions(&mut app);
    assert_eq!(before_positions.len(), 2);
    // Identical nested seeds/inputs produce the same effect-local selected point.
    assert!(before_positions[0].distance(before_positions[1]) < 1e-5);
    let children_before = settle_draws(&mut app, &[a_leaf, b_leaf]);
    let particles = receiver::capture(&mut app, &target);
    assert!(
        particles.0 > 0.5,
        "full plugin must draw actual particle pixels: {particles:?}"
    );
    receiver::until(&mut app, |app| {
        let profile = &app
            .world()
            .get::<crate::project_profile::ProjectProfiler>(a)
            .unwrap()
            .0;
        matches!(profile.total.submitted_instances, aestra_runtime::ProfileValue::Measured(n) if n > 0)
            && matches!(profile.total.submitted_vertices, aestra_runtime::ProfileValue::Measured(n) if n > 0)
    });
    // A paused host placement edit must reach selection through production
    // PostUpdate synchronization, not a fixture-owned render-global upload.
    app.world_mut()
        .get_mut::<Transform>(a)
        .unwrap()
        .translation
        .x = 20.;
    app.world_mut()
        .get_mut::<Transform>(b)
        .unwrap()
        .translation
        .x = 40.;
    receiver::until(&mut app, |app| {
        let positions = proxy_positions(app);
        positions.len() == 2
            && positions
                .iter()
                .zip([20., 40.])
                .all(|(p, shift)| p.distance(before_positions[0] + Vec3::X * shift) < 1e-5)
    });
    let moved_children = settle_draws(&mut app, &[a_leaf, b_leaf]);
    assert_eq!(
        children_before.keys().collect::<Vec<_>>(),
        moved_children.keys().collect::<Vec<_>>()
    );
    for (entity, (owner, center)) in &moved_children {
        let shift = if *owner == a_leaf { 20. } else { 40. };
        assert!(center.distance(children_before[entity].1 + Vec3::X * shift) < 1e-5);
    }
    app.world_mut()
        .get_mut::<Transform>(a)
        .unwrap()
        .translation
        .x = 0.;
    app.world_mut()
        .get_mut::<Transform>(b)
        .unwrap()
        .translation
        .x = 0.;
    receiver::until(&mut app, |app| {
        let positions = proxy_positions(app);
        positions.len() == 2
            && positions
                .iter()
                .zip(&before_positions)
                .all(|(a, b)| a.distance(*b) < 1e-5)
    });
    settle(&mut app, &[a_leaf, b_leaf]);
    assert_eq!(settle_draws(&mut app, &[a_leaf, b_leaf]), children_before);
    // Inherited root visibility must retire actual extracted draws, then restore
    // the same production children without disturbing the other nested root.
    *app.world_mut().get_mut::<Visibility>(a).unwrap() = Visibility::Hidden;
    receiver::until(&mut app, |app| {
        let frame = app.world().resource::<Evidence>().0.lock().unwrap();
        !frame.draws.values().any(|(owner, _)| *owner == a_leaf)
            && children_before
                .iter()
                .filter(|(_, (owner, _))| *owner == b_leaf)
                .all(|(entity, value)| frame.draws.get(entity) == Some(value))
    });
    for entity in children_before.keys() {
        assert!(
            app.world()
                .get::<crate::draw_instance::GpuDrawInstance>(*entity)
                .is_some()
        );
    }
    *app.world_mut().get_mut::<Visibility>(a).unwrap() = Visibility::Inherited;
    app.update();
    assert_eq!(settle_draws(&mut app, &[a_leaf, b_leaf]), children_before);
    settle(&mut app, &[a_leaf, b_leaf]);
    {
        let frame = app.world().resource::<Evidence>().0.lock().unwrap();
        assert_ne!(frame.thread, Some(std::thread::current().id()));
        assert_eq!(frame.stages.len(), 2);
        assert!(
            frame
                .sources
                .iter()
                .all(|s| s.clip_path.len() == 2 && [a, b].contains(&s.root))
        );
        assert_eq!(frame.sources.len(), 2);
    }
    // Independent roots, composed offsets, real live binding and production reset/output delivery.
    advance(&mut app, a, 20);
    settle(&mut app, &[a_leaf, b_leaf]);
    assert_eq!(
        aestra_runtime::trace_tick(
            app.world()
                .get::<crate::PresentedEffect>(b_leaf)
                .unwrap()
                .instance
                .time()
        ),
        30
    );
    *app.world_mut().get_mut::<GlobalTransform>(source).unwrap() =
        GlobalTransform::from_translation(Vec3::splat(100.));
    app.world_mut()
        .get_mut::<crate::EffectPlayer>(a)
        .unwrap()
        .restart();
    advance(&mut app, a, 30);
    settle(&mut app, &[a_leaf, b_leaf]);
    assert_eq!(
        project::force(&app, a_leaf),
        0.,
        "restart must use the new bound source"
    );
    app.world_mut()
        .get_mut::<crate::EffectPlayer>(a)
        .unwrap()
        .seek(1.5);
    settle(&mut app, &[a_leaf, b_leaf]);
    app.world_mut()
        .get_mut::<crate::EffectPlayer>(a)
        .unwrap()
        .seek(0.25);
    settle(&mut app, &[a_leaf, b_leaf]);

    app.insert_resource(ParticleLightMode::SameFrameGpu);
    for (owner, handles) in [a_leaf, b_leaf].into_iter().zip(buffers_before) {
        let gpu = app
            .world()
            .get::<crate::effect_inputs::GpuEffectBuffers>(owner)
            .unwrap();
        assert_eq!(
            (
                gpu.particles.clone(),
                gpu.globals.clone(),
                gpu.render_globals.clone()
            ),
            handles,
            "placement, playback and seeks must not reallocate particle assets"
        );
    }
    receiver::until(&mut app, |app| {
        let stats = app
            .world()
            .resource::<ParticleLightGpuStatistics>()
            .snapshot();
        stats.written_capacity == 2 && stats.rejection.is_none()
    });
    // Let the render thread extract the mode before sampling transport submissions.
    let submitted = app
        .world()
        .resource::<ParticleLightReadback>()
        .take()
        .3
        .submitted;
    let lit = capture_receiver(&mut app, &target, true);
    assert!(lit.0 > 0.5, "combined same-frame receiver: {lit:?}");
    assert_eq!(
        app.world()
            .resource::<ParticleLightReadback>()
            .take()
            .3
            .submitted,
        submitted
    );
    // A paused stage need not have a new timestamp after a seek. Advance both
    // owners, then await their ordinary asynchronous measurement mailboxes.
    advance(&mut app, a, 1);
    advance(&mut app, b, 1);
    settle(&mut app, &[a_leaf, b_leaf]);
    println!(
        "Awaiting stage timestamps: {:?}",
        [a_leaf, b_leaf].map(|owner| app
            .world()
            .get::<crate::GpuStageTiming>(owner)
            .unwrap()
            .time_ns(
                &app.world()
                    .get::<crate::PresentedEffect>(owner)
                    .unwrap()
                    .instance
            ))
    );
    receiver::until(&mut app, |app| {
        [a_leaf, b_leaf].iter().all(|owner| {
            matches!(app.world().get::<crate::GpuStageTiming>(*owner).unwrap().time_ns(
            &app.world().get::<crate::PresentedEffect>(*owner).unwrap().instance),
            aestra_runtime::ProfileValue::Measured(ns) if ns > 0)
        })
    });
    receiver::until(&mut app, |app| {
        [a_leaf, b_leaf].iter().all(|owner| {
            let world = app.world();
            let presented = world.get::<crate::PresentedEffect>(*owner).unwrap();
            let timing = world
                .get::<crate::simulation_timing::GpuSimulationTiming>(*owner)
                .unwrap();
            let particles = world
                .get::<crate::particle_statistics::GpuParticleStatistics>(*owner)
                .unwrap();
            matches!(timing.time_ns(&presented.instance, particles),
                aestra_runtime::ProfileValue::Measured(ns) if ns > 0)
        })
    });
    receiver::until(&mut app, |app| {
        let world = app.world();
        [a, b].iter().all(|owner| {
            let Some(profile) = world.get::<crate::project_profile::ProjectProfiler>(*owner) else {
                return false;
            };
            profile.0.instances.len() == 3
                && matches!(profile.0.total.gpu_simulation_time_ns,
                    aestra_runtime::ProfileValue::Measured(ns) if ns > 0)
        })
    });
    // Both bound sources are now outside the domain: the receiver cannot be
    // illuminated by the volume itself. Admission disable must extinguish it.
    app.world_mut()
        .resource_mut::<crate::AestraParticleLightSettings>()
        .max_lights = 0;
    receiver::until(&mut app, |app| {
        app.world()
            .resource::<ParticleLightGpuStatistics>()
            .snapshot()
            .written_capacity
            == 0
    });
    let rejected = capture_receiver(&mut app, &target, false);
    assert!(
        rejected.0 < 0.1,
        "disabled particle lights left a lit receiver: {rejected:?}"
    );
    app.world_mut()
        .resource_mut::<crate::AestraParticleLightSettings>()
        .max_lights = 2;
    receiver::until(&mut app, |app| {
        app.world()
            .resource::<ParticleLightGpuStatistics>()
            .snapshot()
            .written_capacity
            == 2
    });
    capture_receiver(&mut app, &target, true);
    // Retire one root while its last async packet may still be in flight.
    app.world_mut().despawn(a);
    settle(&mut app, &[b_leaf]);
    receiver::until(&mut app, |app| {
        let frame = app.world().resource::<Evidence>().0.lock().unwrap();
        frame.stages.len() == 1
            && frame.sources.len() == 1
            && frame
                .sources
                .iter()
                .all(|s| s.root == b && s.owner == b_leaf)
    });
    assert!(app.world().get_entity(a_leaf).is_err());
    app.world_mut().despawn(b);
    receiver::until(&mut app, |app| {
        let frame = app.world().resource::<Evidence>().0.lock().unwrap();
        frame.stages.is_empty()
            && frame.sources.is_empty()
            && app
                .world()
                .resource::<ParticleLightGpuStatistics>()
                .snapshot()
                .written_capacity
                == 0
    });
    assert_eq!(
        app.world_mut()
            .query::<&MeshMaterial3d<crate::volume::VolumeMaterial>>()
            .iter(app.world())
            .count(),
        0
    );
    let dark = capture_receiver(&mut app, &target, false);
    assert!(
        dark.0 < 0.1,
        "teardown left illumination or volume draws: {dark:?}"
    );
    drop(app); // Join the real render thread before the test-only validation wait.
    device
        .wgpu_device()
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    assert!(pollster::block_on(scope.pop()).is_none());
}
