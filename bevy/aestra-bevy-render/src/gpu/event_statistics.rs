//! Main-world event-link observations shared by renderer producers and editor consumers.
use bevy::prelude::*;

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
