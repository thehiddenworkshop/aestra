//! Optional CPU-side command-encoding ledger. No GPU waits, timestamps or readbacks.
use bevy::prelude::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

/// One prepared owner in the most recently encoded simulation frame.
#[derive(Clone, Debug)]
pub struct GpuPlaybackSample {
    pub owner: Entity,
    pub requested_time: f32,
    /// Time actually encoded for presentation, or `None` when preparation blocked it.
    /// This is not a GPU completion fence or proof of visible pixels.
    pub encoded_time: Option<f32>,
    /// CPU command-encoding scope, available only with profiling enabled.
    pub cpu_encoding_ns: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct GpuPlaybackFrame {
    pub sequence: u64,
    pub samples: Vec<GpuPlaybackSample>,
    pub cpu_encoding_ns: Option<u64>,
    /// Instrumented simulation call sites only; not a complete GPU/driver allocation ledger.
    pub encoding_operations: Option<Vec<super::encoding_profile::GpuEncodingOperation>>,
    /// Live cache ownership in this render-world frame, excluding in-flight driver resources.
    pub event_scratch: Option<GpuEventScratchRetention>,
    pub parameter_scratch: Option<GpuParameterRetention>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GpuParameterRetention {
    pub destinations: u64,
    pub destination_bytes: u64,
    pub bind_groups: u64,
    pub upload_pages: u64,
    pub upload_bytes: u64,
}

/// Latest stateful render-world preparation system window (not all render preparation).
#[derive(Clone, Copy, Debug, Default)]
pub struct GpuStatefulPreparationFrame {
    pub sequence: u64,
    pub cpu_ns: u64,
    pub allocated_emitters: u64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GpuEventScratchRetention {
    pub links: u64,
    pub buffers: u64,
    pub bytes: u64,
}

/// CPU artifact/buffer-asset preparation at initial activation. This excludes
/// render-thread GPU allocation, submission and GPU execution.
#[derive(Component, Clone, Copy, Debug)]
pub struct GpuActivationTiming {
    pub cpu_preparation_ns: u64,
}

/// Opt-in latest-frame diagnostics, shared with the render world. Storage is bounded
/// by the number of currently prepared owners; old frames are replaced, not retained.
#[derive(Resource, Clone, Default)]
pub struct GpuPlaybackDiagnostics {
    enabled: Arc<AtomicBool>,
    profiling: Arc<AtomicBool>,
    frame: Arc<Mutex<GpuPlaybackFrame>>,
    preparation: Arc<Mutex<GpuStatefulPreparationFrame>>,
}
impl GpuPlaybackDiagnostics {
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }
    pub fn snapshot(&self) -> GpuPlaybackFrame {
        self.frame.lock().expect("playback ledger poisoned").clone()
    }
    pub fn stateful_preparation_snapshot(&self) -> GpuStatefulPreparationFrame {
        *self
            .preparation
            .lock()
            .expect("preparation ledger poisoned")
    }
    pub(super) fn record_stateful_preparation(&self, cpu_ns: u64, allocated_emitters: u64) {
        let mut frame = self
            .preparation
            .lock()
            .expect("preparation ledger poisoned");
        frame.sequence += 1;
        frame.cpu_ns = cpu_ns;
        frame.allocated_emitters = allocated_emitters;
    }
    /// Additional CPU timers; ordinary playback and plain encoding reports leave this off.
    pub fn set_profiling_enabled(&self, enabled: bool) {
        self.profiling.store(enabled, Ordering::Relaxed);
    }
    /// Whether optional CPU preparation/encoding timers are requested.
    pub fn profiling_enabled(&self) -> bool {
        self.profiling.load(Ordering::Relaxed)
    }
    pub(super) fn begin(&self) -> Option<Recording> {
        self.enabled.load(Ordering::Relaxed).then(|| {
            let started = self.profiling_enabled().then(|| {
                super::encoding_profile::begin();
                std::time::Instant::now()
            });
            Recording {
                mailbox: self.clone(),
                samples: Vec::new(),
                started,
                event_scratch: None,
                parameter_scratch: None,
            }
        })
    }
}

pub(super) struct Recording {
    mailbox: GpuPlaybackDiagnostics,
    pub samples: Vec<GpuPlaybackSample>,
    started: Option<std::time::Instant>,
    pub event_scratch: Option<GpuEventScratchRetention>,
    pub parameter_scratch: Option<GpuParameterRetention>,
}
impl Recording {
    pub fn owner(&mut self, owner: Entity) -> Option<OwnerRecording<'_>> {
        let started = self.started.map(|_| std::time::Instant::now());
        self.samples
            .iter_mut()
            .find(|s| s.owner == owner)
            .map(|sample| OwnerRecording { sample, started })
    }
}
pub(super) struct OwnerRecording<'a> {
    sample: &'a mut GpuPlaybackSample,
    started: Option<std::time::Instant>,
}
impl OwnerRecording<'_> {
    pub fn encoded(&mut self, time: f32) {
        self.sample.encoded_time = Some(time);
    }
}
impl Drop for OwnerRecording<'_> {
    fn drop(&mut self) {
        self.sample.cpu_encoding_ns = self
            .started
            .map(|t| t.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64);
    }
}
impl Drop for Recording {
    fn drop(&mut self) {
        let mut frame = self.mailbox.frame.lock().expect("playback ledger poisoned");
        frame.sequence += 1;
        frame.samples = std::mem::take(&mut self.samples);
        frame.cpu_encoding_ns = self
            .started
            .map(|t| t.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64);
        frame.encoding_operations = self.started.map(|_| super::encoding_profile::finish());
        frame.event_scratch = self.event_scratch;
        frame.parameter_scratch = self.parameter_scratch;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ledger_is_opt_in_replaces_frames_and_keeps_blocked_owners_explicit() {
        let ledger = GpuPlaybackDiagnostics::default();
        assert!(ledger.begin().is_none());
        ledger.set_enabled(true);
        let owner = World::new().spawn_empty().id();
        {
            let mut frame = ledger.begin().unwrap();
            frame.samples.push(GpuPlaybackSample {
                owner,
                requested_time: 2.0,
                encoded_time: None,
                cpu_encoding_ns: None,
            });
            frame.owner(owner).unwrap().encoded(1.5);
        }
        assert_eq!(ledger.snapshot().samples[0].encoded_time, Some(1.5));
        {
            let mut frame = ledger.begin().unwrap();
            frame.samples.push(GpuPlaybackSample {
                owner,
                requested_time: 3.0,
                encoded_time: None,
                cpu_encoding_ns: None,
            });
        }
        let snapshot = ledger.snapshot();
        assert_eq!(snapshot.sequence, 2);
        assert_eq!(snapshot.samples.len(), 1);
        assert_eq!(snapshot.samples[0].encoded_time, None);
        assert!(snapshot.cpu_encoding_ns.is_none());
        assert!(snapshot.samples[0].cpu_encoding_ns.is_none());
    }

    #[test]
    fn profiling_timers_are_opt_in_and_owner_scope_closes_on_early_return() {
        let ledger = GpuPlaybackDiagnostics::default();
        ledger.set_enabled(true);
        ledger.set_profiling_enabled(true);
        let owner = World::new().spawn_empty().id();
        {
            let mut frame = ledger.begin().unwrap();
            frame.samples.push(GpuPlaybackSample {
                owner,
                requested_time: 1.0,
                encoded_time: None,
                cpu_encoding_ns: None,
            });
            let leave_blocked = |frame: &mut Recording| {
                let _owner = frame.owner(owner).unwrap();
                // Early returns/continues still close the CPU window without
                // claiming that a simulation was encoded.
            };
            leave_blocked(&mut frame);
        }
        let snapshot = ledger.snapshot();
        assert!(snapshot.cpu_encoding_ns.is_some());
        assert!(snapshot.samples[0].cpu_encoding_ns.is_some());
        assert_eq!(snapshot.samples[0].encoded_time, None);
    }
}
