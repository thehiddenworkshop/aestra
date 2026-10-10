//! Bounded asynchronous timestamp transport shared by simulation and render preparation.
use super::mapped_readback::{Wrapped, wrap};
use bevy::{
    prelude::*,
    render::renderer::{RenderContext, RenderDevice},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

const MAX_INSTANCES: usize = 256;
const MAX_IN_FLIGHT: usize = 3;

/// Work encoded in the same simulation window as a GPU timestamp pair.
/// Counts describe submitted work, not asynchronously read-back live populations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GpuSimulationWork {
    /// Shared fixed ticks actually advanced. Unknown for independent/analytic paths.
    pub fixed_ticks: Option<u32>,
    /// All history observations, including initialization or a dirty paused frame.
    pub trail_observations: u32,
    /// Total dispatched history workgroups across every observation (not ribbons).
    pub trail_workgroups: u64,
    /// GPU bytes actually copied to particle/trail checkpoints in a coupled
    /// window. Unknown for other paths; excludes domain checkpoints and restores.
    pub checkpoint_capture_bytes: Option<u64>,
}

#[derive(Clone, Debug)]
pub(super) struct Sample {
    pub owner: Entity,
    pub token: u32,
    pub time: f32,
    pub nanoseconds: u64,
    pub sequence: u64,
    pub work: Option<GpuSimulationWork>,
}

#[derive(Default)]
struct Mailbox {
    sequence: u64,
    pending: Option<Vec<Sample>>,
}

/// A single latest-frame snapshot, not a growing queue of diagnostic paths/results.
#[derive(Resource, Default, Clone)]
pub(super) struct TimingMailbox(Arc<Mutex<Mailbox>>);

impl TimingMailbox {
    pub(super) fn take(&self) -> Option<Vec<Sample>> {
        self.0.lock().unwrap().pending.take()
    }

    pub(super) fn publish(&self, sequence: u64, samples: Vec<Sample>) {
        let mut mailbox = self.0.lock().unwrap();
        if sequence > mailbox.sequence {
            mailbox.sequence = sequence;
            mailbox.pending = Some(samples);
        }
    }
}

#[derive(Clone)]
struct Slot {
    queries: Wrapped<wgpu::QuerySet>,
    resolve: Wrapped<wgpu::Buffer>,
    readback: Wrapped<wgpu::Buffer>,
    busy: Arc<AtomicBool>,
}

impl Slot {
    fn new(device: &RenderDevice) -> Self {
        let device = device.wgpu_device();
        let size = (MAX_INSTANCES * 2 * size_of::<u64>()) as u64;
        Self {
            queries: wrap(device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("aestra simulation timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: (MAX_INSTANCES * 2) as u32,
            })),
            resolve: wrap(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("aestra simulation timestamp resolve"),
                size,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })),
            readback: wrap(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("aestra simulation timestamp readback"),
                size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            })),
            busy: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[derive(Default)]
pub(super) struct SimulationTimer {
    slots: Vec<Slot>,
    sequence: u64,
}

fn supported(features: wgpu::Features, period: f32) -> bool {
    features.contains(wgpu::Features::TIMESTAMP_QUERY) && period.is_finite() && period > 0.0
}

impl SimulationTimer {
    pub(super) fn begin(&mut self, device: &RenderDevice, period: f32) -> Option<TimingBatch> {
        if !supported(device.features(), period) {
            return None;
        }
        let available = self
            .slots
            .iter()
            .position(|slot| !slot.busy.load(Ordering::Acquire));
        let index = available.or_else(|| {
            if self.slots.len() == MAX_IN_FLIGHT {
                return None;
            }
            self.slots.push(Slot::new(device));
            Some(self.slots.len() - 1)
        })?;
        let slot = self.slots[index].clone();
        slot.busy.store(true, Ordering::Release);
        self.sequence += 1;
        Some(TimingBatch {
            slot,
            sequence: self.sequence,
            period,
            samples: Vec::new(),
        })
    }
}

pub(super) struct TimingBatch {
    slot: Slot,
    sequence: u64,
    period: f32,
    samples: Vec<Sample>,
}

impl TimingBatch {
    /// The query set pass timestamps write into.
    pub(super) fn query_set(&self) -> &wgpu::QuerySet {
        &self.slot.queries
    }

    pub(super) fn instance(&mut self, owner: Entity, token: u32, time: f32) -> Option<u32> {
        if self.samples.len() == MAX_INSTANCES {
            return None;
        }
        let index = (self.samples.len() * 2) as u32;
        self.samples.push(Sample {
            owner,
            token,
            time,
            nanoseconds: 0,
            sequence: self.sequence,
            work: None,
        });
        Some(index)
    }

    pub(super) fn work(&mut self, index: u32, work: GpuSimulationWork) {
        self.samples[index as usize / 2].work = Some(work);
    }

    /// Pass-boundary timestamps need only TIMESTAMP_QUERY (not the optional
    /// inside-encoder/inside-pass features). One pair encloses all replay passes.
    pub(super) fn writes(
        &self,
        index: u32,
        first: bool,
        last: bool,
    ) -> Option<wgpu::ComputePassTimestampWrites<'_>> {
        (first || last).then_some(wgpu::ComputePassTimestampWrites {
            query_set: &self.slot.queries,
            beginning_of_pass_write_index: first.then_some(index),
            end_of_pass_write_index: last.then_some(index + 1),
        })
    }

    pub(super) fn finish(self, context: &mut RenderContext, mailbox: TimingMailbox) {
        let device = context.render_device().clone();
        if let Some(copy) = self.resolve_and_copy(context.command_encoder(), &device, mailbox) {
            // add_command_buffer flushes the current (resolve) encoder first.
            // This preserves ordering without a queue submission or blocking CPU wait.
            context.add_command_buffer(copy);
        }
    }

    fn resolve_and_copy(
        self,
        encoder: &mut wgpu::CommandEncoder,
        device: &RenderDevice,
        mailbox: TimingMailbox,
    ) -> Option<wgpu::CommandBuffer> {
        if self.samples.is_empty() {
            self.slot.busy.store(false, Ordering::Release);
            mailbox.publish(self.sequence, Vec::new());
            return None;
        }
        let count = self.samples.len() as u32 * 2;
        let size = u64::from(count) * 8;
        encoder.resolve_query_set(&self.slot.queries, 0..count, &self.slot.resolve, 0);
        // Vulkan can copy zero/stale query results when resolve and copy share an
        // encoder (https://github.com/gfx-rs/wgpu/issues/6406). Keep the copy in a
        // subsequent command buffer, also used by the native benchmark harness.
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("aestra timestamp readback copy"),
        });
        encoder.copy_buffer_to_buffer(&self.slot.resolve, 0, &self.slot.readback, 0, size);
        let readback = self.slot.readback.clone();
        encoder.map_buffer_on_submit(
            &self.slot.readback,
            wgpu::MapMode::Read,
            0..size,
            move |result| {
                let samples = if result.is_ok() {
                    let samples =
                        super::mapped_readback::with_mapped_range(&readback, 0..size, |bytes| {
                            self.samples
                                .into_iter()
                                .zip(bytes.as_chunks::<16>().0)
                                .filter_map(|(mut sample, pair)| {
                                    let start = u64::from_le_bytes(pair[..8].try_into().unwrap());
                                    let end = u64::from_le_bytes(pair[8..].try_into().unwrap());
                                    sample.nanoseconds = elapsed_ns(start, end, self.period)?;
                                    Some(sample)
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    readback.unmap();
                    samples
                } else {
                    Vec::new()
                };
                mailbox.publish(self.sequence, samples);
                self.slot.busy.store(false, Ordering::Release);
            },
        );
        Some(encoder.finish())
    }
}

fn elapsed_ns(start: u64, end: u64, period: f32) -> Option<u64> {
    // Subtract integer ticks first: converting absolute timestamps to f64 loses
    // short intervals on long-running devices. Counter reset/wrap is unknown.
    if start == 0 || end == 0 {
        return None;
    }
    let ticks = end.checked_sub(start)?;
    let ns = ticks as f64 * f64::from(period);
    (period.is_finite() && period > 0.0 && ns.is_finite() && ns >= 0.0 && ns < u64::MAX as f64)
        .then(|| ns.round() as u64)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn capability_and_tick_conversion_do_not_invent_timings() {
        assert!(!supported(wgpu::Features::empty(), 1.0));
        assert!(!supported(wgpu::Features::TIMESTAMP_QUERY, 0.0));
        assert!(supported(wgpu::Features::TIMESTAMP_QUERY, 0.5));
        assert_eq!(elapsed_ns(u64::MAX - 20, u64::MAX - 10, 2.5), Some(25));
        assert_eq!(elapsed_ns(10, 10, 1.0), Some(0));
        assert_eq!(elapsed_ns(10, 9, 1.0), None);
        assert_eq!(elapsed_ns(0, 0, 1.0), None);
        assert_eq!(elapsed_ns(0, 9, 1.0), None);
        assert_eq!(elapsed_ns(0, 9, f32::NAN), None);
        assert_eq!(elapsed_ns(0, u64::MAX, 2.0), None);
    }

    // Called by the shipping regression and the candidate's opt-in native gate.
    pub(crate) fn gpu_timestamp_batches_resolve_and_recycle_without_unbounded_allocation(
        require_hardware: bool,
    ) {
        let gpu = wgpu::Instance::default();
        let Ok(adapter) =
            pollster::block_on(gpu.request_adapter(&wgpu::RequestAdapterOptions::default()))
        else {
            assert!(
                !require_hardware,
                "timestamp qualification requires hardware"
            );
            eprintln!("Skipping timestamp test: no GPU adapter");
            return;
        };
        if require_hardware {
            assert_ne!(adapter.get_info().device_type, wgpu::DeviceType::Cpu);
        }
        if !adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            assert!(
                !require_hardware,
                "timestamp qualification requires timestamp queries"
            );
            eprintln!("Skipping timestamp test: timestamp queries unsupported");
            return;
        }
        println!("Native timestamp transport: {:?}", adapter.get_info());
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::TIMESTAMP_QUERY,
            ..Default::default()
        }))
        .expect("timestamp-capable adapter must create a device");
        let device = super::super::mapped_readback::render_device(device);
        let mailbox = TimingMailbox::default();
        let period = queue.get_timestamp_period();
        let mut timer = SimulationTimer::default();
        let mut batches: Vec<_> = (0..MAX_IN_FLIGHT)
            .map(|_| timer.begin(&device, period).unwrap())
            .collect();
        assert!(
            timer.begin(&device, period).is_none(),
            "backpressure must skip, not allocate"
        );
        let mut batch = batches.remove(0);
        let index = batch.instance(Entity::PLACEHOLDER, 42, 1.0).unwrap();
        let work = GpuSimulationWork {
            fixed_ticks: Some(3),
            trail_observations: 4,
            trail_workgroups: 120,
            checkpoint_capture_bytes: Some(1024),
        };
        batch.work(index, work);
        let _ = batch.query_set();
        let mut encoder = device.create_command_encoder(&Default::default());
        for (first, last) in [(true, false), (false, true)] {
            let pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                timestamp_writes: batch.writes(index, first, last),
                ..Default::default()
            });
            drop(pass);
        }
        let copy = batch
            .resolve_and_copy(&mut encoder, &device, mailbox.clone())
            .unwrap();
        let submission = queue.submit([encoder.finish(), copy]);
        // Blocking is confined to this regression test, never production profiling.
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(std::time::Duration::from_secs(60)),
            })
            .expect("timestamp readback should complete");
        let samples = mailbox.take().unwrap();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].owner, Entity::PLACEHOLDER);
        assert_eq!(samples[0].time, 1.0);
        assert_eq!(samples[0].token, 42);
        assert_eq!(samples[0].sequence, 1);
        assert_eq!(samples[0].work, Some(work));
        let mut recycled = timer
            .begin(&device, period)
            .expect("completed slot must recycle");
        assert_eq!(timer.slots.len(), MAX_IN_FLIGHT);
        for _ in 0..MAX_INSTANCES {
            assert!(recycled.instance(Entity::PLACEHOLDER, 42, 1.0).is_some());
        }
        assert!(recycled.instance(Entity::PLACEHOLDER, 42, 1.0).is_none());
        // These reservations intentionally have no GPU work.
        recycled.samples.clear();
        assert!(
            recycled
                .resolve_and_copy(
                    &mut device.create_command_encoder(&Default::default()),
                    &device,
                    mailbox.clone(),
                )
                .is_none()
        );
        for batch in batches {
            assert!(
                batch
                    .resolve_and_copy(
                        &mut device.create_command_encoder(&Default::default()),
                        &device,
                        mailbox.clone(),
                    )
                    .is_none()
            );
        }
    }

    #[test]
    fn mailbox_keeps_only_newest_frame_even_after_consumption() {
        let mailbox = TimingMailbox::default();
        mailbox.publish(5, Vec::new());
        assert!(mailbox.0.lock().unwrap().pending.take().is_some());
        mailbox.publish(4, Vec::new());
        assert!(mailbox.0.lock().unwrap().pending.is_none());
        mailbox.publish(6, Vec::new());
        mailbox.publish(7, Vec::new());
        assert_eq!(mailbox.0.lock().unwrap().sequence, 7);
    }
}
