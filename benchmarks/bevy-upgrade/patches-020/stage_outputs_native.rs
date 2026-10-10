//! Real fluid force producers, asynchronous executor callbacks and host delivery.
use crate::{
    PresentedEffect,
    execution::{StageExecutor, StageInputs, StageTimeline},
    output_context::AestraOutputEvent,
    stage_output_delivery::{
        AestraEffectOutputs, StageOutputIdentity, StageOutputMailbox, StageOutputStamp,
        encode_stage_outputs, raise_finished_events, receive_stage_outputs,
    },
};
use aestra_core::{EffectAsset, ModuleParameters, ModuleTypeId, StageKind, Value};
use aestra_runtime::{CompiledEffect, PlaybackHistoryPolicy};
use bevy::{
    prelude::*,
    render::{
        Extract, RenderApp, RenderPlugin,
        renderer::{
            RenderAdapterInfo, RenderContext, RenderDevice, RenderGraph, RenderGraphSystems,
            RenderQueue,
        },
    },
};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Resource, Clone, Default)]
struct Requests(BTreeMap<Entity, u32>);
#[derive(Resource, Clone, Default)]
struct Identities(BTreeMap<Entity, StageOutputIdentity>);
#[derive(Resource, Default)]
struct HoldDelivery(bool);
fn delivery_enabled(hold: Res<HoldDelivery>) -> bool {
    !hold.0
}
struct Runtime {
    owner: Entity,
    stages: Vec<StageTimeline>,
    read_ticks: Vec<Option<StageOutputStamp>>,
    submissions: u32,
}
#[derive(Resource, Default)]
struct Runtimes(Vec<Runtime>);
#[derive(Resource, Default)]
struct Observed {
    changes: BTreeMap<Entity, u32>,
    events: Vec<AestraOutputEvent>,
}
fn extract(
    mut commands: Commands,
    requests: Extract<Res<Requests>>,
    effects: Extract<Query<(Entity, &PresentedEffect)>>,
) {
    commands.insert_resource(requests.clone());
    commands.insert_resource(Identities(
        effects
            .iter()
            .map(|(owner, effect)| (owner, StageOutputIdentity::of_presented(effect)))
            .collect(),
    ));
}
fn collect(
    outputs: Query<Entity, Changed<AestraEffectOutputs>>,
    mut events: MessageReader<AestraOutputEvent>,
    mut observed: ResMut<Observed>,
) {
    for owner in &outputs {
        *observed.changes.entry(owner).or_default() += 1;
    }
    observed.events.extend(events.read().cloned());
}
fn simulate(
    mut context: RenderContext,
    device: Res<RenderDevice>,
    requests: Res<Requests>,
    identities: Res<Identities>,
    mut runtimes: ResMut<Runtimes>,
    mailbox: Res<StageOutputMailbox>,
) {
    for runtime in &mut runtimes.0 {
        let Some(&target) = requests.0.get(&runtime.owner) else {
            continue;
        };
        for (index, stage) in runtime.stages.iter_mut().enumerate() {
            let report = stage
                .advance_to(
                    device.wgpu_device(),
                    context.command_encoder(),
                    target,
                    20,
                    StageInputs::default(),
                    None,
                )
                .unwrap();
            assert!(report.ticks <= 20);
            assert_eq!(
                stage.checkpoint_bytes(),
                0,
                "playback-only must not retain replay snapshots"
            );
            let tick = stage.last_tick();
            let stamp = StageOutputStamp {
                owner: runtime.owner,
                stage: index,
                tick: u64::from(tick),
                identity: identities.0[&runtime.owner].clone(),
            };
            if runtime.read_ticks[index].as_ref() == Some(&stamp) {
                continue;
            }
            if encode_stage_outputs(
                stage.executor(),
                device.wgpu_device(),
                context.command_encoder(),
                &mailbox,
                stamp.clone(),
            ) {
                runtime.read_ticks[index] = Some(stamp);
                runtime.submissions += 1;
            }
        }
    }
}
fn set(asset: &mut EffectAsset, kind: &str, name: &str, value: Value) {
    let module = asset.simulation_stages[0]
        .modules
        .iter_mut()
        .find(|m| m.module_type.0 == kind)
        .unwrap();
    let ModuleParameters::Custom(values) = &mut module.parameters else {
        unreachable!()
    };
    values.insert(name.into(), value);
}
pub(super) fn plate(
    registry: &aestra_compiler::ExtensionRegistry,
    rate: f32,
    second_stage: bool,
    collider: bool,
) -> Arc<CompiledEffect> {
    Arc::new(
        aestra_compiler::EffectCompiler::with_extensions(registry.clone())
            .compile(&plate_asset(registry, rate, second_stage, collider))
            .unwrap(),
    )
}
pub(super) fn plate_asset(
    registry: &aestra_compiler::ExtensionRegistry,
    rate: f32,
    second_stage: bool,
    collider: bool,
) -> EffectAsset {
    let mut asset = aestra_fluid::smoke_effect(registry);
    for (name, value) in [
        ("resolution", Value::U32(16)),
        ("cell_size", Value::Scalar(0.2)),
        ("center", Value::Vec3([0., 1.6, 0.])),
        ("pressure_iterations", Value::U32(24)),
    ] {
        set(&mut asset, aestra_fluid::MODULE_GRID, name, value);
    }
    for (name, value) in [
        ("position", Value::Vec3([0., 0.5, 0.])),
        ("radius", Value::Scalar(0.4)),
        (
            "velocity",
            Value::Vec3([0., if rate > 0. { 1.5 } else { 0. }, 0.]),
        ),
        ("density_rate", Value::Scalar(rate)),
    ] {
        set(&mut asset, aestra_fluid::MODULE_DENSITY_SOURCE, name, value);
    }
    set(
        &mut asset,
        aestra_fluid::MODULE_BUOYANCY,
        "strength",
        Value::Scalar(1.5),
    );
    if collider {
        let mut plate = registry
            .modules
            .instantiate(&ModuleTypeId::new(aestra_fluid::MODULE_BOX_COLLIDER))
            .unwrap();
        plate.stage = StageKind::Simulation(asset.simulation_stages[0].name.clone());
        let ModuleParameters::Custom(values) = &mut plate.parameters else {
            unreachable!()
        };
        values.insert("position".into(), Value::Vec3([0., 1.6, 0.]));
        values.insert("half_extents".into(), Value::Vec3([0.8, 0.2, 0.8]));
        values.insert("impact_threshold".into(), Value::Scalar(0.00001));
        asset.simulation_stages[0].modules.push(plate);
    }
    if second_stage {
        let mut second = asset.simulation_stages[0].clone();
        second.name = "Second plume".into();
        second.id = aestra_core::StageId::for_name(&second.name);
        for module in &mut second.modules {
            module.id = aestra_core::ModuleId::new();
            module.stage = StageKind::Simulation(second.name.clone());
        }
        asset.simulation_stages.push(second);
    }
    asset
}
fn settle(app: &mut App, owners: &[Entity], before: &[u32]) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    for _ in 0..2000 {
        app.update();
        if owners.iter().zip(before).all(|(owner, before)| {
            app.world()
                .resource::<Observed>()
                .changes
                .get(owner)
                .copied()
                .unwrap_or(0)
                > *before
        }) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "asynchronous stage callbacks timed out"
        );
        std::thread::yield_now();
    }
    panic!("asynchronous stage callbacks did not converge");
}

#[test]
#[ignore = "Requires a native Vulkan GPU; run explicitly and serially"]
fn native_020_stage_outputs_reach_host_values_and_impact_messages() {
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(bevy::window::WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                ..default()
            })
            .set(RenderPlugin {
                synchronous_pipeline_compilation: true,
                ..default()
            }),
    );
    let mailbox = StageOutputMailbox::default();
    app.insert_resource(mailbox.clone())
        .init_resource::<Requests>()
        .init_resource::<Observed>()
        .init_resource::<HoldDelivery>()
        .add_message::<AestraOutputEvent>()
        .add_systems(PreUpdate, receive_stage_outputs.run_if(delivery_enabled))
        .add_systems(PostUpdate, raise_finished_events)
        .add_systems(Last, collect);
    let render = app.sub_app_mut(RenderApp);
    render
        .insert_resource(mailbox.clone())
        .init_resource::<Runtimes>()
        .init_resource::<Requests>();
    render.add_systems(bevy::render::ExtractSchedule, extract);
    render.add_systems(
        RenderGraph,
        simulate
            .after(RenderGraphSystems::Begin)
            .before(RenderGraphSystems::Render),
    );
    app.finish();
    app.cleanup();
    let device = app
        .sub_app(RenderApp)
        .world()
        .resource::<RenderDevice>()
        .clone();
    let queue = app
        .sub_app(RenderApp)
        .world()
        .resource::<RenderQueue>()
        .clone();
    let info = app
        .sub_app(RenderApp)
        .world()
        .resource::<RenderAdapterInfo>();
    assert_eq!(info.backend, wgpu::Backend::Vulkan);
    assert_ne!(info.device_type, wgpu::DeviceType::Cpu);
    println!(
        "Native stage outputs: {} / {:?} / {}",
        info.name, info.backend, info.driver_info
    );
    let scope = device
        .wgpu_device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    let mut registry = aestra_compiler::ExtensionRegistry::builtin();
    registry.install(&aestra_fluid::FluidExtension).unwrap();
    let effects = [
        plate(&registry, 5., true, true),
        plate(&registry, 0., false, true),
        plate(&registry, 5., false, false),
    ];
    let owners = effects
        .iter()
        .map(|effect| {
            app.world_mut()
                .spawn(PresentedEffect::new(effect.clone()))
                .id()
        })
        .collect::<Vec<_>>();
    let runtimes = effects
        .iter()
        .zip(&owners)
        .map(|(effect, owner)| {
            let stages = effect
                .all_extension_stages()
                .map(|stage| {
                    let executor = StageExecutor::new(
                        device.wgpu_device(),
                        &queue,
                        &stage.block,
                        &registry.programs,
                        4,
                    )
                    .unwrap();
                    let mut timeline = StageTimeline::new(executor, default(), 7);
                    timeline.set_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
                    timeline
                })
                .collect::<Vec<_>>();
            Runtime {
                owner: *owner,
                read_ticks: vec![None; stages.len()],
                stages,
                submissions: 0,
            }
        })
        .collect();
    app.sub_app_mut(RenderApp)
        .world_mut()
        .insert_resource(Runtimes(runtimes));
    // Clock/lowered inputs are still fixture-controlled; the production callback
    // and main-world receiver run unchanged in their actual schedule phases.
    for target in (20..=120).step_by(20) {
        let before = owners[..2]
            .iter()
            .map(|owner| {
                app.world()
                    .resource::<Observed>()
                    .changes
                    .get(owner)
                    .copied()
                    .unwrap_or(0)
            })
            .collect::<Vec<_>>();
        for owner in &owners {
            app.world_mut()
                .resource_mut::<Requests>()
                .0
                .insert(*owner, target);
        }
        settle(&mut app, &owners[..2], &before);
        // A second stage may complete one update later than its sibling.
        for _ in 0..16 {
            app.update();
        }
        let outputs = app.world().get::<AestraEffectOutputs>(owners[0]).unwrap();
        assert_eq!(
            outputs.values().len(),
            2,
            "first sibling callbacks must merge, not overwrite"
        );
        assert!(
            outputs
                .values()
                .iter()
                .all(|v| v.value.iter().all(|v| v.is_finite()))
        );
        for value in outputs.values() {
            assert_eq!(
                outputs.get(aestra_fluid::OUTPUT_FORCE, value.source.unwrap()),
                Some(value.value.as_slice())
            );
        }
    }
    let outputs = app.world().get::<AestraEffectOutputs>(owners[0]).unwrap();
    assert!(
        outputs.values().iter().all(|v| v.value[1] > 0.),
        "the real plume pushes both plates upward"
    );
    let still = app.world().get::<AestraEffectOutputs>(owners[1]).unwrap();
    assert_eq!(still.values().len(), 1);
    assert!(still.values()[0].magnitude() < 0.01 * outputs.values()[0].value[1]);
    assert!(
        app.world().get::<AestraEffectOutputs>(owners[2]).is_none(),
        "an output-less stage creates no host values"
    );
    let impacts = app
        .world()
        .resource::<Observed>()
        .events
        .iter()
        .filter(|event| event.event.kind == aestra_fluid::EVENT_IMPACT)
        .collect::<Vec<_>>();
    assert!(impacts.len() >= 2);
    for output in impacts {
        assert_eq!(output.effect, owners[0]);
        assert!(output.clip_path.is_empty() && output.playback_epoch.is_none());
        assert_eq!(output.event.output, aestra_fluid::OUTPUT_FORCE);
        assert!(matches!(
            output.event.origin,
            aestra_runtime::EventOrigin::Stage(0 | 1)
        ));
        assert!(output.event.source.is_some());
        assert!(
            output.event.tick > 0
                && output.event.tick <= 120
                && output.event.tick.is_multiple_of(20)
        );
        assert!(output.event.magnitude > 0.00001);
    }
    let events = app.world().resource::<Observed>().events.clone();
    let changes = app.world().resource::<Observed>().changes.clone();
    for _ in 0..24 {
        app.update();
    }
    assert_eq!(app.world().resource::<Observed>().events, events);
    assert_eq!(
        app.world().resource::<Observed>().changes,
        changes,
        "paused stages do not submit duplicate reads"
    );
    let render = app.sub_app(RenderApp).world();
    let runtimes = &render.resource::<Runtimes>().0;
    assert_eq!(runtimes[0].submissions, 12);
    assert_eq!(runtimes[1].submissions, 6);
    assert_eq!(runtimes[2].submissions, 0);
    // After asynchronous delivery only: a test-only oracle checks copy-and-clear.
    for runtime in runtimes {
        for stage in &runtime.stages {
            if stage.executor().block().outputs.is_empty() {
                continue;
            }
            let words = stage
                .executor()
                .read_resource(device.wgpu_device(), &queue, aestra_fluid::RESOURCE_OUTPUTS)
                .unwrap();
            assert!(
                words.iter().all(|word| *word == 0),
                "the producer clears outputs after copying"
            );
        }
    }
    // Hold only host delivery, not the actual GPU callbacks. Wait through normal
    // app updates until both real callbacks are queued, then change context.
    // The fixture controls clocks/runtimes: this proves delivery identity and
    // read gating, not production stage reset/reconstruction scheduling.
    for replacement in [false, true] {
        let target = if replacement { 180 } else { 140 };
        let events_before = app.world().resource::<Observed>().events.clone();
        app.world_mut().resource_mut::<HoldDelivery>().0 = true;
        app.world_mut()
            .resource_mut::<Requests>()
            .0
            .insert(owners[0], target);
        app.update();
        app.world_mut()
            .resource_mut::<Requests>()
            .0
            .remove(&owners[0]);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while mailbox.pending_reads() < 2 {
            app.update();
            assert!(
                std::time::Instant::now() < deadline,
                "real stale callbacks did not arrive"
            );
            std::thread::yield_now();
        }
        {
            let mut presented = app
                .world_mut()
                .get_mut::<PresentedEffect>(owners[0])
                .unwrap();
            if replacement {
                *presented = PresentedEffect::new(presented.effect().clone());
            } else {
                presented.instance.restart();
            }
        }
        app.world_mut().resource_mut::<HoldDelivery>().0 = false;
        app.update();
        assert_eq!(mailbox.pending_reads(), 0);
        assert!(
            app.world()
                .get::<AestraEffectOutputs>(owners[0])
                .unwrap()
                .values()
                .is_empty()
        );
        assert_eq!(app.world().resource::<Observed>().events, events_before);
        let before = app.world().resource::<Observed>().changes[&owners[0]];
        let submissions = app.sub_app(RenderApp).world().resource::<Runtimes>().0[0].submissions;
        // Same simulation tick, new context: a bare tick-only read gate would
        // suppress these fresh callbacks forever while paused.
        app.world_mut()
            .resource_mut::<Requests>()
            .0
            .insert(owners[0], target);
        settle(&mut app, &owners[..1], &[before]);
        for _ in 0..16 {
            app.update();
        }
        assert_eq!(
            app.world()
                .get::<AestraEffectOutputs>(owners[0])
                .unwrap()
                .values()
                .len(),
            2
        );
        assert_eq!(
            app.sub_app(RenderApp).world().resource::<Runtimes>().0[0].submissions,
            submissions + 2
        );
        assert_eq!(
            app.world().resource::<Observed>().events,
            events_before,
            "cleared outputs do not invent impacts"
        );
        let before = app.world().resource::<Observed>().changes[&owners[0]];
        app.world_mut()
            .resource_mut::<Requests>()
            .0
            .insert(owners[0], target + 20);
        settle(&mut app, &owners[..1], &[before]);
        for _ in 0..16 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<Observed>().events.len(),
            events_before.len() + 2,
            "fresh context may raise new impacts"
        );
    }
    let events = app.world().resource::<Observed>().events.clone();
    // A genuinely in-flight completion for a removed owner must be ignored.
    app.world_mut()
        .resource_mut::<Requests>()
        .0
        .insert(owners[0], 220);
    app.update();
    app.world_mut().entity_mut(owners[0]).despawn();
    app.world_mut()
        .resource_mut::<Requests>()
        .0
        .remove(&owners[0]);
    for _ in 0..64 {
        app.update();
    }
    assert_eq!(app.world().resource::<Observed>().events, events);
    assert!(app.world().get_entity(owners[0]).is_err());
    device
        .wgpu_device()
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(60)),
        })
        .unwrap();
    app.update();
    assert_eq!(app.world().resource::<Observed>().events, events);
    assert!(pollster::block_on(scope.pop()).is_none());
    println!(
        "stage_outputs actual_solver=true stages=2 owners=3 actual_async_callback=true actual_main_world_receiver=true force_and_impact=true batch_merge=true copy_clear=true pause_gate=true stale_restart=true stale_replacement=true context_read_gate=true fresh_context_impacts=true removed_owner=true playback_only=true full_host_scheduler_qualified=false"
    );
}
