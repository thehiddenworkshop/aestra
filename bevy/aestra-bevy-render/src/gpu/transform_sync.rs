//! Main-world clock, host placement and stateful input synchronization shared by engine adapters.
use super::{
    effect_inputs::{GpuEffectBuffers, HostEventHistory, TickSchedule},
    output_context::AestraOutputEvent,
    particle_statistics::GpuParticleStatistics,
    physics::AestraPhysicsColliders,
    storage_buffers, trail_context,
    world_sdf::AestraWorldSdf,
};
use crate::PresentedEffect;
use aestra_gpu::{GpuGlobals, GpuRenderGlobals, fold_seed};
use bevy::{prelude::*, render::storage::ShaderBuffer};
use std::sync::Arc;

pub(super) fn install(app: &mut App) {
    app.add_message::<AestraOutputEvent>().add_systems(
        PostUpdate,
        sync_gpu_render_transforms
            .in_set(super::draw_resources::SyncGpuRenderTransforms)
            .after(bevy::transform::TransformSystems::Propagate)
            .before(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
    );
}

/// The spawn placement of an emitter transform, with the rotation normalized for the trig-free kernel.
pub(super) fn spawn_placement(
    transform: aestra_core::EmitterTransform,
) -> aestra_runtime::SpawnPlacement {
    let rotation = Quat::from_array(transform.rotation);
    let rotation = if rotation.length_squared() > 1e-12 {
        rotation.normalize()
    } else {
        Quat::IDENTITY
    };
    aestra_runtime::SpawnPlacement {
        translation: transform.translation,
        rotation: rotation.to_array(),
        scale: transform.scale,
    }
}

/// What [`sync_gpu_render_transforms`] reads and updates per effect.
type RenderTransformSync = (
    Entity,
    &'static PresentedEffect,
    &'static GlobalTransform,
    &'static mut GpuEffectBuffers,
    &'static mut GpuParticleStatistics,
    Option<&'static AestraPhysicsColliders>,
);

// Rendering and culling must see the same frame's propagated effect transform.
pub(super) fn sync_gpu_render_transforms(
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut players: Query<RenderTransformSync>,
    mut events: MessageWriter<AestraOutputEvent>,
    world_sdf: Option<Res<AestraWorldSdf>>,
) {
    for (entity, player, transform, mut gpu, mut statistics, physics) in &mut players {
        let statistics_token = statistics.sync(&player.instance);
        gpu.statistics_token = statistics_token;
        let placement = Mat4::from(transform.affine());
        let world = placement
            * Mat4::from_cols_array(
                &player
                    .instance
                    .host_transform_context()
                    .matrix_at(player.simulation_time()),
            );
        gpu.simulation_time = player.simulation_time();
        gpu.seek_quality = player.seek_quality();
        gpu.history_policy = player.history_policy();
        // `World` and `Physics` colliders (host bindings HB10): where the effect sits in the host's
        // world, which world (a new one restarts their history), and this frame's physics colliders.
        let world_revision = world_sdf.as_deref().map_or(0, AestraWorldSdf::revision);
        let mut reads_physics = false;
        // The host's input events (event system E2–E3), rebuilt only when they change.
        if gpu.host_events.events != HostEventHistory::keys(&player.instance) {
            gpu.host_events = Arc::new(HostEventHistory::of(&player.instance));
        }
        let cutoffs = player.instance.emission_cutoffs();
        for dispatch in &mut gpu.stateful_dispatch {
            dispatch.cutoffs = cutoffs;
            let (mut world_colliders, mut physics_colliders) = (false, false);
            for collider in &dispatch.colliders {
                match collider.shape {
                    aestra_core::ColliderShape::World { .. } => world_colliders = true,
                    aestra_core::ColliderShape::Physics { .. } => physics_colliders = true,
                    _ => {}
                }
            }
            if world_colliders || physics_colliders {
                dispatch.world_from_effect = std::array::from_fn(|row| {
                    let r = world.row(row);
                    [r.x, r.y, r.z, r.w]
                });
            }
            if world_colliders {
                dispatch.world_revision = world_revision;
            }
            reads_physics |= physics_colliders;
        }
        if reads_physics {
            gpu.physics = match physics {
                Some(physics) => aestra_gpu::pack_physics_scene(&physics.0).into(),
                None => aestra_gpu::pack_physics_scene(&Default::default()).into(),
            };
        }
        // Homing targets (host bindings HB7) and attached emitters' placements (HB7b): this frame's,
        // from the bindings, into effect space. An attachment without a pose keeps its last placement.
        if gpu
            .stateful_dispatch
            .iter()
            .any(|dispatch| dispatch.homing.is_some() || dispatch.attachment.is_some())
        {
            let effect_from_world = world.inverse();
            let rows: [[f32; 4]; 3] = std::array::from_fn(|row| {
                let r = effect_from_world.row(row);
                [r.x, r.y, r.z, r.w]
            });
            let effect = player.instance.effect();
            let emitters = &effect.emitters;
            // Under a binding trace (host bindings HB8), each tick's own recorded input — computed
            // once per trace and placement, so replays after a seek steer and place exactly as the
            // uninterrupted run did.
            let trace = player.instance.binding_trace();
            let key = trace.map(|trace| {
                rows.iter().flatten().fold(trace.identity(), |hash, value| {
                    (hash ^ u64::from(value.to_bits())).wrapping_mul(0x0000_0100_0000_01b3)
                })
            });
            for dispatch in &mut gpu.stateful_dispatch {
                match (trace, key) {
                    (Some(trace), Some(key))
                        if dispatch.schedule.as_ref().map(|schedule| schedule.key) != Some(key) =>
                    {
                        let ticks = trace.len() as u64;
                        let authored = emitters
                            .get(dispatch.emitter_index as usize)
                            .map(|emitter| emitter.transform)
                            .unwrap_or_default();
                        dispatch.schedule = Some(Arc::new(TickSchedule {
                            key,
                            homing: dispatch
                                .homing
                                .as_ref()
                                .map(|homing| homing.schedule(effect, trace, ticks, rows))
                                .unwrap_or_default(),
                            placement: dispatch
                                .attachment
                                .as_ref()
                                .map(|attachment| {
                                    attachment
                                        .schedule(effect, trace, ticks, rows, authored)
                                        .into_iter()
                                        .map(spawn_placement)
                                        .collect()
                                })
                                .unwrap_or_default(),
                        }));
                    }
                    (None, _) => dispatch.schedule = None,
                    _ => {}
                }
                if let Some(homing) = &dispatch.homing {
                    let input = homing.resolve(&player.instance, rows);
                    dispatch.homing_target =
                        dispatch.homing_tracker.resolve(homing.config.lost, input);
                    let in_world = |target: aestra_runtime::HomingTarget| {
                        world
                            .transform_point3(Vec3::from_array(target.position))
                            .to_array()
                    };
                    dispatch.homing_world_target = dispatch.homing_target.map(in_world);
                    // The target appearing or vanishing (host bindings HB9), where it was.
                    if let Some(change) = dispatch.homing_tracker.take_change() {
                        let (kind, target) = match change {
                            aestra_runtime::TargetChange::Lost(target) => {
                                (aestra_runtime::EVENT_TARGET_LOST, target)
                            }
                            aestra_runtime::TargetChange::Acquired(target) => {
                                (aestra_runtime::EVENT_TARGET_ACQUIRED, target)
                            }
                        };
                        events.write(AestraOutputEvent::root(
                            entity,
                            aestra_runtime::EffectOutputEvent::new(
                                kind,
                                aestra_runtime::EventOrigin::Emitter(
                                    dispatch.emitter_index as usize,
                                ),
                                "homing",
                                in_world(target).to_vec(),
                                0.0,
                                aestra_runtime::trace_tick(player.instance.time()),
                            ),
                        ));
                    }
                }
                if let Some(attachment) = &dispatch.attachment
                    && dispatch.schedule.is_none()
                    && let Some(emitter) = emitters.get(dispatch.emitter_index as usize)
                    && let Some(transform) =
                        attachment.resolve(&player.instance, rows, emitter.transform)
                {
                    dispatch.placement = spawn_placement(transform);
                }
            }
        }
        gpu.history_epoch = player.instance.history_epoch();
        gpu.output_suppress_through =
            aestra_runtime::trace_tick(player.instance.history_epoch_start_time());
        if gpu.has_trails {
            let seed = player.instance.seed();
            let revision = player.instance.history_revision();
            let context = player.instance.host_transform_context();
            let motion = (!context.is_identity()).then(|| Arc::new(context));
            let mut key = [0; 22];
            key[..6].copy_from_slice(&[
                seed as u32,
                (seed >> 32) as u32,
                revision as u32,
                (revision >> 32) as u32,
                player.effect().duration.to_bits(),
                u32::from(player.effect().playback_mode.is_continuous()),
            ]);
            key[6..].copy_from_slice(
                &Mat4::from(transform.affine())
                    .to_cols_array()
                    .map(f32::to_bits),
            );
            if let Some(data) = buffers.get(&gpu.emitters).and_then(storage_buffers::bytes)
                && (gpu.checkpoint_context.key != key
                    || gpu.checkpoint_context.emitters != data
                    || gpu.checkpoint_context.motion != motion)
            {
                gpu.checkpoint_context = Arc::new(trail_context::TrailContext {
                    emitters: data.to_vec(),
                    key,
                    motion,
                });
            }
        }
        // Bevy and the engine-neutral ABI can use different glam versions.
        // Copy columns as scalars, retaining encase's storage layout below.
        let mut packed_world = GpuGlobals::default().world_from_effect;
        *packed_world.as_mut() = world.to_cols_array();
        if let Some(mut buffer) = buffers.get_mut(&gpu.globals) {
            storage_buffers::update(
                &mut buffer,
                GpuGlobals {
                    time: player.simulation_time(),
                    total_slots: gpu.total_slots,
                    seed: fold_seed(player.instance.seed()),
                    emitter_count: player.effect().emitters.len() as u32,
                    duration: player.effect().duration,
                    continuous: u32::from(player.effect().playback_mode.is_continuous()),
                    _padding: [player.instance.history_epoch(), statistics_token].into(),
                    emission_end: player.instance.emission_cutoffs().emission_end(),
                    kill_time: player.instance.emission_cutoffs().kill_time(),
                    _cutoff_padding: Default::default(),
                    world_from_effect: packed_world,
                },
            );
        }
        if let Some(mut buffer) = buffers.get_mut(&gpu.render_globals) {
            storage_buffers::update(
                &mut buffer,
                GpuRenderGlobals {
                    world_from_effect: packed_world,
                    time: player.simulation_time(),
                    seed: fold_seed(player.instance.seed()),
                    _padding: Default::default(),
                },
            );
        }
    }
}
