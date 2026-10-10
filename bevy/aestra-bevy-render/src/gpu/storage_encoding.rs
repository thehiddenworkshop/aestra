//! WGSL storage layout, independent of Bevy's version-specific ShaderBuffer API.
//! Do not replace this with Rust-memory/Pod casts: vec3, matrices and arrays need
//! encase's shader alignment, including the existing Aestra particle ABI.
use bevy::render::render_resource::encase::{ShaderType, StorageBuffer, internal::WriteInto};

pub(crate) fn encode<T: ShaderType + WriteInto>(value: &T) -> Vec<u8> {
    let mut buffer = StorageBuffer::new(Vec::with_capacity(value.size().get() as usize));
    buffer
        .write(value)
        .expect("Aestra storage layout must encode");
    buffer.into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_gpu::{GpuGlobals, GpuParticle, GpuRenderGlobals, GpuRenderParams};

    #[test]
    fn shader_record_sizes_and_offsets_remain_stable() {
        let globals = GpuGlobals {
            time: 1.25,
            total_slots: 19,
            seed: 73,
            emitter_count: 2,
            ..Default::default()
        };
        let bytes = encode(&globals);
        let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        assert_eq!(bytes.len(), 112);
        assert_eq!(word(0), 1.25_f32.to_bits());
        assert_eq!(word(4), 19);
        assert_eq!(word(8), 73);
        assert_eq!(word(12), 2);
        for index in 0..16 {
            assert_eq!(
                word(32 + index * 4),
                if index % 5 == 0 { 1.0_f32.to_bits() } else { 0 }
            );
        }
        assert_eq!(word(96), f32::MAX.to_bits());
        assert_eq!(word(100), f32::MAX.to_bits());
        assert_eq!(encode(&GpuRenderGlobals::default()).len(), 80);
        assert_eq!(encode(&GpuRenderParams::default()).len(), 80);
        assert_eq!(encode(&GpuParticle::default()).len(), 48);
        assert_eq!(encode(&vec![GpuParticle::default(); 3]).len(), 144);
        assert_eq!(
            encode(&vec![3_u32, 5, 9]),
            [3, 5, 9]
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>()
        );
    }
}
