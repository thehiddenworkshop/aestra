//! Bounded, non-blocking GPU simulation timestamps, separate from draw timings.
use super::*;
use aestra_runtime::{EffectInstance, ProfileValue};
use bevy::render::renderer::WgpuWrapper;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

const MAX_INSTANCES: usize = 256;
const MAX_IN_FLIGHT: usize = 3;

/// Latest context-valid GPU simulation observation. Includes all replay dispatches
/// for this instance in the sampled frame, but never its draw calls or CPU work.
#[derive(Component, Debug, Default)]
pub struct GpuSimulationTiming {
    pub(super) sample: Option<Sample>,
}

impl GpuSimulationTiming {
    pub fn time_ns(
        &self,
        instance: &EffectInstance,
        context: &GpuParticleStatistics,
    ) -> ProfileValue<u64> {
        self.sample
            .as_ref()
            .filter(|sample| {
                context.context_token(instance) == Some(sample.token)
                    && sample.time <= instance.time()
            })
            .map_or(ProfileValue::Unavailable, |sample| {
                ProfileValue::Measured(sample.nanoseconds)
            })
    }
}

#[derive(Clone, Debug)]
pub(super) struct Sample {
    pub owner: Entity,
    pub token: u32,
    pub time: f32,
    pub nanoseconds: u64,
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

pub(super) fn receive_timings(
    mailbox: Res<TimingMailbox>,
    mut players: Query<(
        &PresentedEffect,
        &GpuParticleStatistics,
        &mut GpuSimulationTiming,
    )>,
) {
    let Some(samples) = mailbox.0.lock().unwrap().pending.take() else {
        return;
    };
    // Failed maps and instances omitted by the bounded query budget become unknown,
    // rather than displaying measurements from an older active set.
    for (_, _, mut timing) in &mut players {
        timing.sample = None;
    }
    for sample in samples {
        let Ok((player, context, mut timing)) = players.get_mut(sample.owner) else {
            continue;
        };
        if context.context_token(&player.instance) == Some(sample.token)
            && sample.time.is_finite()
            && sample.time >= 0.0
            && sample.time <= player.instance.time()
        {
            timing.sample = Some(sample);
        }
    }
}

#[derive(Clone)]
struct Slot {
    queries: WgpuWrapper<wgpu::QuerySet>,
    resolve: WgpuWrapper<wgpu::Buffer>,
    readback: WgpuWrapper<wgpu::Buffer>,
    busy: Arc<AtomicBool>,
}

impl Slot {
    fn new(device: &RenderDevice) -> Self {
        let device = device.wgpu_device();
        let size = (MAX_INSTANCES * 2 * size_of::<u64>()) as u64;
        Self {
            queries: WgpuWrapper::new(device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("aestra simulation timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: (MAX_INSTANCES * 2) as u32,
            })),
            resolve: WgpuWrapper::new(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("aestra simulation timestamp resolve"),
                size,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })),
            readback: WgpuWrapper::new(device.create_buffer(&wgpu::BufferDescriptor {
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
        });
        Some(index)
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

    pub(super) fn finish(self, encoder: &mut wgpu::CommandEncoder, mailbox: TimingMailbox) {
        if self.samples.is_empty() {
            self.slot.busy.store(false, Ordering::Release);
            mailbox.publish(self.sequence, Vec::new());
            return;
        }
        let count = self.samples.len() as u32 * 2;
        let size = u64::from(count) * 8;
        encoder.resolve_query_set(&self.slot.queries, 0..count, &self.slot.resolve, 0);
        encoder.copy_buffer_to_buffer(&self.slot.resolve, 0, &self.slot.readback, 0, size);
        let readback = self.slot.readback.clone();
        encoder.map_buffer_on_submit(
            &self.slot.readback,
            wgpu::MapMode::Read,
            0..size,
            move |result| {
                let samples = if result.is_ok() {
                    let bytes = readback.slice(0..size).get_mapped_range();
                    let samples = self
                        .samples
                        .into_iter()
                        .zip(bytes.as_chunks::<16>().0)
                        .filter_map(|(mut sample, pair)| {
                            let start = u64::from_le_bytes(pair[..8].try_into().unwrap());
                            let end = u64::from_le_bytes(pair[8..].try_into().unwrap());
                            sample.nanoseconds = elapsed_ns(start, end, self.period)?;
                            Some(sample)
                        })
                        .collect();
                    drop(bytes);
                    readback.unmap();
                    samples
                } else {
                    Vec::new()
                };
                mailbox.publish(self.sequence, samples);
                self.slot.busy.store(false, Ordering::Release);
            },
        );
    }
}

fn elapsed_ns(start: u64, end: u64, period: f32) -> Option<u64> {
    // Subtract integer ticks first: converting absolute timestamps to f64 loses
    // short intervals on long-running devices. Counter reset/wrap is unknown.
    let ticks = end.checked_sub(start)?;
    let ns = ticks as f64 * f64::from(period);
    (period.is_finite() && period > 0.0 && ns.is_finite() && ns >= 0.0 && ns < u64::MAX as f64)
        .then(|| ns.round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_and_tick_conversion_do_not_invent_timings() {
        assert!(!supported(wgpu::Features::empty(), 1.0));
        assert!(!supported(wgpu::Features::TIMESTAMP_QUERY, 0.0));
        assert!(supported(wgpu::Features::TIMESTAMP_QUERY, 0.5));
        assert_eq!(elapsed_ns(u64::MAX - 20, u64::MAX - 10, 2.5), Some(25));
        assert_eq!(elapsed_ns(10, 10, 1.0), Some(0));
        assert_eq!(elapsed_ns(10, 9, 1.0), None);
        assert_eq!(elapsed_ns(0, 9, f32::NAN), None);
        assert_eq!(elapsed_ns(0, u64::MAX, 2.0), None);
    }

    #[test]
    fn gpu_timestamp_batches_resolve_and_recycle_without_unbounded_allocation() {
        let gpu = wgpu::Instance::default();
        let Ok(adapter) =
            pollster::block_on(gpu.request_adapter(&wgpu::RequestAdapterOptions::default()))
        else {
            eprintln!("Skipping timestamp test: no GPU adapter");
            return;
        };
        if !adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            eprintln!("Skipping timestamp test: timestamp queries unsupported");
            return;
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::TIMESTAMP_QUERY,
            ..Default::default()
        }))
        .expect("timestamp-capable adapter must create a device");
        let device = RenderDevice::new(WgpuWrapper::new(device));
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
        let mut encoder = device.create_command_encoder(&Default::default());
        for (first, last) in [(true, false), (false, true)] {
            let pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                timestamp_writes: batch.writes(index, first, last),
                ..Default::default()
            });
            drop(pass);
        }
        batch.finish(&mut encoder, mailbox.clone());
        let submission = queue.submit([encoder.finish()]);
        // Blocking is confined to this regression test, never production profiling.
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(std::time::Duration::from_secs(60)),
            })
            .expect("timestamp readback should complete");
        let samples = mailbox.0.lock().unwrap().pending.take().unwrap();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].token, 42);
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
        recycled.finish(
            &mut device.create_command_encoder(&Default::default()),
            mailbox.clone(),
        );
        for batch in batches {
            batch.finish(
                &mut device.create_command_encoder(&Default::default()),
                mailbox.clone(),
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

    fn instance() -> EffectInstance {
        let mut effect = aestra_core::EffectAsset::new("Timed", 3.0);
        effect
            .emitters
            .push(aestra_core::Emitter::basic_sprite("Particles", 3.0));
        let mut instance = EffectInstance::new(Arc::new(
            aestra_compiler::EffectCompiler::default()
                .compile(&effect)
                .unwrap(),
        ));
        instance.set_playback_time(2.0);
        instance
    }

    #[test]
    fn timing_is_invalidated_before_render_upload_after_all_context_changes() {
        let mut instance = instance();
        let mut context = GpuParticleStatistics::new(&instance);
        for change in 0..5 {
            let old_token = context.sync(&instance);
            let timing = GpuSimulationTiming {
                sample: Some(Sample {
                    owner: Entity::PLACEHOLDER,
                    token: old_token,
                    time: 0.0,
                    nanoseconds: 123,
                }),
            };
            assert_eq!(
                timing.time_ns(&instance, &context),
                ProfileValue::Measured(123)
            );
            match change {
                0 => instance.seek(2.0),
                1 => instance.restart(),
                2 => instance.set_seed(77),
                3 => instance.invalidate_history(),
                _ => instance = EffectInstance::new(Arc::new((**instance.effect()).clone())),
            }
            assert_eq!(
                timing.time_ns(&instance, &context),
                ProfileValue::Unavailable
            );
            context.sync(&instance);
            assert_eq!(
                timing.time_ns(&instance, &context),
                ProfileValue::Unavailable
            );
        }
    }

    #[test]
    fn frame_results_isolate_repeated_sources_and_discard_expired_owners() {
        use aestra_runtime::{EffectProfile, ProjectInstanceProfile, ProjectProfile};
        let mut app = App::new();
        let mailbox = TimingMailbox::default();
        app.insert_resource(mailbox.clone())
            .add_systems(PreUpdate, receive_timings);
        let instance = instance();
        let mut owners = Vec::new();
        let mut samples = Vec::new();
        for ns in [10, 20, 90] {
            let context = GpuParticleStatistics::new(&instance);
            let token = context.context_token(&instance).unwrap();
            let mut presented = PresentedEffect::new(instance.effect().clone());
            presented.instance = instance.clone();
            let owner = app
                .world_mut()
                .spawn((presented, context, GpuSimulationTiming::default()))
                .id();
            owners.push(owner);
            samples.push(Sample {
                owner,
                token,
                time: 1.5,
                nanoseconds: ns,
            });
        }
        mailbox.publish(1, samples.clone());
        app.update();
        let entries = || {
            owners
                .iter()
                .map(|owner| {
                    let context = app.world().get::<GpuParticleStatistics>(*owner).unwrap();
                    let timing = app.world().get::<GpuSimulationTiming>(*owner).unwrap();
                    let mut profile = EffectProfile::from_compiled(instance.effect());
                    profile.gpu_simulation_time_ns = timing.time_ns(&instance, context);
                    ProjectInstanceProfile {
                        path: vec![
                            aestra_core::EffectClipId::new(),
                            aestra_core::EffectClipId::new(),
                        ],
                        effect: instance.effect().source,
                        name: "Nested repeated source".into(),
                        profile,
                    }
                })
                .collect::<Vec<_>>()
        };
        let entries = entries();
        let mut first_root = ProjectProfile::default();
        first_root.update(entries[..2].to_vec());
        let mut second_root = ProjectProfile::default();
        second_root.update(entries[2..].to_vec());
        assert_eq!(
            first_root.total.gpu_simulation_time_ns,
            ProfileValue::Measured(30)
        );
        assert_eq!(
            second_root.total.gpu_simulation_time_ns,
            ProfileValue::Measured(90)
        );
        assert_eq!(first_root.total.gpu_time_ns, ProfileValue::Unavailable);
        app.world_mut().entity_mut(owners[0]).despawn();
        app.world_mut()
            .get_mut::<PresentedEffect>(owners[1])
            .unwrap()
            .instance
            .seek(2.0);
        mailbox.publish(2, samples);
        app.update();
        assert!(
            app.world()
                .get::<GpuSimulationTiming>(owners[1])
                .unwrap()
                .sample
                .is_none()
        );
        assert_eq!(
            app.world()
                .get::<GpuSimulationTiming>(owners[2])
                .unwrap()
                .sample
                .as_ref()
                .unwrap()
                .nanoseconds,
            90
        );
        // A failed map or a skipped query budget cannot retain old measurements.
        mailbox.publish(3, Vec::new());
        app.update();
        assert!(
            app.world()
                .get::<GpuSimulationTiming>(owners[2])
                .unwrap()
                .sample
                .is_none()
        );
    }
}
