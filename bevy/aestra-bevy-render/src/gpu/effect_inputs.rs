//! Actual simulation extraction metadata; no device state or particle mirror.
/// An effect's event routes as the lockstep loop runs them (event system E3): the bursts its input
/// routes spawn, and its particle output routes, each with the `counters` word its ring starts at.
#[derive(Clone, Copy, Default)]
pub(super) struct RouteWiring<'a> {
    pub bursts: &'a [aestra_runtime::InputSpawnBurst],
    pub outputs: &'a [(aestra_runtime::CompiledParticleOutput, u32)],
    pub output_epoch: u32,
    pub output_suppress_through: u64,
}

pub(super) fn event_link_counter_base(dispatches: &[StatefulDispatch]) -> Option<u32> {
    dispatches
        .iter()
        .flat_map(|dispatch| {
            [
                dispatch.arrival_word.map(|word| word + 1),
                dispatch.overflow_word,
            ]
        })
        .flatten()
        .max()
        .map(|word| word + 1)
}
pub(super) use super::trail_context::TrailContext;
use aestra_gpu::GpuSimulationState;
use aestra_runtime::{PlaybackHistoryPolicy, SeekQuality};
use bevy::{prelude::*, render::storage::ShaderBuffer};
use std::sync::Arc;

#[derive(Component, Clone)]
pub(crate) struct GpuEffectBuffers {
    pub(super) emitters: Handle<ShaderBuffer>,
    pub(super) renderers: Handle<ShaderBuffer>,
    pub(super) particles: Handle<ShaderBuffer>,
    pub(super) alive: Handle<ShaderBuffer>,
    pub(super) dead: Handle<ShaderBuffer>,
    pub(super) counters: Handle<ShaderBuffer>,
    pub(super) indirect: Handle<ShaderBuffer>,
    pub(super) globals: Handle<ShaderBuffer>,
    /// Shared per-slot scratch (3 words/slot) for ribbon link state, kept off the
    /// particle ABI. A 1-word dummy when the effect has no ribbon renderer.
    pub(super) aux: Handle<ShaderBuffer>,
    pub(super) render_globals: Handle<ShaderBuffer>,
    pub(super) workgroups: u32,
    pub(super) has_ribbons: bool,
    pub(super) has_trails: bool,
    pub(super) ribbon_workgroups: u32,
    pub(super) trail_workgroups: u32,
    pub(super) trail_plan: aestra_gpu::TrailScratchPlan,
    pub(super) total_slots: u32,
    pub(super) simulation_time: f32,
    /// The requested fidelity of stateful seeking this frame (hybrid roadmap M12): `Preview` bounds the
    /// per-frame reconstruction while the user scrubs; `Exact` (the default) reconstructs the
    /// authoritative state. Sourced from the player each frame.
    pub(super) seek_quality: SeekQuality,
    pub(super) history_policy: PlaybackHistoryPolicy,
    pub(super) history_epoch: u32,
    pub(super) statistics_token: u32,
    pub(super) checkpoint_context: Arc<TrailContext>,
    pub(super) trail_roots: Vec<(u32, u32)>,
    /// Persistent simulation-state sizing for stateful emitters (hybrid roadmap M6); `records == 0`
    /// for a fully analytic effect. The render world allocates its persistent state buffers from
    /// this (see [`super::StatefulStates`]).
    pub(super) simulation_state: GpuSimulationState,
    /// One stateful dispatch descriptor per enabled stateful emitter (hybrid roadmap M6), in compiled
    /// emitter order. Empty for a fully analytic effect.
    pub(super) stateful_dispatch: Vec<StatefulDispatch>,
    /// The effect's particle event links (host bindings HB9b).
    pub(super) event_links: Vec<aestra_runtime::CompiledEventLink>,
    /// Whether the effect has event routes (event system E3): its stateful emitters then advance in
    /// lockstep, bursts spawning and particle outputs aggregating after each tick's links.
    pub(super) routed: bool,
    /// Its particle output routes, each with the `counters` word its ring of tick records starts at.
    pub(super) particle_outputs: Vec<(aestra_runtime::CompiledParticleOutput, u32)>,
    pub(super) output_suppress_through: u64,
    /// The host's recorded input events, updated each frame (event system E2–E3).
    pub(super) host_events: Arc<HostEventHistory>,
    /// The host's physics colliders around the effect this frame, packed (host bindings HB10).
    pub(super) physics: Arc<[u32]>,
    /// True when *every* enabled emitter is stateful, so the effect skips the analytic reset+simulate
    /// entirely. False for a mixed analytic+stateful effect, where the analytic path runs first (its
    /// `simulate` skips the stateful emitters' slots) and the stateful dispatches fill them after,
    /// reusing the shared counter/telemetry the analytic reset already wrote.
    pub(super) stateful_only: bool,
}

/// The parameters the GPU stateful path needs for one emitter (hybrid roadmap M6), sourced from the
/// compiled emitter at prepare time so the render graph does not need the CPU effect. The stateful
/// integrator is the minimal reference model (deterministic launch direction, constant gravity, fixed
/// lifetime), so it reads scalar midpoints of the authored ranges rather than the full analytic
/// feature set.
#[derive(Clone)]
pub(super) struct StatefulDispatch {
    /// Live-particle capacity — the emitter's `max_particles`, and the persistent state slot count.
    pub(super) capacity: u32,
    /// This emitter's base index into the alive-indices / indirect draw buffers.
    pub(super) slot_offset: u32,
    /// This emitter's index for the packed emitter/alive word and its indirect draw command.
    pub(super) emitter_index: u32,
    /// The effect's enabled-emitter count, locating the statistics telemetry trailer in the indirect
    /// buffer (at `emitter_count * 4`).
    pub(super) emitter_count: u32,
    /// Mean particles emitted per second; fractional per-tick spawns accumulate across ticks.
    pub(super) spawn_rate: f32,
    /// Authored one-shot births, emitted once on the first tick crossing start_time.
    pub(super) burst_count: u32,
    pub(super) burst_tick: u32,
    /// Per-particle launch speed range `(min, max)`.
    pub(super) speed: (f32, f32),
    /// Per-particle lifetime range `(min, max)` in seconds.
    pub(super) lifetime: (f32, f32),
    /// Local launch axis. Mode 0 retains `normalize(direction + spread * random)`.
    pub(super) direction: [f32; 3],
    pub(super) velocity_distribution: u32,
    /// Legacy cone factor (mode 0), otherwise the full cone angle in degrees.
    pub(super) spread: f32,
    /// Linear velocity damping per second (`v -= drag * v * dt`).
    pub(super) drag: f32,
    /// Value-noise turbulence strength.
    pub(super) turbulence: f32,
    /// Spawn shape: 0 = point, 1 = sphere (radius), 2 = box (half extents).
    pub(super) shape_kind: u32,
    /// Sphere radius (when `shape_kind == 1`).
    pub(super) shape_radius: f32,
    /// Box half extents (when `shape_kind == 2`).
    pub(super) shape_half_extents: [f32; 3],
    /// Constant acceleration applied to velocity each tick.
    pub(super) gravity: [f32; 3],
    /// The effect's 64-bit spawn seed.
    pub(super) seed: u64,
    /// Collision primitives resolved after each tick (hybrid roadmap M10), capped at `MAX_COLLIDERS`
    /// when packed into the params buffer. Empty for emitters without a collision module.
    pub(super) colliders: Vec<aestra_core::Collider>,
    /// The domain field these particles follow (fluid F2b); they then advance in lockstep with it.
    pub(super) field_follow: Option<aestra_runtime::CompiledFieldFollow>,
    /// The domain emission list these particles are also born from (fluid F10); lockstep likewise.
    pub(super) domain_spawn: Option<aestra_runtime::CompiledDomainSpawn>,
    /// Homing steering (host bindings HB7), and the target it steers toward this frame — resolved
    /// from the effect's bindings each frame, into effect space, the tracker remembering the last one
    /// seen for [`aestra_runtime::HomingLostPolicy::KeepLastPosition`].
    pub(super) homing: Option<aestra_runtime::CompiledHoming>,
    pub(super) homing_target: Option<aestra_runtime::HomingTarget>,
    pub(super) homing_tracker: aestra_runtime::HomingTracker,
    /// The host binding the emitter follows (host bindings HB7b): `placement` is then resolved from
    /// it each frame, and kept while the binding supplies no pose.
    pub(super) attachment: Option<aestra_runtime::CompiledAttachment>,
    /// The `counters` word this emitter's homing arrivals are counted into (the `impact` event, host
    /// bindings HB9), when it homes. The next word holds the tick they were counted up to (event system
    /// E2b), so a late read-back still dates the `impact`.
    pub(super) arrival_word: Option<u32>,
    /// Where the homing target is this frame, in world space, for the events it raises.
    pub(super) homing_world_target: Option<[f32; 3]>,
    /// The particle events this emitter reports for event links (host bindings HB9b): a mask of
    /// `aestra_runtime::event_trigger_bit`s; zero reports none.
    pub(super) event_mask: u32,
    pub(super) distance_emission: Option<(f32, u32)>,
    /// A hash of every event link into or out of this emitter: a change is a different simulation.
    pub(super) event_signature: u64,
    /// The `counters` word this emitter's event overflow count is copied to each frame, when it
    /// reports events: the events beyond the buffer's capacity, dropped (host bindings HB9b).
    pub(super) overflow_word: Option<u32>,
    /// Under a binding trace (host bindings HB8): the homing target and the spawn placement of every
    /// tick, so each tick — live or replayed after a seek — uses its own recorded input. `None`
    /// without a trace: the frame's input then serves every tick of the frame.
    pub(super) schedule: Option<Arc<TickSchedule>>,
    /// Where the effect sits in the host's world, for `World` colliders (host bindings HB10): the
    /// effect-to-world affine, 3×4 rows, updated each frame.
    pub(super) world_from_effect: [[f32; 4]; 3],
    /// The host world's revision, for an emitter with `World` colliders (0 otherwise): a new world
    /// is a different simulation.
    pub(super) world_revision: u64,
    /// Where a host's `stop_emitting` / `kill` cut emission (event system E2b), updated each frame.
    pub(super) cutoffs: aestra_runtime::EmissionCutoffs,
    /// The emitter transform placing new spawns in effect space. Kept out of the fingerprint: moving
    /// an emitter changes only future spawns, so the live state survives (see
    /// [`super::prepare_stateful_states`]) and a gizmo drag never restarts the simulation.
    pub(super) placement: aestra_runtime::SpawnPlacement,
    /// How `present` draws the particles: the emitter's appearance, as the analytic path samples it.
    /// Kept out of the fingerprint too: a look edit changes no particle's motion.
    pub(super) appearance: StatefulAppearance,
}

/// An emitter's size and opacity over life, colour gradient and largest scale, for stateful `present`.
#[derive(Clone, Copy)]
pub(super) struct StatefulAppearance {
    pub(super) size: aestra_gpu::GpuCurve,
    pub(super) opacity: aestra_gpu::GpuCurve,
    pub(super) color: aestra_gpu::GpuGradient,
    pub(super) max_scale: f32,
}
/// Host input routes and event identities invalidate checkpoints after divergence.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct HostEventHistory {
    pub(super) bursts: Vec<aestra_runtime::InputSpawnBurst>,
    pub(super) events: Vec<(u64, u64)>,
}
/// A stateful emitter's host input per tick under a binding trace (host bindings HB8).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct TickSchedule {
    /// What it was computed from: the trace's identity and the effect's world placement.
    pub(super) key: u64,
    /// The homing target each tick steers toward (empty without homing).
    pub(super) homing: Vec<Option<aestra_runtime::HomingTarget>>,
    /// The spawn placement of each tick (empty for an emitter not attached to a binding).
    pub(super) placement: Vec<aestra_runtime::SpawnPlacement>,
}

impl HostEventHistory {
    pub(super) fn of(instance: &aestra_runtime::EffectInstance) -> Self {
        Self {
            bursts: instance.input_spawn_bursts(),
            events: Self::keys(instance),
        }
    }

    pub(super) fn keys(instance: &aestra_runtime::EffectInstance) -> Vec<(u64, u64)> {
        instance
            .received_events()
            .iter()
            .map(|event| (event.tick, event.identity()))
            .collect()
    }

    /// The first tick two histories (tick-ordered `(tick, identity)` lists) differ at — the first
    /// tick whose advance is not the same — or `None` when they are the same.
    pub(super) fn divergence(before: &[(u64, u64)], after: &[(u64, u64)]) -> Option<u64> {
        let common = before
            .iter()
            .zip(after)
            .take_while(|(before, after)| before == after)
            .count();
        let ticks = [before.get(common), after.get(common)];
        ticks.into_iter().flatten().map(|(tick, _)| *tick).min()
    }
}

impl TickSchedule {
    /// The homing target and placement for `tick`, past the end the last ones.
    pub(super) fn at<T: Copy>(values: &[T], tick: u32) -> Option<T> {
        values
            .get((tick as usize).min(values.len().saturating_sub(1)))
            .copied()
    }
}

impl StatefulDispatch {
    /// A hash of the emitter's seed, slot placement, and dynamics. A change means a different
    /// simulation, so the persistent state and its checkpoints are invalidated (hybrid roadmap M7).
    pub(super) fn fingerprint(&self) -> u64 {
        let mut hash = self.seed;
        for bits in [
            self.capacity,
            self.slot_offset,
            self.emitter_index,
            self.spawn_rate.to_bits(),
            self.burst_count,
            self.burst_tick,
            self.speed.0.to_bits(),
            self.speed.1.to_bits(),
            self.lifetime.0.to_bits(),
            self.lifetime.1.to_bits(),
            self.direction[0].to_bits(),
            self.direction[1].to_bits(),
            self.direction[2].to_bits(),
            self.spread.to_bits(),
            self.velocity_distribution,
            self.drag.to_bits(),
            self.turbulence.to_bits(),
            self.shape_kind,
            self.shape_radius.to_bits(),
            self.shape_half_extents[0].to_bits(),
            self.shape_half_extents[1].to_bits(),
            self.shape_half_extents[2].to_bits(),
            self.gravity[0].to_bits(),
            self.gravity[1].to_bits(),
            self.gravity[2].to_bits(),
            self.colliders.len() as u32,
        ] {
            hash = (hash ^ u64::from(bits)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        // Following a field changes the simulation (fluid F2b).
        if let Some(follow) = &self.field_follow {
            for bits in [
                follow.stage as u32,
                follow.strength.to_bits(),
                follow.field.dims[0],
                follow.field.cell_size.to_bits(),
                follow.field.origin[1].to_bits(),
            ] {
                hash = (hash ^ u64::from(bits)).wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        // So does the world it collides with (host bindings HB10).
        hash = (hash ^ self.world_revision).wrapping_mul(0x0000_0100_0000_01b3);
        // So does the binding trace it replays (host bindings HB8).
        if let Some(schedule) = &self.schedule {
            hash = (hash ^ schedule.key).wrapping_mul(0x0000_0100_0000_01b3);
        }
        // So do its event links (host bindings HB9b).
        hash = (hash ^ u64::from(self.event_mask)).wrapping_mul(0x0000_0100_0000_01b3);
        hash = (hash ^ self.event_signature).wrapping_mul(0x0000_0100_0000_01b3);
        if let Some((spacing, limit)) = self.distance_emission {
            hash = (hash ^ u64::from(spacing.to_bits())).wrapping_mul(0x0000_0100_0000_01b3);
            hash = (hash ^ u64::from(limit)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        // So does homing (host bindings HB7) — its steering, not where the target is.
        if let Some(homing) = &self.homing {
            let config = &homing.config;
            for bits in [
                config.speed.to_bits(),
                config.acceleration.to_bits(),
                config.turn_rate.to_bits(),
                config.arrival_radius.to_bits(),
                config.lost as u32,
            ] {
                hash = (hash ^ u64::from(bits)).wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        // So does spawning from a domain (fluid F10).
        if let Some(spawn) = &self.domain_spawn {
            for bits in [
                spawn.stage as u32,
                spawn.emission.capacity,
                spawn.inherit.to_bits(),
            ] {
                hash = (hash ^ u64::from(bits)).wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        // Colliders change the simulation, so fold each one's shape and response into the fingerprint
        // (hybrid roadmap M10): editing a collider invalidates the persistent state and checkpoints.
        let mut block = [0u32; 27 + 10 * aestra_runtime::MAX_COLLIDERS];
        aestra_gpu::pack_stateful_colliders(&self.colliders, &mut block);
        for bits in &block[26..] {
            hash = (hash ^ u64::from(*bits)).wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }
}
