//! Actual main-world asynchronous counter/output delivery observer.
use super::{
    effect_inputs::{GpuEffectBuffers, event_link_counter_base},
    output_context::{self, AestraOutputEvent, EffectOutputContext},
    particle_outputs,
};
use crate::PresentedEffect;
use bevy::{prelude::*, render::gpu_readback::ReadbackComplete};
use std::collections::BTreeMap;

/// The children each compiled event link (`CompiledEffect::event_links`, same order) could not spawn
/// so far, read back asynchronously (event system E0): for editors and tools to show where a burst
/// loses particles. Totals since the effect's GPU buffers were built; a rebuild starts from zero.
#[derive(Component, Debug, Default, Clone, PartialEq, Eq)]
pub struct GpuEventLinkStatistics {
    pub dropped: Vec<u64>,
    /// Observed activity per compiled link. These asynchronous totals have the
    /// same buffer-lifetime scope as `dropped`, not the current playback epoch.
    /// Replay/reset activity can be counted again; use uninterrupted live runs
    /// when comparing admission against authored work.
    pub links: Vec<GpuEventLinkCounts>,
    /// Events omitted by source capture or distance-sampling work bounds before
    /// any link could expand them.
    /// This is in source events, whereas link counts are in child particles.
    pub source_overflow: u64,
    /// Completed asynchronous counter readbacks (not simulation ticks).
    pub readback_samples: u64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GpuEventLinkCounts {
    pub captured_demand: u64,
    pub expansion_omitted: u64,
    pub accepted: u64,
    pub destination_rejected: u64,
}

impl GpuEventLinkStatistics {
    pub(super) fn record_link(
        &mut self,
        index: usize,
        requested: u32,
        omitted: u32,
        accepted: u32,
    ) {
        let rejected = requested.saturating_sub(omitted).saturating_sub(accepted);
        if let Some(counts) = self.links.get_mut(index) {
            counts.captured_demand += u64::from(requested);
            counts.expansion_omitted += u64::from(omitted);
            counts.accepted += u64::from(accepted);
            counts.destination_rejected += u64::from(rejected);
        }
        if let Some(dropped) = self.dropped.get_mut(index) {
            *dropped += u64::from(omitted) + u64::from(rejected);
        }
    }
}

/// Reads homing arrivals and source/link event overflow counters back, remembering the last
/// value of each `counters` word to report only new activity.
#[derive(Component)]
pub(super) struct GpuArrivalReadback {
    pub(super) effect: Entity,
    pub(super) seen: BTreeMap<u32, u32>,
    pub(super) particle_delivery: particle_outputs::Delivery,
}

/// Raises `impact` for homing arrivals and warns when source capture or link expansion drops work.
pub(super) fn receive_homing_arrivals(
    event: On<ReadbackComplete>,
    mut readbacks: Query<&mut GpuArrivalReadback>,
    effects: Query<(
        &GpuEffectBuffers,
        &PresentedEffect,
        Option<&EffectOutputContext>,
        Option<&GlobalTransform>,
    )>,
    mut link_statistics: Query<&mut GpuEventLinkStatistics>,
    mut events: MessageWriter<AestraOutputEvent>,
) {
    let Ok(mut readback) = readbacks.get_mut(event.event_target()) else {
        return;
    };
    let Ok((gpu, presented, context, placement)) = effects.get(readback.effect) else {
        return;
    };
    let words: Vec<u32> = event.to_shader_type();
    let effect = readback.effect;
    if let Ok(mut statistics) = link_statistics.get_mut(effect) {
        statistics.readback_samples += 1;
    }
    for dispatch in &gpu.stateful_dispatch {
        // Particle events dropped past the per-tick capacity (host bindings HB9b): the sub-emitters
        // missed them, and that tick no longer reproduces exactly.
        if let Some(word) = dispatch.overflow_word
            && let Some(&dropped) = words.get(word as usize)
        {
            let seen = readback.seen.insert(word, dropped).unwrap_or(0);
            if let Ok(mut statistics) = link_statistics.get_mut(effect) {
                statistics.source_overflow += u64::from(dropped.saturating_sub(seen));
            }
            if dropped > seen {
                warn!(
                    "aestra: emitter {} of {effect} raised more than {} particle events in a tick; \
                     {} dropped so far (event links spawn from the rest)",
                    dispatch.emitter_index,
                    aestra_runtime::PARTICLE_EVENT_CAPACITY,
                    dropped
                );
            }
        }
        let Some(word) = dispatch.arrival_word else {
            continue;
        };
        let Some(&count) = words.get(word as usize) else {
            continue;
        };
        let seen = readback.seen.insert(word, count).unwrap_or(0);
        // A smaller count is a rebuilt buffer, not arrivals.
        if count > seen {
            let tick = words
                .get(word as usize + 1)
                .map_or(0, |&tick| u64::from(tick));
            events.write(AestraOutputEvent::root(
                effect,
                aestra_runtime::EffectOutputEvent::new(
                    aestra_runtime::EVENT_IMPACT,
                    aestra_runtime::EventOrigin::Emitter(dispatch.emitter_index as usize),
                    "homing",
                    dispatch
                        .homing_world_target
                        .map(|target| target.to_vec())
                        .unwrap_or_default(),
                    (count - seen) as f32,
                    tick,
                ),
            ));
        }
    }
    if let Some(base) = event_link_counter_base(&gpu.stateful_dispatch) {
        for (index, link) in gpu.event_links.iter().enumerate() {
            let word = base + index as u32 * 3;
            let Some((&requested, &list_dropped, &accepted)) = words
                .get(word as usize)
                .zip(words.get((word + 1) as usize))
                .zip(words.get((word + 2) as usize))
                .map(|((requested, dropped), accepted)| (requested, dropped, accepted))
            else {
                continue;
            };
            let seen_requested = readback.seen.insert(word, requested).unwrap_or(0);
            let seen_list_dropped = readback.seen.insert(word + 1, list_dropped).unwrap_or(0);
            let seen_accepted = readback.seen.insert(word + 2, accepted).unwrap_or(0);
            let new_requested = requested.saturating_sub(seen_requested);
            let new_list_dropped = list_dropped.saturating_sub(seen_list_dropped);
            let new_accepted = accepted.saturating_sub(seen_accepted);
            let new_destination_dropped = new_requested
                .saturating_sub(new_list_dropped)
                .saturating_sub(new_accepted);
            if let Ok(mut statistics) = link_statistics.get_mut(effect) {
                statistics.record_link(index, new_requested, new_list_dropped, new_accepted);
            }
            if new_list_dropped > 0 || new_destination_dropped > 0 {
                let list_capacity = gpu
                    .stateful_dispatch
                    .iter()
                    .find(|dispatch| dispatch.emitter_index as usize == link.source)
                    .map(|dispatch| {
                        crate::execution::EventGatherPipeline::list_capacity(
                            aestra_runtime::event_capture_capacity(dispatch.capacity, link.trigger),
                            link.count,
                        )
                    })
                    .unwrap_or(0);
                warn!(
                    "aestra: event link {index} (emitter {} -> {}) of {effect} captured demand for {} new \
                     children: {} omitted by the per-tick list (capacity {}), {} rejected by \
                     destination slots, {} accepted",
                    link.source,
                    link.target,
                    new_requested,
                    new_list_dropped,
                    list_capacity,
                    new_destination_dropped,
                    new_accepted,
                );
            }
        }
    }
    // Particle output routes (event system E3): each ring slot holding a tick not heard yet raises
    // its outputs, in tick order.
    let mut records = Vec::new();
    for (route, ring) in &gpu.particle_outputs {
        for slot in 0..aestra_gpu::PARTICLE_OUTPUT_RING_TICKS {
            let start = ring + slot * aestra_gpu::PARTICLE_OUTPUT_SLOT_WORDS;
            let Some(record) = words
                .get(start as usize..(start + aestra_gpu::PARTICLE_OUTPUT_SLOT_WORDS) as usize)
            else {
                continue;
            };
            let Some(read) = aestra_gpu::read_particle_output_slot(record) else {
                continue;
            };
            records.push((route, *ring, read));
        }
    }
    // Sort before updating route high-water marks: ring slot order wraps every
    // 32 ticks, and asynchronous readbacks can arrive out of order.
    records.sort_by_key(|(_, _, record)| record.tick);
    for (route, ring, record) in records {
        if readback.particle_delivery.accept(
            ring,
            &record,
            presented.instance.history_epoch(),
            aestra_runtime::trace_tick(presented.instance.history_epoch_start_time()),
        ) {
            for event in route.raise(record.count, &record.first, record.tick) {
                events.write(output_context::particle_event(
                    effect, presented, context, placement, event,
                ));
            }
        }
    }
}
