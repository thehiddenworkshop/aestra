//! Opt-in bounded asynchronous selected-light transport. No particle-buffer map,
//! blocking device poll, event routing or scene-light entities in this layer.
use super::particle_lights::{
    AestraParticleLightSettings, GpuSelectedParticleLights, ParticleLightSource,
};
use bevy::{
    ecs::system::{SystemParam, SystemState},
    prelude::*,
    render::{
        RenderApp,
        diagnostic::{RecordDiagnostics, resolve_encoder},
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_resource::{Buffer, BufferDescriptor, BufferUsages, MapMode},
        renderer::{RenderContext, RenderDevice, RenderGraph, RenderGraphSystems},
    },
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

/// Host-controlled transport budgets, independent of authored/per-output selection.
/// All storage is bounded by these budgets, not by the number of source particles.
#[derive(Resource, Clone, Debug, PartialEq, Eq, ExtractResource)]
pub struct ParticleLightReadbackSettings {
    pub max_lights: u32,
    pub max_in_flight: usize,
    pub max_staging_bytes: u64,
    /// Maximum manifest bytes per in-flight snapshot (including clip-path data).
    pub max_manifest_bytes: usize,
    pub max_age: Duration,
    pub max_frame_lag: u64,
}
impl Default for ParticleLightReadbackSettings {
    fn default() -> Self {
        Self {
            max_lights: 96,
            max_in_flight: 3,
            max_staging_bytes: 1024 * 1024,
            max_manifest_bytes: 1024 * 1024,
            max_age: Duration::from_millis(100),
            max_frame_lag: 8,
        }
    }
}
#[derive(Resource, Clone, Copy, Default, ExtractResource)]
pub struct ParticleLightReadbackFrame(pub u64);
#[derive(Clone, Debug)]
pub struct SelectedParticleLight {
    pub position: Vec3,
    pub range: f32,
    pub linear_color: Vec3,
    pub lumens: f32,
    pub radius: f32,
    pub source_token: u32,
    pub particle_index: u32,
}
#[derive(Clone, Debug)]
pub struct ParticleLightSnapshot {
    pub generation: u64,
    pub sequence: u64,
    pub frame: u64,
    pub captured_at: Instant,
    pub manifest: Arc<[ParticleLightSource]>,
    pub lights: Vec<SelectedParticleLight>,
    pub counters: [u32; 4],
    pub copied_bytes: u64,
    /// Exact originating configuration. A host may change budgets before the
    /// pipelined render world has extracted them; reject older packets at once.
    pub settings: ParticleLightReadbackSettings,
    pub selection: AestraParticleLightSettings,
}
#[derive(Clone, Default, Debug)]
pub struct ParticleLightReadbackStatistics {
    pub submitted: u64,
    pub completed: u64,
    pub skipped_busy: u64,
    pub failed: u64,
    pub stale_callbacks: u64,
    pub overwritten: u64,
    pub rejected: u64,
    pub staging_bytes: u64,
    pub pending: usize,
    pub rejection: Option<String>,
}
#[derive(Default)]
struct Shared {
    generation: u64,
    active: bool,
    newest: u64,
    latest: Option<Arc<ParticleLightSnapshot>>,
    statistics: ParticleLightReadbackStatistics,
}
/// One-result mailbox: completion order cannot resurrect an older selected set.
#[derive(Resource, Clone, Default)]
pub struct ParticleLightReadback(Arc<Mutex<Shared>>);
impl ParticleLightReadback {
    pub fn take(
        &self,
    ) -> (
        u64,
        bool,
        Option<Arc<ParticleLightSnapshot>>,
        ParticleLightReadbackStatistics,
    ) {
        let mut s = self.0.lock().unwrap();
        (
            s.generation,
            s.active,
            s.latest.take(),
            s.statistics.clone(),
        )
    }
    fn invalidate(&self, reason: Option<String>) {
        let mut s = self.0.lock().unwrap();
        s.generation = s.generation.wrapping_add(1);
        s.active = false;
        s.latest = None;
        s.statistics.rejection = reason;
    }
    fn complete(&self, snapshot: ParticleLightSnapshot) {
        let mut s = self.0.lock().unwrap();
        if !s.active || snapshot.generation != s.generation || snapshot.sequence <= s.newest {
            s.statistics.stale_callbacks += 1;
            return;
        }
        s.newest = snapshot.sequence;
        s.statistics.completed += 1;
        s.statistics.overwritten += u64::from(s.latest.is_some());
        s.latest = Some(Arc::new(snapshot));
    }
}
struct Slot {
    buffer: Buffer,
    size: u64,
    busy: Arc<AtomicBool>,
}
#[derive(Resource, Default)]
struct Staging {
    slots: Vec<Slot>,
    signature: Option<(ParticleLightReadbackSettings, AestraParticleLightSettings)>,
    sequence: u64,
}

pub struct AestraParticleLightReadbackPlugin;
/// Copy is ordered after selection and scene rendering, before diagnostic
/// resolve/submission. Hosts can place enclosing diagnostic spans after it.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ParticleLightReadbackSet {
    Copy,
}
impl Plugin for AestraParticleLightReadbackPlugin {
    fn build(&self, app: &mut App) {
        let mailbox = ParticleLightReadback::default();
        app.insert_resource(mailbox.clone())
            .init_resource::<ParticleLightReadbackSettings>()
            .init_resource::<ParticleLightReadbackFrame>()
            .add_plugins((
                ExtractResourcePlugin::<ParticleLightReadbackSettings>::default(),
                ExtractResourcePlugin::<ParticleLightReadbackFrame>::default(),
            ))
            .add_systems(First, |mut frame: ResMut<ParticleLightReadbackFrame>| {
                frame.0 = frame.0.wrapping_add(1)
            });
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .insert_resource(mailbox)
                .init_resource::<Staging>()
                .add_systems(
                    RenderGraph,
                    copy_selected
                        .in_set(ParticleLightReadbackSet::Copy)
                        .after(RenderGraphSystems::Render)
                        .before(resolve_encoder)
                        .before(RenderGraphSystems::Submit),
                );
        }
    }
}

fn manifest_size(manifest: &[ParticleLightSource]) -> Option<usize> {
    manifest.iter().try_fold(0usize, |sum, source| {
        sum.checked_add(std::mem::size_of::<ParticleLightSource>())?
            .checked_add(
                source
                    .clip_path
                    .len()
                    .checked_mul(std::mem::size_of::<aestra_core::EffectClipId>())?,
            )
    })
}

#[derive(SystemParam)]
struct CopyInputs<'w> {
    selected: Res<'w, GpuSelectedParticleLights>,
    settings: Res<'w, ParticleLightReadbackSettings>,
    selection: Res<'w, AestraParticleLightSettings>,
    frame: Res<'w, ParticleLightReadbackFrame>,
    device: Res<'w, RenderDevice>,
    mailbox: Res<'w, ParticleLightReadback>,
    staging: ResMut<'w, Staging>,
}
// Enclosing multi-system diagnostic spans are thread-local: keep this nested
// span on the schedule thread too, and flush RenderContext's deferred buffers.
fn copy_selected(world: &mut World) {
    let mut state = SystemState::<(CopyInputs, RenderContext)>::new(world);
    {
        let (mut inputs, mut context) =
            state.get_mut(world).expect("render context is initialized");
        copy_frame(&mut inputs, &mut context);
    }
    state.apply(world);
}
fn copy_frame(inputs: &mut CopyInputs, context: &mut RenderContext) {
    let selected = &inputs.selected;
    let settings = inputs.settings.as_ref();
    let selection = inputs.selection.as_ref();
    let frame = &inputs.frame;
    let device = &inputs.device;
    let mailbox = &inputs.mailbox;
    let staging = &mut inputs.staging;
    // Busy buffers remain alive until their callbacks unmap them. Do not resize
    // or orphan them on a host budget change; stop admission until they drain.
    let signature = (settings.clone(), *selection);
    if staging.signature.as_ref() != Some(&signature) {
        mailbox.invalidate(None);
        staging.signature = Some(signature);
    }
    let pending = staging
        .slots
        .iter()
        .filter(|s| s.busy.load(Ordering::Acquire))
        .count();
    {
        let mut shared = mailbox.0.lock().unwrap();
        shared.statistics.pending = pending;
        shared.statistics.staging_bytes = staging.slots.iter().map(|s| s.size).sum();
    }
    let Some(set) = selected
        .frame()
        .filter(|_| selection.max_lights != 0 && settings.max_lights != 0)
    else {
        mailbox.invalidate(selected.rejection.clone());
        if pending == 0 {
            staging.slots.clear();
            mailbox.0.lock().unwrap().statistics.staging_bytes = 0;
        }
        return;
    };
    let count = set
        .selected_capacity
        .min(settings.max_lights)
        .min(selection.max_lights);
    let bytes = u64::from(count) * 48 + 16;
    let supported = settings.max_in_flight > 0
        && settings.max_age > Duration::ZERO
        && manifest_size(&set.manifest).is_some_and(|size| size <= settings.max_manifest_bytes)
        && bytes
            .checked_mul(settings.max_in_flight as u64)
            .is_some_and(|total| total <= settings.max_staging_bytes)
        && bytes <= device.limits().max_buffer_size;
    if !supported {
        mailbox.invalidate(Some(
            "selected-light readback exceeds host/device budgets".into(),
        ));
        mailbox.0.lock().unwrap().statistics.rejected += 1;
        if pending == 0 {
            staging.slots.clear();
            mailbox.0.lock().unwrap().statistics.staging_bytes = 0;
        }
        return;
    }
    if staging.slots.iter().any(|s| s.size != bytes) || staging.slots.len() > settings.max_in_flight
    {
        if pending != 0 {
            mailbox.0.lock().unwrap().statistics.skipped_busy += 1;
            return;
        }
        staging.slots.clear();
    }
    let index = staging
        .slots
        .iter()
        .position(|s| !s.busy.load(Ordering::Acquire))
        .or_else(|| {
            if staging.slots.len() >= settings.max_in_flight {
                return None;
            }
            staging.slots.push(Slot {
                size: bytes,
                busy: Arc::new(AtomicBool::new(false)),
                buffer: device.create_buffer(&BufferDescriptor {
                    label: Some("aestra selected-light async staging"),
                    size: bytes,
                    usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
            });
            Some(staging.slots.len() - 1)
        });
    let Some(index) = index else {
        mailbox.0.lock().unwrap().statistics.skipped_busy += 1;
        return;
    };
    let slot = &staging.slots[index];
    slot.busy.store(true, Ordering::Release);
    let buffer = slot.buffer.clone();
    let busy = slot.busy.clone();
    staging.sequence = staging.sequence.wrapping_add(1);
    let mut snapshot = ParticleLightSnapshot {
        generation: 0,
        sequence: staging.sequence,
        frame: frame.0,
        captured_at: Instant::now(),
        manifest: set.manifest.clone().into(),
        lights: Vec::new(),
        counters: [0; 4],
        copied_bytes: bytes,
        settings: settings.clone(),
        selection: *selection,
    };
    {
        let mut shared = mailbox.0.lock().unwrap();
        shared.active = true;
        shared.statistics.rejection = None;
        shared.statistics.submitted += 1;
        shared.statistics.pending = pending + 1;
        shared.statistics.staging_bytes = staging.slots.iter().map(|s| s.size).sum();
        snapshot.generation = shared.generation;
    }
    let recorder = context.diagnostic_recorder();
    let diagnostics = recorder.as_deref();
    let encoder = context.command_encoder();
    let span = diagnostics.time_span(encoder, "aestra::gpu::particle_light_copy");
    encoder.copy_buffer_to_buffer(&set.records, 0, &buffer, 0, bytes - 16);
    encoder.copy_buffer_to_buffer(&set.counters, 0, &buffer, bytes - 16, 16);
    let mailbox = ParticleLightReadback::clone(mailbox);
    let selected_capacity = set.selected_capacity;
    encoder.map_buffer_on_submit(&buffer.clone(), MapMode::Read, 0..bytes, move |result| {
        let decoded = if result.is_ok() {
            let view = buffer.slice(..).get_mapped_range();
            let decoded = decode(&view, count, selected_capacity, snapshot.manifest.len());
            drop(view);
            buffer.unmap();
            decoded
        } else {
            None
        };
        if let Some((lights, counters)) = decoded {
            snapshot.lights = lights;
            snapshot.counters = counters;
            mailbox.complete(snapshot);
        } else {
            mailbox.0.lock().unwrap().statistics.failed += 1;
        }
        busy.store(false, Ordering::Release);
    });
    span.end(encoder);
    // Ordered behind selection in the same render submission, ahead of next
    // frame's overwrite. Callback only; never wait for current-frame GPU data.
}

fn decode(
    bytes: &[u8],
    copied: u32,
    capacity: u32,
    sources: usize,
) -> Option<(Vec<SelectedParticleLight>, [u32; 4])> {
    if bytes.len() != copied as usize * 48 + 16 {
        return None;
    }
    let words = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|w| u32::from_le_bytes(*w))
        .collect::<Vec<_>>();
    let counters: [u32; 4] = words[copied as usize * 12..].try_into().ok()?;
    if counters[0] < counters[1]
        || counters[1] < counters[2]
        || counters[2] > capacity
        || counters[3] != counters[1] - counters[2]
    {
        return None;
    }
    let mut lights = Vec::new();
    let mut identities = std::collections::BTreeSet::new();
    for w in words[..counters[2].min(copied) as usize * 12]
        .as_chunks::<12>()
        .0
    {
        let f = |i| f32::from_bits(w[i]);
        let light = SelectedParticleLight {
            position: Vec3::new(f(0), f(1), f(2)),
            range: f(3),
            linear_color: Vec3::new(f(4), f(5), f(6)),
            lumens: f(7),
            radius: f(8),
            source_token: w[10],
            particle_index: w[11],
        };
        if !light.position.is_finite()
            || !light.range.is_finite()
            || light.range <= 0.0
            || !light.linear_color.is_finite()
            || light.linear_color.min_element() < 0.0
            || light.linear_color.max_element() <= 0.0
            || !light.lumens.is_finite()
            || light.lumens <= 0.0
            || !light.radius.is_finite()
            || light.radius < 0.0
            || light.source_token as usize >= sources
            || !identities.insert((light.source_token, light.particle_index))
        {
            return None;
        }
        lights.push(light);
    }
    Some((lights, counters))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire(records: &[[u32; 12]], counters: [u32; 4]) -> Vec<u8> {
        records
            .iter()
            .flatten()
            .copied()
            .chain(counters)
            .flat_map(u32::to_le_bytes)
            .collect()
    }
    fn record(token: u32, ordinal: u32) -> [u32; 12] {
        [
            1.0f32.to_bits(),
            2.0f32.to_bits(),
            3.0f32.to_bits(),
            12.0f32.to_bits(),
            1.0f32.to_bits(),
            0,
            0,
            4000.0f32.to_bits(),
            0,
            0,
            token,
            ordinal,
        ]
    }
    #[test]
    fn decoder_accepts_bounded_prefix_of_larger_global_selection() {
        let bytes = wire(&[record(0, 2), record(1, 8)], [100, 50, 5, 45]);
        let (lights, counters) = decode(&bytes, 2, 5, 2).unwrap();
        assert_eq!(lights.len(), 2);
        assert_eq!(lights[1].particle_index, 8);
        assert_eq!(lights[0].position, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(counters[2], 5);
        assert!(decode(&bytes[..bytes.len() - 1], 2, 5, 2).is_none());
    }
    #[test]
    fn decoder_fails_closed_on_invalid_records_counters_and_identities() {
        for counters in [[1, 2, 1, 1], [10, 2, 3, 0], [10, 5, 4, 1], [10, 5, 1, 3]] {
            assert!(decode(&wire(&[record(0, 0)], counters), 1, 3, 1).is_none());
        }
        for (word, value) in [
            (0, f32::NAN.to_bits()),
            (3, 0),
            (4, (-1.0f32).to_bits()),
            (7, f32::INFINITY.to_bits()),
            (8, (-1.0f32).to_bits()),
            (10, 1),
        ] {
            let mut bad = record(0, 0);
            bad[word] = value;
            assert!(decode(&wire(&[bad], [1, 1, 1, 0]), 1, 1, 1).is_none());
        }
        assert!(decode(&wire(&[record(0, 0); 2], [2, 2, 2, 0]), 2, 2, 1).is_none());
        // Unused trailing records must not be interpreted as live lights.
        assert!(
            decode(&wire(&[[u32::MAX; 12]], [0; 4]), 1, 1, 0)
                .unwrap()
                .0
                .is_empty()
        );
    }
    fn snapshot(generation: u64, sequence: u64) -> ParticleLightSnapshot {
        ParticleLightSnapshot {
            generation,
            sequence,
            frame: 1,
            captured_at: Instant::now(),
            manifest: Arc::from([]),
            lights: vec![],
            counters: [0; 4],
            copied_bytes: 16,
            settings: ParticleLightReadbackSettings::default(),
            selection: AestraParticleLightSettings::default(),
        }
    }
    #[test]
    fn mailbox_is_bounded_and_rejects_out_of_order_or_invalidated_callbacks() {
        let mailbox = ParticleLightReadback::default();
        mailbox.0.lock().unwrap().active = true;
        mailbox.complete(snapshot(0, 2));
        mailbox.complete(snapshot(0, 1));
        mailbox.complete(snapshot(0, 3));
        let (_, active, latest, stats) = mailbox.take();
        assert!(active);
        assert_eq!(latest.unwrap().sequence, 3);
        assert_eq!(stats.overwritten, 1);
        assert_eq!(stats.stale_callbacks, 1);
        assert!(mailbox.take().2.is_none());
        mailbox.invalidate(None);
        mailbox.complete(snapshot(0, 4));
        mailbox.0.lock().unwrap().active = true;
        mailbox.complete(snapshot(0, 5));
        mailbox.complete(snapshot(1, 6));
        assert_eq!(mailbox.take().2.unwrap().sequence, 6);
        assert_eq!(mailbox.take().3.stale_callbacks, 3);
    }
}
