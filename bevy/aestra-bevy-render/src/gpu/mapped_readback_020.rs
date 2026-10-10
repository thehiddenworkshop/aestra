//! Mapped-view API boundary; callers release the view before unmapping.
pub(super) type Wrapped<T> = T;
pub(super) fn wrap<T>(value: T) -> Wrapped<T> {
    value
}
pub(crate) fn with_mapped_range<T>(
    buffer: &wgpu::Buffer,
    range: std::ops::Range<u64>,
    read: impl FnOnce(&[u8]) -> T,
) -> Option<T> {
    let bytes = buffer.slice(range).get_mapped_range().ok()?;
    Some(read(&bytes))
}

#[cfg(test)]
pub(super) fn render_device(device: wgpu::Device) -> bevy::render::renderer::RenderDevice {
    bevy::render::renderer::RenderDevice::new(device)
}
