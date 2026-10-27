//! One sequential parameter destination per coupled emitter, with current-resource groups.
use super::{parameter_uploads::ParameterUploads, stateful_simulation::*};
use bevy::render::{
    render_resource::{
        BindGroup, BindGroupLayout, BindGroupLayoutId, Buffer, BufferDescriptor, BufferId,
        BufferUsages, CommandEncoder,
    },
    renderer::RenderDevice,
};
use std::cell::RefCell;

pub(super) struct StatefulParameters {
    buffer: Buffer,
    key: Option<(BindGroupLayoutId, [BufferId; 11])>,
    group: Option<BindGroup>,
}
impl StatefulParameters {
    pub fn encode(
        device: &RenderDevice,
        encoder: &mut CommandEncoder,
        layout: &BindGroupLayout,
        persistent: &StatefulPersistentState,
        render: &StatefulRenderBuffers<'_>,
        uploads: &RefCell<ParameterUploads>,
        bytes: &[u8],
    ) -> (BindGroup, Buffer) {
        let mut cached = persistent
            .parameters
            .lock()
            .expect("stateful parameter cache poisoned");
        if cached
            .as_ref()
            .is_none_or(|p| p.buffer.size() != bytes.len() as u64)
        {
            let _allocation = super::encoding_profile::scope(
                super::encoding_profile::Operation::ParameterDestination,
                1,
                bytes.len() as u64,
                0,
                0,
                0,
            );
            *cached = Some(Self {
                buffer: device.create_buffer(&BufferDescriptor {
                    label: Some("aestra coupled stateful parameters"),
                    size: bytes.len() as u64,
                    usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                key: None,
                group: None,
            });
        }
        let cached = cached.as_mut().unwrap();
        uploads
            .borrow_mut()
            .copy(device, encoder, &cached.buffer, bytes);
        let key = (
            layout.id(),
            [
                persistent.state.id(),
                persistent.free_list.id(),
                persistent.free_count.id(),
                persistent.spawn_counter.id(),
                render.particles.id(),
                render.alive.id(),
                render.indirect.id(),
                render.counters.id(),
                persistent.events.id(),
                render.world.id(),
                render.physics.id(),
            ],
        );
        if cached.key != Some(key) {
            cached.group = Some(stateful_bind_group(
                device,
                layout,
                persistent,
                &cached.buffer,
                render,
            ));
            cached.key = Some(key);
        }
        (
            cached.group.as_ref().unwrap().clone(),
            cached.buffer.clone(),
        )
    }
    pub fn bytes(&self) -> u64 {
        self.buffer.size()
    }
}
