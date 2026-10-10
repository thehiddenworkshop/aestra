//! The shipping 0.19 ShaderBuffer adapter. Keep its field/API differences here;
//! the 0.20 qualification uses the same encoder with the new owned-byte API.
use super::storage_encoding;
use bevy::render::{
    render_resource::{
        BufferUsages,
        encase::{ShaderType, internal::WriteInto},
    },
    storage::ShaderBuffer,
};

pub(super) fn new<T: ShaderType + WriteInto>(value: T) -> ShaderBuffer {
    ShaderBuffer {
        data: Some(storage_encoding::encode(&value)),
        ..Default::default()
    }
}

pub(super) fn update<T: ShaderType + WriteInto>(buffer: &mut ShaderBuffer, value: T) {
    buffer.data = Some(storage_encoding::encode(&value));
}

pub(super) fn bytes(buffer: &ShaderBuffer) -> Option<&[u8]> {
    buffer.data.as_deref()
}

pub(super) fn indirect<T: ShaderType + WriteInto>(value: T) -> ShaderBuffer {
    let mut buffer = new(value);
    buffer.buffer_description.usage |= BufferUsages::INDIRECT;
    buffer
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_gpu::{GpuEffectArtifact, GpuGlobals, GpuRenderGlobals, GpuRenderParams};

    fn matches_legacy<T: ShaderType + WriteInto>(value: T) {
        let expected = ShaderBuffer::from(&value);
        let actual = new(&value);
        assert_eq!(actual.data, expected.data);
        assert_eq!(actual.buffer_description, expected.buffer_description);
        assert_eq!(actual.asset_usage, expected.asset_usage);
        assert_eq!(actual.copy_on_resize, expected.copy_on_resize);
        let mut updated = ShaderBuffer::default();
        update(&mut updated, value);
        assert_eq!(updated.data, expected.data);
    }

    #[test]
    fn all_uploaded_record_kinds_match_legacy_encase_bytes() {
        let mut asset = aestra_core::EffectAsset::new("Storage compatibility", 3.0);
        asset
            .emitters
            .push(aestra_core::Emitter::basic_sprite("Particles", 3.0));
        let effect = aestra_compiler::EffectCompiler::default()
            .compile(&asset)
            .unwrap();
        let artifact = GpuEffectArtifact::from_instance(&aestra_runtime::EffectInstance::new(
            std::sync::Arc::new(effect),
        ))
        .unwrap();
        matches_legacy(artifact.emitters);
        matches_legacy(artifact.renderers);
        matches_legacy(artifact.particles);
        matches_legacy(vec![0_u32; 7]);
        matches_legacy(GpuGlobals::default());
        matches_legacy(GpuRenderGlobals::default());
        matches_legacy(GpuRenderParams::default());
        let indirect = indirect(vec![4_u32, 7, 0, 0]);
        assert!(
            indirect
                .buffer_description
                .usage
                .contains(BufferUsages::INDIRECT)
        );
        assert_eq!(
            bytes(&indirect),
            Some(storage_encoding::encode(&vec![4_u32, 7, 0, 0]).as_slice())
        );
        assert!(bytes(&ShaderBuffer::default()).is_none());
    }
}
