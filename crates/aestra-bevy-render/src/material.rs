//! Bevy/WGPU translation and runtime values for the portable semantic-material ABI.

use aestra_core::{
    AssetId, MaterialParameterId, MaterialProgramId,
    material::{
        MaterialAddressMode, MaterialFilterMode, MaterialInstance, MaterialMipFilterMode,
        MaterialParameterValue, MaterialRenderState, MaterialSamplerDescriptor, MaterialValue,
        MaterialValueType,
    },
};
use aestra_gpu::material::{
    CompiledMaterialProgram, MaterialIrConstant, MaterialParameterBinding, MaterialResourceLayout,
};
use bevy::render::render_resource::{
    AddressMode, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, BufferBindingType,
    FilterMode, MipmapFilterMode, SamplerBindingType, SamplerDescriptor, ShaderStages,
    TextureSampleType, TextureViewDimension,
};
use std::{collections::BTreeMap, error::Error, fmt, num::NonZeroU64, sync::Arc};

/// Runtime values for one effect-local instance of a compiled semantic material program.
///
/// Shader code and resource layout are shared through [`CompiledMaterialProgram`]. Ordinary
/// instance edits only replace packed values or texture IDs and therefore do not invalidate the
/// shader or pipeline cache.
#[derive(Debug, Clone)]
pub struct MaterialRuntimeBinding {
    program: Arc<CompiledMaterialProgram>,
    render_state: MaterialRenderState,
    values: BTreeMap<MaterialParameterId, MaterialValue>,
}

impl MaterialRuntimeBinding {
    pub fn new(
        program: Arc<CompiledMaterialProgram>,
        render_state: MaterialRenderState,
    ) -> Result<Self, MaterialBindingError> {
        if !program.render_state_policy.allows(render_state) {
            return Err(MaterialBindingError::RenderStateNotAllowed(render_state));
        }
        Ok(Self {
            program,
            render_state,
            values: BTreeMap::new(),
        })
    }

    /// Builds runtime values from constant instance overrides.
    ///
    /// Effect/emitter/random bindings are intentionally deferred to Material 6, where their
    /// evaluation domains become part of the reflected runtime contract.
    pub fn from_instance(
        program: Arc<CompiledMaterialProgram>,
        instance: &MaterialInstance,
    ) -> Result<Self, MaterialBindingError> {
        if program.source != instance.program.id() {
            return Err(MaterialBindingError::ProgramMismatch {
                expected: program.source,
                actual: instance.program.id(),
            });
        }
        let mut binding = Self::new(program, instance.render_state)?;
        for (&parameter, value) in &instance.values {
            match value {
                MaterialParameterValue::Constant(value) => {
                    binding.set_value(parameter, value.clone())?;
                }
                MaterialParameterValue::EffectParameter(_)
                | MaterialParameterValue::EmitterParameter(_)
                | MaterialParameterValue::RandomRange { .. } => {
                    return Err(MaterialBindingError::UnsupportedDynamicSource(parameter));
                }
            }
        }
        Ok(binding)
    }

    pub fn program(&self) -> &Arc<CompiledMaterialProgram> {
        &self.program
    }

    pub const fn render_state(&self) -> MaterialRenderState {
        self.render_state
    }

    pub fn values(&self) -> &BTreeMap<MaterialParameterId, MaterialValue> {
        &self.values
    }

    pub fn set_value(
        &mut self,
        parameter: MaterialParameterId,
        value: MaterialValue,
    ) -> Result<(), MaterialBindingError> {
        let reflection = self
            .program
            .reflection
            .parameters
            .iter()
            .find(|candidate| candidate.id == parameter)
            .ok_or(MaterialBindingError::UnknownParameter(parameter))?;
        if !reflection.value_type.accepts(&value) {
            return Err(MaterialBindingError::TypeMismatch {
                parameter,
                expected: reflection.value_type,
            });
        }
        if reflection.binding == MaterialParameterBinding::ShaderStatic {
            return Err(MaterialBindingError::ShaderStaticOverride(parameter));
        }
        self.values.insert(parameter, value);
        Ok(())
    }

    pub(crate) fn prepare(&self) -> Result<PreparedMaterialBinding, MaterialBindingError> {
        let layout = &self.program.resource_layout;
        let mut uniforms = vec![0_u8; layout.uniforms.size as usize];
        for slot in &layout.uniforms.slots {
            let reflection = self
                .program
                .reflection
                .parameters
                .iter()
                .find(|candidate| candidate.id == slot.parameter)
                .ok_or(MaterialBindingError::UnknownParameter(slot.parameter))?;
            let destination =
                &mut uniforms[slot.offset as usize..(slot.offset + slot.size) as usize];
            if let Some(value) = self.values.get(&slot.parameter) {
                encode_material_value(destination, value);
            } else if let Some(default) = reflection.default.as_ref() {
                encode_ir_constant(destination, default);
            } else {
                return Err(MaterialBindingError::MissingValue(slot.parameter));
            }
        }
        let mut textures = Vec::with_capacity(layout.textures.len());
        for slot in &layout.textures {
            let reflection = self
                .program
                .reflection
                .parameters
                .iter()
                .find(|candidate| candidate.id == slot.parameter)
                .ok_or(MaterialBindingError::UnknownParameter(slot.parameter))?;
            let asset = match self.values.get(&slot.parameter) {
                Some(MaterialValue::Texture2D(asset)) => *asset,
                Some(_) => {
                    return Err(MaterialBindingError::TypeMismatch {
                        parameter: slot.parameter,
                        expected: reflection.value_type,
                    });
                }
                None => match reflection.default.as_ref() {
                    Some(MaterialIrConstant::Texture2D(asset)) => *asset,
                    _ => return Err(MaterialBindingError::MissingValue(slot.parameter)),
                },
            };
            textures.push((slot.parameter, asset));
        }
        Ok(PreparedMaterialBinding { uniforms, textures })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaterialBindingError {
    ProgramMismatch {
        expected: MaterialProgramId,
        actual: MaterialProgramId,
    },
    RenderStateNotAllowed(MaterialRenderState),
    UnknownParameter(MaterialParameterId),
    MissingValue(MaterialParameterId),
    TypeMismatch {
        parameter: MaterialParameterId,
        expected: MaterialValueType,
    },
    ShaderStaticOverride(MaterialParameterId),
    UnsupportedDynamicSource(MaterialParameterId),
}

impl fmt::Display for MaterialBindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProgramMismatch { expected, actual } => write!(
                formatter,
                "material instance references program {actual}, but the compiled program is {expected}"
            ),
            Self::RenderStateNotAllowed(state) => {
                write!(formatter, "material render state {state:?} is not allowed")
            }
            Self::UnknownParameter(parameter) => {
                write!(formatter, "material parameter {parameter} is not reflected")
            }
            Self::MissingValue(parameter) => {
                write!(
                    formatter,
                    "material parameter {parameter} has no runtime value"
                )
            }
            Self::TypeMismatch {
                parameter,
                expected,
            } => write!(
                formatter,
                "material parameter {parameter} does not accept the supplied value; expected {expected:?}"
            ),
            Self::ShaderStaticOverride(parameter) => write!(
                formatter,
                "material parameter {parameter} is shader-static and requires recompilation"
            ),
            Self::UnsupportedDynamicSource(parameter) => write!(
                formatter,
                "material parameter {parameter} uses a dynamic source deferred to Material 6"
            ),
        }
    }
}

impl Error for MaterialBindingError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedMaterialBinding {
    pub uniforms: Vec<u8>,
    pub textures: Vec<(MaterialParameterId, AssetId)>,
}

fn encode_material_value(destination: &mut [u8], value: &MaterialValue) {
    match value {
        MaterialValue::Float(value) => write_f32(destination, 0, *value),
        MaterialValue::Vec2(value) => write_f32s(destination, value),
        MaterialValue::Vec3(value) => write_f32s(destination, value),
        MaterialValue::Vec4(value) => write_f32s(destination, value),
        MaterialValue::ColorSrgb(value) => {
            let linear = [
                srgb_to_linear(value[0]),
                srgb_to_linear(value[1]),
                srgb_to_linear(value[2]),
                value[3],
            ];
            write_f32s(destination, &linear);
        }
        MaterialValue::Bool(value) => {
            destination[..4].copy_from_slice(&u32::from(*value).to_le_bytes())
        }
        MaterialValue::Texture2D(_) => {}
    }
}

fn encode_ir_constant(destination: &mut [u8], value: &MaterialIrConstant) {
    match value {
        MaterialIrConstant::Float(value) => write_f32(destination, 0, *value),
        MaterialIrConstant::Vec2(value) => write_f32s(destination, value),
        MaterialIrConstant::Vec3(value) => write_f32s(destination, value),
        MaterialIrConstant::Vec4(value) | MaterialIrConstant::ColorLinear(value) => {
            write_f32s(destination, value);
        }
        MaterialIrConstant::Bool(value) => {
            destination[..4].copy_from_slice(&u32::from(*value).to_le_bytes())
        }
        MaterialIrConstant::Texture2D(_) => {}
    }
}

fn write_f32(destination: &mut [u8], index: usize, value: f32) {
    let offset = index * 4;
    destination[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_f32s(destination: &mut [u8], values: &[f32]) {
    for (index, value) in values.iter().enumerate() {
        write_f32(destination, index, *value);
    }
}

fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

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
            visibility: ShaderStages::FRAGMENT,
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
            visibility: ShaderStages::FRAGMENT,
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
            visibility: ShaderStages::FRAGMENT,
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
    use bevy::render::render_resource::BindingType;
    use std::sync::Arc;

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

    #[test]
    fn runtime_binding_packs_uniforms_and_resolves_texture_defaults() {
        use aestra_compiler::MaterialCompiler;
        use aestra_core::MaterialExpressionId;
        use aestra_core::material::{
            MaterialExpression, MaterialExpressionKind, MaterialInput, MaterialOutputs,
            MaterialParameter, MaterialProgram, MaterialValue,
        };
        use aestra_gpu::material::{MaterialBackendCapabilities, MaterialShaderCompiler};

        let mut program = MaterialProgram::additive_sprite("Runtime binding");
        let tint = MaterialParameterId::from_u128(11);
        let texture = MaterialParameterId::from_u128(12);
        let tint_expression = MaterialExpressionId::from_u128(21);
        let texture_expression = MaterialExpressionId::from_u128(22);
        let uv_expression = MaterialExpressionId::from_u128(23);
        let sample_expression = MaterialExpressionId::from_u128(24);
        let alpha_expression = MaterialExpressionId::from_u128(25);
        let texture_asset = AssetId::from_u128(31);
        let descriptor = MaterialTextureDescriptor {
            color_space: MaterialTextureColorSpace::SrgbColor,
            sampler: MaterialSamplerDescriptor::default(),
        };
        program.parameters = vec![
            MaterialParameter {
                id: tint,
                name: "Tint".into(),
                value_type: MaterialValueType::Color,
                evaluation_domain: aestra_core::material::MaterialEvaluationDomain::Instance,
                default: Some(MaterialValue::ColorSrgb([1.0, 1.0, 1.0, 1.0])),
            },
            MaterialParameter {
                id: texture,
                name: "Texture".into(),
                value_type: MaterialValueType::Texture2D(descriptor),
                evaluation_domain: aestra_core::material::MaterialEvaluationDomain::Instance,
                default: Some(MaterialValue::Texture2D(texture_asset)),
            },
        ];
        program.expressions = vec![
            MaterialExpression {
                id: tint_expression,
                kind: MaterialExpressionKind::Parameter(tint),
            },
            MaterialExpression {
                id: texture_expression,
                kind: MaterialExpressionKind::Parameter(texture),
            },
            MaterialExpression {
                id: uv_expression,
                kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
            },
            MaterialExpression {
                id: sample_expression,
                kind: MaterialExpressionKind::SampleTexture {
                    texture: texture_expression,
                    uv: uv_expression,
                },
            },
            MaterialExpression {
                id: alpha_expression,
                kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
            },
        ];
        program.outputs = MaterialOutputs {
            color: tint_expression,
            alpha: alpha_expression,
        };
        // Keep the sample live so the resource remains in the lowered ABI.
        program.expressions.push(MaterialExpression {
            id: MaterialExpressionId::from_u128(26),
            kind: MaterialExpressionKind::Multiply(tint_expression, sample_expression),
        });
        program.outputs.color = MaterialExpressionId::from_u128(26);

        let ir = MaterialCompiler.compile(&program).unwrap();
        let compiled = Arc::new(
            MaterialShaderCompiler
                .compile(&ir, &MaterialBackendCapabilities::portable_minimum())
                .unwrap(),
        );
        let mut binding =
            MaterialRuntimeBinding::new(compiled, MaterialRenderState::additive_sprite()).unwrap();
        binding
            .set_value(tint, MaterialValue::ColorSrgb([0.5, 0.25, 1.0, 0.75]))
            .unwrap();

        let prepared = binding.prepare().unwrap();
        assert_eq!(prepared.textures, vec![(texture, texture_asset)]);
        assert_eq!(prepared.uniforms.len(), 16);
        assert!(
            (f32::from_le_bytes(prepared.uniforms[0..4].try_into().unwrap()) - 0.214_041_14).abs()
                < 0.000_001
        );
        assert!(
            (f32::from_le_bytes(prepared.uniforms[12..16].try_into().unwrap()) - 0.75).abs()
                < f32::EPSILON
        );
    }
}
