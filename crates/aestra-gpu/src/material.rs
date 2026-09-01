//! Portable material resource ABI, reflection, cache identity, and WESL lowering.

use crate::shader::{CompiledWesl, GpuShaderError, compile_wesl};
pub use aestra_compiler::MaterialIrConstant;
use aestra_compiler::{
    MaterialIrInstruction, MaterialIrProgram, MaterialIrSourceMap, MaterialIrValue,
    MaterialIrValueId,
};
use aestra_core::{
    MaterialExpressionId, MaterialParameterId, MaterialProgramId,
    material::{
        MaterialAddressMode, MaterialDomain, MaterialEvaluationDomain, MaterialFilterMode,
        MaterialInput, MaterialMipFilterMode, MaterialRenderState, MaterialRenderStatePolicy,
        MaterialSamplerDescriptor, MaterialTextureColorSpace, MaterialTextureDescriptor,
        MaterialValueType, MaterialVectorComponent,
    },
};
use std::{
    collections::BTreeMap,
    fmt,
    hash::{Hash, Hasher},
};
use thiserror::Error;

pub const MATERIAL_ABI_VERSION: u32 = 1;
pub const MATERIAL_SHADER_GENERATOR_VERSION: u32 = 1;
pub const MATERIAL_BIND_GROUP: u32 = 2;
pub const MATERIAL_FRAGMENT_ENTRY_POINT: &str = "fragment_material";
pub const MISSING_TEXTURE_FALLBACK_RGBA: [u8; 4] = [255, 0, 255, 255];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialMissingResourceFallback {
    Magenta,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialTextureSlot {
    pub parameter: MaterialParameterId,
    pub descriptor: MaterialTextureDescriptor,
    pub binding: u32,
    pub sampler_binding: u32,
    pub fallback: MaterialMissingResourceFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterialSamplerSlot {
    pub descriptor: MaterialSamplerDescriptor,
    pub binding: u32,
}

impl MaterialSamplerSlot {
    pub fn is_filtering(self) -> bool {
        self.descriptor.filter == MaterialFilterMode::Linear
            || self.descriptor.mip_filter == MaterialMipFilterMode::Linear
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterialUniformSlot {
    pub parameter: MaterialParameterId,
    pub value_type: MaterialValueType,
    pub offset: u32,
    pub size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialUniformLayout {
    pub binding: Option<u32>,
    pub size: u32,
    pub slots: Vec<MaterialUniformSlot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialResourceLayout {
    pub group: u32,
    pub textures: Vec<MaterialTextureSlot>,
    pub samplers: Vec<MaterialSamplerSlot>,
    pub uniforms: MaterialUniformLayout,
}

impl MaterialResourceLayout {
    pub fn binding_count(&self) -> u32 {
        u32::from(self.uniforms.binding.is_some())
            + self.textures.len() as u32
            + self.samplers.len() as u32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialParameterBinding {
    ShaderStatic,
    Uniform { binding: u32, offset: u32 },
    Texture { binding: u32, sampler_binding: u32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct MaterialParameterReflection {
    pub id: MaterialParameterId,
    pub name: String,
    pub value_type: MaterialValueType,
    pub evaluation_domain: MaterialEvaluationDomain,
    pub default: Option<MaterialIrConstant>,
    pub binding: MaterialParameterBinding,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MaterialReflection {
    pub parameters: Vec<MaterialParameterReflection>,
    pub required_vertex_inputs: Vec<MaterialInput>,
    pub required_particle_inputs: Vec<MaterialInput>,
    pub required_scene_inputs: Vec<MaterialInput>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MaterialShaderSourceMap {
    pub ir: MaterialIrSourceMap,
    /// One-based generated WESL line to the IR value authored on that line.
    pub wesl_lines: BTreeMap<u32, MaterialIrValueId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MaterialProgramFingerprint(pub [u8; 32]);

impl fmt::Display for MaterialProgramFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MaterialColorTargetFormat {
    Rgba8UnormSrgb,
    Bgra8UnormSrgb,
    Rgba16Float,
    Other(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MaterialPipelineVariant {
    pub target_format: MaterialColorTargetFormat,
    pub sample_count: u32,
    /// Portable feature bits selected by the concrete view/backend adapter.
    pub feature_bits: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterialPipelineKey {
    pub program: MaterialProgramFingerprint,
    pub render_state: MaterialRenderState,
    pub variant: MaterialPipelineVariant,
    digest: [u8; 32],
}

impl MaterialPipelineKey {
    pub fn digest(self) -> [u8; 32] {
        self.digest
    }
}

impl Hash for MaterialPipelineKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.digest.hash(state);
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MaterialBackendCapabilities {
    pub max_bind_groups: u32,
    pub max_bindings_per_bind_group: u32,
    pub max_sampled_textures_per_shader_stage: u32,
    pub max_samplers_per_shader_stage: u32,
    pub max_uniform_buffer_binding_size: u64,
}

impl MaterialBackendCapabilities {
    pub const fn portable_minimum() -> Self {
        Self {
            max_bind_groups: 4,
            max_bindings_per_bind_group: 16,
            max_sampled_textures_per_shader_stage: 8,
            max_samplers_per_shader_stage: 8,
            max_uniform_buffer_binding_size: 16_384,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaterialCapabilityIssueCode {
    BindGroupUnavailable,
    BindingLimitExceeded,
    TextureLimitExceeded,
    SamplerLimitExceeded,
    UniformLimitExceeded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialCapabilityIssue {
    pub code: MaterialCapabilityIssueCode,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MaterialCapabilityReport {
    pub issues: Vec<MaterialCapabilityIssue>,
}

impl MaterialCapabilityReport {
    pub fn is_compatible(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompiledMaterialProgram {
    pub source: MaterialProgramId,
    pub shader: CompiledWesl,
    pub source_map: MaterialShaderSourceMap,
    pub resource_layout: MaterialResourceLayout,
    pub reflection: MaterialReflection,
    pub render_state_policy: MaterialRenderStatePolicy,
    pub program_fingerprint: MaterialProgramFingerprint,
}

impl CompiledMaterialProgram {
    pub fn pipeline_key(
        &self,
        render_state: MaterialRenderState,
        variant: MaterialPipelineVariant,
    ) -> Result<MaterialPipelineKey, MaterialGpuError> {
        if !self.render_state_policy.allows(render_state) {
            return Err(MaterialGpuError::RenderStateNotAllowed(render_state));
        }
        let mut fingerprint = FingerprintBuilder::new(b"aestra.material.pipeline");
        fingerprint.bytes(&self.program_fingerprint.0);
        hash_render_state(&mut fingerprint, render_state);
        hash_target_format(&mut fingerprint, variant.target_format);
        fingerprint.u32(variant.sample_count);
        fingerprint.u64(variant.feature_bits);
        Ok(MaterialPipelineKey {
            program: self.program_fingerprint,
            render_state,
            variant,
            digest: fingerprint.finish(),
        })
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MaterialGpuError {
    #[error("material input {input:?} is not supported by the initial WESL backend")]
    UnsupportedInput {
        input: MaterialInput,
        expressions: Vec<MaterialExpressionId>,
    },
    #[error("material parameter {0} is missing from IR reflection")]
    MissingParameter(MaterialParameterId),
    #[error("shader-static material parameter {0} has no default")]
    MissingShaderStaticDefault(MaterialParameterId),
    #[error("texture sample value {0:?} does not reference a Texture2D parameter")]
    InvalidTextureSource(MaterialIrValueId),
    #[error("material resource layout is incompatible with the backend: {0:?}")]
    Capabilities(MaterialCapabilityReport),
    #[error("material render state {0:?} is not allowed by its program")]
    RenderStateNotAllowed(MaterialRenderState),
    #[error("material shader generation failed: {error}")]
    Shader {
        #[source]
        error: GpuShaderError,
        source_map: Box<MaterialShaderSourceMap>,
    },
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MaterialShaderCompiler;

impl MaterialShaderCompiler {
    pub fn compile(
        &self,
        ir: &MaterialIrProgram,
        capabilities: &MaterialBackendCapabilities,
    ) -> Result<CompiledMaterialProgram, MaterialGpuError> {
        let resource_layout = build_resource_layout(ir);
        let report = validate_capabilities(&resource_layout, capabilities);
        if !report.is_compatible() {
            return Err(MaterialGpuError::Capabilities(report));
        }
        let reflection = build_reflection(ir, &resource_layout)?;
        let program_fingerprint = fingerprint_program(ir, &resource_layout);
        let (wesl, wesl_lines) = generate_wesl(ir, &resource_layout)?;
        let source_map = MaterialShaderSourceMap {
            ir: ir.source_map.clone(),
            wesl_lines,
        };
        let module_name = format!(
            "package::aestra_material_{}",
            &program_fingerprint.to_string()[..16]
        );
        let shader = compile_wesl(&module_name, &wesl, &[MATERIAL_FRAGMENT_ENTRY_POINT]).map_err(
            |error| MaterialGpuError::Shader {
                error,
                source_map: Box::new(source_map.clone()),
            },
        )?;
        Ok(CompiledMaterialProgram {
            source: ir.source,
            shader,
            source_map,
            resource_layout,
            reflection,
            render_state_policy: ir.render_state_policy.clone(),
            program_fingerprint,
        })
    }
}

fn build_resource_layout(ir: &MaterialIrProgram) -> MaterialResourceLayout {
    let uniform_parameters = ir
        .parameters
        .iter()
        .filter(|parameter| {
            parameter.evaluation_domain != MaterialEvaluationDomain::ShaderStatic
                && !matches!(parameter.value_type, MaterialValueType::Texture2D(_))
        })
        .collect::<Vec<_>>();
    let texture_parameters = ir
        .parameters
        .iter()
        .filter_map(|parameter| match parameter.value_type {
            MaterialValueType::Texture2D(descriptor) => Some((parameter.source, descriptor)),
            _ => None,
        })
        .collect::<Vec<_>>();

    let uniform_binding = (!uniform_parameters.is_empty()).then_some(0);
    let mut next_binding = u32::from(uniform_binding.is_some());
    let texture_bindings = texture_parameters
        .iter()
        .map(|(parameter, _)| {
            let binding = next_binding;
            next_binding += 1;
            (*parameter, binding)
        })
        .collect::<BTreeMap<_, _>>();
    let sampler_descriptors = texture_parameters
        .iter()
        .map(|(_, descriptor)| (sampler_key(descriptor.sampler), descriptor.sampler))
        .collect::<BTreeMap<_, _>>();
    let samplers = sampler_descriptors
        .values()
        .map(|descriptor| {
            let binding = next_binding;
            next_binding += 1;
            MaterialSamplerSlot {
                descriptor: *descriptor,
                binding,
            }
        })
        .collect::<Vec<_>>();
    let sampler_bindings = samplers
        .iter()
        .map(|sampler| (sampler_key(sampler.descriptor), sampler.binding))
        .collect::<BTreeMap<_, _>>();
    let textures = texture_parameters
        .into_iter()
        .map(|(parameter, descriptor)| MaterialTextureSlot {
            parameter,
            descriptor,
            binding: texture_bindings[&parameter],
            sampler_binding: sampler_bindings[&sampler_key(descriptor.sampler)],
            fallback: MaterialMissingResourceFallback::Magenta,
        })
        .collect();
    let slots = uniform_parameters
        .into_iter()
        .enumerate()
        .map(|(index, parameter)| MaterialUniformSlot {
            parameter: parameter.source,
            value_type: parameter.value_type,
            offset: index as u32 * 16,
            size: 16,
        })
        .collect::<Vec<_>>();
    MaterialResourceLayout {
        group: MATERIAL_BIND_GROUP,
        textures,
        samplers,
        uniforms: MaterialUniformLayout {
            binding: uniform_binding,
            size: slots.len() as u32 * 16,
            slots,
        },
    }
}

fn validate_capabilities(
    layout: &MaterialResourceLayout,
    capabilities: &MaterialBackendCapabilities,
) -> MaterialCapabilityReport {
    let mut report = MaterialCapabilityReport::default();
    if capabilities.max_bind_groups <= layout.group {
        report.issues.push(MaterialCapabilityIssue {
            code: MaterialCapabilityIssueCode::BindGroupUnavailable,
            message: format!(
                "material group {} requires at least {} bind groups, but the backend exposes {}",
                layout.group,
                layout.group + 1,
                capabilities.max_bind_groups
            ),
        });
    }
    if layout.binding_count() > capabilities.max_bindings_per_bind_group {
        report.issues.push(MaterialCapabilityIssue {
            code: MaterialCapabilityIssueCode::BindingLimitExceeded,
            message: format!(
                "material uses {} bindings, but the backend supports {} per group",
                layout.binding_count(),
                capabilities.max_bindings_per_bind_group
            ),
        });
    }
    if layout.textures.len() as u32 > capabilities.max_sampled_textures_per_shader_stage {
        report.issues.push(MaterialCapabilityIssue {
            code: MaterialCapabilityIssueCode::TextureLimitExceeded,
            message: format!(
                "material uses {} sampled textures, but the backend supports {}",
                layout.textures.len(),
                capabilities.max_sampled_textures_per_shader_stage
            ),
        });
    }
    if layout.samplers.len() as u32 > capabilities.max_samplers_per_shader_stage {
        report.issues.push(MaterialCapabilityIssue {
            code: MaterialCapabilityIssueCode::SamplerLimitExceeded,
            message: format!(
                "material uses {} samplers, but the backend supports {}",
                layout.samplers.len(),
                capabilities.max_samplers_per_shader_stage
            ),
        });
    }
    if u64::from(layout.uniforms.size) > capabilities.max_uniform_buffer_binding_size {
        report.issues.push(MaterialCapabilityIssue {
            code: MaterialCapabilityIssueCode::UniformLimitExceeded,
            message: format!(
                "material uniforms require {} bytes, but the backend supports {}",
                layout.uniforms.size, capabilities.max_uniform_buffer_binding_size
            ),
        });
    }
    report
}

fn build_reflection(
    ir: &MaterialIrProgram,
    layout: &MaterialResourceLayout,
) -> Result<MaterialReflection, MaterialGpuError> {
    let mut required_vertex_inputs = Vec::new();
    let mut required_particle_inputs = Vec::new();
    let mut required_scene_inputs = Vec::new();
    for value in &ir.values {
        let MaterialIrInstruction::Input(input) = value.instruction else {
            continue;
        };
        let target = match input {
            MaterialInput::Uv0 => &mut required_vertex_inputs,
            MaterialInput::ParticleColor | MaterialInput::ParticleOpacity => {
                &mut required_particle_inputs
            }
            MaterialInput::EffectTime => &mut required_scene_inputs,
            _ => {
                return Err(MaterialGpuError::UnsupportedInput {
                    input,
                    expressions: ir
                        .source_map
                        .expressions
                        .get(&value.id)
                        .cloned()
                        .unwrap_or_default(),
                });
            }
        };
        if !target.contains(&input) {
            target.push(input);
        }
    }
    let parameters = ir
        .parameters
        .iter()
        .map(|parameter| {
            let binding = if parameter.evaluation_domain == MaterialEvaluationDomain::ShaderStatic {
                MaterialParameterBinding::ShaderStatic
            } else if let Some(texture) = layout
                .textures
                .iter()
                .find(|slot| slot.parameter == parameter.source)
            {
                MaterialParameterBinding::Texture {
                    binding: texture.binding,
                    sampler_binding: texture.sampler_binding,
                }
            } else {
                let slot = layout
                    .uniforms
                    .slots
                    .iter()
                    .find(|slot| slot.parameter == parameter.source)
                    .ok_or(MaterialGpuError::MissingParameter(parameter.source))?;
                MaterialParameterBinding::Uniform {
                    binding: layout
                        .uniforms
                        .binding
                        .expect("uniform slots require a uniform binding"),
                    offset: slot.offset,
                }
            };
            Ok(MaterialParameterReflection {
                id: parameter.source,
                name: parameter.name.clone(),
                value_type: parameter.value_type,
                evaluation_domain: parameter.evaluation_domain,
                default: parameter.default.clone(),
                binding,
            })
        })
        .collect::<Result<Vec<_>, MaterialGpuError>>()?;
    Ok(MaterialReflection {
        parameters,
        required_vertex_inputs,
        required_particle_inputs,
        required_scene_inputs,
    })
}

fn generate_wesl(
    ir: &MaterialIrProgram,
    layout: &MaterialResourceLayout,
) -> Result<(String, BTreeMap<u32, MaterialIrValueId>), MaterialGpuError> {
    let mut source = String::new();
    let mut lines = BTreeMap::new();
    source.push_str("// Generated by Aestra's portable material backend.\n");
    source.push_str("// Resource bindings are described by MaterialResourceLayout.\n\n");
    source.push_str(
        "struct MaterialFragmentInput {\n    @location(6) uv0: vec2<f32>,\n    @location(7) particle_color: vec4<f32>,\n    @location(8) particle_opacity: f32,\n    @location(9) effect_time: f32,\n    @location(10) quad_position: vec2<f32>,\n    @location(11) softness: f32,\n    @location(12) @interpolate(flat) textured: u32,\n    @location(13) @interpolate(flat) visible: u32,\n}\n\n",
    );
    if let Some(binding) = layout.uniforms.binding {
        source.push_str("struct MaterialUniforms {\n");
        for slot in &layout.uniforms.slots {
            source.push_str(&format!(
                "    @align(16) {}: {},\n",
                parameter_name(slot.parameter),
                uniform_wgsl_type(slot.value_type)
            ));
        }
        source.push_str("}\n\n");
        source.push_str(&format!(
            "@group({}) @binding({binding}) var<uniform> material_uniforms: MaterialUniforms;\n",
            layout.group
        ));
    }
    for texture in &layout.textures {
        source.push_str(&format!(
            "@group({}) @binding({}) var {}: texture_2d<f32>;\n",
            layout.group,
            texture.binding,
            texture_name(texture.parameter)
        ));
    }
    for sampler in &layout.samplers {
        source.push_str(&format!(
            "@group({}) @binding({}) var material_sampler_{}: sampler;\n",
            layout.group, sampler.binding, sampler.binding
        ));
    }
    source.push_str("\nfn aestra_evaluate_material(input: MaterialFragmentInput) -> vec4<f32> {\n");
    for value in &ir.values {
        if matches!(value.value_type, MaterialValueType::Texture2D(_)) {
            continue;
        }
        let expression = instruction_expression(ir, layout, value)?;
        let line = source.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1;
        lines.insert(line, value.id);
        source.push_str(&format!(
            "    let {}: {} = {expression};\n",
            value_name(value.id),
            wgsl_type(value.value_type)
        ));
    }
    let color = ir
        .value(ir.outputs.color)
        .expect("typed IR output must reference a live value");
    let color_rgb = match color.value_type {
        MaterialValueType::Vec3 => value_name(color.id),
        MaterialValueType::Vec4 | MaterialValueType::Color => {
            format!("{}.rgb", value_name(color.id))
        }
        _ => unreachable!("typed IR validates the material color output"),
    };
    source.push_str(&format!(
        "    return vec4<f32>({color_rgb}, {});\n",
        value_name(ir.outputs.alpha)
    ));
    source.push_str("}\n\n");
    source.push_str(&format!(
        "@fragment\nfn {MATERIAL_FRAGMENT_ENTRY_POINT}(input: MaterialFragmentInput) -> @location(0) vec4<f32> {{\n    if input.visible == 0u {{\n        discard;\n    }}\n    let output = aestra_evaluate_material(input);\n    let feather = clamp(input.softness, 0.001, 1.0);\n    let distance = select(length(input.quad_position), max(abs(input.quad_position.x), abs(input.quad_position.y)), input.textured != 0u);\n    let coverage = 1.0 - smoothstep(1.0 - feather, 1.0, distance);\n    return vec4<f32>(output.rgb, clamp(output.a, 0.0, 1.0) * coverage);\n}}\n"
    ));
    Ok((source, lines))
}

fn instruction_expression(
    ir: &MaterialIrProgram,
    layout: &MaterialResourceLayout,
    value: &MaterialIrValue,
) -> Result<String, MaterialGpuError> {
    let expression = match &value.instruction {
        MaterialIrInstruction::Constant(constant) => constant_expression(constant),
        MaterialIrInstruction::Input(input) => input_expression(*input).map(str::to_owned).ok_or(
            MaterialGpuError::UnsupportedInput {
                input: *input,
                expressions: ir
                    .source_map
                    .expressions
                    .get(&value.id)
                    .cloned()
                    .unwrap_or_default(),
            },
        )?,
        MaterialIrInstruction::Parameter(parameter) => {
            let parameter_info = ir
                .parameters
                .iter()
                .find(|candidate| candidate.source == *parameter)
                .ok_or(MaterialGpuError::MissingParameter(*parameter))?;
            if parameter_info.evaluation_domain == MaterialEvaluationDomain::ShaderStatic {
                constant_expression(
                    parameter_info
                        .default
                        .as_ref()
                        .ok_or(MaterialGpuError::MissingShaderStaticDefault(*parameter))?,
                )
            } else if parameter_info.value_type == MaterialValueType::Bool {
                format!("(material_uniforms.{} != 0u)", parameter_name(*parameter))
            } else {
                format!("material_uniforms.{}", parameter_name(*parameter))
            }
        }
        MaterialIrInstruction::Add(left, right) => binary_expression(ir, *left, *right, "+", value),
        MaterialIrInstruction::Subtract(left, right) => {
            binary_expression(ir, *left, *right, "-", value)
        }
        MaterialIrInstruction::Multiply(left, right) => {
            binary_expression(ir, *left, *right, "*", value)
        }
        MaterialIrInstruction::Divide(left, right) => {
            binary_expression(ir, *left, *right, "/", value)
        }
        MaterialIrInstruction::Lerp { start, end, factor } => format!(
            "mix({}, {}, {})",
            value_name(*start),
            value_name(*end),
            value_name(*factor)
        ),
        MaterialIrInstruction::Clamp { value, min, max } => format!(
            "clamp({}, {}, {})",
            value_name(*value),
            value_name(*min),
            value_name(*max)
        ),
        MaterialIrInstruction::SampleTexture { texture, uv } => {
            let texture_value = ir
                .value(*texture)
                .ok_or(MaterialGpuError::InvalidTextureSource(*texture))?;
            let MaterialIrInstruction::Parameter(parameter) = texture_value.instruction else {
                return Err(MaterialGpuError::InvalidTextureSource(*texture));
            };
            let slot = layout
                .textures
                .iter()
                .find(|slot| slot.parameter == parameter)
                .ok_or(MaterialGpuError::MissingParameter(parameter))?;
            format!(
                "textureSample({}, material_sampler_{}, {})",
                texture_name(parameter),
                slot.sampler_binding,
                value_name(*uv)
            )
        }
        MaterialIrInstruction::ExtractComponent { value, component } => {
            format!("{}.{}", value_name(*value), component_name(*component))
        }
    };
    Ok(expression)
}

fn binary_expression(
    ir: &MaterialIrProgram,
    left: MaterialIrValueId,
    right: MaterialIrValueId,
    operator: &str,
    result: &MaterialIrValue,
) -> String {
    format!(
        "({} {operator} {})",
        promote_operand(ir, left, result.value_type),
        promote_operand(ir, right, result.value_type)
    )
}

fn promote_operand(
    ir: &MaterialIrProgram,
    id: MaterialIrValueId,
    result_type: MaterialValueType,
) -> String {
    let value = ir.value(id).expect("typed IR dependencies must be live");
    if value.value_type == MaterialValueType::Float
        && result_type != MaterialValueType::Float
        && result_type.is_numeric()
    {
        format!("{}({})", wgsl_type(result_type), value_name(id))
    } else {
        value_name(id)
    }
}

fn input_expression(input: MaterialInput) -> Option<&'static str> {
    match input {
        MaterialInput::Uv0 => Some("input.uv0"),
        MaterialInput::ParticleColor => Some("input.particle_color"),
        MaterialInput::ParticleOpacity => Some("input.particle_opacity"),
        MaterialInput::EffectTime => Some("input.effect_time"),
        _ => None,
    }
}

fn constant_expression(constant: &MaterialIrConstant) -> String {
    match constant {
        MaterialIrConstant::Float(value) => float_literal(*value),
        MaterialIrConstant::Vec2(value) => vector_literal("vec2<f32>", value),
        MaterialIrConstant::Vec3(value) => vector_literal("vec3<f32>", value),
        MaterialIrConstant::Vec4(value) | MaterialIrConstant::ColorLinear(value) => {
            vector_literal("vec4<f32>", value)
        }
        MaterialIrConstant::Bool(value) => value.to_string(),
        MaterialIrConstant::Texture2D(_) => {
            unreachable!("validated sampled textures are material parameters")
        }
    }
}

fn vector_literal<const N: usize>(ty: &str, values: &[f32; N]) -> String {
    format!(
        "{ty}({})",
        values
            .iter()
            .map(|value| float_literal(*value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn float_literal(value: f32) -> String {
    let value = if value == -0.0 { 0.0 } else { value };
    let mut rendered = format!("{value:.9}");
    while rendered.contains('.') && rendered.ends_with('0') {
        rendered.pop();
    }
    if rendered.ends_with('.') {
        rendered.push('0');
    }
    rendered
}

fn wgsl_type(value_type: MaterialValueType) -> &'static str {
    match value_type {
        MaterialValueType::Float => "f32",
        MaterialValueType::Vec2 => "vec2<f32>",
        MaterialValueType::Vec3 => "vec3<f32>",
        MaterialValueType::Vec4 | MaterialValueType::Color => "vec4<f32>",
        MaterialValueType::Bool => "bool",
        MaterialValueType::Texture2D(_) => "texture_2d<f32>",
    }
}

fn uniform_wgsl_type(value_type: MaterialValueType) -> &'static str {
    match value_type {
        MaterialValueType::Bool => "u32",
        _ => wgsl_type(value_type),
    }
}

fn value_name(id: MaterialIrValueId) -> String {
    format!("value_{}", id.0)
}

fn parameter_name(id: MaterialParameterId) -> String {
    format!("parameter_{:032x}", id.as_uuid().as_u128())
}

fn texture_name(id: MaterialParameterId) -> String {
    format!("material_texture_{:032x}", id.as_uuid().as_u128())
}

fn sampler_key(descriptor: MaterialSamplerDescriptor) -> (u8, u8, u8, u8) {
    (
        filter_key(descriptor.filter),
        mip_filter_key(descriptor.mip_filter),
        address_key(descriptor.address_u),
        address_key(descriptor.address_v),
    )
}

fn fingerprint_program(
    ir: &MaterialIrProgram,
    layout: &MaterialResourceLayout,
) -> MaterialProgramFingerprint {
    let mut fingerprint = FingerprintBuilder::new(b"aestra.material.program");
    fingerprint.u32(MATERIAL_ABI_VERSION);
    fingerprint.u32(MATERIAL_SHADER_GENERATOR_VERSION);
    hash_domain(&mut fingerprint, ir.domain);
    for parameter in &ir.parameters {
        fingerprint.u128(parameter.source.as_uuid().as_u128());
        hash_value_type(&mut fingerprint, parameter.value_type);
        hash_evaluation_domain(&mut fingerprint, parameter.evaluation_domain);
        if parameter.evaluation_domain == MaterialEvaluationDomain::ShaderStatic
            && let Some(default) = &parameter.default
        {
            hash_constant(&mut fingerprint, default, true);
        }
    }
    for value in &ir.values {
        fingerprint.u32(value.id.0);
        hash_value_type(&mut fingerprint, value.value_type);
        hash_expression_domain(&mut fingerprint, value.evaluation_domain);
        hash_instruction(&mut fingerprint, &value.instruction);
    }
    fingerprint.u32(ir.outputs.color.0);
    fingerprint.u32(ir.outputs.alpha.0);
    hash_resource_layout(&mut fingerprint, layout);
    MaterialProgramFingerprint(fingerprint.finish())
}

fn hash_resource_layout(fingerprint: &mut FingerprintBuilder, layout: &MaterialResourceLayout) {
    fingerprint.u32(layout.group);
    fingerprint.u32(layout.uniforms.binding.unwrap_or(u32::MAX));
    fingerprint.u32(layout.uniforms.size);
    for slot in &layout.uniforms.slots {
        fingerprint.u128(slot.parameter.as_uuid().as_u128());
        hash_value_type(fingerprint, slot.value_type);
        fingerprint.u32(slot.offset);
        fingerprint.u32(slot.size);
    }
    for slot in &layout.textures {
        fingerprint.u128(slot.parameter.as_uuid().as_u128());
        hash_texture_descriptor(fingerprint, slot.descriptor);
        fingerprint.u32(slot.binding);
        fingerprint.u32(slot.sampler_binding);
    }
    for slot in &layout.samplers {
        hash_sampler_descriptor(fingerprint, slot.descriptor);
        fingerprint.u32(slot.binding);
    }
}

fn hash_instruction(fingerprint: &mut FingerprintBuilder, instruction: &MaterialIrInstruction) {
    match instruction {
        MaterialIrInstruction::Constant(value) => {
            fingerprint.byte(0);
            hash_constant(fingerprint, value, false);
        }
        MaterialIrInstruction::Input(input) => {
            fingerprint.byte(1);
            fingerprint.byte(input_key(*input));
        }
        MaterialIrInstruction::Parameter(parameter) => {
            fingerprint.byte(2);
            fingerprint.u128(parameter.as_uuid().as_u128());
        }
        MaterialIrInstruction::Add(left, right) => hash_binary(fingerprint, 3, *left, *right),
        MaterialIrInstruction::Subtract(left, right) => hash_binary(fingerprint, 4, *left, *right),
        MaterialIrInstruction::Multiply(left, right) => hash_binary(fingerprint, 5, *left, *right),
        MaterialIrInstruction::Divide(left, right) => hash_binary(fingerprint, 6, *left, *right),
        MaterialIrInstruction::Lerp { start, end, factor } => {
            fingerprint.byte(7);
            fingerprint.u32(start.0);
            fingerprint.u32(end.0);
            fingerprint.u32(factor.0);
        }
        MaterialIrInstruction::Clamp { value, min, max } => {
            fingerprint.byte(8);
            fingerprint.u32(value.0);
            fingerprint.u32(min.0);
            fingerprint.u32(max.0);
        }
        MaterialIrInstruction::SampleTexture { texture, uv } => {
            fingerprint.byte(9);
            fingerprint.u32(texture.0);
            fingerprint.u32(uv.0);
        }
        MaterialIrInstruction::ExtractComponent { value, component } => {
            fingerprint.byte(10);
            fingerprint.u32(value.0);
            fingerprint.byte(match component {
                MaterialVectorComponent::X => 0,
                MaterialVectorComponent::Y => 1,
                MaterialVectorComponent::Z => 2,
                MaterialVectorComponent::W => 3,
            });
        }
    }
}

fn component_name(component: MaterialVectorComponent) -> &'static str {
    match component {
        MaterialVectorComponent::X => "x",
        MaterialVectorComponent::Y => "y",
        MaterialVectorComponent::Z => "z",
        MaterialVectorComponent::W => "w",
    }
}

fn hash_binary(
    fingerprint: &mut FingerprintBuilder,
    tag: u8,
    left: MaterialIrValueId,
    right: MaterialIrValueId,
) {
    fingerprint.byte(tag);
    fingerprint.u32(left.0);
    fingerprint.u32(right.0);
}

fn hash_constant(
    fingerprint: &mut FingerprintBuilder,
    constant: &MaterialIrConstant,
    include_texture_asset: bool,
) {
    match constant {
        MaterialIrConstant::Float(value) => {
            fingerprint.byte(0);
            fingerprint.u32(value.to_bits());
        }
        MaterialIrConstant::Vec2(value) => hash_floats(fingerprint, 1, value),
        MaterialIrConstant::Vec3(value) => hash_floats(fingerprint, 2, value),
        MaterialIrConstant::Vec4(value) => hash_floats(fingerprint, 3, value),
        MaterialIrConstant::ColorLinear(value) => hash_floats(fingerprint, 4, value),
        MaterialIrConstant::Texture2D(asset) => {
            fingerprint.byte(5);
            if include_texture_asset {
                fingerprint.u128(asset.as_uuid().as_u128());
            }
        }
        MaterialIrConstant::Bool(value) => {
            fingerprint.byte(6);
            fingerprint.byte(u8::from(*value));
        }
    }
}

fn hash_floats<const N: usize>(fingerprint: &mut FingerprintBuilder, tag: u8, values: &[f32; N]) {
    fingerprint.byte(tag);
    for value in values {
        fingerprint.u32(value.to_bits());
    }
}

fn hash_value_type(fingerprint: &mut FingerprintBuilder, value_type: MaterialValueType) {
    match value_type {
        MaterialValueType::Float => fingerprint.byte(0),
        MaterialValueType::Vec2 => fingerprint.byte(1),
        MaterialValueType::Vec3 => fingerprint.byte(2),
        MaterialValueType::Vec4 => fingerprint.byte(3),
        MaterialValueType::Color => fingerprint.byte(4),
        MaterialValueType::Texture2D(descriptor) => {
            fingerprint.byte(5);
            hash_texture_descriptor(fingerprint, descriptor);
        }
        MaterialValueType::Bool => fingerprint.byte(6),
    }
}

fn hash_texture_descriptor(
    fingerprint: &mut FingerprintBuilder,
    descriptor: MaterialTextureDescriptor,
) {
    fingerprint.byte(match descriptor.color_space {
        MaterialTextureColorSpace::SrgbColor => 0,
        MaterialTextureColorSpace::LinearData => 1,
    });
    hash_sampler_descriptor(fingerprint, descriptor.sampler);
}

fn hash_sampler_descriptor(
    fingerprint: &mut FingerprintBuilder,
    descriptor: MaterialSamplerDescriptor,
) {
    fingerprint.byte(filter_key(descriptor.filter));
    fingerprint.byte(mip_filter_key(descriptor.mip_filter));
    fingerprint.byte(address_key(descriptor.address_u));
    fingerprint.byte(address_key(descriptor.address_v));
}

fn hash_domain(fingerprint: &mut FingerprintBuilder, domain: MaterialDomain) {
    fingerprint.byte(match domain {
        MaterialDomain::Sprite => 0,
        MaterialDomain::Mesh => 1,
        MaterialDomain::Ribbon => 2,
        MaterialDomain::Decal => 3,
        MaterialDomain::Screen => 4,
    });
}

fn hash_evaluation_domain(fingerprint: &mut FingerprintBuilder, domain: MaterialEvaluationDomain) {
    fingerprint.byte(match domain {
        MaterialEvaluationDomain::ShaderStatic => 0,
        MaterialEvaluationDomain::Instance => 1,
        MaterialEvaluationDomain::Effect => 2,
        MaterialEvaluationDomain::Emitter => 3,
    });
}

fn hash_expression_domain(
    fingerprint: &mut FingerprintBuilder,
    domain: aestra_core::material::MaterialExpressionDomain,
) {
    fingerprint.byte(match domain {
        aestra_core::material::MaterialExpressionDomain::ShaderStatic => 0,
        aestra_core::material::MaterialExpressionDomain::Instance => 1,
        aestra_core::material::MaterialExpressionDomain::Effect => 2,
        aestra_core::material::MaterialExpressionDomain::Emitter => 3,
        aestra_core::material::MaterialExpressionDomain::Particle => 4,
        aestra_core::material::MaterialExpressionDomain::Vertex => 5,
        aestra_core::material::MaterialExpressionDomain::Fragment => 6,
    });
}

fn hash_render_state(fingerprint: &mut FingerprintBuilder, state: MaterialRenderState) {
    fingerprint.byte(match state.blend {
        aestra_core::BlendMode::Alpha => 0,
        aestra_core::BlendMode::Additive => 1,
        aestra_core::BlendMode::Multiply => 2,
    });
    fingerprint.byte(match state.depth_test {
        aestra_core::material::MaterialDepthTest::Disabled => 0,
        aestra_core::material::MaterialDepthTest::Less => 1,
        aestra_core::material::MaterialDepthTest::LessEqual => 2,
        aestra_core::material::MaterialDepthTest::Always => 3,
    });
    fingerprint.byte(u8::from(state.depth_write));
    fingerprint.byte(match state.cull_mode {
        aestra_core::material::MaterialCullMode::None => 0,
        aestra_core::material::MaterialCullMode::Front => 1,
        aestra_core::material::MaterialCullMode::Back => 2,
    });
}

fn hash_target_format(fingerprint: &mut FingerprintBuilder, format: MaterialColorTargetFormat) {
    match format {
        MaterialColorTargetFormat::Rgba8UnormSrgb => fingerprint.byte(0),
        MaterialColorTargetFormat::Bgra8UnormSrgb => fingerprint.byte(1),
        MaterialColorTargetFormat::Rgba16Float => fingerprint.byte(2),
        MaterialColorTargetFormat::Other(value) => {
            fingerprint.byte(3);
            fingerprint.u32(value);
        }
    }
}

fn filter_key(mode: MaterialFilterMode) -> u8 {
    match mode {
        MaterialFilterMode::Nearest => 0,
        MaterialFilterMode::Linear => 1,
    }
}

fn mip_filter_key(mode: MaterialMipFilterMode) -> u8 {
    match mode {
        MaterialMipFilterMode::None => 0,
        MaterialMipFilterMode::Nearest => 1,
        MaterialMipFilterMode::Linear => 2,
    }
}

fn address_key(mode: MaterialAddressMode) -> u8 {
    match mode {
        MaterialAddressMode::ClampToEdge => 0,
        MaterialAddressMode::Repeat => 1,
        MaterialAddressMode::MirrorRepeat => 2,
    }
}

fn input_key(input: MaterialInput) -> u8 {
    match input {
        MaterialInput::Uv0 => 0,
        MaterialInput::Uv1 => 1,
        MaterialInput::LocalPosition => 2,
        MaterialInput::WorldPosition => 3,
        MaterialInput::Normal => 4,
        MaterialInput::Tangent => 5,
        MaterialInput::ViewDirection => 6,
        MaterialInput::ScreenUv => 7,
        MaterialInput::ParticleColor => 8,
        MaterialInput::ParticleOpacity => 9,
        MaterialInput::ParticleAge => 10,
        MaterialInput::ParticleNormalizedAge => 11,
        MaterialInput::ParticleLifetime => 12,
        MaterialInput::ParticleVelocity => 13,
        MaterialInput::ParticleSpeed => 14,
        MaterialInput::ParticleRandom => 15,
        MaterialInput::ParticleId => 16,
        MaterialInput::ParticleSize => 17,
        MaterialInput::ParticleRotation => 18,
        MaterialInput::EffectTime => 19,
        MaterialInput::EmitterTime => 20,
        MaterialInput::EffectNormalizedTime => 21,
        MaterialInput::EmitterNormalizedTime => 22,
        MaterialInput::SceneDepth => 23,
        MaterialInput::CameraPosition => 24,
        MaterialInput::CameraDirection => 25,
        MaterialInput::PixelDepth => 26,
    }
}

struct FingerprintBuilder {
    lanes: [u64; 4],
}

impl FingerprintBuilder {
    fn new(domain: &[u8]) -> Self {
        let mut builder = Self {
            lanes: [
                0xcbf2_9ce4_8422_2325,
                0x8422_2325_cbf2_9ce4,
                0x9e37_79b9_7f4a_7c15,
                0xd6e8_feb8_6659_fd93,
            ],
        };
        builder.bytes(domain);
        builder
    }

    fn byte(&mut self, byte: u8) {
        const PRIMES: [u64; 4] = [
            0x0000_0100_0000_01b3,
            0x1000_0000_01b3_0001,
            0x9e37_79b1_85eb_ca87,
            0xc2b2_ae3d_27d4_eb4f,
        ];
        for (index, lane) in self.lanes.iter_mut().enumerate() {
            *lane ^= u64::from(byte).wrapping_add(index as u64 * 0x9e37);
            *lane = lane.wrapping_mul(PRIMES[index]);
            *lane ^= *lane >> (29 + index as u32);
        }
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.byte(*byte);
        }
        self.byte(0xff);
    }

    fn u32(&mut self, value: u32) {
        self.bytes(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes(&value.to_le_bytes());
    }

    fn u128(&mut self, value: u128) {
        self.bytes(&value.to_le_bytes());
    }

    fn finish(self) -> [u8; 32] {
        let mut bytes = [0; 32];
        for (index, lane) in self.lanes.into_iter().enumerate() {
            bytes[index * 8..(index + 1) * 8].copy_from_slice(&lane.to_le_bytes());
        }
        bytes
    }
}
