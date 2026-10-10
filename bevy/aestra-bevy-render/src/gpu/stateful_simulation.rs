//! Shared stateful setup, persistent lifecycle and independent fixed-tick scheduling.
use super::catchup_pacing::stateful_catchup_budget;
use super::effect_inputs::{GpuEffectBuffers, HostEventHistory, StatefulDispatch, TickSchedule};
use aestra_gpu::{GpuParticle, WORKGROUP_SIZE};
use aestra_runtime::{PlaybackHistoryPolicy, SeekQuality};
use bevy::{
    prelude::*,
    render::{
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupLayout, BindGroupLayoutDescriptor,
            BindGroupLayoutEntries, Buffer, BufferInitDescriptor, BufferUsages,
            CachedComputePipelineId, CommandEncoder, ComputePassDescriptor, ComputePipeline,
            ComputePipelineDescriptor, DownlevelFlags, PipelineCache, ShaderStages,
            binding_types::{storage_buffer, storage_buffer_read_only},
        },
        renderer::{RenderAdapter, RenderDevice},
    },
};
use std::collections::BTreeMap;

pub(super) const STATEFUL_SIMULATION_SHADER_PATH: &str =
    "embedded://aestra_bevy_render/shaders/aestra_stateful_simulation.wgsl";

/// The stateful GPU backend's compute pipelines, built from
/// `aestra_gpu::stateful_simulation_wgsl` over the shared twelve-binding layout:
/// persistent/free-slot state, tick parameters, presentation/compaction outputs,
/// particle events and the host's world/physics inputs.
#[derive(Resource)]
pub(super) struct StatefulSimulationPipeline {
    pub(super) layout: BindGroupLayoutDescriptor,
    /// Advances each live slot by one fixed tick and frees the ones that died this tick.
    pub(super) death_integrate: CachedComputePipelineId,
    /// Claims a free slot and a fresh ordinal for each of this tick's new particles.
    pub(super) spawn: CachedComputePipelineId,
    /// Extracts live persistent state into the 48-byte presentation particle buffer.
    pub(super) present: CachedComputePipelineId,
    /// Canonicalizes transparent draw order by spawn ordinal for bounded live sets.
    pub(super) order_present: CachedComputePipelineId,
}

pub(super) fn init_stateful_pipeline(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    pipeline_cache: Res<PipelineCache>,
    render_device: Res<RenderDevice>,
    adapter: Res<RenderAdapter>,
) {
    const STATEFUL_BINDING_COUNT: u32 = 12;
    let limits = render_device.limits();
    if !adapter
        .get_downlevel_capabilities()
        .flags
        .contains(DownlevelFlags::COMPUTE_SHADERS)
        || limits.max_storage_buffers_per_shader_stage < STATEFUL_BINDING_COUNT
        || limits.max_bindings_per_bind_group < STATEFUL_BINDING_COUNT
        || limits.max_compute_invocations_per_workgroup < WORKGROUP_SIZE
        || limits.max_compute_workgroup_size_x < WORKGROUP_SIZE
    {
        return;
    }
    let layout = BindGroupLayoutDescriptor::new(
        "aestra_gpu_stateful_simulation",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                storage_buffer::<Vec<f32>>(false),           // 0: persistent state
                storage_buffer::<Vec<u32>>(false),           // 1: free list
                storage_buffer::<Vec<u32>>(false),           // 2: free count (atomic)
                storage_buffer::<Vec<u32>>(false),           // 3: spawn counter (atomic)
                storage_buffer_read_only::<Vec<u32>>(false), // 4: params
                storage_buffer::<Vec<GpuParticle>>(false),   // 5: presentation output
                storage_buffer::<Vec<u32>>(false),           // 6: alive indices (compaction)
                storage_buffer::<Vec<u32>>(false),           // 7: indirect draw commands (atomic)
                storage_buffer::<Vec<u32>>(false),           // 8: live counters (atomic)
                storage_buffer::<Vec<u32>>(false),           // 9: particle events (HB9b)
                storage_buffer_read_only::<Vec<u32>>(false), // 10: the host's world SDF (HB10)
                storage_buffer_read_only::<Vec<u32>>(false), // 11: the host's physics (HB10)
            ),
        ),
    );
    let shader = asset_server.load(STATEFUL_SIMULATION_SHADER_PATH);
    let pipeline = |label: &'static str, entry: &'static str| {
        pipeline_cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some(label.into()),
            layout: vec![layout.clone()],
            shader: shader.clone(),
            entry_point: Some(entry.into()),
            ..default()
        })
    };
    commands.insert_resource(StatefulSimulationPipeline {
        layout: layout.clone(),
        death_integrate: pipeline("aestra stateful death+integrate", "death_integrate"),
        spawn: pipeline("aestra stateful spawn", "spawn"),
        present: pipeline("aestra stateful present", "present"),
        order_present: pipeline("aestra stateful presentation order", "order_present"),
    });
}

/// One GPU-resident snapshot of a stateful emitter's full persistent state at a fixed tick (hybrid
/// roadmap M7): copies of all four buffers plus the CPU-side spawn accumulator. A backward seek
/// restores the nearest snapshot at or before the target and replays forward the short remainder,
/// instead of replaying from tick 0.
pub(super) struct StatefulCheckpoint {
    pub(super) tick: u32,
    pub(super) state: Buffer,
    pub(super) free_list: Buffer,
    pub(super) free_count: Buffer,
    pub(super) spawn_counter: Buffer,
    pub(super) spawn_accumulator: f32,
}

/// The persistent GPU state for one stateful effect (hybrid roadmap M6/M7). Unlike the analytic
/// particle buffer — recomputed from scratch every frame — this survives across frames so per-particle
/// state advances incrementally, and it carries a store of GPU-resident checkpoints for cheap backward
/// seek. Reallocated (which drops the checkpoints) when the emitter's capacity or dynamics fingerprint
/// changes.
pub(super) struct StatefulPersistentState {
    /// Slot capacity these buffers were sized for; a change triggers reallocation.
    pub(super) records: u32,
    /// `f32` components of persistent state per slot.
    pub(super) stride: u32,
    /// Identity of the emitter's dynamics + seed; a change invalidates the state and its checkpoints.
    pub(super) fingerprint: u64,
    /// `records * stride` persistent state floats (position, velocity, age, lifetime, ordinal bits).
    pub(super) state: Buffer,
    /// Free-slot indices for death/reuse; initialised to every slot free.
    pub(super) free_list: Buffer,
    /// Atomic count of free slots; initialised to `records`.
    pub(super) free_count: Buffer,
    /// Atomic spawn ordinal counter; initialised to 0.
    pub(super) spawn_counter: Buffer,
    /// The particle events of the current tick, for event links (host bindings HB9b): see
    /// `aestra_gpu::particle_event_words`. Cleared each tick, never checkpointed; four words when the
    /// emitter reports none.
    pub(super) events: Buffer,
    /// The last fixed tick the persistent state was advanced to. A target below this is a backward seek.
    pub(super) last_tick: u32,
    /// Fractional spawn carry, so a non-integer per-tick spawn rate emits the right long-run count.
    pub(super) spawn_accumulator: f32,
    /// GPU-resident checkpoints, ascending by tick (hybrid roadmap M7).
    pub(super) checkpoints: Vec<StatefulCheckpoint>,
    pub(super) history_policy: PlaybackHistoryPolicy,
    /// The spawn placement the live state is advancing under.
    pub(super) placement: aestra_runtime::SpawnPlacement,
    /// The live state mixes spawns from more than one placement (the emitter moved mid-run), so it is
    /// no longer what a replay reproduces: no checkpoint is captured from it until the next reset.
    pub(super) mixed_placement: bool,
    /// The host input events the state and its checkpoints were advanced under (event system E2–E3):
    /// see [`HostEventHistory`].
    pub(super) host_events: Vec<(u64, u64)>,
    /// A changed input history differs from this tick on, which the state already advanced past:
    /// the next advance first goes back to a checkpoint at or before it.
    pub(super) rewind: Option<u32>,
}

/// Fixed tick cadence between checkpoints (~1/3 s at 60 Hz).
pub(super) const STATEFUL_CHECKPOINT_CADENCE: u32 = 20;
/// Checkpoint count budget per emitter; exceeding it coarsens the store (drops every other), doubling
/// the effective cadence and keeping memory bounded with full-timeline coverage.
pub(super) const MAX_STATEFUL_CHECKPOINTS: usize = 64;

impl StatefulPersistentState {
    pub(super) fn set_history_policy(&mut self, policy: PlaybackHistoryPolicy) {
        self.history_policy = policy;
        if !policy.captures_checkpoints() {
            self.checkpoints.clear();
        }
    }

    /// Allocates and initialises one emitter's persistent buffers for `records` slots: zeroed state, a
    /// full free list (`0..records`), a free count of `records`, and a spawn counter of 0. All four
    /// buffers are copy source+dest so they can be snapshot to / restored from a checkpoint.
    pub(super) fn allocate(
        render_device: &RenderDevice,
        records: u32,
        stride: u32,
        fingerprint: u64,
        reports_events: bool,
    ) -> Self {
        let (state, free_list, free_count, spawn_counter) =
            Self::fresh_buffers(render_device, records, stride);
        let event_bytes = if reports_events {
            aestra_gpu::particle_event_words(aestra_runtime::PARTICLE_EVENT_CAPACITY) as u64 * 4
        } else {
            16
        };
        let events = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("aestra stateful particle events"),
            contents: &vec![0u8; event_bytes as usize],
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
        });
        Self {
            records,
            stride,
            fingerprint,
            state,
            free_list,
            free_count,
            spawn_counter,
            events,
            last_tick: 0,
            spawn_accumulator: 0.0,
            checkpoints: Vec::new(),
            history_policy: PlaybackHistoryPolicy::default(),
            placement: aestra_runtime::SpawnPlacement::IDENTITY,
            mixed_placement: false,
            host_events: Vec::new(),
            rewind: None,
        }
    }

    /// Adopts a changed host input history (event system E2–E3): checkpoints past the first tick it
    /// differs at are dropped, and a state already past that tick rewinds on its next advance.
    pub(super) fn set_host_events(&mut self, events: &[(u64, u64)]) {
        let Some(divergence) = HostEventHistory::divergence(&self.host_events, events) else {
            return;
        };
        self.host_events = events.to_vec();
        let divergence = u32::try_from(divergence).unwrap_or(u32::MAX);
        self.checkpoints
            .retain(|checkpoint| checkpoint.tick <= divergence);
        if self.last_tick > divergence {
            self.rewind = Some(
                self.rewind
                    .map_or(divergence, |rewind| rewind.min(divergence)),
            );
        }
    }

    /// Moves future spawns to `placement` without restarting (a gizmo drag stays live). Particles
    /// already in flight keep their motion, so the checkpoints — recorded under the old placement —
    /// are dropped and none is captured until a reset replays under a single placement.
    pub(super) fn set_placement(&mut self, placement: aestra_runtime::SpawnPlacement) {
        if self.placement == placement {
            return;
        }
        self.placement = placement;
        self.checkpoints.clear();
        // A state still at tick 0 holds no spawns yet, so it is not mixed.
        self.mixed_placement = self.last_tick > 0;
    }

    /// Creates the four persistent buffers initialised to tick 0.
    pub(super) fn fresh_buffers(
        render_device: &RenderDevice,
        records: u32,
        stride: u32,
    ) -> (Buffer, Buffer, Buffer, Buffer) {
        const COPYABLE: BufferUsages = BufferUsages::STORAGE
            .union(BufferUsages::COPY_SRC)
            .union(BufferUsages::COPY_DST);
        let state_floats = records as usize * stride as usize;
        let state = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("aestra stateful state"),
            contents: &vec![0_u8; state_floats * std::mem::size_of::<f32>()],
            usage: COPYABLE,
        });
        let free_list_bytes: Vec<u8> = (0..records).flat_map(u32::to_le_bytes).collect();
        let free_list = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("aestra stateful free list"),
            contents: &free_list_bytes,
            usage: COPYABLE,
        });
        let free_count = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("aestra stateful free count"),
            contents: &records.to_le_bytes(),
            usage: COPYABLE,
        });
        let spawn_counter = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("aestra stateful spawn counter"),
            contents: &0_u32.to_le_bytes(),
            usage: COPYABLE,
        });
        (state, free_list, free_count, spawn_counter)
    }

    /// Re-initialises the four buffers to tick 0 (keeping the checkpoint store), for a backward seek
    /// before the earliest checkpoint.
    pub(super) fn reset_to_zero(&mut self, render_device: &RenderDevice) {
        let (state, free_list, free_count, spawn_counter) =
            Self::fresh_buffers(render_device, self.records, self.stride);
        self.state = state;
        self.free_list = free_list;
        self.free_count = free_count;
        self.spawn_counter = spawn_counter;
        self.last_tick = 0;
        self.spawn_accumulator = 0.0;
        self.mixed_placement = false;
    }

    /// Captures a GPU-resident checkpoint of the current state at `tick` (copying all four buffers
    /// GPU→GPU), unless one already exists at that tick. Coarsens the store when it exceeds the budget.
    pub(super) fn capture(
        &mut self,
        render_device: &RenderDevice,
        encoder: &mut CommandEncoder,
        tick: u32,
    ) -> u64 {
        if !self.history_policy.captures_checkpoints()
            || self.mixed_placement
            || self
                .checkpoints
                .iter()
                .any(|checkpoint| checkpoint.tick == tick)
        {
            return 0;
        }
        let (state, free_list, free_count, spawn_counter) =
            Self::fresh_buffers(render_device, self.records, self.stride);
        encoder.copy_buffer_to_buffer(&self.state, 0, &state, 0, self.state.size());
        encoder.copy_buffer_to_buffer(&self.free_list, 0, &free_list, 0, self.free_list.size());
        encoder.copy_buffer_to_buffer(&self.free_count, 0, &free_count, 0, self.free_count.size());
        encoder.copy_buffer_to_buffer(
            &self.spawn_counter,
            0,
            &spawn_counter,
            0,
            self.spawn_counter.size(),
        );
        let checkpoint = StatefulCheckpoint {
            tick,
            state,
            free_list,
            free_count,
            spawn_counter,
            spawn_accumulator: self.spawn_accumulator,
        };
        // Insert keeping the store ascending by tick.
        let position = self
            .checkpoints
            .partition_point(|existing| existing.tick < tick);
        self.checkpoints.insert(position, checkpoint);
        if self.checkpoints.len() > MAX_STATEFUL_CHECKPOINTS {
            retain_every_other(&mut self.checkpoints);
        }
        self.state.size()
            + self.free_list.size()
            + self.free_count.size()
            + self.spawn_counter.size()
    }

    /// Restores the checkpoint captured exactly at `tick` (fluid F2b's joint seek), setting the last
    /// tick and spawn carry. False when this store has no checkpoint there.
    pub(super) fn restore_at(&mut self, encoder: &mut CommandEncoder, tick: u32) -> bool {
        if !self
            .checkpoints
            .iter()
            .any(|checkpoint| checkpoint.tick == tick)
        {
            return false;
        }
        if let Some((at, accumulator)) = self.restore_nearest(encoder, tick) {
            self.last_tick = at;
            self.spawn_accumulator = accumulator;
        }
        true
    }

    /// Restores the nearest checkpoint at or before `target` (copying its four buffers back GPU→GPU),
    /// returning its `(tick, spawn_accumulator)`, or `None` when no checkpoint is at or before `target`.
    pub(super) fn restore_nearest(
        &self,
        encoder: &mut CommandEncoder,
        target: u32,
    ) -> Option<(u32, f32)> {
        // The store is ascending by tick; the nearest at or before `target` is just before the first
        // one past it.
        let index = self
            .checkpoints
            .partition_point(|checkpoint| checkpoint.tick <= target)
            .checked_sub(1)?;
        let checkpoint = &self.checkpoints[index];
        encoder.copy_buffer_to_buffer(&checkpoint.state, 0, &self.state, 0, self.state.size());
        encoder.copy_buffer_to_buffer(
            &checkpoint.free_list,
            0,
            &self.free_list,
            0,
            self.free_list.size(),
        );
        encoder.copy_buffer_to_buffer(
            &checkpoint.free_count,
            0,
            &self.free_count,
            0,
            self.free_count.size(),
        );
        encoder.copy_buffer_to_buffer(
            &checkpoint.spawn_counter,
            0,
            &self.spawn_counter,
            0,
            self.spawn_counter.size(),
        );
        Some((checkpoint.tick, checkpoint.spawn_accumulator))
    }
}

/// Halves a store by retaining every other entry (index 0, 2, 4, …), doubling the effective cadence
/// while keeping the first and (for an odd length) last, so full-timeline coverage survives.
pub(super) fn retain_every_other<T>(items: &mut Vec<T>) {
    let mut keep = false;
    items.retain(|_| {
        keep = !keep;
        keep
    });
}

/// Per-entity persistent state for stateful effects, kept in the render world across frames (the
/// `TrailHistories` pattern). One [`StatefulPersistentState`] per stateful emitter, in the same order
/// as the effect's `stateful_dispatch`. Populated by [`prepare_stateful_states`].
#[derive(Resource, Default)]
pub(super) struct StatefulStates(pub(super) BTreeMap<Entity, Vec<StatefulPersistentState>>);

/// Allocates and retains one set of persistent state buffers per stateful emitter, reallocating only
/// when an emitter's capacity changes (or the emitter set changes) and dropping them when the effect
/// stops being stateful or is removed. This is the render-world lifecycle the analytic path does not
/// need (it recomputes every frame); the stateful dispatch reads these buffers.
pub(super) fn prepare_stateful_states(
    mut states: ResMut<StatefulStates>,
    render_device: Res<RenderDevice>,
    effects: Query<(Entity, &GpuEffectBuffers)>,
) {
    states.0.retain(|entity, _| {
        effects
            .get(*entity)
            .is_ok_and(|(_, effect)| !effect.stateful_dispatch.is_empty())
    });
    for (entity, effect) in &effects {
        if effect.stateful_dispatch.is_empty() {
            continue;
        }
        let stride = effect.simulation_state.stride;
        let current = states.0.get(&entity);
        // Reallocate (dropping the checkpoints) when the emitter set, a capacity, or a dynamics
        // fingerprint changes — any of which means a different simulation.
        let matches = current.is_some_and(|states| {
            states.len() == effect.stateful_dispatch.len()
                && states
                    .iter()
                    .zip(&effect.stateful_dispatch)
                    .all(|(state, dispatch)| {
                        state.records == dispatch.capacity
                            && state.stride == stride
                            && state.fingerprint == dispatch.fingerprint()
                    })
        });
        if !matches {
            let allocated = effect
                .stateful_dispatch
                .iter()
                .map(|dispatch| {
                    let mut state = StatefulPersistentState::allocate(
                        &render_device,
                        dispatch.capacity,
                        stride,
                        dispatch.fingerprint(),
                        dispatch.event_mask != 0,
                    );
                    state.placement = dispatch.placement;
                    state.host_events = effect.host_events.events.clone();
                    state.set_history_policy(effect.history_policy);
                    state
                })
                .collect();
            states.0.insert(entity, allocated);
        } else if let Some(states) = states.0.get_mut(&entity) {
            for state in states.iter_mut() {
                state.set_history_policy(effect.history_policy);
                state.set_host_events(&effect.host_events.events);
            }
            // Only the emitter transform changed: keep simulating (see `set_placement`). A placement
            // scheduled from a binding trace (host bindings HB8) is part of the replayed history.
            for (state, dispatch) in states.iter_mut().zip(&effect.stateful_dispatch) {
                if dispatch
                    .schedule
                    .as_ref()
                    .is_none_or(|schedule| schedule.placement.is_empty())
                {
                    state.set_placement(dispatch.placement);
                }
            }
        }
    }
}

/// The canonical fixed simulation tick, matching `aestra_runtime::StatefulSimulation::TICK_DT`.
pub(super) const STATEFUL_TICK_DT: f32 = 1.0 / 60.0;

/// The stateful params words for one dispatch (`aestra_gpu::STATEFUL_SIMULATION_PARAM_WORDS`):
/// `spawn_per_tick` varies across advance ticks and `subtick` is the presentation-interpolation time
/// `present` uses. `live` ticks count homing arrivals for the `impact` event; replays do not.
/// `tick` is the tick being advanced from, whose recorded input a binding trace supplies (host
/// bindings HB8); `None` (presentation) uses the frame's.
pub(super) fn stateful_params_bytes(
    dispatch: &StatefulDispatch,
    spawn_per_tick: u32,
    subtick: f32,
    live: bool,
    tick: Option<u32>,
) -> Vec<u8> {
    let scheduled = tick.and_then(|tick| Some((dispatch.schedule.as_deref()?, tick)));
    let homing_target = match scheduled {
        Some((schedule, tick)) if !schedule.homing.is_empty() => {
            TickSchedule::at(&schedule.homing, tick).flatten()
        }
        _ => dispatch.homing_target,
    };
    let placement = scheduled
        .and_then(|(schedule, tick)| TickSchedule::at(&schedule.placement, tick))
        .unwrap_or(dispatch.placement);
    let mut words = vec![0u32; aestra_gpu::STATEFUL_SIMULATION_PARAM_WORDS];
    words[aestra_gpu::STATEFUL_VELOCITY_MODE_INDEX] = dispatch.velocity_distribution;
    if let Some((spacing, limit)) = dispatch.distance_emission {
        words[aestra_gpu::STATEFUL_DISTANCE_BASE] = spacing.to_bits();
        words[aestra_gpu::STATEFUL_DISTANCE_BASE + 1] = limit;
    }
    words[..26].copy_from_slice(&[
        dispatch.capacity,
        spawn_per_tick,
        dispatch.seed as u32,
        (dispatch.seed >> 32) as u32,
        dispatch.speed.0.to_bits(),
        dispatch.speed.1.to_bits(),
        dispatch.lifetime.0.to_bits(),
        dispatch.lifetime.1.to_bits(),
        STATEFUL_TICK_DT.to_bits(),
        dispatch.gravity[0].to_bits(),
        dispatch.gravity[1].to_bits(),
        dispatch.gravity[2].to_bits(),
        dispatch.direction[0].to_bits(),
        dispatch.direction[1].to_bits(),
        dispatch.direction[2].to_bits(),
        dispatch.spread.to_bits(),
        dispatch.drag.to_bits(),
        dispatch.emitter_index,
        dispatch.slot_offset,
        dispatch.turbulence.to_bits(),
        dispatch.shape_kind,
        dispatch.shape_radius.to_bits(),
        dispatch.shape_half_extents[0].to_bits(),
        dispatch.shape_half_extents[1].to_bits(),
        dispatch.shape_half_extents[2].to_bits(),
        subtick.to_bits(),
    ]);
    // Collider block (hybrid roadmap M10): a count word at 26, then up to MAX_COLLIDERS 10-word
    // records from 27 (see aestra_gpu::STATEFUL_COLLISION_WGSL).
    aestra_gpu::pack_stateful_colliders(&dispatch.colliders, &mut words);
    aestra_gpu::pack_spawn_placement(&placement, &mut words);
    aestra_gpu::pack_stateful_homing_counted(
        dispatch.homing.as_ref().map(|homing| &homing.config),
        homing_target.as_ref(),
        dispatch.arrival_word.filter(|_| live),
        &mut words,
    );
    aestra_gpu::pack_stateful_world(&dispatch.world_from_effect, &mut words);
    aestra_gpu::pack_stateful_events(
        dispatch.event_mask,
        aestra_runtime::PARTICLE_EVENT_CAPACITY,
        &mut words,
    );
    // A host's kill (event system E2b) retires everything from its tick on.
    aestra_gpu::pack_stateful_kill(
        tick.zip(dispatch.cutoffs.kill_tick)
            .is_some_and(|(tick, kill)| u64::from(tick) >= kill),
        &mut words,
    );
    let look = &dispatch.appearance;
    aestra_gpu::pack_stateful_appearance(
        &look.size,
        &look.opacity,
        &look.color,
        look.max_scale,
        &mut words,
    );
    words.into_iter().flat_map(u32::to_le_bytes).collect()
}

/// Ticks one frame may advance and still count as live playback, whose runtime events (host bindings
/// HB9) are raised: real time at down to 6 frames per second. Longer advances are catch-up after a
/// seek or a rebuild — replays of ticks whose events were raised already, or never happened live.
pub(super) const LIVE_EVENT_TICKS: u32 = 10;

/// Whether advancing from `last` to `target` is live playback (see [`LIVE_EVENT_TICKS`]): forward, and
/// no longer than real time allows.
pub(super) fn is_live_advance(last: u32, target: u32) -> bool {
    target >= last && target - last <= LIVE_EVENT_TICKS
}

/// The host's physics colliders around an effect, on the device for this frame (host bindings HB10):
/// a few kilobytes at most, so simply uploaded again every frame the bodies may have moved.
pub(super) fn physics_scene_buffer(device: &RenderDevice, words: &[u32]) -> Buffer {
    device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("aestra particle physics"),
        contents: &words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect::<Vec<u8>>(),
        usage: BufferUsages::STORAGE,
    })
}

/// The effect-wide render buffers a stateful emitter presents into.
pub(super) struct StatefulRenderBuffers<'a> {
    pub(super) particles: &'a Buffer,
    pub(super) alive: &'a Buffer,
    pub(super) indirect: &'a Buffer,
    pub(super) counters: &'a Buffer,
    /// The host's world SDF, for `World` colliders (host bindings HB10).
    pub(super) world: &'a Buffer,
    /// The host's physics colliders around the effect, for `Physics` colliders (HB10).
    pub(super) physics: &'a Buffer,
}

/// A bind group over one emitter's persistent buffers, `params`, and the effect's render buffers.
pub(super) fn stateful_bind_group(
    device: &RenderDevice,
    layout: &BindGroupLayout,
    persistent: &StatefulPersistentState,
    params: &Buffer,
    render: &StatefulRenderBuffers<'_>,
) -> BindGroup {
    device.create_bind_group(
        Some("aestra_gpu_stateful"),
        layout,
        &BindGroupEntries::sequential((
            persistent.state.as_entire_buffer_binding(),
            persistent.free_list.as_entire_buffer_binding(),
            persistent.free_count.as_entire_buffer_binding(),
            persistent.spawn_counter.as_entire_buffer_binding(),
            params.as_entire_buffer_binding(),
            render.particles.as_entire_buffer_binding(),
            render.alive.as_entire_buffer_binding(),
            render.indirect.as_entire_buffer_binding(),
            render.counters.as_entire_buffer_binding(),
            persistent.events.as_entire_buffer_binding(),
            render.world.as_entire_buffer_binding(),
            render.physics.as_entire_buffer_binding(),
        )),
    )
}

/// One tick's bind group and params (with this tick's spawn count, advancing the fractional spawn
/// carry). A `live` tick raises its events; a replayed one does not.
pub(super) fn stateful_tick_group(
    device: &RenderDevice,
    layout: &BindGroupLayout,
    persistent: &mut StatefulPersistentState,
    dispatch: &StatefulDispatch,
    render: &StatefulRenderBuffers<'_>,
    live: bool,
) -> (BindGroup, Buffer) {
    // A host's stop_emitting / kill (event system E2b): nothing spawns from its tick on.
    let stopped = dispatch
        .cutoffs
        .stop_tick
        .is_some_and(|stop| u64::from(persistent.last_tick) >= stop);
    let spawn_count = if stopped {
        0
    } else {
        persistent.spawn_accumulator += dispatch.spawn_rate * STATEFUL_TICK_DT;
        let spawn_count = persistent.spawn_accumulator.floor();
        persistent.spawn_accumulator -= spawn_count;
        let burst = if persistent.last_tick == dispatch.burst_tick {
            dispatch.burst_count
        } else {
            0
        };
        (spawn_count as u32)
            .saturating_add(burst)
            .min(dispatch.capacity)
    };
    let params = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("aestra stateful tick params"),
        contents: &stateful_params_bytes(
            dispatch,
            spawn_count,
            0.0,
            live,
            Some(persistent.last_tick),
        ),
        usage: BufferUsages::STORAGE,
    });
    let group = stateful_bind_group(device, layout, persistent, &params, render);
    (group, params)
}

/// Resets one emitter's indirect instance count, then presents + compacts its live slots into the
/// effect-wide alive/indirect/counters buffers the render path draws. The vertex count (word 0 of the
/// draw command) is preserved; only the instance count (word 1) is zeroed so the compaction rebuilds
/// it. Presentation interpolation (hybrid roadmap M8) extrapolates by the sub-tick time — how far past
/// the last simulated tick the requested time is, bounded to one tick in case the fixed-tick advance
/// lags the presentation time.
#[allow(clippy::too_many_arguments)]
pub(super) fn present_stateful_emitter(
    device: &RenderDevice,
    encoder: &mut CommandEncoder,
    present: &ComputePipeline,
    order_present: Option<&ComputePipeline>,
    layout: &BindGroupLayout,
    persistent: &StatefulPersistentState,
    dispatch: &StatefulDispatch,
    render: &StatefulRenderBuffers<'_>,
    simulation_time: f32,
) {
    let instance_count_offset = u64::from(dispatch.emitter_index * 4 + 1) * 4;
    encoder.clear_buffer(render.indirect, instance_count_offset, Some(4));
    let subtick = (simulation_time - persistent.last_tick as f32 * STATEFUL_TICK_DT)
        .clamp(0.0, STATEFUL_TICK_DT);
    let params = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("aestra stateful present params"),
        contents: &stateful_params_bytes(dispatch, 0, subtick, false, None),
        usage: BufferUsages::STORAGE,
    });
    let group = stateful_bind_group(device, layout, persistent, &params, render);
    let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
        label: Some("aestra stateful present"),
        timestamp_writes: None,
    });
    pass.set_bind_group(0, &group, &[]);
    pass.set_pipeline(present);
    pass.dispatch_workgroups(dispatch.capacity.div_ceil(WORKGROUP_SIZE), 1, 1);
    drop(pass);
    if let Some(order_present) = order_present {
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("aestra stateful presentation order"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, &group, &[]);
        pass.set_pipeline(order_present);
        pass.dispatch_workgroups(1, 1, 1);
    }
}

/// Encodes one stateful *emitter's* per-frame GPU work (hybrid roadmap M6/M7): advance its persistent
/// state from its last tick to the tick for `simulation_time` (death loop + spawn per tick), capturing
/// GPU-resident checkpoints at a fixed cadence, then present. A backward seek restores the nearest
/// checkpoint at or before the target and replays only the remainder forward (the derived
/// restart+replay seek mode; never a reverse integration). The caller clears the shared live counter
/// once before the emitter loop and stamps the statistics telemetry once after it. Every kernel here is
/// conformance-proven on real GPU.
#[allow(clippy::too_many_arguments)]
pub(super) fn dispatch_stateful_effect(
    device: &RenderDevice,
    encoder: &mut CommandEncoder,
    death_integrate: &ComputePipeline,
    spawn: &ComputePipeline,
    present: &ComputePipeline,
    order_present: Option<&ComputePipeline>,
    layout: &BindGroupLayout,
    persistent: &mut StatefulPersistentState,
    dispatch: &StatefulDispatch,
    render: &StatefulRenderBuffers<'_>,
    simulation_time: f32,
    seek_quality: SeekQuality,
) {
    let target_tick = aestra_runtime::trace_tick(simulation_time).min(u64::from(u32::MAX)) as u32;
    // A changed host input history (event system E2–E3) replays from where it differs; a replay is
    // not live.
    let rewind = persistent.rewind.take();
    let live = rewind.is_none() && is_live_advance(persistent.last_tick, target_tick);
    let back_to = rewind.map_or(target_tick, |tick| tick.min(target_tick));
    if back_to < persistent.last_tick {
        match persistent.restore_nearest(encoder, back_to) {
            Some((tick, accumulator)) => {
                persistent.last_tick = tick;
                persistent.spawn_accumulator = accumulator;
            }
            None => persistent.reset_to_zero(device),
        }
    }
    let workgroups = dispatch.capacity.div_ceil(WORKGROUP_SIZE);
    // Advance in cadence-aligned segments (one compute pass each), capturing a GPU-resident checkpoint
    // at each cadence boundary reached. Bounded per frame so a large jump cannot stall the GPU.
    let mut remaining =
        (target_tick - persistent.last_tick).min(stateful_catchup_budget(seek_quality));
    while remaining > 0 {
        let to_boundary =
            STATEFUL_CHECKPOINT_CADENCE - (persistent.last_tick % STATEFUL_CHECKPOINT_CADENCE);
        let segment = remaining.min(to_boundary);
        let groups: Vec<BindGroup> = (0..segment)
            .map(|_| {
                let group =
                    stateful_tick_group(device, layout, persistent, dispatch, render, live).0;
                // Each encoded tick needs its own burst/cutoff/trace inputs, even
                // when all dispatches share one compute pass.
                persistent.last_tick += 1;
                group
            })
            .collect();
        {
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("aestra stateful advance"),
                timestamp_writes: None,
            });
            for group in &groups {
                pass.set_bind_group(0, group, &[]);
                pass.set_pipeline(death_integrate);
                pass.dispatch_workgroups(workgroups, 1, 1);
                pass.set_pipeline(spawn);
                pass.dispatch_workgroups(workgroups, 1, 1);
            }
        }
        remaining -= segment;
        if persistent
            .last_tick
            .is_multiple_of(STATEFUL_CHECKPOINT_CADENCE)
        {
            persistent.capture(device, encoder, persistent.last_tick);
        }
    }
    present_stateful_emitter(
        device,
        encoder,
        present,
        order_present,
        layout,
        persistent,
        dispatch,
        render,
        simulation_time,
    );
}
