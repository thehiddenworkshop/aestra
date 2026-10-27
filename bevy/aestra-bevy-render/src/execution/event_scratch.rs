//! Reusable event-link scratch. No particle state, history or cross-owner sharing.
use super::*;

pub(crate) struct EventGatherBuffers {
    pub(super) words: [u32; 6],
    pub(super) params: wgpu::Buffer,
    pub(super) order: wgpu::Buffer,
    pub(super) indirect: wgpu::Buffer,
}
impl EventGatherBuffers {
    pub(crate) fn bytes(&self) -> u64 {
        self.params.size() + self.order.size() + self.indirect.size()
    }
    pub(crate) fn words(
        capacity: u32,
        link: &aestra_runtime::CompiledEventLink,
        requested: u32,
        dropped: u32,
    ) -> [u32; 6] {
        [
            aestra_runtime::event_trigger_bit(link.trigger),
            link.count,
            capacity,
            aestra_runtime::PARTICLE_EVENT_CAPACITY,
            requested,
            dropped,
        ]
    }
    pub(crate) fn new(device: &wgpu::Device, words: [u32; 6]) -> Self {
        let order_bytes = (6 + u64::from(aestra_runtime::PARTICLE_EVENT_CAPACITY)) * 4;
        let _allocation = crate::gpu::encoding_profile::scope(
            crate::gpu::encoding_profile::Operation::EventGatherPrepare,
            3,
            24 + order_bytes + 12,
            24,
            0,
            0,
        );
        Self {
            words,
            params: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("aestra event gather params"),
                contents: &words_to_bytes(&words),
                usage: wgpu::BufferUsages::STORAGE,
            }),
            order: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("aestra event order and dispatch plan"),
                size: order_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            indirect: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("aestra event expansion indirect dispatch"),
                size: 12,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::INDIRECT,
                mapped_at_creation: false,
            }),
        }
    }
    pub(crate) fn matches(&self, words: [u32; 6]) -> bool {
        self.words == words
    }
}

pub(crate) struct DomainSpawnBuffers {
    pub(super) words: [u32; 4],
    pub(super) params: wgpu::Buffer,
    pub(super) plan: wgpu::Buffer,
}
impl DomainSpawnBuffers {
    pub(crate) fn bytes(&self) -> u64 {
        self.params.size() + self.plan.size()
    }
    pub(crate) fn words(
        spawn: &aestra_runtime::CompiledDomainSpawn,
        acceptance: u32,
        births: Option<&wgpu::Buffer>,
    ) -> [u32; 4] {
        let mut words = aestra_gpu::domain_spawn_params(spawn);
        words[2] = acceptance;
        words[3] = births.map_or(0, |events| {
            (events.size().saturating_sub(16) / 32)
                .min(u64::from(aestra_runtime::PARTICLE_EVENT_CAPACITY)) as u32
        });
        words
    }
    pub(crate) fn new(device: &wgpu::Device, words: [u32; 4]) -> Self {
        let bytes = aestra_gpu::DOMAIN_SPAWN_PLAN_WORDS as u64 * 4;
        let _allocation = crate::gpu::encoding_profile::scope(
            crate::gpu::encoding_profile::Operation::EventSpawnPrepare,
            2,
            16 + bytes,
            16,
            0,
            0,
        );
        Self {
            words,
            params: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("aestra domain spawn params"),
                contents: &words_to_bytes(&words),
                usage: wgpu::BufferUsages::STORAGE,
            }),
            plan: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("aestra domain spawn plan"),
                size: bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::INDIRECT,
                mapped_at_creation: false,
            }),
        }
    }
    pub(crate) fn matches(&self, words: [u32; 4]) -> bool {
        self.words == words
    }
}
