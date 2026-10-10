//! Shared lockstep particle/domain/event scheduling and stateful reset/telemetry.
use super::{
    catchup_pacing::{CatchupPacer, stateful_catchup_budget},
    effect_inputs::{RouteWiring, StatefulDispatch, event_link_counter_base},
    stateful_simulation::*,
};
use aestra_gpu::WORKGROUP_SIZE;
use aestra_runtime::SeekQuality;
use bevy::{
    prelude::*,
    render::{
        render_resource::{
            BindGroupLayout, Buffer, BufferInitDescriptor, BufferUsages, CommandEncoder,
            ComputePassDescriptor, ComputePipeline,
        },
        renderer::RenderDevice,
    },
};

/// The history coordination boundary; the shipping trail observer implements it.
/// It keeps lockstep scheduling independent of how history records are rendered.
pub(super) trait CoupledHistory {
    fn contains(&self, tick: u32) -> bool;
    fn is_current(&self, tick: u32) -> bool;
    fn is_unobserved(&self) -> bool;
    fn retain_through(&mut self, tick: u32);
    fn restore(&mut self, encoder: &mut CommandEncoder, tick: u32);
    fn reset_history(&mut self, encoder: &mut CommandEncoder);
    fn prepare(&self, device: &RenderDevice, encoder: &mut CommandEncoder, tick: u32);
    fn record(&mut self, encoder: &mut CommandEncoder, tick: u32);
    fn capture(&mut self, device: &RenderDevice, encoder: &mut CommandEncoder, tick: u32) -> u64;
}

/// What couples an effect's stateful emitters to its domains (fluid F2b): the domains' timelines
/// (indexed like `CompiledEffect::all_extension_stages`), the host inputs they tick with, and the
/// Follow Field pipeline.
pub(super) struct Coupling<'a> {
    pub domains: &'a mut [Option<crate::execution::StageTimeline>],
    pub inputs: crate::execution::StageInputs<'a>,
    pub follower: &'a crate::execution::FieldFollowPipeline,
    pub spawner: &'a crate::execution::DomainSpawnPipeline,
    /// Gathers event links' events (host bindings HB9b).
    pub gatherer: &'a crate::execution::EventGatherPipeline,
}

/// The latest tick at or before `target` that every store holds a checkpoint for.
pub(super) fn joint_checkpoint_tick(
    persistent_states: &[StatefulPersistentState],
    domains: &[Option<crate::execution::StageTimeline>],
    target: u32,
    trails: Option<&dyn CoupledHistory>,
) -> Option<u32> {
    let first = persistent_states.first()?;
    first
        .checkpoints
        .iter()
        .rev()
        .map(|checkpoint| checkpoint.tick)
        .filter(|tick| *tick <= target)
        .find(|tick| {
            trails.is_none_or(|trails| trails.contains(*tick))
                && persistent_states
                    .iter()
                    .all(|state| state.checkpoints.iter().any(|c| c.tick == *tick))
                && domains
                    .iter()
                    .flatten()
                    .all(|domain| domain.checkpoint_ticks().contains(tick))
        })
}

/// Advances an effect whose stateful emitters follow a domain's field (fluid F2b) — every store in
/// lockstep, tick by tick: each tick advances the domains one tick, then every stateful emitter one
/// tick (death loop + spawn), then pulls the following emitters toward their fields. All stores
/// checkpoint at the same cadence; a backward seek (or stores that fell out of step, e.g. a rebuilt
/// domain) restores every store at the latest tick they all hold — else resets them all to tick 0 —
/// and replays, so scrubbing reproduces the uninterrupted run. Then every emitter presents.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_coupled_stateful(
    device: &RenderDevice,
    encoder: &mut CommandEncoder,
    pipelines: (
        &ComputePipeline,
        &ComputePipeline,
        &ComputePipeline,
        Option<&ComputePipeline>,
    ),
    layout: &BindGroupLayout,
    persistent_states: &mut [StatefulPersistentState],
    dispatches: &[StatefulDispatch],
    coupling: Coupling<'_>,
    links: &[aestra_runtime::CompiledEventLink],
    routes: &RouteWiring<'_>,
    render: &StatefulRenderBuffers<'_>,
    simulation_time: f32,
    budget: u32,
    mut trails: Option<&mut dyn CoupledHistory>,
) -> (u32, u64) {
    let bursts = routes.bursts;
    let birth_outputs: Vec<bool> = dispatches
        .iter()
        .map(|dispatch| {
            routes.outputs.iter().any(|(route, _)| {
                route.source == dispatch.emitter_index as usize
                    && route.trigger == aestra_core::EventTrigger::OnSpawn
            })
        })
        .collect();
    let (death_integrate, spawn, present, order_present) = pipelines;
    let domains = coupling.domains;
    // Each link's events and emission list (host bindings HB9b): the dispatches at either end, and a
    // list buffer reused across this frame's ticks.
    let dispatch_of = |emitter: usize| {
        dispatches
            .iter()
            .position(|dispatch| dispatch.emitter_index as usize == emitter)
    };
    let link_ends: Vec<(usize, usize, usize, &aestra_runtime::CompiledEventLink)> = links
        .iter()
        .enumerate()
        .filter_map(|(index, link)| {
            Some((
                index,
                dispatch_of(link.source)?,
                dispatch_of(link.target)?,
                link,
            ))
        })
        .collect();
    let counter_base = event_link_counter_base(dispatches);
    let target = aestra_runtime::trace_tick(simulation_time).min(u64::from(u32::MAX)) as u32;
    let last = persistent_states.first().map_or(0, |state| state.last_tick);
    let in_step = persistent_states
        .iter()
        .all(|state| state.last_tick == last)
        && domains
            .iter()
            .flatten()
            .all(|domain| domain.last_tick() == last);
    // A changed host input history (event system E2–E3) replays every store from where it differs.
    let rewind = persistent_states
        .iter_mut()
        .filter_map(|state| state.rewind.take())
        .min();
    let history_discontinuity = trails
        .as_ref()
        .is_some_and(|trails| !trails.is_current(last));
    if let Some(rewind) = rewind
        && let Some(trails) = trails.as_mut()
    {
        trails.retain_through(rewind);
    }
    if !in_step || target < last || rewind.is_some() || history_discontinuity {
        let back_to = rewind.map_or(target.min(last), |tick| tick.min(target).min(last));
        match joint_checkpoint_tick(persistent_states, domains, back_to, trails.as_deref()) {
            Some(tick) => {
                for state in persistent_states.iter_mut() {
                    state.restore_at(encoder, tick);
                }
                for domain in domains.iter_mut().flatten() {
                    domain.restore_to(encoder, tick);
                }
                if let Some(trails) = trails.as_mut() {
                    trails.restore(encoder, tick);
                }
            }
            None => {
                for state in persistent_states.iter_mut() {
                    state.reset_to_zero(device);
                }
                for domain in domains.iter_mut().flatten() {
                    domain.restore_to(encoder, 0);
                }
                if let Some(trails) = trails.as_mut() {
                    trails.reset_history(encoder);
                }
            }
        }
    }
    let now = persistent_states.first().map_or(0, |state| state.last_tick);
    let live =
        in_step && rewind.is_none() && !history_discontinuity && is_live_advance(last, target);
    let ticks = target.saturating_sub(now).min(budget);
    // Observe tick zero too, so a paused empty effect has valid history/telemetry.
    if let Some(trails) = trails.as_mut()
        && trails.is_unobserved()
    {
        trails.prepare(device, encoder, now);
        for (dispatch, persistent) in dispatches.iter().zip(persistent_states.iter()) {
            present_stateful_emitter(
                device,
                encoder,
                present,
                order_present,
                layout,
                persistent,
                dispatch,
                render,
                now as f32 * STATEFUL_TICK_DT,
            );
        }
        trails.record(encoder, now);
    }
    let lists: Vec<Buffer> = if ticks > 0 {
        link_ends
            .iter()
            .map(|(_, source, _, link)| {
                let capacity = crate::execution::EventGatherPipeline::list_capacity(
                    aestra_runtime::event_capture_capacity(
                        dispatches[*source].capacity,
                        link.trigger,
                    ),
                    link.count,
                );
                device.create_buffer_with_data(&BufferInitDescriptor {
                    label: Some("aestra event list"),
                    contents: &vec![
                        0u8;
                        crate::execution::EventGatherPipeline::list_bytes(capacity)
                            as usize
                    ],
                    usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    let mut checkpoint_capture_bytes = 0;
    for _ in 0..ticks {
        let mut tick_params = Vec::with_capacity(dispatches.len());
        let next = persistent_states[0].last_tick + 1;
        for domain in domains.iter_mut().flatten() {
            if let Err(error) = domain.advance_to(
                device.wgpu_device(),
                encoder,
                next,
                1,
                coupling.inputs,
                None,
            ) {
                warn!("coupled domain stopped: {error}");
            }
        }
        for (index, (dispatch, persistent)) in dispatches
            .iter()
            .zip(persistent_states.iter_mut())
            .enumerate()
        {
            let (group, params) =
                stateful_tick_group(device, layout, persistent, dispatch, render, live);
            if dispatch.event_mask != 0 {
                encoder.clear_buffer(&persistent.events, 0, Some(4));
            }
            {
                let workgroups = dispatch.capacity.div_ceil(WORKGROUP_SIZE);
                let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                    label: Some("aestra coupled stateful tick"),
                    timestamp_writes: None,
                });
                pass.set_bind_group(0, &group, &[]);
                pass.set_pipeline(death_integrate);
                pass.dispatch_workgroups(workgroups, 1, 1);
                pass.set_pipeline(spawn);
                pass.dispatch_workgroups(workgroups, 1, 1);
            }
            // Particles the domain asked for this tick are born after the emitter's own (fluid F10).
            if let Some(spawn) = &dispatch.domain_spawn
                && let Some(Some(domain)) = domains.get(spawn.stage)
                && let Some(emission) = domain.executor().buffer(spawn.emission.resource.as_str())
            {
                coupling.spawner.encode(
                    device.wgpu_device(),
                    encoder,
                    crate::execution::SpawnState {
                        state: &persistent.state,
                        free_list: &persistent.free_list,
                        free_count: &persistent.free_count,
                        spawn_counter: &persistent.spawn_counter,
                        params: &params,
                        output_births: (birth_outputs[index]
                            && live
                            && u64::from(next) > routes.output_suppress_through)
                            .then_some(&persistent.events),
                    },
                    emission,
                    spawn,
                );
            }
            if let Some(follow) = &dispatch.field_follow
                && let Some(Some(domain)) = domains.get(follow.stage)
                && let Some(field) = domain.executor().field_buffers(&follow.field)
            {
                coupling.follower.encode(
                    device.wgpu_device(),
                    encoder,
                    &persistent.state,
                    dispatch.capacity,
                    field,
                    follow,
                    STATEFUL_TICK_DT,
                );
            }
            tick_params.push(params);
        }
        // Event links (host bindings HB9b): after every emitter's tick, each link's events of it
        // become its target's particles, in link order.
        for ((index, source, target, link), list) in link_ends.iter().zip(&lists) {
            let list_capacity = crate::execution::EventGatherPipeline::list_capacity(
                aestra_runtime::event_capture_capacity(dispatches[*source].capacity, link.trigger),
                link.count,
            );
            coupling.gatherer.encode_with_overflow(
                device.wgpu_device(),
                encoder,
                &persistent_states[*source].events,
                crate::execution::EventEmissionList {
                    buffer: list,
                    capacity: list_capacity,
                },
                link,
                crate::execution::EventLinkCounters {
                    buffer: render.counters,
                    requested_word: counter_base.map_or(u32::MAX, |base| base + *index as u32 * 3),
                    dropped_word: counter_base
                        .map_or(u32::MAX, |base| base + *index as u32 * 3 + 1),
                },
            );
            let persistent = &persistent_states[*target];
            coupling.spawner.encode_with_acceptance(
                device.wgpu_device(),
                encoder,
                crate::execution::SpawnState {
                    state: &persistent.state,
                    free_list: &persistent.free_list,
                    free_count: &persistent.free_count,
                    spawn_counter: &persistent.spawn_counter,
                    params: &tick_params[*target],
                    output_births: (birth_outputs[*target]
                        && live
                        && u64::from(next) > routes.output_suppress_through)
                        .then_some(&persistent.events),
                },
                list,
                &crate::execution::EventGatherPipeline::spawn(link, list_capacity),
                crate::execution::SpawnAcceptanceCounter {
                    buffer: render.counters,
                    word: counter_base.map_or(u32::MAX, |base| base + *index as u32 * 3 + 2),
                },
            );
        }
        // Input routes (event system E3): after the links, the bursts the host's events of this
        // tick spawn, in route order.
        let tick = u64::from(persistent_states[0].last_tick);
        for burst in bursts.iter().filter(|burst| burst.tick == tick) {
            let Some(target) = dispatch_of(burst.target) else {
                continue;
            };
            let (list, spawn) = crate::execution::input_burst_list(burst);
            let list = device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("aestra input burst"),
                contents: &list
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<u8>>(),
                usage: BufferUsages::STORAGE,
            });
            let persistent = &persistent_states[target];
            coupling.spawner.encode(
                device.wgpu_device(),
                encoder,
                crate::execution::SpawnState {
                    state: &persistent.state,
                    free_list: &persistent.free_list,
                    free_count: &persistent.free_count,
                    spawn_counter: &persistent.spawn_counter,
                    params: &tick_params[target],
                    output_births: (birth_outputs[target]
                        && live
                        && u64::from(next) > routes.output_suppress_through)
                        .then_some(&persistent.events),
                },
                &list,
                &spawn,
            );
        }
        // Outputs follow ALL births, including host-input bursts. External birth records are
        // output-only: they cannot feed back into the already processed event links.
        // Particle output routes (event system E3): a live tick's events of their trigger,
        // aggregated into the tick's slot of the route's ring, which the host reads back. Replayed
        // ticks raise nothing.
        if live {
            let reached = persistent_states[0].last_tick + 1;
            for (route, ring) in routes.outputs {
                if u64::from(reached) <= routes.output_suppress_through {
                    continue;
                }
                if let Some(source) = dispatch_of(route.source) {
                    coupling.gatherer.encode_output(
                        device.wgpu_device(),
                        encoder,
                        &persistent_states[source].events,
                        crate::execution::ParticleOutputSlot {
                            counters: render.counters,
                            ring: *ring,
                            tick: reached,
                            epoch: routes.output_epoch,
                        },
                        route,
                    );
                }
            }
        }
        // Checkpoints capture each emitter after the tick's event spawns.
        for persistent in persistent_states.iter_mut() {
            persistent.last_tick += 1;
            if persistent
                .last_tick
                .is_multiple_of(STATEFUL_CHECKPOINT_CADENCE)
            {
                checkpoint_capture_bytes +=
                    persistent.capture(device, encoder, persistent.last_tick);
            }
        }
        if let Some(trails) = trails.as_mut() {
            trails.prepare(device, encoder, next);
            for (dispatch, persistent) in dispatches.iter().zip(persistent_states.iter()) {
                present_stateful_emitter(
                    device,
                    encoder,
                    present,
                    order_present,
                    layout,
                    persistent,
                    dispatch,
                    render,
                    next as f32 * STATEFUL_TICK_DT,
                );
            }
            trails.record(encoder, next);
            if next.is_multiple_of(STATEFUL_CHECKPOINT_CADENCE)
                && persistent_states.iter().all(|state| !state.mixed_placement)
            {
                checkpoint_capture_bytes += trails.capture(device, encoder, next);
            }
        }
    }
    // With histories, the canonical final tick is already presented and stamped;
    // never replace it with a sub-frame head or expire tails at an unprocessed seek target.
    if ticks == 0
        && let Some(trails) = trails.as_mut()
    {
        trails.prepare(device, encoder, now);
        for (dispatch, persistent) in dispatches.iter().zip(persistent_states.iter()) {
            present_stateful_emitter(
                device,
                encoder,
                present,
                order_present,
                layout,
                persistent,
                dispatch,
                render,
                now as f32 * STATEFUL_TICK_DT,
            );
        }
        trails.record(encoder, now);
    }
    if trails.is_none() {
        for (dispatch, persistent) in dispatches.iter().zip(persistent_states.iter()) {
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
    }
    (ticks, checkpoint_capture_bytes)
}
/// Stamps the particle-statistics telemetry trailer the analytic reset writes, so the live-count
/// readback accepts a stateful frame: `[MAGIC, context token, history epoch, time]` at the indirect
/// buffer's telemetry offset (`emitter_count * 4`). Called once per effect after every emitter has
/// presented; the per-emitter alive counts were rebuilt by each present's compaction.
pub(super) fn stamp_stateful_statistics(
    device: &RenderDevice,
    encoder: &mut CommandEncoder,
    indirect: &Buffer,
    emitter_count: u32,
    statistics_token: u32,
    history_epoch: u32,
    simulation_time: f32,
) {
    let telemetry: [u32; 4] = [
        aestra_gpu::PARTICLE_STATISTICS_MAGIC,
        statistics_token,
        history_epoch,
        simulation_time.to_bits(),
    ];
    let telemetry_src = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("aestra stateful statistics telemetry"),
        contents: &telemetry
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<u8>>(),
        usage: BufferUsages::COPY_SRC,
    });
    let telemetry_offset = u64::from(emitter_count * 4) * 4;
    encoder.copy_buffer_to_buffer(&telemetry_src, 0, indirect, telemetry_offset, 16);
}

/// Runs the per-emitter stateful dispatches for one effect. When `owns_shared_reset` is true (a fully
/// stateful effect, with no analytic reset), it clears the shared live counter before the emitter loop
/// and stamps the statistics telemetry after it; when false (a mixed effect), the analytic reset
/// already did both, so it only runs the emitters — their presents add to the counter the analytic
/// `simulate` already contributed to.
#[allow(clippy::too_many_arguments)]
pub(super) fn run_stateful_dispatches(
    device: &RenderDevice,
    encoder: &mut CommandEncoder,
    pipelines: (
        &ComputePipeline,
        &ComputePipeline,
        &ComputePipeline,
        Option<&ComputePipeline>,
    ),
    layout: &BindGroupLayout,
    persistent_states: &mut [StatefulPersistentState],
    dispatches: &[StatefulDispatch],
    links: &[aestra_runtime::CompiledEventLink],
    routes: Option<RouteWiring<'_>>,
    render: &StatefulRenderBuffers<'_>,
    coupling: Option<Coupling<'_>>,
    simulation_time: f32,
    seek_quality: SeekQuality,
    statistics_token: u32,
    history_epoch: u32,
    owns_shared_reset: bool,
    pacer: Option<&mut CatchupPacer>,
    trails: Option<&mut dyn CoupledHistory>,
) -> Option<(u32, u64)> {
    if owns_shared_reset {
        // Clear the shared live counter once, before any emitter's present bumps it.
        encoder.clear_buffer(render.counters, 0, Some(4));
    }
    // Emitters following a domain's field (fluid F2b) or born from it (fluid F10) advance in lockstep
    // with it; so do emitters joined by event links (host bindings HB9b), with one another, and
    // those of an effect with event routes (event system E3, `routes` is then given).
    let has_trails = trails.is_some();
    let coupled = has_trails
        || !links.is_empty()
        || routes.is_some()
        || dispatches
            .iter()
            .any(|dispatch| dispatch.field_follow.is_some() || dispatch.domain_spawn.is_some());
    let ticks = match coupling.filter(|_| coupled) {
        Some(coupling) => {
            // A coupled domain's ticks are fluid ticks: paced by frame time, not a fixed count.
            let budget = pacer.as_ref().map_or_else(
                || stateful_catchup_budget(seek_quality),
                |pacer| pacer.budget(seek_quality),
            );
            let (ticks, checkpoint_capture_bytes) = run_coupled_stateful(
                device,
                encoder,
                pipelines,
                layout,
                persistent_states,
                dispatches,
                coupling,
                links,
                &routes.unwrap_or_default(),
                render,
                simulation_time,
                budget,
                trails,
            );
            if let Some(pacer) = pacer {
                pacer.spent(ticks, budget);
            }
            Some((ticks, checkpoint_capture_bytes))
        }
        None if has_trails => {
            // Never silently use the independent path: it cannot observe every
            // emitter at a shared tick or restore histories with particles.
            warn!("stateful trail simulation is waiting for lockstep pipelines");
            return None;
        }
        None => {
            for (dispatch, persistent) in dispatches.iter().zip(persistent_states.iter_mut()) {
                dispatch_stateful_effect(
                    device,
                    encoder,
                    pipelines.0,
                    pipelines.1,
                    pipelines.2,
                    pipelines.3,
                    layout,
                    persistent,
                    dispatch,
                    render,
                    simulation_time,
                    seek_quality,
                );
            }
            None
        }
    };
    // Event overflow counts (host bindings HB9b), for the host to read back and report; after each
    // homing emitter's arrival count, the tick it was counted up to (event system E2b).
    for (dispatch, persistent) in dispatches.iter().zip(persistent_states.iter()) {
        if let Some(word) = dispatch.arrival_word {
            let tick = device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("aestra homing arrival tick"),
                contents: &persistent.last_tick.to_le_bytes(),
                usage: BufferUsages::COPY_SRC,
            });
            encoder.copy_buffer_to_buffer(&tick, 0, render.counters, u64::from(word + 1) * 4, 4);
        }
        if let Some(word) = dispatch.overflow_word {
            encoder.copy_buffer_to_buffer(
                &persistent.events,
                4,
                render.counters,
                u64::from(word) * 4,
                4,
            );
        }
    }
    if owns_shared_reset
        && !has_trails
        && let Some(first) = dispatches.first()
    {
        stamp_stateful_statistics(
            device,
            encoder,
            render.indirect,
            first.emitter_count,
            statistics_token,
            history_epoch,
            simulation_time,
        );
    }
    ticks
}
