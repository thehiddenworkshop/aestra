//! Per-effect link buffers, owned by its first persistent emitter and dropped with it.
use crate::execution::{DomainSpawnBuffers, EventGatherBuffers, EventGatherPipeline};
use bevy::render::{
    render_resource::{Buffer, BufferDescriptor, BufferUsages},
    renderer::RenderDevice,
};

pub(super) struct EventLinkScratch {
    pub list: Buffer,
    pub gather: EventGatherBuffers,
    pub spawn: Option<DomainSpawnBuffers>,
}
impl EventLinkScratch {
    pub fn bytes(&self) -> u64 {
        self.list.size() + self.gather.bytes() + self.spawn.as_ref().map_or(0, |s| s.bytes())
    }
    pub fn prepare(device: &RenderDevice, slot: &mut Option<Self>, words: [u32; 6]) {
        if slot.as_ref().is_some_and(|s| s.gather.matches(words)) {
            return;
        }
        let bytes = EventGatherPipeline::list_bytes(words[2]);
        let list = {
            let _allocation = super::encoding_profile::scope(
                super::encoding_profile::Operation::EventList,
                1,
                bytes,
                0,
                0,
                0,
            );
            // The gather clears the count before every tick. Only written records below
            // that count can be consumed; no CPU zero payload is required.
            device.create_buffer(&BufferDescriptor {
                label: Some("aestra reusable event list"),
                size: bytes,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        *slot = Some(Self {
            list,
            gather: EventGatherBuffers::new(device.wgpu_device(), words),
            spawn: None,
        });
    }
}
