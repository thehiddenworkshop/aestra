//! Host bindings for Bevy (host bindings HB5): the reference adapter from Bevy entities to Aestra's
//! portable binding snapshots.
//!
//! An effect declares binding slots by name (`Target`, `Source`, …). A game maps them to entities
//! with [`AestraBindings`] on the player entity; once per frame, before playback,
//! [`AestraSet::ResolveHostInputs`](crate::AestraSet::ResolveHostInputs) reads each bound entity's
//! [`GlobalTransform`] into a [`SpatialBindingSnapshot`] and pushes one [`BindingFrame`] per effect.
//! No Bevy type crosses into the runtime: entities stay here, the runtime only sees snapshots.
//!
//! Velocity is not derived: an entity supplies it by carrying [`AestraLinearVelocity`] (a physics
//! integration can keep it current). Without it the optional `linear_velocity` field is absent and
//! consumers use their authored fallback.
//!
//! Timing: Bevy propagates `GlobalTransform` in `PostUpdate`, so the resolve step in `Update` sees the
//! previous frame's world transform — one frame of latency, as for any `Update` system.
//!
//! Recorded input (host bindings HB8): [`AestraBindingRecorder`] records what a player's bindings
//! resolve to, tick by tick, into a [`BindingTrace`]; [`AestraBindingTrace`] drives a player from a
//! trace instead of from entities, so scrubbing its timeline replays exactly — the editor preview
//! and deterministic-test path. Live bindings nobody records make a stateful effect's backward
//! seek inexact, which [`crate::EffectPlayer::supports_exact_backward_seek`] reports.

use crate::{EffectClipInstance, EffectPlayer, PresentedEffect};
use aestra_core::{AESTRA_BINDING_SPATIAL, BindingKindId};
use aestra_runtime::{
    BindingFrame, BindingRecorder, BindingSlot, BindingSnapshot, BindingTrace, CompiledEffect,
    SpatialBindingSnapshot,
};
use bevy::prelude::*;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Maps an effect's binding slots (by their public name) to the entities that fill them.
#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
pub struct AestraBindings {
    by_name: BTreeMap<String, Entity>,
}

impl AestraBindings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder form: binds `name` to `entity`.
    pub fn bind(mut self, name: impl Into<String>, entity: Entity) -> Self {
        self.set(name, entity);
        self
    }

    pub fn set(&mut self, name: impl Into<String>, entity: Entity) {
        self.by_name.insert(name.into(), entity);
    }

    pub fn unbind(&mut self, name: &str) -> Option<Entity> {
        self.by_name.remove(name)
    }

    pub fn get(&self, name: &str) -> Option<Entity> {
        self.by_name.get(name).copied()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, Entity)> {
        self.by_name
            .iter()
            .map(|(name, entity)| (name.as_str(), *entity))
    }
}

/// World-space linear velocity (units per second) supplied for a bound entity, e.g. by a physics
/// integration. Aestra does not depend on any physics crate; hosts write this component.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
pub struct AestraLinearVelocity(pub Vec3);

/// Drives a player's bindings from recorded input instead of from entities (host bindings HB8):
/// every time the player is put at — playing, seeking, restarting — reads the trace's frame for that
/// tick, and its [`AestraBindings`] are ignored meanwhile. Remove the component to return the player
/// to its entities.
#[derive(Component, Debug, Clone)]
pub struct AestraBindingTrace(pub Arc<BindingTrace>);

/// Records what a player's [`AestraBindings`] resolve to, tick by tick (host bindings HB8). Seeking
/// back and playing on records a new history from there. [`Self::trace`] is the recording so far,
/// ready for [`AestraBindingTrace`].
#[derive(Component, Debug, Clone, Default)]
pub struct AestraBindingRecorder {
    recorder: BindingRecorder,
}

impl AestraBindingRecorder {
    pub fn new() -> Self {
        Self::default()
    }

    /// The trace recorded so far.
    pub fn trace(&self) -> BindingTrace {
        self.recorder.trace()
    }

    /// Ticks recorded.
    pub fn len(&self) -> usize {
        self.recorder.len()
    }

    pub fn is_empty(&self) -> bool {
        self.recorder.is_empty()
    }
}

/// Installs and removes binding traces (host bindings HB8), before host input is resolved.
pub(crate) fn apply_binding_traces(
    mut traced: Query<(&mut EffectPlayer, &AestraBindingTrace)>,
    mut removed: RemovedComponents<AestraBindingTrace>,
    mut players: Query<&mut EffectPlayer, Without<AestraBindingTrace>>,
) {
    for (mut player, trace) in &mut traced {
        let installed = player
            .instance()
            .binding_trace()
            .map(|installed| installed.identity());
        if installed != Some(trace.0.identity()) {
            player
                .instance_mut()
                .set_binding_trace(Some(trace.0.clone()));
        }
    }
    for entity in removed.read() {
        if let Ok(mut player) = players.get_mut(entity) {
            player.instance_mut().set_binding_trace(None);
        }
    }
}

/// The entity each slot resolved to last frame, to detect retargeting.
#[derive(Component, Debug, Default)]
pub(crate) struct ResolvedBindingTargets(Vec<Option<Entity>>);

/// Builds a spatial snapshot from an entity's world transform and optional velocity.
pub fn spatial_snapshot(
    transform: &GlobalTransform,
    velocity: Option<&AestraLinearVelocity>,
) -> SpatialBindingSnapshot {
    let (scale, rotation, translation) = transform.to_scale_rotation_translation();
    SpatialBindingSnapshot {
        position: translation.to_array(),
        rotation: rotation.to_array(),
        scale: scale.to_array(),
        linear_velocity: velocity.map(|velocity| velocity.0.to_array()),
    }
}

/// One frame of snapshots for `effect`: each slot bound to an existing spatial entity gets its
/// transform; unbound, despawned, non-spatial, or incomplete (a required field the entity cannot
/// supply) slots are `None`.
pub fn binding_frame(
    effect: &CompiledEffect,
    bindings: &AestraBindings,
    resolve: impl Fn(Entity) -> Option<SpatialBindingSnapshot>,
) -> (BindingFrame, Vec<Option<Entity>>) {
    let spatial = BindingKindId::new(AESTRA_BINDING_SPATIAL);
    let mut targets = Vec::with_capacity(effect.bindings.len());
    let snapshots = effect
        .bindings
        .iter()
        .map(|binding| {
            let entity = bindings.get(&binding.name);
            targets.push(entity);
            if binding.kind != spatial {
                // Only the spatial kind has a Bevy mapping; plugin kinds need their own adapter.
                return None;
            }
            let packed: BindingSnapshot = resolve(entity?)?.to_snapshot(&binding.layout);
            binding
                .required_fields
                .iter()
                .all(|field| packed.field(&binding.layout, field).is_some())
                .then_some(packed)
        })
        .collect();
    (BindingFrame { snapshots }, targets)
}

/// A player whose bindings its entities drive: what [`resolve_host_bindings`] reads and records.
type LivePlayer = (
    Entity,
    &'static mut EffectPlayer,
    &'static AestraBindings,
    Option<&'static mut ResolvedBindingTargets>,
    Option<&'static mut AestraBindingRecorder>,
);

/// Resolves every bound entity once and pushes one frame per effect player
/// ([`AestraSet::ResolveHostInputs`](crate::AestraSet::ResolveHostInputs)).
pub(crate) fn resolve_host_bindings(
    mut commands: Commands,
    mut players: Query<LivePlayer, Without<AestraBindingTrace>>,
    objects: Query<(&GlobalTransform, Option<&AestraLinearVelocity>)>,
) {
    for (entity, mut player, bindings, previous, recorder) in &mut players {
        let effect = player.effect().clone();
        if effect.bindings.is_empty() {
            continue;
        }
        let (frame, targets) = binding_frame(&effect, bindings, |target| {
            objects
                .get(target)
                .ok()
                .map(|(transform, velocity)| spatial_snapshot(transform, velocity))
        });
        let instance = player.instance_mut();
        // A slot now filled by a different entity is a new target, not a moving one.
        if let Some(previous) = previous.as_deref() {
            for (index, (before, now)) in previous.0.iter().zip(&targets).enumerate() {
                if let (Some(before), Some(now)) = (before, now)
                    && before != now
                {
                    let _ = instance.rebind(BindingSlot(index));
                }
            }
        }
        if let Err(error) = instance.apply_binding_frame(&frame) {
            warn!("aestra: could not apply host bindings to {entity}: {error}");
        } else if let Some(mut recorder) = recorder {
            // The frame serves the ticks this frame advances from the current time.
            let tick = aestra_runtime::trace_tick(instance.time());
            recorder.recorder.record(tick, &frame);
        }
        match previous {
            Some(mut previous) => previous.0 = targets,
            None => {
                commands
                    .entity(entity)
                    .insert(ResolvedBindingTargets(targets));
            }
        }
    }
}

/// Fills nested clip presentations' forwarded bindings from their parents, shallow to deep, so a
/// host binds only the root player (host bindings roadmap §9.6).
pub(crate) fn forward_project_bindings(
    roots: Query<(Entity, &EffectPlayer)>,
    mut children: Query<(Entity, &EffectClipInstance, &mut PresentedEffect), Without<EffectPlayer>>,
) {
    type Key = (Entity, Vec<aestra_core::EffectClipId>);
    let mut resolved: BTreeMap<Key, (Arc<CompiledEffect>, Vec<Option<BindingSnapshot>>)> =
        BTreeMap::new();
    for (root, player) in &roots {
        if player.project().is_some() {
            resolved.insert(
                (root, Vec::new()),
                (
                    player.effect().clone(),
                    player.instance().resolved_bindings(),
                ),
            );
        }
    }
    let mut order: Vec<(usize, Entity)> = children
        .iter()
        .map(|(entity, clip, _)| (clip.path.len(), entity))
        .collect();
    order.sort();
    for (_, entity) in order {
        let Ok((_, clip, mut presented)) = children.get_mut(entity) else {
            continue;
        };
        let Some((last, parent_path)) = clip.path.split_last() else {
            continue;
        };
        let key = (clip.root, clip.path.clone());
        if let Some((parent_effect, parent_values)) =
            resolved.get(&(clip.root, parent_path.to_vec()))
            && let Some(compiled_clip) = parent_effect
                .effect_clips
                .iter()
                .find(|candidate| candidate.source_clip == *last)
        {
            presented.instance.apply_forwarded_bindings(
                parent_effect,
                parent_values,
                &compiled_clip.binding_forwards,
            );
        }
        resolved.insert(
            key,
            (
                presented.effect().clone(),
                presented.instance.resolved_bindings(),
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_core::{
        AESTRA_FIELD_LINEAR_VELOCITY, AESTRA_FIELD_POSITION, BindingFieldId, BindingUpdateMode,
        EffectAsset, EffectBinding, EffectClip, Emitter,
    };
    use aestra_runtime::BindingState;

    const SOURCE: BindingSlot = BindingSlot(0);
    const TARGET: BindingSlot = BindingSlot(1);

    fn position() -> BindingFieldId {
        BindingFieldId::new(AESTRA_FIELD_POSITION)
    }

    fn velocity() -> BindingFieldId {
        BindingFieldId::new(AESTRA_FIELD_LINEAR_VELOCITY)
    }

    fn effect(target_needs_velocity: bool) -> EffectAsset {
        let mut effect = EffectAsset::new("Homing", 2.0);
        effect.emitters.push(Emitter::basic_sprite("Fireball", 2.0));
        let mut target = EffectBinding::spatial("Target", BindingUpdateMode::Live);
        if target_needs_velocity {
            target.required_fields.insert(velocity());
        } else {
            target.optional_fields.insert(velocity());
        }
        effect.bindings = vec![
            EffectBinding::spatial("Source", BindingUpdateMode::SnapshotOnSpawn),
            target,
        ];
        effect
    }

    fn test_app() -> App {
        let mut app = App::new();
        app.add_systems(Update, resolve_host_bindings);
        app
    }

    fn object(app: &mut App, position: Vec3) -> Entity {
        app.world_mut()
            .spawn(GlobalTransform::from_translation(position))
            .id()
    }

    fn move_to(app: &mut App, entity: Entity, position: Vec3) {
        *app.world_mut().get_mut::<GlobalTransform>(entity).unwrap() =
            GlobalTransform::from_translation(position);
    }

    fn field(
        app: &App,
        player: Entity,
        slot: BindingSlot,
        field: &BindingFieldId,
    ) -> Option<Vec<f32>> {
        app.world()
            .get::<EffectPlayer>(player)
            .unwrap()
            .instance()
            .binding_field(slot, field)
            .map(<[f32]>::to_vec)
    }

    #[test]
    fn bound_entities_are_resolved_every_frame_and_the_source_stays_latched() {
        let mut app = test_app();
        let staff = object(&mut app, Vec3::new(0.0, 1.0, 0.0));
        let enemy = object(&mut app, Vec3::new(5.0, 0.0, 0.0));
        let player = app
            .world_mut()
            .spawn((
                EffectPlayer::new(&effect(false)),
                AestraBindings::new()
                    .bind("Source", staff)
                    .bind("Target", enemy),
            ))
            .id();
        app.update();
        assert_eq!(
            field(&app, player, TARGET, &position()),
            Some(vec![5.0, 0.0, 0.0])
        );

        move_to(&mut app, enemy, Vec3::new(6.0, 0.0, 1.0));
        move_to(&mut app, staff, Vec3::new(9.0, 9.0, 9.0));
        app.update();
        assert_eq!(
            field(&app, player, TARGET, &position()),
            Some(vec![6.0, 0.0, 1.0])
        );
        assert_eq!(
            field(&app, player, SOURCE, &position()),
            Some(vec![0.0, 1.0, 0.0]),
            "SnapshotOnSpawn keeps the staff position from the first frame"
        );
        // Velocity is not derived: absent until the entity supplies it.
        assert_eq!(field(&app, player, TARGET, &velocity()), None);
        app.world_mut()
            .entity_mut(enemy)
            .insert(AestraLinearVelocity(Vec3::new(0.0, 0.0, 3.0)));
        app.update();
        assert_eq!(
            field(&app, player, TARGET, &velocity()),
            Some(vec![0.0, 0.0, 3.0])
        );
    }

    #[test]
    fn despawned_retargeted_and_incomplete_bindings_are_handled() {
        let mut app = test_app();
        let staff = object(&mut app, Vec3::ZERO);
        let enemy = object(&mut app, Vec3::X);
        let other = object(&mut app, Vec3::Y);
        let player = app
            .world_mut()
            .spawn((
                EffectPlayer::new(&effect(false)),
                AestraBindings::new()
                    .bind("Source", staff)
                    .bind("Target", enemy),
            ))
            .id();
        app.update();
        let epoch = |app: &App| {
            app.world()
                .get::<EffectPlayer>(player)
                .unwrap()
                .instance()
                .host_input_epoch()
        };
        let bound = epoch(&app);

        // Retargeting to another live entity is announced as a rebind.
        app.world_mut()
            .get_mut::<AestraBindings>(player)
            .unwrap()
            .set("Target", other);
        app.update();
        assert_ne!(epoch(&app), bound);
        assert_eq!(
            field(&app, player, TARGET, &position()),
            Some(vec![0.0, 1.0, 0.0])
        );

        // The target despawns: the slot is unbound and reported missing.
        app.world_mut().despawn(other);
        app.update();
        let status = app
            .world()
            .get::<EffectPlayer>(player)
            .unwrap()
            .instance()
            .binding_status();
        assert_eq!(status.slots[1].state, BindingState::Unbound);
        assert_eq!(
            status
                .missing_required()
                .map(|slot| slot.name.as_str())
                .collect::<Vec<_>>(),
            ["Target"]
        );

        // A required velocity the entity cannot supply leaves the slot unbound, not invalid.
        let mut strict = test_app();
        let enemy = object(&mut strict, Vec3::X);
        let player = strict
            .world_mut()
            .spawn((
                EffectPlayer::new(&effect(true)),
                AestraBindings::new().bind("Target", enemy),
            ))
            .id();
        strict.update();
        assert_eq!(field(&strict, player, TARGET, &position()), None);
        strict
            .world_mut()
            .entity_mut(enemy)
            .insert(AestraLinearVelocity(Vec3::Z));
        strict.update();
        assert_eq!(
            field(&strict, player, TARGET, &position()),
            Some(vec![1.0, 0.0, 0.0])
        );
    }

    #[test]
    fn world_rotation_and_scale_come_from_the_global_transform() {
        let transform = GlobalTransform::from(
            Transform::from_translation(Vec3::new(1.0, 2.0, 3.0))
                .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2))
                .with_scale(Vec3::splat(2.0)),
        );
        let snapshot = spatial_snapshot(&transform, None);
        assert_eq!(snapshot.position, [1.0, 2.0, 3.0]);
        assert!((snapshot.scale[0] - 2.0).abs() < 1e-5);
        let expected = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2).to_array();
        for (actual, expected) in snapshot.rotation.iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-5);
        }
        assert_eq!(snapshot.linear_velocity, None);
    }

    #[test]
    fn a_child_clip_reads_the_root_binding_it_forwards() {
        let aim = EffectBinding::spatial("Aim", BindingUpdateMode::Live);
        let mut child = EffectAsset::new("Child", 2.0);
        child.emitters.push(Emitter::basic_sprite("Sparks", 2.0));
        child.bindings = vec![aim.clone()];
        let root_effect = effect(false);
        let mut root = root_effect.clone();
        let mut clip = EffectClip::new(child.id, 0.0, 2.0);
        clip.binding_forwards.insert(aim.id, root.bindings[1].id);
        root.effect_clips.push(clip);
        let project = crate::EffectCompiler::default()
            .compile_resolved_project(&aestra_project::ResolvedEffectProject {
                root,
                dependencies: BTreeMap::from([(child.id, child)]),
                material_programs: BTreeMap::new(),
                material_functions: BTreeMap::new(),
            })
            .unwrap();

        let mut app = App::new();
        app.add_systems(
            Update,
            (
                resolve_host_bindings,
                crate::project::sync_project_instances,
                forward_project_bindings,
            )
                .chain(),
        );
        let enemy = object(&mut app, Vec3::new(7.0, 0.0, 0.0));
        app.world_mut().spawn((
            EffectPlayer::from_project(Arc::new(project)),
            AestraBindings::new().bind("Target", enemy),
        ));
        // Frame 1 spawns the child presentation; frame 2 forwards into it.
        app.update();
        app.update();
        let mut children = app
            .world_mut()
            .query::<(&EffectClipInstance, &PresentedEffect)>();
        let (_, presented) = children.single(app.world()).unwrap();
        assert_eq!(
            presented
                .instance
                .binding_field(BindingSlot(0), &position())
                .map(<[f32]>::to_vec),
            Some(vec![7.0, 0.0, 0.0])
        );
    }

    #[test]
    fn a_recorded_trace_replaces_the_entities_and_scrubs_exactly() {
        let mut app = App::new();
        app.add_systems(
            Update,
            (apply_binding_traces, resolve_host_bindings).chain(),
        );
        let enemy = object(&mut app, Vec3::new(1.0, 0.0, 0.0));
        let player = app
            .world_mut()
            .spawn((
                EffectPlayer::new(&effect(false)),
                AestraBindings::new().bind("Target", enemy),
                AestraBindingRecorder::new(),
            ))
            .id();
        // Play forward a tick per frame while the enemy moves; the recorder keeps each frame.
        for tick in 0..30 {
            move_to(&mut app, enemy, Vec3::new(tick as f32, 0.0, 0.0));
            app.update();
            let mut entity = app.world_mut().entity_mut(player);
            let mut player = entity.get_mut::<EffectPlayer>().unwrap();
            let time = (tick + 1) as f32 * aestra_runtime::StatefulSimulation::TICK_DT + 1e-4;
            player.instance_mut().set_playback_time(time);
        }
        let recorder = app
            .world()
            .get::<AestraBindingRecorder>(player)
            .unwrap()
            .clone();
        assert_eq!(recorder.len(), 30);
        let trace = Arc::new(recorder.trace());

        // Replay the trace: the entity no longer drives the player, and a seek reads the past.
        app.world_mut()
            .entity_mut(player)
            .insert(AestraBindingTrace(trace.clone()));
        move_to(&mut app, enemy, Vec3::new(500.0, 0.0, 0.0));
        app.update();
        let seek = |app: &mut App, time: f32| {
            app.world_mut()
                .get_mut::<EffectPlayer>(player)
                .unwrap()
                .instance_mut()
                .seek(time);
            field(app, player, TARGET, &position())
        };
        let tick = aestra_runtime::StatefulSimulation::TICK_DT;
        assert_eq!(seek(&mut app, 10.5 * tick), Some(vec![10.0, 0.0, 0.0]));
        assert_eq!(seek(&mut app, 3.5 * tick), Some(vec![3.0, 0.0, 0.0]));
        let instance = app.world().get::<EffectPlayer>(player).unwrap().instance();
        assert_eq!(instance.host_input_epoch(), trace.identity());

        // Removing the trace returns the player to its entity.
        app.world_mut()
            .entity_mut(player)
            .remove::<AestraBindingTrace>();
        app.update();
        assert_eq!(
            field(&app, player, TARGET, &position()),
            Some(vec![500.0, 0.0, 0.0])
        );
    }
}
