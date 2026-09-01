//! Bevy/WGPU translation and runtime values for the portable semantic-material ABI.

use aestra_core::{
    AssetId, EmitterId, MaterialId, MaterialParameterId, MaterialProgramId, ParameterId,
    material::{
        LEGACY_SPRITE_SOFTNESS_PARAMETER, MaterialAddressMode, MaterialEvaluationDomain,
        MaterialFilterMode, MaterialInstance, MaterialMipFilterMode, MaterialParameterValue,
        MaterialRenderState, MaterialSamplerDescriptor, MaterialValue, MaterialValueType,
    },
};
use aestra_gpu::material::{
    CompiledMaterialProgram, MaterialIrConstant, MaterialParameterBinding, MaterialResourceLayout,
};
use aestra_runtime::{EffectInstance, RuntimeValue};
use bevy::render::render_resource::{
    AddressMode, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, BufferBindingType,
    FilterMode, MipmapFilterMode, SamplerBindingType, SamplerDescriptor, ShaderStages,
    TextureSampleType, TextureViewDimension,
};
use std::{collections::BTreeMap, error::Error, fmt, num::NonZeroU64, sync::Arc};

/// Typed values and deterministic seeds available while resolving one material instance.
///
/// Effect and emitter values are deliberately separate even though both currently reference
/// Aestra [`ParameterId`] values. This preserves evaluation scope and prevents an emitter-rate
/// binding from silently reading an effect-rate value. The context is reusable across every
/// material bound to the same effect/emitter instance.
#[derive(Debug, Clone, Default)]
pub struct MaterialBindingContext {
    effect_parameters: BTreeMap<ParameterId, MaterialValue>,
    emitter_parameters: BTreeMap<ParameterId, MaterialValue>,
    instance_seed: u64,
    effect_seed: u64,
    emitter_seed: Option<u64>,
}

impl MaterialBindingContext {
    pub fn new(instance_seed: u64, effect_seed: u64) -> Self {
        Self {
            instance_seed,
            effect_seed,
            ..Self::default()
        }
    }

    /// Captures the current packed values of one effect instance for effect-rate resolution.
    pub fn from_effect_instance(instance: &EffectInstance) -> Self {
        let mut context = Self::new(instance.seed(), instance.seed());
        for (parameter, value) in instance
            .effect()
            .parameters
            .iter()
            .zip(instance.parameter_values())
        {
            if let Some(value) = runtime_material_value(value) {
                context.set_effect_parameter(parameter.source, value);
            }
        }
        context
    }

    /// Captures an emitter-scoped view of the current effect parameters and derives its seed.
    ///
    /// Aestra's current runtime parameter table is effect-owned. The explicit emitter projection
    /// preserves scope today and leaves room for independently stored emitter parameters later.
    pub fn for_emitter(instance: &EffectInstance, emitter: EmitterId) -> Self {
        let mut context = Self::from_effect_instance(instance);
        context.emitter_parameters = context.effect_parameters.clone();
        let id = emitter.as_uuid().as_u128();
        context.emitter_seed = Some(mix_material_seed(
            instance.seed() ^ id as u64 ^ (id >> 64) as u64,
        ));
        context
    }

    pub fn set_effect_parameter(&mut self, parameter: ParameterId, value: MaterialValue) {
        self.effect_parameters.insert(parameter, value);
    }

    pub fn set_effect_runtime_parameter(
        &mut self,
        parameter: ParameterId,
        value: &RuntimeValue,
    ) -> bool {
        let Some(value) = runtime_material_value(value) else {
            return false;
        };
        self.set_effect_parameter(parameter, value);
        true
    }

    pub fn set_emitter_parameter(&mut self, parameter: ParameterId, value: MaterialValue) {
        self.emitter_parameters.insert(parameter, value);
    }

    pub fn set_emitter_runtime_parameter(
        &mut self,
        parameter: ParameterId,
        value: &RuntimeValue,
    ) -> bool {
        let Some(value) = runtime_material_value(value) else {
            return false;
        };
        self.set_emitter_parameter(parameter, value);
        true
    }

    pub fn set_emitter_seed(&mut self, seed: u64) {
        self.emitter_seed = Some(seed);
    }

    pub fn clear_emitter_seed(&mut self) {
        self.emitter_seed = None;
    }

    fn parameter(
        &self,
        source: MaterialParameterSource,
        parameter: ParameterId,
    ) -> Option<&MaterialValue> {
        match source {
            MaterialParameterSource::Effect => self.effect_parameters.get(&parameter),
            MaterialParameterSource::Emitter => self.emitter_parameters.get(&parameter),
        }
    }

    fn seed(&self, domain: MaterialEvaluationDomain) -> Option<u64> {
        match domain {
            MaterialEvaluationDomain::Instance => Some(self.instance_seed),
            MaterialEvaluationDomain::Effect => Some(self.effect_seed),
            MaterialEvaluationDomain::Emitter => self.emitter_seed,
            MaterialEvaluationDomain::ShaderStatic => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialParameterSource {
    Effect,
    Emitter,
}

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
    source_instance: Option<MaterialId>,
    dynamic_sources: BTreeMap<MaterialParameterId, MaterialParameterValue>,
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
            source_instance: None,
            dynamic_sources: BTreeMap::new(),
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
        binding.source_instance = Some(instance.id);
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

    /// Builds and immediately resolves a material instance with dynamic parameter sources.
    pub fn from_instance_with_context(
        program: Arc<CompiledMaterialProgram>,
        instance: &MaterialInstance,
        context: &MaterialBindingContext,
    ) -> Result<Self, MaterialBindingError> {
        if program.source != instance.program.id() {
            return Err(MaterialBindingError::ProgramMismatch {
                expected: program.source,
                actual: instance.program.id(),
            });
        }
        let mut binding = Self::new(program, instance.render_state)?;
        binding.source_instance = Some(instance.id);
        for (&parameter, source) in &instance.values {
            match source {
                MaterialParameterValue::Constant(value) => {
                    binding.set_value(parameter, value.clone())?;
                }
                dynamic => {
                    binding.dynamic_sources.insert(parameter, dynamic.clone());
                }
            }
        }
        binding.refresh_dynamic_values(context)?;
        Ok(binding)
    }

    /// Re-evaluates effect/emitter/random sources without replacing the compiled program.
    ///
    /// Applications call this after automation or parameter state changes. The shader and pipeline
    /// stay shared; only the packed values/resources produced by [`Self::prepare`] change.
    pub fn refresh_dynamic_values(
        &mut self,
        context: &MaterialBindingContext,
    ) -> Result<(), MaterialBindingError> {
        let instance = self
            .source_instance
            .ok_or(MaterialBindingError::MissingSourceInstance)?;
        let resolved = self
            .dynamic_sources
            .iter()
            .map(|(&parameter, source)| {
                let expected = self
                    .program
                    .reflection
                    .parameters
                    .iter()
                    .find(|candidate| candidate.id == parameter)
                    .ok_or(MaterialBindingError::UnknownParameter(parameter))?
                    .value_type;
                resolve_dynamic_source(instance, parameter, source, expected, context)
                    .map(|value| (parameter, value))
            })
            .collect::<Result<Vec<_>, _>>()?;
        for (parameter, value) in &resolved {
            self.validate_value(*parameter, value)?;
        }
        for (parameter, value) in resolved {
            self.values.insert(parameter, value);
        }
        Ok(())
    }

    pub fn program(&self) -> &Arc<CompiledMaterialProgram> {
        &self.program
    }

    pub const fn render_state(&self) -> MaterialRenderState {
        self.render_state
    }

    pub(crate) fn uses_sampled_textures(&self) -> bool {
        !self.program.resource_layout.textures.is_empty()
    }

    pub(crate) fn legacy_sprite_softness(&self) -> Option<f32> {
        let parameter = self
            .program
            .reflection
            .parameters
            .iter()
            .find(|parameter| parameter.name == LEGACY_SPRITE_SOFTNESS_PARAMETER)?;
        match self.values.get(&parameter.id) {
            Some(MaterialValue::Float(value)) => Some(*value),
            Some(_) => None,
            None => match parameter.default.as_ref() {
                Some(MaterialIrConstant::Float(value)) => Some(*value),
                _ => None,
            },
        }
    }

    pub fn values(&self) -> &BTreeMap<MaterialParameterId, MaterialValue> {
        &self.values
    }

    pub fn set_value(
        &mut self,
        parameter: MaterialParameterId,
        value: MaterialValue,
    ) -> Result<(), MaterialBindingError> {
        self.validate_value(parameter, &value)?;
        self.values.insert(parameter, value);
        Ok(())
    }

    fn validate_value(
        &self,
        parameter: MaterialParameterId,
        value: &MaterialValue,
    ) -> Result<(), MaterialBindingError> {
        let reflection = self
            .program
            .reflection
            .parameters
            .iter()
            .find(|candidate| candidate.id == parameter)
            .ok_or(MaterialBindingError::UnknownParameter(parameter))?;
        if !reflection.value_type.accepts(value) {
            return Err(MaterialBindingError::TypeMismatch {
                parameter,
                expected: reflection.value_type,
            });
        }
        if reflection.binding == MaterialParameterBinding::ShaderStatic {
            return Err(MaterialBindingError::ShaderStaticOverride(parameter));
        }
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
    MissingSourceInstance,
    MissingParameter {
        source: MaterialParameterSource,
        parameter: ParameterId,
    },
    MissingEvaluationContext(MaterialEvaluationDomain),
    UnsupportedRandomRange(MaterialParameterId),
    UnknownMaterial(MaterialId),
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
                "material parameter {parameter} requires an explicit runtime binding context"
            ),
            Self::MissingSourceInstance => {
                write!(formatter, "dynamic material binding has no source instance")
            }
            Self::MissingParameter { source, parameter } => write!(
                formatter,
                "material {source:?} parameter {parameter} has no runtime value"
            ),
            Self::MissingEvaluationContext(domain) => write!(
                formatter,
                "material random source requires a {domain:?} evaluation seed"
            ),
            Self::UnsupportedRandomRange(parameter) => write!(
                formatter,
                "material parameter {parameter} uses a random range with a non-numeric value type"
            ),
            Self::UnknownMaterial(material) => {
                write!(
                    formatter,
                    "presented effect has no material binding {material}"
                )
            }
        }
    }
}

impl Error for MaterialBindingError {}

fn resolve_dynamic_source(
    instance: MaterialId,
    parameter: MaterialParameterId,
    source: &MaterialParameterValue,
    expected: MaterialValueType,
    context: &MaterialBindingContext,
) -> Result<MaterialValue, MaterialBindingError> {
    match source {
        MaterialParameterValue::Constant(value) => Ok(value.clone()),
        MaterialParameterValue::EffectParameter(binding) => context
            .parameter(MaterialParameterSource::Effect, *binding)
            .map(|value| material_value_for_type(value, expected))
            .ok_or(MaterialBindingError::MissingParameter {
                source: MaterialParameterSource::Effect,
                parameter: *binding,
            }),
        MaterialParameterValue::EmitterParameter(binding) => context
            .parameter(MaterialParameterSource::Emitter, *binding)
            .map(|value| material_value_for_type(value, expected))
            .ok_or(MaterialBindingError::MissingParameter {
                source: MaterialParameterSource::Emitter,
                parameter: *binding,
            }),
        MaterialParameterValue::RandomRange { min, max, domain } => {
            let base_seed = context
                .seed(*domain)
                .ok_or(MaterialBindingError::MissingEvaluationContext(*domain))?;
            sample_material_range(instance, parameter, min, max, base_seed)
        }
    }
}

fn material_value_for_type(value: &MaterialValue, expected: MaterialValueType) -> MaterialValue {
    match (value, expected) {
        (MaterialValue::Vec4(value), MaterialValueType::Color) => MaterialValue::ColorSrgb(*value),
        (MaterialValue::ColorSrgb(value), MaterialValueType::Vec4) => MaterialValue::Vec4(*value),
        _ => value.clone(),
    }
}

fn runtime_material_value(value: &RuntimeValue) -> Option<MaterialValue> {
    match value {
        RuntimeValue::Bool(value) => Some(MaterialValue::Bool(*value)),
        RuntimeValue::Scalar(value) => Some(MaterialValue::Float(*value)),
        RuntimeValue::Vec2(value) => Some(MaterialValue::Vec2(*value)),
        RuntimeValue::Vec3(value) => Some(MaterialValue::Vec3(*value)),
        RuntimeValue::Vec4(value) => Some(MaterialValue::Vec4(*value)),
        RuntimeValue::Asset(value) => Some(MaterialValue::Texture2D(*value)),
        RuntimeValue::U32(_)
        | RuntimeValue::Vec3Range(_)
        | RuntimeValue::Vec3Curve(_)
        | RuntimeValue::Text(_)
        | RuntimeValue::Range(_)
        | RuntimeValue::Curve(_)
        | RuntimeValue::Gradient(_)
        | RuntimeValue::Shape(_)
        | RuntimeValue::Material(_) => None,
    }
}

fn sample_material_range(
    instance: MaterialId,
    parameter: MaterialParameterId,
    min: &MaterialValue,
    max: &MaterialValue,
    seed: u64,
) -> Result<MaterialValue, MaterialBindingError> {
    let sample = |channel| material_random01(seed, instance, parameter, channel);
    let lerp = |min: f32, max: f32, channel| min + (max - min) * sample(channel);
    match (min, max) {
        (MaterialValue::Float(min), MaterialValue::Float(max)) => {
            Ok(MaterialValue::Float(lerp(*min, *max, 0)))
        }
        (MaterialValue::Vec2(min), MaterialValue::Vec2(max)) => {
            Ok(MaterialValue::Vec2(std::array::from_fn(|channel| {
                lerp(min[channel], max[channel], channel as u64)
            })))
        }
        (MaterialValue::Vec3(min), MaterialValue::Vec3(max)) => {
            Ok(MaterialValue::Vec3(std::array::from_fn(|channel| {
                lerp(min[channel], max[channel], channel as u64)
            })))
        }
        (MaterialValue::Vec4(min), MaterialValue::Vec4(max)) => {
            Ok(MaterialValue::Vec4(std::array::from_fn(|channel| {
                lerp(min[channel], max[channel], channel as u64)
            })))
        }
        (MaterialValue::ColorSrgb(min), MaterialValue::ColorSrgb(max)) => {
            Ok(MaterialValue::ColorSrgb(std::array::from_fn(|channel| {
                lerp(min[channel], max[channel], channel as u64)
            })))
        }
        _ => Err(MaterialBindingError::UnsupportedRandomRange(parameter)),
    }
}

fn material_random01(
    seed: u64,
    instance: MaterialId,
    parameter: MaterialParameterId,
    channel: u64,
) -> f32 {
    let instance = instance.as_uuid().as_u128();
    let parameter = parameter.as_uuid().as_u128();
    let mixed = mix_material_seed(
        seed ^ instance as u64
            ^ (instance >> 64) as u64
            ^ parameter as u64
            ^ (parameter >> 64) as u64
            ^ channel.wrapping_mul(0x9e37_79b9_7f4a_7c15),
    );
    ((mixed >> 40) as f32) / ((1_u64 << 24) - 1) as f32
}

fn mix_material_seed(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

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
        MaterialExpressionId, MaterialId, MaterialParameterId, MaterialProgramId, ParameterId,
        material::{
            MaterialAddressMode, MaterialEvaluationDomain, MaterialExpression,
            MaterialExpressionKind, MaterialFilterMode, MaterialInstance, MaterialMipFilterMode,
            MaterialParameter, MaterialParameterValue, MaterialProgram, MaterialProgramRef,
            MaterialSamplerDescriptor, MaterialTextureColorSpace, MaterialTextureDescriptor,
            MaterialValue,
        },
    };
    use aestra_gpu::material::{
        MaterialMissingResourceFallback, MaterialSamplerSlot, MaterialTextureSlot,
        MaterialUniformLayout, MaterialUniformSlot,
    };
    use bevy::render::render_resource::BindingType;
    use std::{collections::BTreeMap, sync::Arc};

    fn scalar_material_fixture(
        program_id: u128,
        parameter_id: u128,
        domain: MaterialEvaluationDomain,
        source: MaterialParameterValue,
        instance_id: u128,
    ) -> (
        Arc<CompiledMaterialProgram>,
        MaterialInstance,
        MaterialParameterId,
    ) {
        use aestra_compiler::MaterialCompiler;
        use aestra_gpu::material::{MaterialBackendCapabilities, MaterialShaderCompiler};

        let parameter = MaterialParameterId::from_u128(parameter_id);
        let expression = MaterialExpressionId::from_u128(parameter_id + 1);
        let mut program = MaterialProgram::additive_sprite("Dynamic scalar");
        program.id = MaterialProgramId::from_u128(program_id);
        program.parameters.push(MaterialParameter {
            id: parameter,
            name: "dynamic".into(),
            value_type: MaterialValueType::Float,
            evaluation_domain: domain,
            default: Some(MaterialValue::Float(1.0)),
        });
        program.expressions.push(MaterialExpression {
            id: expression,
            kind: MaterialExpressionKind::Parameter(parameter),
        });
        program.outputs.alpha = expression;
        let ir = MaterialCompiler.compile(&program).unwrap();
        let compiled = Arc::new(
            MaterialShaderCompiler
                .compile(&ir, &MaterialBackendCapabilities::portable_minimum())
                .unwrap(),
        );
        let instance = MaterialInstance {
            id: MaterialId::from_u128(instance_id),
            program: MaterialProgramRef::Project(program.id),
            values: BTreeMap::from([(parameter, source)]),
            render_state: MaterialRenderState::additive_sprite(),
        };
        (compiled, instance, parameter)
    }

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
        assert!(binding.uses_sampled_textures());
    }

    #[test]
    fn legacy_softness_reflection_drives_the_coverage_compatibility_value() {
        use aestra_compiler::MaterialCompiler;
        use aestra_core::MaterialExpressionId;
        use aestra_core::material::{
            LEGACY_SPRITE_SOFTNESS_PARAMETER, MaterialEvaluationDomain, MaterialExpression,
            MaterialExpressionKind, MaterialParameter, MaterialProgram, MaterialValue,
        };
        use aestra_gpu::material::{MaterialBackendCapabilities, MaterialShaderCompiler};

        let softness = MaterialParameterId::from_u128(0x501);
        let mut program = MaterialProgram::additive_sprite("Migrated softness");
        program.parameters.push(MaterialParameter {
            id: softness,
            name: LEGACY_SPRITE_SOFTNESS_PARAMETER.into(),
            value_type: MaterialValueType::Float,
            evaluation_domain: MaterialEvaluationDomain::Instance,
            default: Some(MaterialValue::Float(1.0)),
        });
        program.expressions.push(MaterialExpression {
            id: MaterialExpressionId::from_u128(0x502),
            kind: MaterialExpressionKind::Parameter(softness),
        });
        let ir = MaterialCompiler.compile(&program).unwrap();
        let compiled = Arc::new(
            MaterialShaderCompiler
                .compile(&ir, &MaterialBackendCapabilities::portable_minimum())
                .unwrap(),
        );
        let mut binding =
            MaterialRuntimeBinding::new(compiled, MaterialRenderState::additive_sprite()).unwrap();
        assert_eq!(binding.legacy_sprite_softness(), Some(1.0));
        binding
            .set_value(softness, MaterialValue::Float(0.18))
            .unwrap();
        assert_eq!(binding.legacy_sprite_softness(), Some(0.18));
    }

    #[test]
    fn effect_parameter_binding_refreshes_without_recompiling_the_program() {
        let source = ParameterId::from_u128(0x610);
        let (program, instance, parameter) = scalar_material_fixture(
            0x611,
            0x612,
            MaterialEvaluationDomain::Effect,
            MaterialParameterValue::EffectParameter(source),
            0x613,
        );
        let shared_program = Arc::clone(&program);
        let mut context = MaterialBindingContext::new(7, 11);
        context.set_effect_parameter(source, MaterialValue::Float(2.5));
        context.set_emitter_parameter(source, MaterialValue::Float(99.0));

        let mut binding =
            MaterialRuntimeBinding::from_instance_with_context(program, &instance, &context)
                .unwrap();
        assert_eq!(
            binding.values().get(&parameter),
            Some(&MaterialValue::Float(2.5))
        );
        assert!(Arc::ptr_eq(binding.program(), &shared_program));

        context.set_effect_parameter(source, MaterialValue::Float(4.25));
        binding.refresh_dynamic_values(&context).unwrap();
        assert_eq!(
            binding.values().get(&parameter),
            Some(&MaterialValue::Float(4.25))
        );
        assert!(Arc::ptr_eq(binding.program(), &shared_program));
    }

    #[test]
    fn emitter_parameter_does_not_fall_back_to_effect_scope() {
        let source = ParameterId::from_u128(0x620);
        let (program, instance, parameter) = scalar_material_fixture(
            0x621,
            0x622,
            MaterialEvaluationDomain::Emitter,
            MaterialParameterValue::EmitterParameter(source),
            0x623,
        );
        let mut context = MaterialBindingContext::new(1, 2);
        context.set_effect_parameter(source, MaterialValue::Float(5.0));

        assert_eq!(
            MaterialRuntimeBinding::from_instance_with_context(
                Arc::clone(&program),
                &instance,
                &context,
            )
            .unwrap_err(),
            MaterialBindingError::MissingParameter {
                source: MaterialParameterSource::Emitter,
                parameter: source,
            }
        );

        context.set_emitter_parameter(source, MaterialValue::Float(8.0));
        let binding =
            MaterialRuntimeBinding::from_instance_with_context(program, &instance, &context)
                .unwrap();
        assert_eq!(
            binding.values().get(&parameter),
            Some(&MaterialValue::Float(8.0))
        );
    }

    #[test]
    fn random_ranges_are_deterministic_and_keyed_by_emitter_context() {
        let (program, instance, parameter) = scalar_material_fixture(
            0x631,
            0x632,
            MaterialEvaluationDomain::Emitter,
            MaterialParameterValue::RandomRange {
                min: MaterialValue::Float(10.0),
                max: MaterialValue::Float(20.0),
                domain: MaterialEvaluationDomain::Emitter,
            },
            0x633,
        );
        let context_without_emitter = MaterialBindingContext::new(3, 5);
        assert_eq!(
            MaterialRuntimeBinding::from_instance_with_context(
                Arc::clone(&program),
                &instance,
                &context_without_emitter,
            )
            .unwrap_err(),
            MaterialBindingError::MissingEvaluationContext(MaterialEvaluationDomain::Emitter)
        );

        let mut first_context = MaterialBindingContext::new(3, 5);
        first_context.set_emitter_seed(7);
        let first = MaterialRuntimeBinding::from_instance_with_context(
            Arc::clone(&program),
            &instance,
            &first_context,
        )
        .unwrap();
        let repeated = MaterialRuntimeBinding::from_instance_with_context(
            Arc::clone(&program),
            &instance,
            &first_context,
        )
        .unwrap();
        assert_eq!(
            first.values().get(&parameter),
            repeated.values().get(&parameter)
        );
        let MaterialValue::Float(first_value) = first.values()[&parameter] else {
            panic!("expected scalar random value");
        };
        assert!((10.0..=20.0).contains(&first_value));

        let mut second_context = MaterialBindingContext::new(3, 5);
        second_context.set_emitter_seed(8);
        let second =
            MaterialRuntimeBinding::from_instance_with_context(program, &instance, &second_context)
                .unwrap();
        assert_ne!(
            first.values().get(&parameter),
            second.values().get(&parameter)
        );
    }

    #[test]
    fn dynamic_parameter_values_still_use_reflected_type_validation() {
        let source = ParameterId::from_u128(0x640);
        let (program, instance, _) = scalar_material_fixture(
            0x641,
            0x642,
            MaterialEvaluationDomain::Effect,
            MaterialParameterValue::EffectParameter(source),
            0x643,
        );
        let mut context = MaterialBindingContext::new(1, 1);
        context.set_effect_parameter(source, MaterialValue::Vec2([1.0, 2.0]));

        assert!(matches!(
            MaterialRuntimeBinding::from_instance_with_context(program, &instance, &context),
            Err(MaterialBindingError::TypeMismatch {
                expected: MaterialValueType::Float,
                ..
            })
        ));
    }
}
