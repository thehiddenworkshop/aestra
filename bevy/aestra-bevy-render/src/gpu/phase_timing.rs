//! Opt-in, bounded round-robin GPU timing of shared simulation phases.
use super::timestamp_transport::{SimulationTimer, TimestampFrame, TimingBatch, TimingMailbox};
use bevy::{
    prelude::*,
    render::renderer::{RenderContext, RenderDevice},
};
use std::{
    cell::RefCell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GpuSimulationPhase {
    Particles,
    Events,
    Presentation,
    History,
}
impl GpuSimulationPhase {
    pub const ALL: [Self; 4] = [
        Self::Particles,
        Self::Events,
        Self::Presentation,
        Self::History,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Particles => "particle_ticks",
            Self::Events => "event_routes",
            Self::Presentation => "presentation",
            Self::History => "trail_history",
        }
    }
}

/// One sampled render-frame category. Categories rotate across frames: their
/// percentiles must not be added. Unknown/overflowed windows have no GPU total.
#[derive(Clone, Debug)]
pub struct GpuSimulationPhaseSample {
    pub phase: GpuSimulationPhase,
    pub sequence: u64,
    pub requested_segments: usize,
    pub measured_segments: usize,
    pub nanoseconds: Option<u64>,
}

/// Disabled by default. At most 1024 timestamp pairs per sampled frame and three
/// in-flight slots; no blocking waits, per-owner paths or growing result queue.
/// `samples()` returns the latest completed window of each category, possibly
/// delayed. Consumers deduplicate by phase/sequence rather than counting stale data.
#[derive(Resource, Clone, Default)]
pub struct GpuSimulationPhaseTimings {
    enabled: Arc<AtomicBool>,
    mailboxes: [TimingMailbox; 4],
}
impl GpuSimulationPhaseTimings {
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
    }
    pub fn samples(&self) -> Vec<GpuSimulationPhaseSample> {
        if !self.enabled.load(Ordering::Acquire) {
            return Vec::new();
        }
        self.mailboxes
            .iter()
            .zip(GpuSimulationPhase::ALL)
            .filter_map(|(m, p)| m.latest().map(|f| summarize(p, f)))
            .collect()
    }
}

fn summarize(phase: GpuSimulationPhase, frame: TimestampFrame) -> GpuSimulationPhaseSample {
    let measured_segments = frame.samples.len();
    let total = frame
        .samples
        .iter()
        .try_fold(0u64, |n, s| n.checked_add(s.nanoseconds));
    GpuSimulationPhaseSample {
        phase,
        sequence: frame.sequence,
        requested_segments: frame.requested,
        measured_segments,
        nanoseconds: (frame.requested == measured_segments)
            .then_some(total)
            .flatten(),
    }
}

pub(super) struct PhaseState {
    timer: SimulationTimer,
    next: usize,
}
impl Default for PhaseState {
    fn default() -> Self {
        Self {
            timer: SimulationTimer::with_capacity(1024),
            next: 0,
        }
    }
}
struct Recording {
    phase: GpuSimulationPhase,
    mailbox: TimingMailbox,
    batch: TimingBatch,
    owner: Entity,
    token: u32,
    time: f32,
}
thread_local! { static ACTIVE: RefCell<Option<Recording>> = const { RefCell::new(None) }; }

pub(super) fn begin_frame(
    state: &mut PhaseState,
    timings: &GpuSimulationPhaseTimings,
    device: &RenderDevice,
    period: f32,
) {
    ACTIVE.with(|a| assert!(a.borrow().is_none(), "phase timing must not nest"));
    if !timings.enabled.load(Ordering::Acquire) {
        return;
    }
    let index = state.next;
    state.next = (index + 1) % 4;
    if let Some(batch) = state.timer.begin(device, period) {
        ACTIVE.with(|a| {
            *a.borrow_mut() = Some(Recording {
                phase: GpuSimulationPhase::ALL[index],
                mailbox: timings.mailboxes[index].clone(),
                batch,
                owner: Entity::PLACEHOLDER,
                token: 0,
                time: 0.0,
            })
        });
    }
}
pub(super) fn owner(owner: Entity, token: u32, time: f32) {
    ACTIVE.with(|a| {
        if let Some(r) = a.borrow_mut().as_mut() {
            r.owner = owner;
            r.token = token;
            r.time = time;
        }
    });
}
pub(super) fn begin(encoder: &mut wgpu::CommandEncoder, phase: GpuSimulationPhase) -> Option<u32> {
    ACTIVE.with(|a| {
        let mut a = a.borrow_mut();
        let r = a.as_mut().filter(|r| r.phase == phase)?;
        let index = r.batch.instance(r.owner, r.token, r.time)?;
        drop(encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("aestra phase timing begin"),
            timestamp_writes: r.batch.writes(index, true, false),
        }));
        Some(index)
    })
}
pub(super) fn end(encoder: &mut wgpu::CommandEncoder, index: Option<u32>) {
    if let Some(index) = index {
        ACTIVE.with(|a| {
            let a = a.borrow();
            let r = a.as_ref().expect("phase timing window");
            drop(encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("aestra phase timing end"),
                timestamp_writes: r.batch.writes(index, false, true),
            }));
        });
    }
}
pub(super) fn finish(context: &mut RenderContext) {
    if let Some(r) = ACTIVE.with(|a| a.borrow_mut().take()) {
        r.batch.finish(context, r.mailbox);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_or_overflowed_phase_windows_remain_unknown() {
        let frame = |requested, count| TimestampFrame {
            sequence: 7,
            requested,
            samples: (0..count)
                .map(|_| super::super::timestamp_observation::Sample {
                    owner: Entity::PLACEHOLDER,
                    token: 1,
                    time: 1.0,
                    nanoseconds: 10,
                    sequence: 7,
                    work: None,
                })
                .collect(),
        };
        assert_eq!(
            summarize(GpuSimulationPhase::Events, frame(2, 2)).nanoseconds,
            Some(20)
        );
        assert_eq!(
            summarize(GpuSimulationPhase::Events, frame(3, 2)).nanoseconds,
            None
        );
        assert_eq!(
            summarize(GpuSimulationPhase::Events, frame(1, 0)).nanoseconds,
            None
        );
        let timings = GpuSimulationPhaseTimings::default();
        assert!(timings.samples().is_empty());
    }
}
