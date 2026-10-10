//! Candidate 0.20 adapter, compiled by the migration qualification workspace.
//! Select this instead of storage_buffers.rs at the coherent engine switch.
use super::storage_encoding;
use bevy::render::{
    render_resource::{
        BufferUsages,
        encase::{ShaderType, internal::WriteInto},
    },
    storage::ShaderBuffer,
};

pub(super) fn new<T: ShaderType + WriteInto>(value: T) -> ShaderBuffer {
    ShaderBuffer::new(storage_encoding::encode(&value), Default::default())
}

pub(super) fn update<T: ShaderType + WriteInto>(buffer: &mut ShaderBuffer, value: T) {
    // Extraction drains the CPU data. Reinitialize it, or clear an unextracted
    // value first; extending without clearing would concatenate whole frames.
    buffer.clear();
    buffer.extend_from_slice(&storage_encoding::encode(&value));
}

pub(super) fn bytes(buffer: &ShaderBuffer) -> Option<&[u8]> {
    buffer.cast_slice::<u8>()
}

pub(super) fn indirect<T: ShaderType + WriteInto>(value: T) -> ShaderBuffer {
    let mut buffer = new(value);
    buffer.buffer_usage |= BufferUsages::INDIRECT;
    buffer
}
