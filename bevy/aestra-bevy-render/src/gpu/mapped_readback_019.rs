//! Mapped-view API boundary; callers release the view before unmapping.
pub(super) type Wrapped<T> = bevy::render::renderer::WgpuWrapper<T>;
pub(super) fn wrap<T>(value: T) -> Wrapped<T> {
    Wrapped::new(value)
}
pub(crate) fn with_mapped_range<T>(
    buffer: &wgpu::Buffer,
    range: std::ops::Range<u64>,
    read: impl FnOnce(&[u8]) -> T,
) -> Option<T> {
    let bytes = buffer.slice(range).get_mapped_range();
    Some(read(&bytes))
}

#[cfg(test)]
pub(super) fn render_device(device: wgpu::Device) -> bevy::render::renderer::RenderDevice {
    bevy::render::renderer::RenderDevice::new(bevy::render::renderer::WgpuWrapper::new(device))
}

#[cfg(test)]
pub(super) fn test_shader_buffer(
    buffer: bevy::render::render_resource::Buffer,
) -> bevy::render::storage::GpuShaderBuffer {
    bevy::render::storage::GpuShaderBuffer {
        buffer_descriptor: wgpu::BufferDescriptor {
            label: None,
            size: buffer.size(),
            usage: buffer.usage(),
            mapped_at_creation: false,
        },
        buffer,
        had_data: true,
    }
}

#[cfg(test)]
pub(super) fn pending_commands(
    pending: &mut bevy::render::renderer::PendingCommandBuffers,
) -> Vec<wgpu::CommandBuffer> {
    pending.take()
}
