//! Production outer simulation graph: readiness, branch ordering, history and timing.
use super::{
    catchup_pacing::CatchupPacer,
    coupled_simulation::*,
    draw_instance::GpuDrawInstance,
    effect_inputs::{GpuEffectBuffers, RouteWiring},
    paged_trails,
    presentation_readiness::PresentedOwners,
    render,
    simulation_pipeline::{self, GpuBindGroup, SimulationPipeline},
    stage_coupling,
    stage_inputs::ExtractedStages,
    stage_runtimes::StageRuntimes,
    stateful_simulation::*,
    stateful_trails, timestamp_transport, trail_checkpoints, trail_context,
};
use crate::{AestraRenderSettings, TransparentOrderMode};
use bevy::{
    prelude::*,
    render::{
        diagnostic::RecordDiagnostics,
        render_asset::RenderAssets,
        render_resource::{
            BufferInitDescriptor, BufferUsages, ComputePassDescriptor, ComputePipeline,
            PipelineCache,
        },
        renderer::{RenderContext, RenderDevice},
        storage::{GpuShaderBuffer, ShaderBuffer},
    },
};
use std::{collections::BTreeMap, sync::Arc};

type TrailHistories = BTreeMap<
    Entity,
    (
        AssetId<ShaderBuffer>,
        Arc<trail_context::TrailContext>,
        trail_checkpoints::TrailHistory,
    ),
>;

/// What the simulation system keeps across frames: trail histories, the timestamp timer, stateful
/// pipelines and states, and the domains coupled emitters follow (fluid F2b), with their pipeline.
type SimulationState<'w, 's> = (
    Local<'s, TrailHistories>,
    Local<'s, stateful_trails::Histories>,
    Local<'s, timestamp_transport::SimulationTimer>,
    Option<Res<'w, StatefulSimulationPipeline>>,
    ResMut<'w, StatefulStates>,
    ResMut<'w, StageRuntimes>,
    Option<Res<'w, stage_coupling::FieldFollow>>,
    Option<ResMut<'w, CatchupPacer>>,
    Option<Res<'w, stage_coupling::ParticleWorldBuffer>>,
);

type SimulationGpuResources<'w> = (
    Res<'w, RenderAssets<GpuShaderBuffer>>,
    Res<'w, RenderDevice>,
    Res<'w, bevy::render::renderer::RenderQueue>,
    Res<'w, timestamp_transport::TimingMailbox>,
    Res<'w, AestraRenderSettings>,
);

#[allow(clippy::too_many_arguments)]
pub(super) fn run_simulation(
    mut render_context: RenderContext,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Option<Res<SimulationPipeline>>,
    effects: Query<(
        Entity,
        &bevy::render::sync_world::MainEntity,
        &GpuEffectBuffers,
        &GpuBindGroup,
        Option<&ExtractedStages>,
    )>,
    mesh_draws: Query<(&GpuDrawInstance, &render::PreparedMeshDraw)>,
    gpu_resources: SimulationGpuResources,
    state: SimulationState,
    mut light_readiness: ResMut<PresentedOwners>,
    playback_diagnostics: Res<super::playback_diagnostics::GpuPlaybackDiagnostics>,
    prepared_owners: Query<(&bevy::render::sync_world::MainEntity, &GpuEffectBuffers)>,
) {
    let mut playback_recording = playback_diagnostics.begin();
    if let Some(recording) = &mut playback_recording {
        recording
            .samples
            .extend(prepared_owners.iter().map(|(owner, effect)| {
                super::playback_diagnostics::GpuPlaybackSample {
                    owner: owner.id(),
                    requested_time: effect.simulation_time,
                    encoded_time: None,
                    cpu_encoding_ns: None,
                }
            }));
    }
    light_readiness.owners.clear();
    let _span = bevy::log::info_span!("aestra::gpu::simulate").entered();
    let Some(pipeline) = pipeline else {
        return;
    };
    let (buffers, render_device, queue, timing_mailbox, render_settings) = gpu_resources;
    let (
        mut histories,
        mut stateful_histories,
        mut timer,
        stateful_pipeline,
        mut stateful_states,
        mut stage_runtimes,
        follower,
        mut pacer,
        particle_world,
    ) = state;
    let Some(particle_world) = particle_world else {
        return;
    };
    // Resolve the stateful compute pipelines once (present only when the device supports the path and
    // the pipelines have finished compiling). The stateful branch below drives one enabled stateful
    // emitter end-to-end; other effects take the analytic path unchanged.
    let stateful = stateful_pipeline.as_ref().and_then(|sp| {
        let order_present = match render_settings.transparent_order {
            TransparentOrderMode::Fast | TransparentOrderMode::DepthBackToFront => None,
            TransparentOrderMode::StableCapture => {
                Some(pipeline_cache.get_compute_pipeline(sp.order_present)?)
            }
        };
        Some((
            sp,
            pipeline_cache.get_compute_pipeline(sp.death_integrate)?,
            pipeline_cache.get_compute_pipeline(sp.spawn)?,
            pipeline_cache.get_compute_pipeline(sp.present)?,
            order_present,
        ))
    });
    let link_ribbons = pipeline_cache.get_compute_pipeline(pipeline.link_ribbons);
    let update_trails = pipeline_cache.get_compute_pipeline(pipeline.update_trails);
    let paged_pipelines: Option<[&ComputePipeline; 9]> = pipeline
        .paged_trails
        .map(|id| pipeline_cache.get_compute_pipeline(id))
        .into_iter()
        .collect::<Option<Vec<_>>>()
        .and_then(|pipelines| pipelines.try_into().ok());
    let (Some(reset), Some(simulate)) = (
        pipeline_cache.get_compute_pipeline(pipeline.reset),
        pipeline_cache.get_compute_pipeline(pipeline.simulate),
    ) else {
        return;
    };
    // GPU timestamp span around the whole simulation. This is a no-op unless the
    // host app added `RenderDiagnosticsPlugin`; on Vulkan/DX12 it records real GPU
    // elapsed time, surfaced through the diagnostics store and Tracy.
    let diagnostics = render_context.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let gpu_span = diagnostics.time_span(render_context.command_encoder(), "aestra::gpu::simulate");
    let mut timing_batch = timer.begin(&render_device, queue.get_timestamp_period());
    histories.retain(|entity, _| {
        effects
            .get(*entity)
            .is_ok_and(|(_, _, e, _, _)| e.has_trails && e.stateful_dispatch.is_empty())
    });
    stateful_histories.retain(|entity, _| {
        effects
            .get(*entity)
            .is_ok_and(|(_, _, e, _, _)| e.has_trails && !e.stateful_dispatch.is_empty())
    });
    let mut allocated: u64 = histories
        .values()
        .map(|h| h.2.checkpoints.bytes())
        .sum::<u64>()
        + stateful_histories
            .values()
            .map(|h| h.1.checkpoints.bytes())
            .sum::<u64>();
    for (entity, main_entity, effect, bind_group, extracted_stages) in &effects {
        let mut owner_recording = playback_recording
            .as_mut()
            .and_then(|r| r.owner(main_entity.id()));
        let mut fully_presented = effect.stateful_dispatch.is_empty();
        let paged = if effect.trail_plan.paged() {
            let Some(pipelines) = paged_pipelines else {
                continue;
            };
            Some(paged_trails::Dispatch::new(
                &render_device,
                effect.trail_plan,
                pipelines,
                effect.trail_workgroups,
            ))
        } else {
            None
        };
        // Histories shared with stateful emitters advance in the lockstep path:
        // reset/present analytic heads, present stateful heads, then record trails
        // after each tick's event births. Other mixed effects retain the old path.
        let stateful_trails = effect.has_trails && !effect.stateful_dispatch.is_empty();
        if (effect.stateful_only || stateful_trails)
            && let (
                Some((sp, death_integrate, spawn, present, order_present)),
                Some(persistent_states),
            ) = (&stateful, stateful_states.0.get_mut(&entity))
        {
            let render_buffers = [
                &effect.particles,
                &effect.alive,
                &effect.indirect,
                &effect.counters,
            ]
            .map(|handle| buffers.get(handle).map(|buffer| &buffer.buffer));
            if let [Some(particles), Some(alive), Some(indirect), Some(counters)] = render_buffers
                && persistent_states.len() == effect.stateful_dispatch.len()
            {
                let mut observer = if stateful_trails {
                    let Some(update) = update_trails else {
                        continue;
                    };
                    if effect.has_ribbons && link_ribbons.is_none() {
                        continue;
                    }
                    let (Some(globals), Some(render_globals)) = (
                        buffers.get(&effect.globals),
                        buffers.get(&effect.render_globals),
                    ) else {
                        continue;
                    };
                    let (Some(dead), Some(aux)) =
                        (buffers.get(&effect.dead), buffers.get(&effect.aux))
                    else {
                        continue;
                    };
                    let history = stateful_histories
                        .entry(entity)
                        .or_insert_with(|| (effect.particles.id(), default()));
                    allocated -= history.1.checkpoints.bytes();
                    if history.0 != effect.particles.id() {
                        *history = (effect.particles.id(), default());
                    }
                    history.1.sync(effect, persistent_states);
                    Some(stateful_trails::Observer {
                        history: &mut history.1,
                        effect,
                        group: &bind_group.0,
                        reset,
                        simulate,
                        update,
                        paged: paged.as_ref(),
                        ribbons: link_ribbons,
                        globals: &globals.buffer,
                        render_globals: &render_globals.buffer,
                        buffers: [
                            particles,
                            alive,
                            &dead.buffer,
                            counters,
                            indirect,
                            &aux.buffer,
                        ],
                        memory_budget: trail_checkpoints::MEMORY_LIMIT.saturating_sub(allocated),
                        diagnostics,
                        observations: 0,
                    })
                } else {
                    None
                };
                let layout = pipeline_cache.get_bind_group_layout(&sp.layout);
                let physics_buffer = physics_scene_buffer(&render_device, &effect.physics);
                let timing_index = timing_batch.as_mut().and_then(|batch| {
                    batch.instance(
                        main_entity.id(),
                        effect.statistics_token,
                        effect.simulation_time,
                    )
                });
                if let Some((batch, index)) = timing_batch.as_ref().zip(timing_index) {
                    drop(render_context.command_encoder().begin_compute_pass(
                        &ComputePassDescriptor {
                            label: Some("aestra stateful timing begin"),
                            timestamp_writes: batch.writes(index, true, false),
                        },
                    ));
                }
                let stateful_work = run_stateful_dispatches(
                    &render_device,
                    render_context.command_encoder(),
                    (death_integrate, spawn, present, *order_present),
                    &layout,
                    persistent_states,
                    &effect.stateful_dispatch,
                    &effect.event_links,
                    effect.routed.then_some(RouteWiring {
                        bursts: &effect.host_events.bursts,
                        outputs: &effect.particle_outputs,
                        output_epoch: effect.history_epoch,
                        output_suppress_through: effect.output_suppress_through,
                    }),
                    &StatefulRenderBuffers {
                        particles,
                        alive,
                        indirect,
                        counters,
                        world: &particle_world.buffer,
                        physics: &physics_buffer,
                    },
                    stage_coupling::coupling(
                        &mut stage_runtimes,
                        entity,
                        extracted_stages,
                        follower.as_deref(),
                    )
                    .or_else(|| {
                        (!effect.event_links.is_empty() || effect.routed || stateful_trails)
                            .then(|| stage_coupling::link_coupling(follower.as_deref()))
                            .flatten()
                    }),
                    effect.simulation_time,
                    effect.seek_quality,
                    effect.statistics_token,
                    effect.history_epoch,
                    true,
                    pacer.as_deref_mut(),
                    observer
                        .as_mut()
                        .map(|observer| observer as &mut dyn CoupledHistory),
                );
                if stateful_work.presented && light_readiness.enabled {
                    light_readiness.owners.insert(entity);
                }
                if stateful_work.presented
                    && let Some(recording) = &mut owner_recording
                {
                    let reached = persistent_states
                        .iter()
                        .map(|state| state.last_tick)
                        .min()
                        .unwrap_or(0);
                    recording.encoded(stateful_tick_time(reached));
                }
                if let Some((batch, index)) = timing_batch.as_mut().zip(timing_index) {
                    let trail_observations = observer.as_ref().map_or(0, |o| o.observations);
                    batch.work(
                        index,
                        timestamp_transport::GpuSimulationWork {
                            fixed_ticks: stateful_work.work.map(|work| work.0),
                            checkpoint_capture_bytes: stateful_work.work.map(|work| work.1),
                            trail_observations,
                            trail_workgroups: u64::from(trail_observations)
                                * (u64::from(effect.trail_workgroups)
                                    + paged.as_ref().map_or(0, |p| p.workgroups())),
                        },
                    );
                }
                if let Some(observer) = observer {
                    allocated += observer.history.checkpoints.bytes();
                }
                if let Some((batch, index)) = timing_batch.as_ref().zip(timing_index) {
                    drop(render_context.command_encoder().begin_compute_pass(
                        &ComputePassDescriptor {
                            label: Some("aestra stateful timing end"),
                            timestamp_writes: batch.writes(index, false, true),
                        },
                    ));
                }
            }
        }
        if effect.stateful_only || stateful_trails {
            continue;
        }
        if (effect.has_ribbons && link_ribbons.is_none())
            || (effect.has_trails && update_trails.is_none())
        {
            continue;
        }
        let replay = if effect.has_trails {
            let Some(globals) = buffers.get(&effect.globals) else {
                continue;
            };
            let Some(render_globals) = buffers.get(&effect.render_globals) else {
                continue;
            };
            let handles = [
                &effect.particles,
                &effect.alive,
                &effect.dead,
                &effect.counters,
                &effect.indirect,
                &effect.aux,
            ];
            let Some(state) = handles
                .iter()
                .map(|h| buffers.get(*h).map(|b| &b.buffer))
                .collect::<Option<Vec<_>>>()
            else {
                continue;
            };
            let history = histories.entry(entity).or_insert_with(|| {
                (
                    effect.particles.id(),
                    effect.checkpoint_context.clone(),
                    default(),
                )
            });
            if !effect.history_policy.captures_checkpoints() {
                allocated -= history.2.checkpoints.bytes();
                history.2.checkpoints = default();
            }
            if history.0 != effect.particles.id() {
                allocated -= history.2.checkpoints.bytes();
                *history = (
                    effect.particles.id(),
                    effect.checkpoint_context.clone(),
                    default(),
                );
            } else if history.1 != effect.checkpoint_context {
                allocated -= history.2.checkpoints.bytes();
                history.2.checkpoints = default();
                if history.1.motion.is_some() || effect.checkpoint_context.motion.is_some() {
                    history.2.replay = default();
                    // Placement edits while paused at time zero need an explicit reset too.
                    for &(_, root) in &effect.trail_roots {
                        render_context.command_encoder().clear_buffer(
                            state[0],
                            root as u64 * 48 + 40,
                            Some(4),
                        );
                    }
                } else {
                    history.2.replay.context_changed();
                }
                history.1 = effect.checkpoint_context.clone();
            }
            let previous = history.2.checkpoints.bytes();
            history.2.sync_buffers(&state);
            allocated = allocated - previous + history.2.checkpoints.bytes();
            if effect.checkpoint_context.motion.is_some() {
                history.2.replay.prepare_tracked(effect.simulation_time);
            }
            if history
                .2
                .replay
                .needs_restore(effect.history_epoch, effect.simulation_time)
                && let Some(time) = history.2.checkpoints.restore(
                    render_context.command_encoder(),
                    &state,
                    effect.simulation_time,
                )
            {
                trail_checkpoints::rebase_epoch(
                    render_context.command_encoder(),
                    &globals.buffer,
                    state[5],
                    state[3],
                    &effect.trail_roots,
                );
                history.2.replay.restore(effect.history_epoch, time);
            }
            let times = history
                .2
                .replay
                .observations(effect.history_epoch, effect.simulation_time);
            // A queue.write_buffer loop would expose only the final time to all
            // dispatches. Encoder copies make each observation visible in order.
            let placement = Mat4::from_cols_array(&std::array::from_fn(|i| {
                f32::from_bits(effect.checkpoint_context.key[6 + i])
            }));
            let bytes = crate::host_transform::observation_bytes(
                &times,
                placement,
                effect.checkpoint_context.motion.as_deref(),
            );
            let times_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("aestra trail replay times and host transforms"),
                contents: &bytes,
                usage: BufferUsages::COPY_SRC,
            });
            // GpuRenderGlobals starts with a mat4, followed by time. During a
            // multi-frame replay, draw the processed time rather than expiring
            // all intermediate history against the eventual seek target.
            render_context.command_encoder().copy_buffer_to_buffer(
                &times_buffer,
                (times.len() as u64 - 1) * 68,
                &render_globals.buffer,
                64,
                4,
            );
            render_context.command_encoder().copy_buffer_to_buffer(
                &times_buffer,
                (times.len() as u64 - 1) * 68 + 4,
                &render_globals.buffer,
                0,
                64,
            );
            Some((times_buffer, times, globals, state))
        } else {
            None
        };
        let observation_count = replay.as_ref().map_or(1, |(_, times, _, _)| times.len());
        let observed_time = replay
            .as_ref()
            .map_or(effect.simulation_time, |(_, times, _, _)| {
                *times.last().unwrap()
            });
        let timing_index = timing_batch.as_mut().and_then(|batch| {
            batch.instance(main_entity.id(), effect.statistics_token, observed_time)
        });
        // The legacy mixed analytic/stateful timer excludes the later stateful
        // dispatches. Do not advertise its partial window as a complete frame.
        if effect.stateful_dispatch.is_empty()
            && let Some((batch, index)) = timing_batch.as_mut().zip(timing_index)
        {
            let trail_observations = if effect.has_trails {
                observation_count as u32
            } else {
                0
            };
            batch.work(
                index,
                timestamp_transport::GpuSimulationWork {
                    fixed_ticks: None,
                    checkpoint_capture_bytes: None,
                    trail_observations,
                    trail_workgroups: u64::from(trail_observations)
                        * (u64::from(effect.trail_workgroups)
                            + paged.as_ref().map_or(0, |p| p.workgroups())),
                },
            );
        }
        let mut trails_recorded = true;
        for observation in 0..observation_count {
            if let Some((times, _, globals, _)) = &replay {
                render_context.command_encoder().copy_buffer_to_buffer(
                    times,
                    observation as u64 * 68,
                    &globals.buffer,
                    0,
                    4,
                );
                render_context.command_encoder().copy_buffer_to_buffer(
                    times,
                    observation as u64 * 68 + 4,
                    &globals.buffer,
                    32,
                    64,
                );
            }
            let particle_span = effect.has_trails.then(|| {
                diagnostics.time_span(
                    render_context.command_encoder(),
                    "aestra::gpu::trail_particles",
                )
            });
            simulation_pipeline::record_particles(
                render_context.command_encoder(),
                bind_group,
                effect,
                reset,
                simulate,
                link_ribbons,
                timing_batch
                    .as_ref()
                    .zip(timing_index)
                    .and_then(|(batch, index)| {
                        batch.writes(
                            index,
                            observation == 0,
                            observation + 1 == observation_count && !effect.has_trails,
                        )
                    }),
            );
            if let Some(span) = particle_span {
                span.end(render_context.command_encoder());
            }
            if effect.has_trails
                && let Some(update_trails) = update_trails
                && !simulation_pipeline::record_trails(
                    render_context.command_encoder(),
                    bind_group,
                    effect,
                    simulation_pipeline::TrailPipelines {
                        update: update_trails,
                        link_ribbons,
                        paged: paged.as_ref(),
                    },
                    &buffers,
                    simulation_pipeline::TrailTimestamps {
                        history: timing_batch.as_ref().zip(timing_index).and_then(
                            |(batch, index)| {
                                batch.writes(
                                    index,
                                    false,
                                    observation + 1 == observation_count && paged.is_none(),
                                )
                            },
                        ),
                        paged_end: timing_batch.as_ref().zip(timing_index).and_then(
                            |(batch, index)| {
                                batch.writes(index, false, observation + 1 == observation_count)
                            },
                        ),
                    },
                    diagnostics,
                )
            {
                trails_recorded = false;
                continue;
            }
            if let Some((_, times, _, state)) = &replay {
                let history = &mut histories.get_mut(&entity).unwrap().2;
                let time = times[observation];
                if effect.history_policy.captures_checkpoints()
                    && history.replay.should_capture(time)
                {
                    let previous = history.checkpoints.bytes();
                    history.checkpoints.capture(
                        &render_device,
                        render_context.command_encoder(),
                        state,
                        time,
                        trail_checkpoints::MEMORY_LIMIT.saturating_sub(allocated),
                    );
                    allocated = allocated - previous + history.checkpoints.bytes();
                }
            }
        }
        if effect.stateful_dispatch.is_empty()
            && trails_recorded
            && let Some(recording) = &mut owner_recording
        {
            recording.encoded(observed_time);
        }
        // Mixed effect (some analytic, some stateful emitters): the analytic reset+simulate above ran
        // (its `simulate` skipped the stateful emitters' slots), so now fill those slots with the
        // stateful path. The analytic reset already cleared the shared counter and stamped telemetry,
        // so these dispatches must not repeat that (`owns_shared_reset = false`). Pure-stateful effects
        // took the branch at the top of the loop and never reach here; pure-analytic effects have an
        // empty dispatch list, so this is a no-op for them.
        if !effect.stateful_dispatch.is_empty()
            && let (
                Some((sp, death_integrate, spawn, present, order_present)),
                Some(persistent_states),
            ) = (&stateful, stateful_states.0.get_mut(&entity))
        {
            let render_buffers = [
                &effect.particles,
                &effect.alive,
                &effect.indirect,
                &effect.counters,
            ]
            .map(|handle| buffers.get(handle).map(|buffer| &buffer.buffer));
            if let [Some(particles), Some(alive), Some(indirect), Some(counters)] = render_buffers
                && persistent_states.len() == effect.stateful_dispatch.len()
            {
                let layout = pipeline_cache.get_bind_group_layout(&sp.layout);
                let physics_buffer = physics_scene_buffer(&render_device, &effect.physics);
                let work = run_stateful_dispatches(
                    &render_device,
                    render_context.command_encoder(),
                    (death_integrate, spawn, present, *order_present),
                    &layout,
                    persistent_states,
                    &effect.stateful_dispatch,
                    &effect.event_links,
                    effect.routed.then_some(RouteWiring {
                        bursts: &effect.host_events.bursts,
                        outputs: &effect.particle_outputs,
                        output_epoch: effect.history_epoch,
                        output_suppress_through: effect.output_suppress_through,
                    }),
                    &StatefulRenderBuffers {
                        particles,
                        alive,
                        indirect,
                        counters,
                        world: &particle_world.buffer,
                        physics: &physics_buffer,
                    },
                    stage_coupling::coupling(
                        &mut stage_runtimes,
                        entity,
                        extracted_stages,
                        follower.as_deref(),
                    )
                    .or_else(|| {
                        (!effect.event_links.is_empty() || effect.routed)
                            .then(|| stage_coupling::link_coupling(follower.as_deref()))
                            .flatten()
                    }),
                    effect.simulation_time,
                    effect.seek_quality,
                    effect.statistics_token,
                    effect.history_epoch,
                    false,
                    pacer.as_deref_mut(),
                    None,
                );
                fully_presented = work.presented;
                if work.presented
                    && trails_recorded
                    && let Some(recording) = &mut owner_recording
                {
                    let reached = persistent_states
                        .iter()
                        .map(|state| state.last_tick)
                        .min()
                        .unwrap_or(0);
                    recording.encoded(observed_time.min(stateful_tick_time(reached)));
                }
            }
        }
        if fully_presented && light_readiness.enabled {
            light_readiness.owners.insert(entity);
        }
    }
    // Copy only instance counts after simulation; mesh commands retain their own geometry ranges.
    // This avoids CPU readback and preserves the existing per-emitter simulation ABI.
    for (draw, mesh) in &mesh_draws {
        if let Some(indirect) = buffers.get(&draw.indirect) {
            render_context.command_encoder().copy_buffer_to_buffer(
                &indirect.buffer,
                draw.indirect_offset + 4,
                &mesh.indirect,
                4,
                4,
            );
        }
    }
    gpu_span.end(render_context.command_encoder());
    if let Some(batch) = timing_batch {
        batch.finish(&mut render_context, timing_mailbox.clone());
    }
}
