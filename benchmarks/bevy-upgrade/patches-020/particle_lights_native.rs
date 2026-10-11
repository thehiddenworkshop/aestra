//! Production light installer and outer graph, without a substitute selector.
//! Buffer reads and device waits are test-only; no host light realization is installed.
use crate::{
    capabilities::{ActiveBackend, EffectRuntimeStatus},
    effect_inputs::GpuEffectBuffers,
    particle_lights::*,
    presentation_readiness::PresentedOwners,
    stateful_simulation::{StatefulSimulationPipeline, StatefulStates},
};
use aestra_gpu::{GpuEffectArtifact, GpuRenderGlobals};
use bevy::{
    asset::io::embedded::EmbeddedAssetRegistry,
    prelude::*,
    render::{
        Render, RenderApp, RenderPlugin, RenderStartup, RenderSystems, render_asset::RenderAssets,
        renderer::RenderAdapterInfo, storage::GpuShaderBuffer,
    },
};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

fn app() -> App {
    app_inner(false)
}
pub(super) fn host_app() -> App {
    app_inner(true)
}
fn app_inner(host: bool) -> App {
    let mut app = App::new();
    let plugins = DefaultPlugins
        .set(bevy::window::WindowPlugin {
            primary_window: None,
            exit_condition: bevy::window::ExitCondition::DontExit,
            ..default()
        })
        .set(RenderPlugin {
            render_creation: bevy::render::settings::RenderCreation::Automatic(Box::new(
                bevy::render::settings::WgpuSettings {
                    backends: Some(wgpu::Backends::VULKAN),
                    features: wgpu::Features::TIMESTAMP_QUERY,
                    ..default()
                },
            )),
            synchronous_pipeline_compilation: true,
            ..default()
        });
    #[cfg(feature = "async-qualification")]
    let plugins = plugins.disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>();
    app.add_plugins(plugins);
    crate::extraction::install(&mut app);
    if host {
        aestra_fluid::link();
        crate::outer_graph_native::install_with_stage_installer(&mut app);
        crate::extension_stages::install(&mut app);
        crate::volume::install(&mut app);
    } else {
        crate::outer_graph_native::install(&mut app);
    }
    crate::particle_lights::install(&mut app);
    if host {
        app.add_plugins(crate::particle_light_realization::AestraParticleLightPlugin);
    }
    app.init_resource::<crate::AestraRenderSettings>()
        .insert_resource(crate::catchup_pacing::AestraCatchupPacing { paced: false });
    app.sub_app_mut(RenderApp)
        .init_resource::<StatefulStates>()
        .add_systems(
            RenderStartup,
            crate::stateful_simulation::init_stateful_pipeline,
        )
        .add_systems(
            Render,
            crate::stateful_simulation::prepare_stateful_states
                .in_set(RenderSystems::PrepareBindGroups),
        );
    let path = "aestra_bevy_render/shaders/aestra_stateful_simulation.wgsl";
    app.world()
        .resource::<EmbeddedAssetRegistry>()
        .insert_asset(
            std::path::PathBuf::from(path),
            std::path::Path::new(path),
            aestra_gpu::stateful_simulation_wgsl().into_bytes(),
        );
    app.finish();
    app.cleanup();
    let info = app
        .sub_app(RenderApp)
        .world()
        .resource::<RenderAdapterInfo>();
    assert_ne!(info.device_type, wgpu::DeviceType::Cpu, "hardware required");
    println!(
        "Native production light selection: {} / {:?} / {}",
        info.name, info.backend, info.driver_info
    );
    app
}

fn instance(stateful: bool, seed: u64) -> aestra_runtime::EffectInstance {
    use aestra_core::*;
    let mut source = EffectAsset::new("Mixed particle lights", 10.);
    source.playback_mode = EffectPlaybackMode::Once;
    let mut emitter = Emitter::basic_sprite("Light source", 10.);
    emitter.max_particles = if stateful { 257 } else { 129 };
    emitter.modules[0] = ModuleInstance::emission(120., 0);
    emitter.modules[1] = ModuleInstance::shape(EmitterShape::Sphere { radius: 0.2 });
    emitter.modules[2] = ModuleInstance::initialize(
        ScalarRange::new(1., 2.),
        ScalarRange::new(2., 3.),
        [0., 0., 1.],
        30.,
        ScalarRange::new(0., 0.),
    );
    emitter.modules[3] = ModuleInstance::motion([0., -0.4, 0.], 0.1, 0.02);
    if stateful {
        emitter.modules.push(ModuleInstance::persistent());
    }
    let mut light = ParticlePointLightProperties::new(1000., 12.);
    light.intensity_curve = Curve::new(vec![CurveKey::new(0., 1000.)]);
    light.max_lights_by_quality.insert("high".into(), 3);
    emitter
        .scene_outputs
        .push(SceneOutputInstance::particle_point_light(light));
    source.emitters.push(emitter);
    aestra_runtime::EffectInstance::with_seed(
        Arc::new(
            aestra_compiler::EffectCompiler::default()
                .compile(&source)
                .unwrap(),
        ),
        seed,
    )
}

pub(super) fn spawn(app: &mut App, stateful: bool, seed: u64, world: Mat4) -> Entity {
    let instance = instance(stateful, seed);
    let mut player = crate::PresentedEffect::new(instance.effect().clone())
        .with_history_policy(aestra_runtime::PlaybackHistoryPolicy::PlaybackOnly);
    player.instance.set_seed(seed);
    let artifact = GpuEffectArtifact::from_instance(&instance).unwrap();
    let pools = Pools(
        artifact
            .emitters
            .iter()
            .map(|e| (e.slot_offset, e.max_particles))
            .collect(),
    );
    let effect = if stateful {
        crate::stateful_native::inputs(app.world_mut(), &instance)
    } else {
        crate::simulation_native::inputs(app.world_mut(), &instance, 0.)
    };
    let owner = app
        .world_mut()
        .spawn((
            player,
            effect,
            pools,
            EffectRuntimeStatus {
                active: ActiveBackend::Gpu,
                reason: "native qualification".into(),
                compatibility: crate::CompatibilityReport::compatible(
                    crate::CompatibilityTarget::NativeGpu,
                ),
            },
        ))
        .id();
    advance(app, owner, 0.5001, world);
    owner
}

pub(super) fn advance(app: &mut App, owner: Entity, time: f32, world: Mat4) {
    let seed = {
        let mut player = app
            .world_mut()
            .get_mut::<crate::PresentedEffect>(owner)
            .unwrap();
        player.instance.set_playback_time(time);
        player.instance.seed()
    };
    let mut effect = app.world_mut().get_mut::<GpuEffectBuffers>(owner).unwrap();
    effect.simulation_time = time;
    let effect = effect.clone();
    crate::queue_native::update(
        app.world_mut(),
        &effect.globals,
        aestra_gpu::GpuGlobals {
            time,
            total_slots: effect.total_slots,
            seed: aestra_gpu::fold_seed(seed),
            emitter_count: 1,
            duration: 10.,
            ..default()
        },
    );
    let mut globals = GpuRenderGlobals {
        time,
        seed: aestra_gpu::fold_seed(seed),
        ..default()
    };
    AsMut::<[f32; 16]>::as_mut(&mut globals.world_from_effect)
        .copy_from_slice(&world.to_cols_array());
    crate::queue_native::update(app.world_mut(), &effect.render_globals, globals);
}

pub(super) fn snapshot(app: &App) -> Option<(SelectedGpuParticleLights, Vec<u32>, Vec<u32>)> {
    let state = app
        .sub_app(RenderApp)
        .world()
        .resource::<GpuSelectedParticleLights>();
    assert!(state.rejection.is_none(), "{:?}", state.rejection);
    let frame = state.frame()?.clone();
    let records =
        crate::trail_native::read(app, &frame.records, frame.selected_capacity as usize * 12);
    let counters = crate::trail_native::read(app, &frame.counters, 4);
    Some((frame, records, counters))
}

fn settle(app: &mut App, count: u32) -> (SelectedGpuParticleLights, Vec<u32>, Vec<u32>) {
    let deadline = Instant::now() + Duration::from_secs(45);
    while Instant::now() < deadline {
        app.update();
        if let Some(snapshot) = snapshot(app)
            && snapshot.2[2] == count
        {
            return snapshot;
        }
        std::thread::yield_now();
    }
    panic!("production light selection did not converge to {count}");
}

fn verify(
    app: &App,
    frame: &SelectedGpuParticleLights,
    records: &[u32],
    counters: &[u32],
    worlds: &BTreeMap<Entity, Mat4>,
) {
    assert_eq!(frame.rejected_outputs, 0);
    assert_eq!(counters[0], counters[1], "all live candidates are valid");
    assert_eq!(counters[3], counters[1] - counters[2]);
    assert!(frame.reserved_bytes > frame.scratch_bytes);
    assert!(
        frame.reserved_bytes
            <= app
                .world()
                .resource::<AestraParticleLightSettings>()
                .max_scratch_bytes
    );
    assert!(frame.manifest.windows(2).all(|pair| pair[0] < pair[1]));
    let render = app.sub_app(RenderApp).world();
    let gpu = render.resource::<RenderAssets<GpuShaderBuffer>>();
    let mut counts = BTreeMap::<_, u32>::new();
    for record in records
        .as_chunks::<12>()
        .0
        .iter()
        .take(counters[2] as usize)
    {
        let source = &frame.manifest[record[10] as usize];
        let player = app
            .world()
            .get::<crate::PresentedEffect>(source.owner)
            .unwrap();
        assert!(source.artifact.matches(player.effect()));
        assert_eq!(source.seed, player.instance.seed());
        if let Some(context) = app.world().get::<crate::EffectOutputContext>(source.owner) {
            assert_eq!(source.root, context.root);
            assert_eq!(source.root_epoch, context.playback_epoch);
            assert_eq!(source.clip_path, context.clip_path);
        } else {
            assert_eq!(source.root, source.owner);
            assert!(source.clip_path.is_empty());
        }
        let effect = app.world().get::<GpuEffectBuffers>(source.owner).unwrap();
        let particles = crate::trail_native::read(
            app,
            &gpu.get(&effect.particles).unwrap().buffer,
            effect.total_slots as usize * 12,
        );
        let particle = particles
            .as_chunks::<12>()
            .0
            .iter()
            .find(|p| p[10] == 1 && p[11] == record[11])
            .expect("selected live source ordinal");
        let position = Vec3::new(
            f32::from_bits(particle[4]),
            f32::from_bits(particle[5]),
            f32::from_bits(particle[6]),
        );
        let expected = worlds[&source.owner].transform_point3(position);
        let actual = Vec3::new(
            f32::from_bits(record[0]),
            f32::from_bits(record[1]),
            f32::from_bits(record[2]),
        );
        assert!(
            actual.abs_diff_eq(expected, 0.002),
            "{actual:?} != {expected:?}"
        );
        assert_eq!(record[3], 12.0f32.to_bits());
        assert_eq!(record[7], 1000.0f32.to_bits());
        *counts.entry(source.owner).or_default() += 1;
    }
    assert!(
        counts.values().all(|count| *count <= 3),
        "authored per-output cap"
    );
    assert!(
        records[counters[2] as usize * 12..]
            .iter()
            .all(|word| *word == 0)
    );
    for states in render.resource::<StatefulStates>().0.values() {
        assert!(states.iter().all(|state| state.checkpoints.is_empty()));
    }
}

#[test]
#[ignore = "Requires a native Vulkan GPU; run explicitly and serially"]
fn native_020_production_light_installer_selects_mixed_playback_and_retires_stale_sources() {
    let mut app = app();
    let analytic_world = Mat4::from_scale_rotation_translation(
        Vec3::new(2., 0.7, 1.3),
        Quat::from_rotation_y(0.4),
        Vec3::new(100., 3., -4.),
    );
    let stateful_world = Mat4::from_scale_rotation_translation(
        Vec3::new(0.8, 1.2, 1.4),
        Quat::from_rotation_x(-0.2),
        Vec3::new(-100., 2., 5.),
    );
    let analytic = spawn(&mut app, false, 41, analytic_world);
    let stateful = spawn(&mut app, true, 42, stateful_world);
    let root = app.world_mut().spawn_empty().id();
    app.world_mut()
        .entity_mut(stateful)
        .insert(crate::EffectOutputContext {
            root,
            clip_path: vec![
                aestra_core::EffectClipId::new(),
                aestra_core::EffectClipId::new(),
            ],
            playback_epoch: 7,
        });
    let mut worlds = BTreeMap::from([(analytic, analytic_world), (stateful, stateful_world)]);
    for _ in 0..4 {
        app.update();
    }
    assert!(snapshot(&app).is_none(), "disabled by default");
    app.world_mut()
        .resource_mut::<AestraParticleLightSettings>()
        .max_lights = 5;
    let (first, records, counters) = settle(&mut app, 5);
    assert_eq!(first.manifest.len(), 2);
    verify(&app, &first, &records, &counters, &worlds);
    let sources: std::collections::BTreeSet<_> = records
        .as_chunks::<12>()
        .0
        .iter()
        .take(5)
        .map(|record| first.manifest[record[10] as usize].owner)
        .collect();
    assert_eq!(
        sources.len(),
        2,
        "both analytic and independent stateful are eligible"
    );
    let (paused, paused_records, paused_counters) = settle(&mut app, 5);
    assert!(paused.sequence > first.sequence);
    assert_eq!(records, paused_records);
    assert_eq!(counters, paused_counters);

    // Withhold the real stateful pipelines: stale particles remain in storage,
    // but only this frame's actually presented analytic source may select lights.
    let pipeline = app
        .sub_app_mut(RenderApp)
        .world_mut()
        .remove_resource::<StatefulSimulationPipeline>()
        .unwrap();
    for _ in 0..4 {
        let (frame, records, counters) = settle(&mut app, 3);
        verify(&app, &frame, &records, &counters, &worlds);
        assert!(
            records
                .as_chunks::<12>()
                .0
                .iter()
                .take(3)
                .all(|r| frame.manifest[r[10] as usize].owner == analytic)
        );
        assert_eq!(
            app.sub_app(RenderApp)
                .world()
                .resource::<PresentedOwners>()
                .owners
                .len(),
            1
        );
    }
    app.sub_app_mut(RenderApp)
        .world_mut()
        .insert_resource(pipeline);
    let (_, recovered, _) = settle(&mut app, 5);
    assert_eq!(records, recovered, "exact paused recovery");

    // Change time and the same presentation transform buffer. Reuse allocations
    // while checking actual selected records, not a seeded selection buffer.
    let moved_world = Mat4::from_translation(Vec3::new(-40., 12., 6.)) * stateful_world;
    advance(&mut app, analytic, 0.7501, analytic_world);
    advance(&mut app, stateful, 0.7501, moved_world);
    worlds.insert(stateful, moved_world);
    let (moved, moved_records, moved_counters) = settle(&mut app, 5);
    verify(&app, &moved, &moved_records, &moved_counters, &worlds);
    assert_eq!(
        first.records.id(),
        moved.records.id(),
        "reuse global selection storage"
    );
    assert_ne!(records, moved_records);

    app.world_mut()
        .get_mut::<crate::EffectOutputContext>(stateful)
        .unwrap()
        .playback_epoch = 8;
    let (rebound, rebound_records, rebound_counters) = settle(&mut app, 5);
    verify(&app, &rebound, &rebound_records, &rebound_counters, &worlds);
    assert!(
        first
            .manifest
            .iter()
            .any(|s| s.owner == stateful && s.root_epoch == 7)
    );
    assert!(
        rebound
            .manifest
            .iter()
            .any(|s| s.owner == stateful && s.root_epoch == 8)
    );

    // In-place artifact replacement preserves authored IDs but must retire the
    // old allocation/identity even though the entity and pool sizes are unchanged.
    let old_artifact = app
        .world()
        .get::<crate::PresentedEffect>(stateful)
        .unwrap()
        .effect()
        .clone();
    let mut replacement = crate::PresentedEffect::new(Arc::new((*old_artifact).clone()))
        .with_history_policy(aestra_runtime::PlaybackHistoryPolicy::PlaybackOnly);
    replacement.instance.set_seed(77);
    let old_particles = app
        .world()
        .get::<GpuEffectBuffers>(stateful)
        .unwrap()
        .particles
        .clone();
    let replacement_inputs = crate::stateful_native::inputs(app.world_mut(), &replacement.instance);
    assert_ne!(old_particles.id(), replacement_inputs.particles.id());
    app.world_mut()
        .entity_mut(stateful)
        .insert((replacement, replacement_inputs));
    advance(&mut app, stateful, 0.5001, moved_world);
    let (replaced, replaced_records, replaced_counters) = settle(&mut app, 5);
    verify(
        &app,
        &replaced,
        &replaced_records,
        &replaced_counters,
        &worlds,
    );
    assert!(
        replaced
            .manifest
            .iter()
            .filter(|s| s.owner == stateful)
            .all(|s| s.seed == 77 && !s.artifact.matches(&old_artifact))
    );

    app.world_mut()
        .resource_mut::<AestraParticleLightSettings>()
        .max_scratch_bytes = 1;
    for _ in 0..3 {
        app.update();
        let state = app
            .sub_app(RenderApp)
            .world()
            .resource::<GpuSelectedParticleLights>();
        assert!(state.frame().is_none());
        assert!(
            state.rejection.is_some(),
            "budget rejection must be explicit"
        );
    }
    app.world_mut()
        .resource_mut::<AestraParticleLightSettings>()
        .max_scratch_bytes = 64 * 1024 * 1024;
    settle(&mut app, 5);
    app.world_mut()
        .resource_mut::<AestraParticleLightSettings>()
        .max_lights = 0;
    for _ in 0..3 {
        app.update();
        assert!(snapshot(&app).is_none());
    }
    app.world_mut()
        .resource_mut::<AestraParticleLightSettings>()
        .max_lights = 2;
    let (limited, limited_records, limited_counters) = settle(&mut app, 2);
    verify(&app, &limited, &limited_records, &limited_counters, &worlds);
    assert_eq!(limited.selected_capacity, 2);
    app.world_mut()
        .resource_mut::<AestraParticleLightSettings>()
        .max_lights = 5;

    // Component removal retires the source even while its particle buffers live.
    app.world_mut().entity_mut(stateful).remove::<Pools>();
    let (remaining, remaining_records, remaining_counters) = settle(&mut app, 3);
    verify(
        &app,
        &remaining,
        &remaining_records,
        &remaining_counters,
        &worlds,
    );
    assert_eq!(remaining.manifest.len(), 1);
    assert_eq!(remaining.manifest[0].owner, analytic);
    app.world_mut()
        .entity_mut(stateful)
        .insert(Pools(vec![(0, 257)]));
    settle(&mut app, 5);
    app.world_mut()
        .get_mut::<EffectRuntimeStatus>(stateful)
        .unwrap()
        .active = ActiveBackend::CpuReference;
    let (remaining, _, _) = settle(&mut app, 3);
    assert_eq!(remaining.manifest.len(), 1);
    app.world_mut()
        .get_mut::<EffectRuntimeStatus>(stateful)
        .unwrap()
        .active = ActiveBackend::Gpu;
    settle(&mut app, 5);
    app.world_mut().despawn(stateful);
    let (remaining, _, _) = settle(&mut app, 3);
    assert_eq!(remaining.manifest.len(), 1);
    let replacement = spawn(&mut app, true, 99, stateful_world);
    worlds.insert(replacement, stateful_world);
    let (replaced, replaced_records, replaced_counters) = settle(&mut app, 5);
    verify(
        &app,
        &replaced,
        &replaced_records,
        &replaced_counters,
        &worlds,
    );
    assert!(replaced.manifest.iter().all(|s| s.owner != stateful));
    assert!(
        replaced
            .manifest
            .iter()
            .any(|s| s.owner == replacement && s.seed == 99)
    );
    app.world_mut().despawn(analytic);
    app.world_mut().despawn(replacement);
    app.world_mut().despawn(root);
    for _ in 0..5 {
        app.update();
        assert!(snapshot(&app).is_none());
    }
    assert!(
        app.sub_app(RenderApp)
            .world()
            .resource::<StatefulStates>()
            .0
            .is_empty()
    );
    crate::outer_graph_native::assert_timings(&app);
    println!(
        "production_light_installer=true collect_extract_prepare_select=true mixed_analytic_stateful=true global_cap=5 per_output_cap=3 same_frame_readiness=true exact_pause=true transform_refresh=true nested_root_rebind=true budget_rejection=true disable_reenable=true component_backend_retirement=true artifact_seed_replacement=true teardown=true playback_only=true test_only_readback=true host_realization_qualified=false"
    );
}
