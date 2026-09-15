//! GPU-resident snapshots; no readback and no serialized device state.
use super::trail_replay::TrailReplay;
use bevy::render::{
    render_resource::{Buffer, BufferDescriptor, BufferId, BufferUsages, CommandEncoder},
    renderer::RenderDevice,
};

pub(super) const MEMORY_LIMIT: u64 = 64 * 1024 * 1024;
const MAX_CHECKPOINTS: usize = 4;

#[derive(Default, PartialEq)]
pub(super) struct TrailContext {
    pub emitters: Vec<u8>,
    // Full seed, explicit context revision, playback mode/duration, world transform.
    pub key: [u32; 22],
    pub motion: Option<std::sync::Arc<aestra_runtime::HostTransformContext>>,
}

struct Checkpoint {
    time: f32,
    buffer: Buffer,
}

#[derive(Default)]
pub(super) struct TrailCheckpoints {
    snapshots: Vec<Checkpoint>,
    sizes: Vec<u64>,
}

impl TrailCheckpoints {
    pub fn bytes(&self) -> u64 {
        self.snapshots.iter().map(|s| s.buffer.size()).sum()
    }

    /// Restore all mutable simulation state, then rebase only the discontinuity
    /// tags. Copying old globals would incorrectly restore a stale target/transform.
    pub fn restore(
        &self,
        encoder: &mut CommandEncoder,
        buffers: &[&Buffer],
        target: f32,
    ) -> Option<f32> {
        if self.sizes != buffers.iter().map(|b| b.size()).collect::<Vec<_>>() {
            return None;
        }
        let snapshot = self.snapshots.iter().rev().find(|s| s.time <= target)?;
        let mut offset = 0;
        for buffer in buffers {
            encoder.copy_buffer_to_buffer(&snapshot.buffer, offset, buffer, 0, buffer.size());
            offset += buffer.size();
        }
        Some(snapshot.time)
    }

    pub fn capture(
        &mut self,
        device: &RenderDevice,
        encoder: &mut CommandEncoder,
        buffers: &[&Buffer],
        time: f32,
        available: u64,
    ) {
        let sizes = buffers.iter().map(|b| b.size()).collect::<Vec<_>>();
        let size: u64 = sizes.iter().sum();
        if self.sizes != sizes {
            self.snapshots.clear();
            self.sizes = sizes;
        }
        if size > MEMORY_LIMIT
            || size > device.limits().max_buffer_size
            || self.snapshots.iter().any(|s| s.time == time)
        {
            return;
        }
        // Recycle the oldest allocation at capacity instead of allocating on
        // every replay second. Global available memory excludes our existing slots.
        let buffer = if self.snapshots.len() == MAX_CHECKPOINTS || size > available {
            if self.snapshots.is_empty() {
                return;
            }
            self.snapshots.remove(0).buffer
        } else {
            device.create_buffer(&BufferDescriptor {
                label: Some("aestra trail checkpoint"),
                size,
                usage: BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let mut offset = 0;
        for source in buffers {
            encoder.copy_buffer_to_buffer(source, 0, &buffer, offset, source.size());
            offset += source.size();
        }
        self.snapshots.push(Checkpoint { time, buffer });
        self.snapshots.sort_by(|a, b| a.time.total_cmp(&b.time));
    }
}

pub(super) fn rebase_epoch(
    encoder: &mut CommandEncoder,
    globals: &Buffer,
    aux: &Buffer,
    counters: &Buffer,
    roots: &[(u32, u32)],
) {
    // GpuGlobals._padding.x is the current epoch (byte 24).
    for &(emitter, root) in roots {
        encoder.copy_buffer_to_buffer(globals, 24, aux, root as u64 * 12, 4);
        encoder.copy_buffer_to_buffer(globals, 24, counters, (2 + emitter as u64 * 6 + 4) * 4, 4);
    }
}

#[derive(Default)]
pub(super) struct TrailHistory {
    pub replay: TrailReplay,
    pub checkpoints: TrailCheckpoints,
    state_ids: Vec<BufferId>,
}

impl TrailHistory {
    pub fn sync_buffers(&mut self, buffers: &[&Buffer]) {
        let ids = buffers.iter().map(|b| b.id()).collect::<Vec<_>>();
        if self.state_ids != ids {
            *self = Self::default();
            self.state_ids = ids;
        }
    }
}
