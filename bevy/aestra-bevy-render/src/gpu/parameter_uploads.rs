//! Frame upload pages feeding ordered copies, never queue-writing a live tick's destination.
use bevy::render::{
    render_resource::{Buffer, BufferDescriptor, BufferUsages, CommandEncoder},
    renderer::RenderDevice,
};

const PAGE_BYTES: u64 = 64 * 1024;
struct Page {
    buffer: Buffer,
    bytes: Vec<u8>,
}
#[derive(Default)]
pub(super) struct ParameterUploads {
    pages: Vec<Page>,
    current: usize,
}
impl ParameterUploads {
    pub fn begin(&mut self) {
        self.current = 0;
        for page in &mut self.pages {
            page.bytes.clear();
        }
    }
    pub fn copy(
        &mut self,
        device: &RenderDevice,
        encoder: &mut CommandEncoder,
        destination: &Buffer,
        bytes: &[u8],
    ) {
        let _copy = super::encoding_profile::transfer(
            super::encoding_profile::Operation::ParameterCopy,
            bytes.len() as u64,
            0,
        );
        if self
            .pages
            .get(self.current)
            .is_some_and(|page| page.bytes.len() as u64 + bytes.len() as u64 > page.buffer.size())
        {
            self.current += 1;
        }
        if self.current == self.pages.len() {
            let size = PAGE_BYTES.max(bytes.len() as u64);
            let _allocation = super::encoding_profile::scope(
                super::encoding_profile::Operation::ParameterPage,
                1,
                size,
                0,
                0,
                0,
            );
            self.pages.push(Page {
                buffer: device.create_buffer(&BufferDescriptor {
                    label: Some("aestra frame parameter upload page"),
                    size,
                    usage: BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                bytes: Vec::with_capacity(size as usize),
            });
        }
        let page = &mut self.pages[self.current];
        let offset = page.bytes.len() as u64;
        page.bytes.extend_from_slice(bytes);
        encoder.copy_buffer_to_buffer(&page.buffer, offset, destination, 0, bytes.len() as u64);
    }
    /// Must run before submitting ANY encoder that references these pages. Each copy
    /// has a distinct source range, so later ticks cannot replace earlier tick inputs.
    /// The previous frame has already submitted before this frame's writes are queued.
    pub fn flush(&mut self, queue: &wgpu::Queue) {
        let mut used = false;
        for page in &self.pages {
            if !page.bytes.is_empty() {
                let _upload = super::encoding_profile::transfer(
                    super::encoding_profile::Operation::ParameterUpload,
                    0,
                    page.bytes.len() as u64,
                );
                queue.write_buffer(&page.buffer, 0, &page.bytes);
                used = true;
            }
        }
        // Retain the busy period's high water, release everything on an idle frame.
        if !used {
            self.pages.clear();
        }
    }
    pub fn retained(&self) -> (u64, u64) {
        (
            self.pages.len() as u64,
            self.pages.iter().map(|p| p.buffer.size()).sum(),
        )
    }
}
