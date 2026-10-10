//! Exact Bevy layout translation for the portable semantic material ABI.
use aestra_core::material::{
    MaterialAddressMode, MaterialFilterMode, MaterialMipFilterMode, MaterialSamplerDescriptor,
};
use aestra_gpu::material::MaterialResourceLayout;
use bevy::render::render_resource::{AddressMode, FilterMode, MipmapFilterMode, SamplerDescriptor};
use bevy::render::render_resource::{
    BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, BufferBindingType,
    SamplerBindingType, ShaderStages, TextureSampleType, TextureViewDimension,
};
use std::num::NonZeroU64;

pub(crate) fn bevy_sampler_descriptor(
    descriptor: MaterialSamplerDescriptor,
) -> SamplerDescriptor<'static> {
    SamplerDescriptor {
        label: Some("aestra semantic material sampler"),
        address_mode_u: address_mode(descriptor.address_u),
        address_mode_v: address_mode(descriptor.address_v),
        address_mode_w: AddressMode::ClampToEdge,
        mag_filter: filter_mode(descriptor.filter),
        min_filter: filter_mode(descriptor.filter),
        mipmap_filter: mip_filter_mode(descriptor.mip_filter),
        ..Default::default()
    }
}

const fn address_mode(mode: MaterialAddressMode) -> AddressMode {
    match mode {
        MaterialAddressMode::ClampToEdge => AddressMode::ClampToEdge,
        MaterialAddressMode::Repeat => AddressMode::Repeat,
        MaterialAddressMode::MirrorRepeat => AddressMode::MirrorRepeat,
    }
}

const fn filter_mode(mode: MaterialFilterMode) -> FilterMode {
    match mode {
        MaterialFilterMode::Nearest => FilterMode::Nearest,
        MaterialFilterMode::Linear => FilterMode::Linear,
    }
}

const fn mip_filter_mode(mode: MaterialMipFilterMode) -> MipmapFilterMode {
    match mode {
        MaterialMipFilterMode::None | MaterialMipFilterMode::Nearest => MipmapFilterMode::Nearest,
        MaterialMipFilterMode::Linear => MipmapFilterMode::Linear,
    }
}

/// Creates the exact Bevy/WGPU bind-group layout described by portable compiler output.
pub fn material_bind_group_layout(layout: &MaterialResourceLayout) -> BindGroupLayoutDescriptor {
    let mut entries = Vec::with_capacity(layout.binding_count() as usize);
    if let Some(binding) = layout.uniforms.binding {
        entries.push(BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::VERTEX_FRAGMENT,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: NonZeroU64::new(u64::from(layout.uniforms.size)),
            },
            count: None,
        });
    }
    for texture in &layout.textures {
        let filterable = layout
            .samplers
            .iter()
            .find(|sampler| sampler.binding == texture.sampler_binding)
            .is_some_and(|sampler| sampler.is_filtering());
        entries.push(BindGroupLayoutEntry {
            binding: texture.binding,
            visibility: ShaderStages::VERTEX_FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable },
                view_dimension: TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
    }
    for sampler in &layout.samplers {
        entries.push(BindGroupLayoutEntry {
            binding: sampler.binding,
            visibility: ShaderStages::VERTEX_FRAGMENT,
            ty: BindingType::Sampler(if sampler.is_filtering() {
                SamplerBindingType::Filtering
            } else {
                SamplerBindingType::NonFiltering
            }),
            count: None,
        });
    }
    entries.sort_by_key(|entry| entry.binding);
    BindGroupLayoutDescriptor {
        label: "aestra semantic material".into(),
        entries,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_core::{
        MaterialParameterId,
        material::{
            MaterialAddressMode, MaterialFilterMode, MaterialMipFilterMode,
            MaterialSamplerDescriptor, MaterialTextureColorSpace, MaterialTextureDescriptor,
        },
    };
    use aestra_gpu::material::{
        MaterialMissingResourceFallback, MaterialSamplerSlot, MaterialTextureSlot,
        MaterialUniformLayout, MaterialUniformSlot,
    };

    #[test]
    fn portable_layout_translates_without_reassigning_bindings() {
        let sampler = MaterialSamplerDescriptor {
            filter: MaterialFilterMode::Linear,
            mip_filter: MaterialMipFilterMode::Linear,
            address_u: MaterialAddressMode::Repeat,
            address_v: MaterialAddressMode::Repeat,
        };
        let layout = MaterialResourceLayout {
            group: 2,
            uniforms: MaterialUniformLayout {
                binding: Some(0),
                size: 16,
                slots: vec![MaterialUniformSlot {
                    parameter: MaterialParameterId::from_u128(1),
                    value_type: aestra_core::material::MaterialValueType::Float,
                    offset: 0,
                    size: 16,
                }],
            },
            textures: vec![MaterialTextureSlot {
                parameter: MaterialParameterId::from_u128(2),
                descriptor: MaterialTextureDescriptor {
                    color_space: MaterialTextureColorSpace::SrgbColor,
                    sampler,
                },
                binding: 1,
                sampler_binding: 2,
                fallback: MaterialMissingResourceFallback::Magenta,
            }],
            samplers: vec![MaterialSamplerSlot {
                descriptor: sampler,
                binding: 2,
            }],
        };

        let descriptor = material_bind_group_layout(&layout);

        assert_eq!(
            descriptor
                .entries
                .iter()
                .map(|entry| entry.binding)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert!(matches!(
            descriptor.entries[0].ty,
            BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                ..
            }
        ));
        assert!(matches!(
            descriptor.entries[2].ty,
            BindingType::Sampler(SamplerBindingType::Filtering)
        ));
    }
}
