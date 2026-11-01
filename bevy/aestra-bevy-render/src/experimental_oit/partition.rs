//! Opt-in native phase transport. Repeats draws with scissors, never simulation.
#[path = "partition_readback.rs"]
mod readback;
#[path = "partition_render.rs"]
mod rendering;
use super::{OitApproved, resolve::AdapterResolveId};
use bevy::{
    camera::MainPassResolutionOverride,
    core_pipeline::{
        core_3d::{Transparent3d, main_transparent_pass_3d},
        oit::{OrderIndependentTransparencySettings, resolve::OitResolvePipelineId},
        schedule::{Core3d, Core3dSystems},
    },
    extract::extract_component::{ExtractComponent, ExtractComponentPlugin},
    pbr::Transmissive3d,
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        camera::{ExtractedCamera, TemporalJitter},
        render_phase::{SortedRenderPhase, ViewSortedRenderPhases},
        view::{ExtractedView, RetainedViewEntity},
    },
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// Experimental single-view scissored fragment-list transport. Requires
/// `ExperimentalOitSettings`, Msaa::Off, one full-target 3D view and exclusively
/// eligible native Aestra transparent draws. Opaque host geometry is supported.
/// No subviews, depth-prepass materials, jitter, resolution overrides or foreign
/// transparent commands. Rejections suppress the requested transparent phase;
/// inspect `ExperimentalOitPartitionStatus` and stop the host on failure.
///
/// Shares the existing node/head allocation and byte ceiling. More partitions
/// repeat vertex/material/draw work; they are not a performance guarantee.
/// At most 64 regions bounds diagnostic transport, not effect/owner admission.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, ExtractComponent)]
#[extract_component_sync_target(Self)]
#[extract_app(RenderApp)]
pub struct ExperimentalOitPartitions {
    pub grid: UVec2,
}

#[derive(Clone, Debug)]
pub struct ExperimentalOitPartitionSample {
    pub sequence: u64,
    /// No queued transparent draws: one full-target GPU reset audit replaces
    /// per-region rendering. `regions` then contains that single audit result.
    pub empty_phase: bool,
    /// Sum across regions, not a capacity to subtract from the frame total.
    pub attempted_fragments: u64,
    pub dropped_fragments: u64,
    pub invalid_links: u64,
    /// [attempted, over-fast-sort pixels, invalid links, capped depth]. One row
    /// per region, or one full-target audit row when `empty_phase` is true.
    pub regions: Vec<[u32; 4]>,
    pub node_capacity: u32,
}

/// Latest prepared configuration and asynchronous observation. `sample: None`
/// means unavailable/pending, never loss-free. Samples can lag live rendering.
#[derive(Clone, Debug)]
pub struct ExperimentalOitPartitionSnapshot {
    pub target_size: UVec2,
    pub grid: UVec2,
    pub phase_draws: usize,
    pub ready: bool,
    pub rejection: Option<&'static str>,
    /// Sticky for this admitted configuration: a later clean callback cannot
    /// erase an already observed loss before the main thread sees it.
    pub observed_loss: bool,
    pub sample: Option<ExperimentalOitPartitionSample>,
}

#[derive(Default)]
struct State {
    key: Option<Key>,
    generation: u64,
    snapshot: Option<ExperimentalOitPartitionSnapshot>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Key {
    camera: Entity,
    size: UVec2,
    grid: UVec2,
    cameras: usize,
    capacity: u32,
    settings: (u32, u32),
}

/// Bounded (three in-flight slots, <=64 regions) native phase health mailbox.
/// Only requested partition views allocate/copy diagnostic data. No particle or
/// fragment payloads are read back. Does not certify arbitrary custom passes.
#[derive(Resource, Clone, Default)]
pub struct ExperimentalOitPartitionStatus(Arc<Mutex<State>>);

impl ExperimentalOitPartitionStatus {
    pub fn snapshot(&self) -> Option<ExperimentalOitPartitionSnapshot> {
        self.0.lock().unwrap().snapshot.clone()
    }

    fn configure(&self, key: Option<Key>, ready: bool, rejection: Option<&'static str>) -> u64 {
        let mut state = self.0.lock().unwrap();
        if state.key != key
            || state
                .snapshot
                .as_ref()
                .is_some_and(|s| s.ready != ready || s.rejection != rejection)
        {
            let observed_loss =
                state.key == key && state.snapshot.as_ref().is_some_and(|s| s.observed_loss);
            state.generation += 1;
            state.key = key;
            state.snapshot = key.map(|key| ExperimentalOitPartitionSnapshot {
                target_size: key.size,
                grid: key.grid,
                phase_draws: 0,
                ready,
                rejection,
                observed_loss,
                sample: None,
            });
        }
        state.generation
    }

    fn publish(&self, generation: u64, sample: ExperimentalOitPartitionSample) {
        let mut state = self.0.lock().unwrap();
        if state.generation == generation
            && let Some(snapshot) = &mut state.snapshot
            && snapshot.ready
        {
            snapshot.observed_loss |= sample.dropped_fragments > 0 || sample.invalid_links > 0;
            if snapshot
                .sample
                .as_ref()
                .is_none_or(|s| s.sequence < sample.sequence)
            {
                snapshot.sample = Some(sample);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Region {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

fn regions(size: UVec2, grid: UVec2) -> Result<Vec<Region>, &'static str> {
    if size.min_element() == 0
        || grid.min_element() == 0
        || grid.x > size.x
        || grid.y > size.y
        || u64::from(grid.x) * u64::from(grid.y) > 64
        || u64::from(size.x) * u64::from(size.y) > u64::from(u32::MAX)
    {
        return Err("invalid or unsupported partition extent (maximum 64 regions)");
    }
    let mut result = Vec::new();
    for y in 0..grid.y {
        for x in 0..grid.x {
            let lo = UVec2::new(
                (u64::from(x) * u64::from(size.x) / u64::from(grid.x)) as u32,
                (u64::from(y) * u64::from(size.y) / u64::from(grid.y)) as u32,
            );
            let hi = UVec2::new(
                (u64::from(x + 1) * u64::from(size.x) / u64::from(grid.x)) as u32,
                (u64::from(y + 1) * u64::from(size.y) / u64::from(grid.y)) as u32,
            );
            result.push(Region {
                x: lo.x,
                y: lo.y,
                w: hi.x - lo.x,
                h: hi.y - lo.y,
            });
        }
    }
    Ok(result)
}

struct Held {
    phase: SortedRenderPhase<Transparent3d>,
    regions: Vec<Region>,
    generation: u64,
    ready: bool,
}

#[derive(Resource, Default)]
struct Transport {
    phases: HashMap<RetainedViewEntity, Held>,
}

pub(super) fn install(app: &mut App) {
    let status = ExperimentalOitPartitionStatus::default();
    app.insert_resource(status.clone())
        .add_plugins(ExtractComponentPlugin::<ExperimentalOitPartitions, RenderApp>::default());
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render
            .insert_resource(status)
            .init_resource::<Transport>()
            .init_resource::<readback::Readbacks>()
            .add_systems(
                Render,
                route
                    .after(RenderSystems::Prepare)
                    .before(RenderSystems::Render),
            )
            .add_systems(
                Render,
                restore
                    .after(RenderSystems::Render)
                    .before(RenderSystems::Cleanup),
            )
            .add_systems(
                Core3d,
                rendering::render
                    .after(main_transparent_pass_3d)
                    .in_set(Core3dSystems::MainPass),
            );
    }
}

/// Move the *whole* retained phase out only after queue/sort/bind-group prepare.
/// Native capture sees an empty phase, and native sort/diagnostic/resolve queries
/// cannot match (their resolve id is removed). Restore before retained cleanup.
fn route(world: &mut World) {
    let requests: Vec<_> = world
        .query::<(
            Entity,
            &ExperimentalOitPartitions,
            &ExtractedView,
            &ExtractedCamera,
            Has<OitApproved>,
            Has<MainPassResolutionOverride>,
            Has<TemporalJitter>,
        )>()
        .iter(world)
        .map(|(e, p, v, c, approved, resolution, jitter)| {
            (
                e,
                *p,
                v.retained_view_entity,
                c.physical_target_size.unwrap_or(UVec2::ZERO),
                approved,
                resolution || jitter || c.viewport.is_some(),
            )
        })
        .collect();
    let oit_count = world
        .query_filtered::<Entity, With<OrderIndependentTransparencySettings>>()
        .iter(world)
        .count();
    let status = world.resource::<ExperimentalOitPartitionStatus>().clone();
    if requests.is_empty() {
        status.configure(None, false, None);
        return;
    }
    let mut phases = world
        .remove_resource::<ViewSortedRenderPhases<Transparent3d>>()
        .unwrap();
    let mut held = world.remove_resource::<Transport>().unwrap();
    debug_assert!(held.phases.is_empty());
    for (entity, request, retained, size, approved, unsupported) in &requests {
        let phase = phases
            .get_mut(retained)
            .map(std::mem::take)
            .unwrap_or_default();
        let region_result = regions(*size, request.grid);
        let mut rejection = if requests.len() != 1 || oit_count != 1 {
            Some("partitioned OIT requires exactly one OIT view")
        } else if *unsupported {
            Some(
                "partitioned OIT requires a full-target view without jitter or resolution overrides",
            )
        } else if !approved {
            Some("partitioned OIT camera is not approved")
        } else {
            region_result.as_ref().err().copied()
        };
        let mut draws_ready = true;
        if world
            .get_resource::<ViewSortedRenderPhases<Transmissive3d>>()
            .and_then(|phases| phases.get(retained))
            .is_some_and(|phase| !phase.items.is_empty())
        {
            rejection = Some("partitioned OIT does not qualify foreign transmissive draws");
        }
        for item in phase.items.values() {
            match crate::gpu::partition_draw_check(world, item) {
                Ok(ready) => draws_ready &= ready,
                Err(reason) => rejection = Some(reason),
            }
        }
        let adapter_ready = world.get::<AdapterResolveId>(*entity).is_some_and(|id| {
            world
                .resource::<bevy::render::render_resource::PipelineCache>()
                .get_render_pipeline(id.0)
                .is_some()
        });
        let sort_ready = world
            .get_resource::<super::sort::Pipeline>()
            .is_some_and(|p| {
                p.group.is_some()
                    && world
                        .resource::<bevy::render::render_resource::PipelineCache>()
                        .get_compute_pipeline(p.id)
                        .is_some()
                    && world
                        .resource::<bevy::render::render_resource::PipelineCache>()
                        .get_compute_pipeline(p.region_id)
                        .is_some()
            });
        let ready = rejection.is_none() && draws_ready && adapter_ready && sort_ready;
        let capacity = world
            .get_resource::<bevy::core_pipeline::oit::OitBuffers>()
            .map_or(0, |b| b.nodes.capacity() as u32);
        let generation = status.configure(
            Some(Key {
                camera: *entity,
                size: *size,
                grid: request.grid,
                cameras: requests.len(),
                capacity,
                settings: world
                    .get::<OrderIndependentTransparencySettings>(*entity)
                    .map_or((0, 0), |s| {
                        (
                            s.sorted_fragment_max_count,
                            s.fragments_per_pixel_average.to_bits(),
                        )
                    }),
            }),
            ready,
            rejection,
        );
        // Publish the actual count without using it as an admission limit.
        if let Some(snapshot) = &mut status.0.lock().unwrap().snapshot {
            snapshot.phase_draws = phase.items.len();
        }
        world.entity_mut(*entity).remove::<OitResolvePipelineId>();
        held.phases.insert(
            *retained,
            Held {
                phase,
                regions: region_result.unwrap_or_default(),
                generation,
                ready,
            },
        );
    }
    world.insert_resource(phases);
    world.insert_resource(held);
}

fn restore(mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>, mut held: ResMut<Transport>) {
    for (view, held) in held.phases.drain() {
        if let Some(phase) = phases.get_mut(&view) {
            *phase = held.phase;
        }
    }
}

#[cfg(test)]
#[path = "tests/partition.rs"]
mod tests;
