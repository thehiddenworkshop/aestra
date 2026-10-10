//! Main-world delivery and render callback wiring for extension-stage outputs.
use super::output_context::AestraOutputEvent;
use crate::{PresentedEffect, execution::StageExecutor};
use aestra_core::ResourceTypeId;
use bevy::prelude::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

/// What the stages of an effect report to the host (fluid F11, host bindings HB9): the latest value of
/// every output (the force on each fluid collider, say), as read after the last frame that advanced
/// them. Values arrive a frame or two after the ticks that produced them.
#[derive(Component, Debug, Default, Clone)]
pub struct AestraEffectOutputs {
    values: Vec<aestra_runtime::StageOutputValue>,
    identity: Option<StageOutputIdentity>,
}

impl AestraEffectOutputs {
    /// Every output's latest value.
    pub fn values(&self) -> &[aestra_runtime::StageOutputValue] {
        &self.values
    }

    /// The latest value of the output `name` of authored module `source`.
    pub fn get(&self, name: &str, source: aestra_core::ModuleId) -> Option<&[f32]> {
        self.values
            .iter()
            .find(|value| value.name == name && value.source == Some(source))
            .map(|value| value.value.as_slice())
    }
}

/// Snapshot at submission, never reconstructed from the owner at callback time.
#[derive(Clone, Debug)]
pub(super) struct StageOutputIdentity {
    effect: Arc<aestra_runtime::CompiledEffect>,
    presentation: Arc<()>,
    seed: u32,
    epoch: u32,
    revision: u64,
    host_epoch: u64,
}

impl PartialEq for StageOutputIdentity {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.effect, &other.effect)
            && Arc::ptr_eq(&self.presentation, &other.presentation)
            && self.seed == other.seed
            && self.epoch == other.epoch
            && self.revision == other.revision
            && self.host_epoch == other.host_epoch
    }
}

impl StageOutputIdentity {
    pub(super) fn of_presented(presented: &PresentedEffect) -> Self {
        let instance = &presented.instance;
        Self {
            effect: instance.effect().clone(),
            presentation: presented.output_identity.clone(),
            seed: instance.seed() as u32,
            epoch: instance.history_epoch(),
            revision: instance.history_revision(),
            host_epoch: instance.host_input_epoch(),
        }
    }

    pub(super) fn of_extracted(extracted: &super::stage_inputs::ExtractedStages) -> Self {
        Self {
            effect: extracted.effect.clone(),
            presentation: extracted.output_identity.clone(),
            seed: extracted.seed,
            epoch: extracted.history_epoch,
            revision: extracted.history_revision,
            host_epoch: extracted.host_epoch,
        }
    }
}

/// Also used as the render-side read gate: a new context may read the same tick.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct StageOutputStamp {
    pub(super) owner: Entity,
    pub(super) stage: usize,
    pub(super) tick: u64,
    pub(super) identity: StageOutputIdentity,
}

/// Where the render world leaves context-stamped output readbacks for the main world.
#[derive(Resource, Default, Clone)]
pub(super) struct StageOutputMailbox(Arc<Mutex<Vec<OutputRead>>>);

#[cfg(test)]
impl StageOutputMailbox {
    pub(super) fn pending_reads(&self) -> usize {
        self.0.lock().unwrap().len()
    }
}

type OutputRead = (StageOutputStamp, BTreeMap<ResourceTypeId, Vec<u32>>);

/// The events an effect's outputs have raised so far, to raise each one once.
#[derive(Component, Default)]
pub(super) struct OutputEvents {
    tracker: aestra_runtime::OutputEventTracker,
    identity: Option<StageOutputIdentity>,
    ticks: BTreeMap<usize, u64>,
}

impl OutputEvents {
    #[cfg(test)]
    pub(super) fn accepted_tick(&self, stage: usize) -> Option<u64> {
        self.ticks.get(&stage).copied()
    }

    fn synchronize(&mut self, identity: &StageOutputIdentity) {
        if self.identity.as_ref() != Some(identity) {
            *self = Self {
                identity: Some(identity.clone()),
                ..default()
            };
        }
    }
}

impl AestraEffectOutputs {
    fn synchronize(&mut self, identity: &StageOutputIdentity) {
        if self.identity.as_ref() != Some(identity) {
            self.values.clear();
            self.identity = Some(identity.clone());
        }
    }
}

pub(super) fn receive_stage_outputs(
    mailbox: Res<StageOutputMailbox>,
    mut commands: Commands,
    mut effects: Query<(
        &PresentedEffect,
        Option<&mut AestraEffectOutputs>,
        Option<&mut OutputEvents>,
    )>,
    mut events: MessageWriter<AestraOutputEvent>,
) {
    let mut reads = match mailbox.0.lock() {
        Ok(mut reads) => std::mem::take(&mut *reads),
        Err(_) => return,
    };
    // Invalidate visible values even if no fresh callback has arrived yet. Avoid
    // marking components changed on every idle frame.
    for (presented, outputs, tracker) in &mut effects {
        let identity = StageOutputIdentity::of_presented(presented);
        if let Some(mut outputs) = outputs
            && outputs.identity.as_ref() != Some(&identity)
        {
            outputs.synchronize(&identity);
        }
        if let Some(mut tracker) = tracker
            && tracker.identity.as_ref() != Some(&identity)
        {
            tracker.synchronize(&identity);
        }
    }
    // Within this drain retain chronological edges; across drains the per-stage
    // high-water mark rejects callbacks that have already been superseded.
    reads.sort_by_key(|(stamp, _)| stamp.tick);
    // Commands are deferred: several first completions for one owner may arrive
    // in this drain before either component exists. Share their pending state.
    let mut pending_outputs = BTreeMap::<Entity, AestraEffectOutputs>::new();
    let mut pending_trackers = BTreeMap::<Entity, OutputEvents>::new();
    for (stamp, words) in reads {
        let StageOutputStamp {
            owner: entity,
            stage,
            tick,
            identity,
        } = stamp;
        let Ok((presented, outputs, tracker)) = effects.get_mut(entity) else {
            continue;
        };
        if identity != StageOutputIdentity::of_presented(presented) {
            continue;
        }
        let Some(block) = presented
            .effect()
            .all_extension_stages()
            .nth(stage)
            .map(|stage| &stage.block)
        else {
            continue;
        };
        let values = aestra_runtime::read_stage_outputs(stage, block, &words);
        let mut tracker = tracker;
        let tracker = match tracker.as_deref_mut() {
            Some(tracker) => tracker,
            None => pending_trackers.entry(entity).or_default(),
        };
        tracker.synchronize(&identity);
        if tracker.ticks.get(&stage).is_some_and(|last| tick <= *last) {
            continue;
        }
        let previous = tracker.ticks.insert(stage, tick);
        // Aggregate stage outputs have no per-tick event records. Suppress the
        // reconstruction and the first read straddling its boundary; only a
        // wholly live interval may arm gameplay edges. Latest values still update.
        let boundary = aestra_runtime::trace_tick(presented.instance.history_epoch_start_time());
        let raised = if tick > boundary
            && (boundary == 0 || previous.is_some_and(|last| last >= boundary))
        {
            tracker.tracker.observe(&values, tick)
        } else {
            Vec::new()
        };
        events.write_batch(
            raised
                .into_iter()
                .map(|event| AestraOutputEvent::root(entity, event)),
        );
        match outputs {
            Some(mut outputs) => {
                outputs.synchronize(&identity);
                outputs.values.retain(|value| value.stage != stage);
                outputs.values.extend(values);
            }
            None => {
                let outputs = pending_outputs.entry(entity).or_default();
                outputs.synchronize(&identity);
                outputs.values.retain(|value| value.stage != stage);
                outputs.values.extend(values);
            }
        }
    }
    for (entity, outputs) in pending_outputs {
        commands.entity(entity).insert(outputs);
    }
    for (entity, tracker) in pending_trackers {
        commands.entity(entity).insert(tracker);
    }
}

/// Whether an effect's playback has finished (host bindings HB9), to raise `finished` once.
#[derive(Component, Default)]
pub(super) struct FinishedWatch(aestra_runtime::FinishedTracker);

pub(super) fn raise_finished_events(
    mut commands: Commands,
    mut effects: Query<(Entity, &PresentedEffect, Option<&mut FinishedWatch>)>,
    mut events: MessageWriter<AestraOutputEvent>,
) {
    for (entity, presented, watch) in &mut effects {
        let mut fresh = FinishedWatch::default();
        let raised = match watch {
            Some(mut watch) => watch.0.observe(&presented.instance),
            None => {
                let raised = fresh.0.observe(&presented.instance);
                commands.entity(entity).insert(fresh);
                raised
            }
        };
        if let Some(event) = raised {
            events.write(AestraOutputEvent::root(entity, event));
        }
    }
}

/// Shared render-side submission; the caller retains its per-stage context/tick gate.
pub(super) fn encode_stage_outputs(
    executor: &StageExecutor,
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    mailbox: &StageOutputMailbox,
    stamp: StageOutputStamp,
) -> bool {
    let mailbox = mailbox.0.clone();
    executor.encode_output_readback(device, encoder, move |words| {
        if let Ok(mut reads) = mailbox.lock() {
            reads.push((stamp, words));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_compiler::ExtensionRegistry;

    fn force_effect() -> Arc<aestra_runtime::CompiledEffect> {
        let mut registry = ExtensionRegistry::builtin();
        registry.install(&aestra_fluid::FluidExtension).unwrap();
        let mut asset = aestra_fluid::smoke_effect(&registry);
        let mut collider = registry
            .modules
            .instantiate(&aestra_core::ModuleTypeId::new(
                aestra_fluid::MODULE_SPHERE_COLLIDER,
            ))
            .unwrap();
        collider.stage =
            aestra_core::StageKind::Simulation(asset.simulation_stages[0].name.clone());
        let aestra_core::ModuleParameters::Custom(values) = &mut collider.parameters else {
            unreachable!()
        };
        values.insert("impact_threshold".into(), aestra_core::Value::Scalar(10.));
        asset.simulation_stages[0].modules.push(collider);
        let mut compiled = aestra_compiler::EffectCompiler::with_extensions(registry)
            .compile(&asset)
            .unwrap();
        compiled
            .extension_stages
            .push(compiled.extension_stages[0].clone());
        Arc::new(compiled)
    }

    fn enqueue(mailbox: &StageOutputMailbox, stamp: StageOutputStamp, force: f32) {
        mailbox.0.lock().unwrap().push((
            stamp,
            BTreeMap::from([(
                ResourceTypeId::new(aestra_fluid::RESOURCE_OUTPUTS),
                vec![0, force.to_bits(), 0, 0],
            )]),
        ));
    }

    fn drain(world: &mut World) -> Vec<AestraOutputEvent> {
        use bevy::ecs::{message::Messages, system::RunSystemOnce};
        world.run_system_once(receive_stage_outputs).unwrap();
        world.flush();
        world
            .resource_mut::<Messages<AestraOutputEvent>>()
            .drain()
            .collect()
    }

    #[test]
    fn extracted_identity_preserves_every_discontinuity_and_artifact_pointer() {
        let presented = PresentedEffect::new(force_effect());
        let instance = &presented.instance;
        let extracted = super::super::stage_inputs::ExtractedStages {
            effect: instance.effect().clone(),
            output_identity: presented.output_identity.clone(),
            seed: instance.seed() as u32,
            time: instance.time(),
            quality: aestra_runtime::SeekQuality::Exact,
            history_policy: instance.history_policy(),
            host: aestra_gpu::GpuHostBindings::from_instance(instance),
            history_epoch: instance.history_epoch(),
            history_epoch_start_time: instance.history_epoch_start_time(),
            history_revision: instance.history_revision(),
            host_epoch: instance.host_input_epoch(),
            world_to_effect: [[0.; 4]; 3],
            coupled: false,
            view: None,
            volumes: Vec::new(),
            world: None,
        };
        let identity = StageOutputIdentity::of_presented(&presented);
        assert_eq!(
            identity,
            StageOutputIdentity::of_presented(&presented.clone())
        );
        assert_eq!(identity, StageOutputIdentity::of_extracted(&extracted));
        for field in 0..6 {
            let mut changed = extracted.clone();
            match field {
                0 => changed.effect = Arc::new((*changed.effect).clone()),
                1 => changed.seed = changed.seed.wrapping_add(1),
                2 => changed.history_epoch = changed.history_epoch.wrapping_add(1),
                3 => changed.history_revision = changed.history_revision.wrapping_add(1),
                4 => changed.host_epoch = changed.host_epoch.wrapping_add(1),
                5 => changed.output_identity = Arc::new(()),
                _ => unreachable!(),
            }
            assert_ne!(identity, StageOutputIdentity::of_extracted(&changed));
        }
    }

    #[test]
    fn context_changes_clear_values_and_reject_old_callbacks_before_and_after_fresh_reads() {
        use bevy::ecs::message::Messages;
        for transition in 0..6 {
            let mut world = World::new();
            world.init_resource::<Messages<AestraOutputEvent>>();
            let mailbox = StageOutputMailbox::default();
            world.insert_resource(mailbox.clone());
            let owner = world.spawn(PresentedEffect::new(force_effect())).id();
            let old =
                StageOutputIdentity::of_presented(world.get::<PresentedEffect>(owner).unwrap());
            let stamp = |identity: &StageOutputIdentity, stage, tick| StageOutputStamp {
                owner,
                stage,
                tick,
                identity: identity.clone(),
            };
            enqueue(&mailbox, stamp(&old, 0, 90), 20.);
            enqueue(&mailbox, stamp(&old, 1, 90), 30.);
            assert_eq!(drain(&mut world).len(), 2);
            {
                let mut presented = world.get_mut::<PresentedEffect>(owner).unwrap();
                match transition {
                    0 => presented.instance.restart(),
                    1 => presented.instance.seek(0.5),
                    // Even a change in seed bits above the GPU's u32 invalidates history.
                    2 => {
                        let seed = presented.instance.seed();
                        presented.instance.set_seed(seed.wrapping_add(1 << 32));
                    }
                    3 => presented.instance.invalidate_history(),
                    4 => {
                        *presented = PresentedEffect::new(Arc::new((**presented.effect()).clone()))
                    }
                    5 => *presented = PresentedEffect::new(presented.effect().clone()),
                    _ => unreachable!(),
                }
            }
            assert!(drain(&mut world).is_empty());
            assert!(
                world
                    .get::<AestraEffectOutputs>(owner)
                    .unwrap()
                    .values()
                    .is_empty()
            );
            enqueue(&mailbox, stamp(&old, 0, 900), 100.);
            assert!(drain(&mut world).is_empty());
            assert!(
                world
                    .get::<AestraEffectOutputs>(owner)
                    .unwrap()
                    .values()
                    .is_empty()
            );
            let fresh =
                StageOutputIdentity::of_presented(world.get::<PresentedEffect>(owner).unwrap());
            if transition == 1 {
                enqueue(&mailbox, stamp(&fresh, 0, 30), 20.);
                assert!(
                    drain(&mut world).is_empty(),
                    "seek reconstruction is silent"
                );
            }
            let live_tick = if transition == 1 { 31 } else { 1 };
            enqueue(&mailbox, stamp(&fresh, 0, live_tick), 20.);
            assert_eq!(
                drain(&mut world).len(),
                1,
                "new context resets edges/high-water marks"
            );
            enqueue(&mailbox, stamp(&old, 1, 901), 100.);
            enqueue(&mailbox, stamp(&fresh, 0, live_tick), 0.);
            assert!(drain(&mut world).is_empty());
            let outputs = world.get::<AestraEffectOutputs>(owner).unwrap().values();
            assert_eq!(
                outputs.len(),
                1,
                "no values from sibling stages in the previous context"
            );
            assert_eq!(outputs[0].value, [0., 20., 0.]);
        }
    }

    #[test]
    fn seek_outputs_update_values_but_only_wholly_live_reads_raise_events() {
        use bevy::ecs::message::Messages;
        for first in [30, 35] {
            let mut world = World::new();
            world.init_resource::<Messages<AestraOutputEvent>>();
            let mailbox = StageOutputMailbox::default();
            world.insert_resource(mailbox.clone());
            let mut presented = PresentedEffect::new(force_effect());
            presented.instance.seek(0.5);
            let identity = StageOutputIdentity::of_presented(&presented);
            let owner = world.spawn(presented).id();
            let stamp = |tick| StageOutputStamp {
                owner,
                stage: 0,
                tick,
                identity: identity.clone(),
            };
            enqueue(&mailbox, stamp(first), 20.);
            assert!(drain(&mut world).is_empty());
            assert_eq!(
                world.get::<OutputEvents>(owner).unwrap().accepted_tick(0),
                Some(first)
            );
            assert_eq!(
                world.get::<AestraEffectOutputs>(owner).unwrap().values()[0].value,
                [0., 20., 0.]
            );
            enqueue(&mailbox, stamp(first + 1), 25.);
            assert_eq!(drain(&mut world)[0].event.tick, first + 1);
            enqueue(&mailbox, stamp(first + 2), 30.);
            assert!(drain(&mut world).is_empty());
        }
    }

    #[test]
    fn reordered_and_duplicate_reads_cannot_roll_back_values_or_rearm_edges() {
        use bevy::ecs::message::Messages;
        let mut world = World::new();
        world.init_resource::<Messages<AestraOutputEvent>>();
        let mailbox = StageOutputMailbox::default();
        world.insert_resource(mailbox.clone());
        let owner = world.spawn(PresentedEffect::new(force_effect())).id();
        let identity =
            StageOutputIdentity::of_presented(world.get::<PresentedEffect>(owner).unwrap());
        let stamp = |stage, tick| StageOutputStamp {
            owner,
            stage,
            tick,
            identity: identity.clone(),
        };
        for (tick, force) in [(12, 0.), (10, 20.), (11, 30.)] {
            enqueue(&mailbox, stamp(0, tick), force);
        }
        assert_eq!(mailbox.pending_reads(), 3);
        let raised = drain(&mut world);
        assert_eq!(raised.len(), 1);
        assert_eq!(raised[0].event.tick, 10);
        enqueue(&mailbox, stamp(0, 11), 20.);
        enqueue(&mailbox, stamp(0, 12), 20.);
        assert!(drain(&mut world).is_empty());
        assert_eq!(
            world.get::<AestraEffectOutputs>(owner).unwrap().values()[0].value,
            [0.; 3]
        );
        enqueue(&mailbox, stamp(1, 3), 20.);
        assert_eq!(
            drain(&mut world)[0].event.tick,
            3,
            "stage high-water marks are independent"
        );
        enqueue(&mailbox, stamp(0, 13), 20.);
        assert_eq!(drain(&mut world)[0].event.tick, 13);
    }

    #[test]
    fn first_completion_batches_merge_stages_and_keep_one_tracker_per_owner() {
        use bevy::ecs::{message::Messages, system::RunSystemOnce};
        let mut registry = ExtensionRegistry::builtin();
        registry.install(&aestra_fluid::FluidExtension).unwrap();
        let mut asset = aestra_fluid::smoke_effect(&registry);
        let mut collider = registry
            .modules
            .instantiate(&aestra_core::ModuleTypeId::new(
                aestra_fluid::MODULE_SPHERE_COLLIDER,
            ))
            .unwrap();
        collider.stage =
            aestra_core::StageKind::Simulation(asset.simulation_stages[0].name.clone());
        let aestra_core::ModuleParameters::Custom(values) = &mut collider.parameters else {
            unreachable!()
        };
        values.insert("impact_threshold".into(), aestra_core::Value::Scalar(10.));
        asset.simulation_stages[0].modules.push(collider);
        let mut compiled = aestra_compiler::EffectCompiler::with_extensions(registry)
            .compile(&asset)
            .unwrap();
        compiled
            .extension_stages
            .push(compiled.extension_stages[0].clone());
        let compiled = Arc::new(compiled);
        // The two optional components may also have been independently removed.
        for existing in 0..4 {
            let mut world = World::new();
            world.init_resource::<Messages<AestraOutputEvent>>();
            let mailbox = StageOutputMailbox::default();
            world.insert_resource(mailbox.clone());
            let owner = world.spawn(PresentedEffect::new(compiled.clone())).id();
            let identity =
                StageOutputIdentity::of_presented(world.get::<PresentedEffect>(owner).unwrap());
            if existing & 1 != 0 {
                world
                    .entity_mut(owner)
                    .insert(AestraEffectOutputs::default());
            }
            if existing & 2 != 0 {
                world.entity_mut(owner).insert(OutputEvents::default());
            }
            let push = |stage, tick, force: f32| {
                mailbox.0.lock().unwrap().push((
                    StageOutputStamp {
                        owner,
                        stage,
                        tick,
                        identity: identity.clone(),
                    },
                    BTreeMap::from([(
                        ResourceTypeId::new(aestra_fluid::RESOURCE_OUTPUTS),
                        vec![0, force.to_bits(), 0, 0],
                    )]),
                ));
            };
            push(0, 1, 20.);
            push(0, 2, 25.);
            push(1, 2, 30.);
            world.run_system_once(receive_stage_outputs).unwrap();
            world.flush();
            let events = world
                .resource_mut::<Messages<AestraOutputEvent>>()
                .drain()
                .collect::<Vec<_>>();
            assert_eq!(
                events.len(),
                2,
                "one rising edge per stage, existing={existing}"
            );
            let outputs = world.get::<AestraEffectOutputs>(owner).unwrap().values();
            assert_eq!(outputs.len(), 2);
            assert_eq!(
                outputs.iter().find(|v| v.stage == 0).unwrap().value,
                [0., 25., 0.]
            );
            assert_eq!(
                outputs.iter().find(|v| v.stage == 1).unwrap().value,
                [0., 30., 0.]
            );
            // Removing only the latest-values component must not reset edge tracking.
            world.entity_mut(owner).remove::<AestraEffectOutputs>();
            push(0, 3, 40.);
            push(1, 3, 50.);
            world.run_system_once(receive_stage_outputs).unwrap();
            world.flush();
            assert_eq!(world.resource::<Messages<AestraOutputEvent>>().len(), 0);
            assert_eq!(
                world
                    .get::<AestraEffectOutputs>(owner)
                    .unwrap()
                    .values()
                    .len(),
                2
            );
        }
    }
    /// Fluid F11: the render world's output reads become the effect's latest outputs, and an impact
    /// reaches gameplay as a message — once per rise past the collider's threshold.
    #[test]
    fn output_reads_update_the_effect_and_raise_impacts_once() {
        use bevy::ecs::message::Messages;
        use bevy::ecs::system::RunSystemOnce;
        let mut registry = ExtensionRegistry::builtin();
        registry.install(&aestra_fluid::FluidExtension).unwrap();
        let mut effect = aestra_fluid::fire_effect(&registry);
        let mut shield = registry
            .modules
            .instantiate(&aestra_core::ModuleTypeId::new(
                aestra_fluid::MODULE_SPHERE_COLLIDER,
            ))
            .unwrap();
        shield.stage = aestra_core::StageKind::Simulation(effect.simulation_stages[0].name.clone());
        let shield_id = shield.id;
        let aestra_core::ModuleParameters::Custom(values) = &mut shield.parameters else {
            unreachable!("plugin modules carry a generic payload");
        };
        values.insert("impact_threshold".into(), aestra_core::Value::Scalar(10.0));
        effect.simulation_stages[0].modules.push(shield);
        let compiled = aestra_compiler::EffectCompiler::with_extensions(registry)
            .compile(&effect)
            .unwrap();
        let mut world = World::new();
        world.init_resource::<Messages<AestraOutputEvent>>();
        let mailbox = StageOutputMailbox::default();
        world.insert_resource(mailbox.clone());
        let entity = world.spawn(PresentedEffect::new(Arc::new(compiled))).id();
        let identity =
            StageOutputIdentity::of_presented(world.get::<PresentedEffect>(entity).unwrap());
        let push = |tick, force: [f32; 3]| {
            let mut words = vec![0u32; 16];
            for (axis, value) in force.iter().enumerate() {
                words[axis] = value.to_bits();
            }
            mailbox.0.lock().unwrap().push((
                StageOutputStamp {
                    owner: entity,
                    stage: 0,
                    tick,
                    identity: identity.clone(),
                },
                BTreeMap::from([(ResourceTypeId::new(aestra_fluid::RESOURCE_OUTPUTS), words)]),
            ));
        };
        let mut raised = Vec::new();
        let mut run = |world: &mut World| {
            world.run_system_once(receive_stage_outputs).unwrap();
            world.flush();
            raised.extend(
                world
                    .resource_mut::<Messages<AestraOutputEvent>>()
                    .drain()
                    .collect::<Vec<_>>(),
            );
        };
        push(42, [0.0, 20.0, 0.0]);
        run(&mut world);
        assert_eq!(
            world
                .get::<AestraEffectOutputs>(entity)
                .unwrap()
                .get(aestra_fluid::OUTPUT_FORCE, shield_id),
            Some(&[0.0, 20.0, 0.0][..])
        );
        push(43, [0.0, 25.0, 0.0]);
        run(&mut world);
        push(44, [0.0, 1.0, 0.0]);
        run(&mut world);
        push(45, [30.0, 0.0, 0.0]);
        run(&mut world);
        assert_eq!(raised.len(), 2, "one impact per rise: {raised:?}");
        assert!(raised.iter().all(|message| message.effect == entity
            && message.event.kind == aestra_fluid::EVENT_IMPACT
            && message.event.source == Some(shield_id)));
        assert_eq!(
            raised
                .iter()
                .map(|message| message.event.tick)
                .collect::<Vec<_>>(),
            [42, 45]
        );
        assert_eq!(raised[1].event.value, [30.0, 0.0, 0.0]);
    }

    /// Host bindings HB9: a play-once effect raises `finished` once when its playback reaches the end,
    /// and again after a restart reaches it again; a looping one never does.
    #[test]
    fn a_play_once_effect_raises_finished_once_per_playthrough() {
        use bevy::ecs::message::Messages;
        use bevy::ecs::system::RunSystemOnce;
        let compile = |mode: aestra_core::EffectPlaybackMode| {
            let mut effect = aestra_core::EffectAsset::new("Burst", 2.0);
            effect.playback_mode = mode;
            effect
                .emitters
                .push(aestra_core::Emitter::basic_sprite("Sparks", 2.0));
            Arc::new(
                aestra_compiler::EffectCompiler::default()
                    .compile(&effect)
                    .unwrap(),
            )
        };
        let mut world = World::new();
        world.init_resource::<Messages<AestraOutputEvent>>();
        let once = world
            .spawn(PresentedEffect::new(compile(
                aestra_core::EffectPlaybackMode::Once,
            )))
            .id();
        let looping = world
            .spawn(PresentedEffect::new(compile(
                aestra_core::EffectPlaybackMode::LoopRestart,
            )))
            .id();
        let frame = |world: &mut World, time: f32| -> Vec<AestraOutputEvent> {
            for entity in [once, looping] {
                world
                    .get_mut::<PresentedEffect>(entity)
                    .unwrap()
                    .instance
                    .set_playback_time(time);
            }
            world.run_system_once(raise_finished_events).unwrap();
            world.flush();
            world
                .resource_mut::<Messages<AestraOutputEvent>>()
                .drain()
                .collect()
        };
        let mut raised = Vec::new();
        for time in [0.5, 1.9, 2.0, 2.5, 3.0] {
            raised.extend(frame(&mut world, time));
        }
        assert_eq!(raised.len(), 1, "{raised:?}");
        assert_eq!(raised[0].effect, once);
        assert_eq!(raised[0].event.kind, aestra_runtime::EVENT_FINISHED);
        assert_eq!(raised[0].event.origin, aestra_runtime::EventOrigin::Effect);
        // A restart, played through again.
        raised.extend(frame(&mut world, 0.0));
        raised.extend(frame(&mut world, 2.0));
        assert_eq!(raised.len(), 2);
    }
}
