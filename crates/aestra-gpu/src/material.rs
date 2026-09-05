//! Portable material resource ABI, reflection, cache identity, and WESL lowering.

mod varying;
pub use varying::{MaterialVarying, MaterialVaryingLayout, MaterialVaryingSlot};

use crate::shader::{CompiledWesl, GpuShaderError, compile_wesl};
pub use aestra_compiler::MaterialIrConstant;
use aestra_compiler::{
    MaterialIrInstruction, MaterialIrProgram, MaterialIrSourceMap, MaterialIrValue,
    MaterialIrValueId, MaterialTextureSamplingMode, reflect_material_inputs,
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
    collections::{BTreeMap, BTreeSet},
    fmt,
    hash::{Hash, Hasher},
};
use thiserror::Error;

pub const MATERIAL_ABI_VERSION: u32 = 3;
pub const MATERIAL_SHADER_GENERATOR_VERSION: u32 = 19;
pub const MATERIAL_BIND_GROUP: u32 = 2;
/// Renderer-owned scene inputs used by fragment operations such as `DepthFade`.
pub const MATERIAL_SCENE_BIND_GROUP: u32 = 3;
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
    /// Authored metadata retained for a parameter removed from the optimized shader.
    Inactive,
    Uniform {
        binding: u32,
        offset: u32,
    },
    Texture {
        binding: u32,
        sampler_binding: u32,
    },
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
    /// Compact interface shared by the generated vertex and fragment entry points.
    pub varying_layout: MaterialVaryingLayout,
    /// Shader variant for multisampled scene-depth attachments. This is equal
    /// to `shader` when the program does not read depth.
    pub multisampled_shader: CompiledWesl,
    pub source_map: MaterialShaderSourceMap,
    pub resource_layout: MaterialResourceLayout,
    pub reflection: MaterialReflection,
    pub render_state_policy: MaterialRenderStatePolicy,
    pub program_fingerprint: MaterialProgramFingerprint,
    pub has_vertex_offset: bool,
    /// Safe mesh-local expansion. Dynamic displacement has no proven bound in this slice.
    pub vertex_offset_bounds: Option<[f32; 3]>,
}

impl CompiledMaterialProgram {
    pub fn requires_scene_depth(&self) -> bool {
        self.reflection
            .required_scene_inputs
            .iter()
            .any(|input| matches!(input, MaterialInput::SceneDepth | MaterialInput::PixelDepth))
    }

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
        let reflection = build_reflection(ir, &resource_layout)?;
        let requires_scene_depth = reflection
            .required_scene_inputs
            .iter()
            .any(|input| matches!(input, MaterialInput::SceneDepth | MaterialInput::PixelDepth));
        let report = validate_capabilities(&resource_layout, capabilities, requires_scene_depth);
        if !report.is_compatible() {
            return Err(MaterialGpuError::Capabilities(report));
        }
        let varying_layout = MaterialVaryingLayout::from_ir(ir);
        let program_fingerprint = fingerprint_program(ir, &resource_layout, &varying_layout);
        let (wesl, wesl_lines) = generate_wesl(ir, &resource_layout, &varying_layout, false)?;
        let source_map = MaterialShaderSourceMap {
            ir: ir.source_map.clone(),
            wesl_lines,
        };
        let module_name = format!(
            "package::aestra_material_{}",
            &program_fingerprint.to_string()[..16]
        );
        let mut entry_points = vec!["vertex", MATERIAL_FRAGMENT_ENTRY_POINT];
        if ir.outputs.vertex_offset.is_some() {
            entry_points.extend(["vertex_mesh_wireframe", "fragment_mesh_wireframe"]);
        }
        let shader = compile_wesl(&module_name, &wesl, &entry_points).map_err(|error| {
            MaterialGpuError::Shader {
                error,
                source_map: Box::new(source_map.clone()),
            }
        })?;
        let multisampled_shader = if requires_scene_depth {
            let (wesl, _) = generate_wesl(ir, &resource_layout, &varying_layout, true)?;
            compile_wesl(
                &format!("{module_name}_multisampled"),
                &wesl,
                &["vertex", MATERIAL_FRAGMENT_ENTRY_POINT],
            )
            .map_err(|error| MaterialGpuError::Shader {
                error,
                source_map: Box::new(source_map.clone()),
            })?
        } else {
            shader.clone()
        };
        Ok(CompiledMaterialProgram {
            source: ir.source,
            shader,
            varying_layout,
            multisampled_shader,
            source_map,
            resource_layout,
            reflection,
            render_state_policy: ir.render_state_policy.clone(),
            program_fingerprint,
            has_vertex_offset: ir.outputs.vertex_offset.is_some(),
            vertex_offset_bounds: match ir.outputs.vertex_offset.and_then(|id| ir.value(id)) {
                None => Some([0.0; 3]),
                Some(MaterialIrValue {
                    instruction: MaterialIrInstruction::Constant(MaterialIrConstant::Vec3(offset)),
                    ..
                }) => Some(offset.map(f32::abs)),
                _ => None,
            },
        })
    }
}

fn build_resource_layout(ir: &MaterialIrProgram) -> MaterialResourceLayout {
    let live_parameters = ir
        .values
        .iter()
        .filter_map(|value| match value.instruction {
            MaterialIrInstruction::Parameter(parameter) => Some(parameter),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let uniform_parameters = ir
        .parameters
        .iter()
        .filter(|parameter| {
            live_parameters.contains(&parameter.source)
                && parameter.evaluation_domain != MaterialEvaluationDomain::ShaderStatic
                && !matches!(parameter.value_type, MaterialValueType::Texture2D(_))
        })
        .collect::<Vec<_>>();
    let texture_parameters = ir
        .parameters
        .iter()
        .filter_map(|parameter| match parameter.value_type {
            MaterialValueType::Texture2D(descriptor)
                if live_parameters.contains(&parameter.source) =>
            {
                Some((parameter.source, descriptor))
            }
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
    requires_scene_depth: bool,
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
    if requires_scene_depth && capabilities.max_bind_groups <= MATERIAL_SCENE_BIND_GROUP {
        report.issues.push(MaterialCapabilityIssue {
            code: MaterialCapabilityIssueCode::BindGroupUnavailable,
            message: format!(
                "scene depth group {MATERIAL_SCENE_BIND_GROUP} requires at least {} bind groups, but the backend exposes {}",
                MATERIAL_SCENE_BIND_GROUP + 1,
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
    if requires_scene_depth && capabilities.max_bindings_per_bind_group < 2 {
        report.issues.push(MaterialCapabilityIssue {
            code: MaterialCapabilityIssueCode::BindingLimitExceeded,
            message: format!(
                "scene depth uses 2 bindings, but the backend supports {} per group",
                capabilities.max_bindings_per_bind_group
            ),
        });
    }
    let sampled_texture_count = layout.textures.len() as u32 + u32::from(requires_scene_depth);
    if sampled_texture_count > capabilities.max_sampled_textures_per_shader_stage {
        report.issues.push(MaterialCapabilityIssue {
            code: MaterialCapabilityIssueCode::TextureLimitExceeded,
            message: format!(
                "material uses {} sampled textures, but the backend supports {}",
                sampled_texture_count, capabilities.max_sampled_textures_per_shader_stage
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
    let requirements = reflect_material_inputs(ir);
    for input in requirements.all() {
        if !matches!(
            input,
            MaterialInput::Uv0
                | MaterialInput::Normal
                | MaterialInput::ViewDirection
                | MaterialInput::ParticleColor
                | MaterialInput::ParticleOpacity
                | MaterialInput::ParticleNormalizedAge
                | MaterialInput::EffectTime
                | MaterialInput::SceneDepth
                | MaterialInput::PixelDepth
        ) && !(ir.domain == MaterialDomain::Mesh
            && matches!(
                input,
                MaterialInput::WorldPosition | MaterialInput::LocalPosition
            ))
        {
            let expressions = ir
                .values
                .iter()
                .find_map(|value| {
                    matches!(value.instruction, MaterialIrInstruction::Input(candidate) if candidate == input)
                        .then(|| {
                            ir.source_map
                                .expressions
                                .get(&value.id)
                                .cloned()
                                .unwrap_or_default()
                        })
                })
                .unwrap_or_default();
            return Err(MaterialGpuError::UnsupportedInput { input, expressions });
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
            } else if let Some(slot) = layout
                .uniforms
                .slots
                .iter()
                .find(|slot| slot.parameter == parameter.source)
            {
                MaterialParameterBinding::Uniform {
                    binding: layout
                        .uniforms
                        .binding
                        .expect("uniform slots require a uniform binding"),
                    offset: slot.offset,
                }
            } else {
                MaterialParameterBinding::Inactive
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
        required_vertex_inputs: requirements.vertex,
        required_particle_inputs: requirements.particle,
        required_scene_inputs: requirements.scene,
    })
}

fn generate_wesl(
    ir: &MaterialIrProgram,
    layout: &MaterialResourceLayout,
    varyings: &MaterialVaryingLayout,
    multisampled_depth: bool,
) -> Result<(String, BTreeMap<u32, MaterialIrValueId>), MaterialGpuError> {
    let mut source = String::new();
    let mut lines = BTreeMap::new();
    source.push_str("// Generated by Aestra's portable material backend.\n");
    source.push_str("// Resource bindings are described by MaterialResourceLayout.\n\n");
    source.push_str(&varyings.vertex_wesl(ir.outputs.vertex_offset.is_some()));
    source.push_str(
        "struct MaterialFragmentInput {\n    @builtin(position) fragment_position: vec4<f32>,\n",
    );
    source.push_str(&varyings.declarations());
    if multisampled_depth {
        source.push_str("    @builtin(sample_index) sample_index: u32,\n");
    }
    source.push_str("}\n\n");
    let requires_scene_depth = reflect_material_inputs(ir)
        .scene
        .iter()
        .any(|input| matches!(input, MaterialInput::SceneDepth | MaterialInput::PixelDepth));
    if requires_scene_depth {
        source.push_str(
            "struct MaterialSceneUniforms {\n    view_from_clip: mat4x4<f32>,\n    viewport: vec4<f32>,\n}\n\n",
        );
        source.push_str(&format!(
            "@group({MATERIAL_SCENE_BIND_GROUP}) @binding(0) var<uniform> material_scene: MaterialSceneUniforms;\n"
        ));
        source.push_str(&format!(
            "@group({MATERIAL_SCENE_BIND_GROUP}) @binding(1) var material_scene_depth: {};\n\n",
            if multisampled_depth {
                "texture_depth_multisampled_2d"
            } else {
                "texture_depth_2d"
            }
        ));
        source.push_str(
            "fn aestra_linear_view_depth(raw_depth: f32, fragment_position: vec4<f32>) -> f32 {\n    let uv = (fragment_position.xy - material_scene.viewport.xy) / material_scene.viewport.zw;\n    let ndc = vec2<f32>((uv.x * 2.0) - 1.0, ((1.0 - uv.y) * 2.0) - 1.0);\n    let view_position = material_scene.view_from_clip * vec4<f32>(ndc, raw_depth, 1.0);\n    return abs(view_position.z / view_position.w);\n}\n\n",
        );
        let load = if multisampled_depth {
            "textureLoad(material_scene_depth, vec2<i32>(input.fragment_position.xy), i32(input.sample_index))"
        } else {
            "textureLoad(material_scene_depth, vec2<i32>(input.fragment_position.xy), 0)"
        };
        source.push_str(&format!(
            "fn aestra_scene_depth(input: MaterialFragmentInput) -> f32 {{\n    return aestra_linear_view_depth({load}, input.fragment_position);\n}}\n\nfn aestra_pixel_depth(input: MaterialFragmentInput) -> f32 {{\n    return aestra_linear_view_depth(input.fragment_position.z, input.fragment_position);\n}}\n\n"
        ));
    }
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
    let custom_sources = ir
        .values
        .iter()
        .filter_map(|value| match &value.instruction {
            MaterialIrInstruction::CustomWeslCall {
                function, source, ..
            } => Some((*function, (source.as_str(), value.id))),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    for (function, (custom_source, value)) in custom_sources {
        source.push('\n');
        let custom_source = namespace_custom_wesl(function.as_uuid().as_u128(), custom_source);
        let first_line = source.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1;
        for offset in 0..custom_source.lines().count() as u32 {
            lines.insert(first_line + offset, value);
        }
        source.push_str(&custom_source);
        source.push('\n');
    }
    if let Some(offset) = ir.outputs.vertex_offset {
        source.push_str("\nfn aestra_vertex_offset(input: MaterialVertexOutput) -> vec3<f32> {\n");
        let live = ir.live_values([offset]);
        for value in ir.values.iter().filter(|value| live.contains(&value.id)) {
            if matches!(value.value_type, MaterialValueType::Texture2D(_)) {
                continue;
            }
            let expression = instruction_expression(ir, layout, varyings, value)?;
            let line = source.bytes().filter(|byte| *byte == b'\n').count() as u32 + 1;
            lines.insert(line, value.id);
            source.push_str(&format!(
                "    let {}: {} = {expression};\n",
                value_name(value.id),
                wgsl_type(value.value_type)
            ));
        }
        source.push_str(&format!("    return {};\n}}\n", value_name(offset)));
    }
    source.push_str("\nfn aestra_evaluate_material(input: MaterialFragmentInput) -> vec4<f32> {\n");
    let fragment_live = ir.live_values([ir.outputs.color, ir.outputs.alpha]);
    for value in ir
        .values
        .iter()
        .filter(|value| fragment_live.contains(&value.id))
    {
        if matches!(value.value_type, MaterialValueType::Texture2D(_)) {
            continue;
        }
        let expression = instruction_expression(ir, layout, varyings, value)?;
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
    if ir.domain == MaterialDomain::Mesh {
        if ir.outputs.vertex_offset.is_some() {
            source.push_str("@fragment\nfn fragment_mesh_wireframe(input: MaterialFragmentInput) -> @location(0) vec4<f32> {\n    if input.visible == 0u { discard; }\n    return vec4<f32>(0.72, 0.56, 1.0, 0.92);\n}\n");
        }
        source.push_str(&format!("@fragment\nfn {MATERIAL_FRAGMENT_ENTRY_POINT}(input: MaterialFragmentInput) -> @location(0) vec4<f32> {{\n    if input.visible == 0u {{ discard; }}\n    let output = aestra_evaluate_material(input);\n    return vec4<f32>(output.rgb, clamp(output.a, 0.0, 1.0));\n}}\n"));
        return Ok((source, lines));
    }
    source.push_str(&format!(
        "@fragment\nfn {MATERIAL_FRAGMENT_ENTRY_POINT}(input: MaterialFragmentInput) -> @location(0) vec4<f32> {{\n    if input.visible == 0u {{\n        discard;\n    }}\n    let output = aestra_evaluate_material(input);\n    let feather = clamp(input.softness, 0.001, 1.0);\n    let distance = select(length(input.quad_position), max(abs(input.quad_position.x), abs(input.quad_position.y)), input.textured != 0u);\n    let coverage = 1.0 - smoothstep(1.0 - feather, 1.0, distance);\n    return vec4<f32>(output.rgb, clamp(output.a, 0.0, 1.0) * coverage);\n}}\n"
    ));
    Ok((source, lines))
}

fn instruction_expression(
    ir: &MaterialIrProgram,
    layout: &MaterialResourceLayout,
    varyings: &MaterialVaryingLayout,
    value: &MaterialIrValue,
) -> Result<String, MaterialGpuError> {
    let expression = match &value.instruction {
        MaterialIrInstruction::Constant(constant) => constant_expression(constant),
        MaterialIrInstruction::Input(input) => input_expression(*input, varyings)
            .map(str::to_owned)
            .ok_or(MaterialGpuError::UnsupportedInput {
                input: *input,
                expressions: ir
                    .source_map
                    .expressions
                    .get(&value.id)
                    .cloned()
                    .unwrap_or_default(),
            })?,
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
        MaterialIrInstruction::CustomWeslCall {
            function,
            entry_point,
            arguments,
            ..
        } => format!(
            "{}({})",
            custom_wesl_symbol(function.as_uuid().as_u128(), entry_point),
            arguments
                .iter()
                .map(|argument| value_name(*argument))
                .collect::<Vec<_>>()
                .join(", ")
        ),
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
        MaterialIrInstruction::Select {
            condition,
            if_false,
            if_true,
        } => format!(
            "select({}, {}, {})",
            value_name(*if_false),
            value_name(*if_true),
            value_name(*condition)
        ),
        MaterialIrInstruction::Remap {
            value: remap_value,
            input_min,
            input_max,
            output_min,
            output_max,
        } => remap_expression(
            ir,
            *remap_value,
            *input_min,
            *input_max,
            *output_min,
            *output_max,
            value.value_type,
        ),
        MaterialIrInstruction::Smoothstep {
            edge_min,
            edge_max,
            value: smoothstep_value,
        } => smoothstep_expression(
            ir,
            *edge_min,
            *edge_max,
            *smoothstep_value,
            value.value_type,
        ),
        MaterialIrInstruction::Fresnel {
            normal,
            view,
            power,
        } => format!(
            "pow(clamp(1.0 - dot(normalize({}), normalize({})), 0.0, 1.0), max({}, 0.000001))",
            value_name(*normal),
            value_name(*view),
            value_name(*power),
        ),
        MaterialIrInstruction::RadialMask {
            uv,
            center,
            radius,
            softness,
            invert,
        } => radial_mask_expression(*uv, *center, *radius, *softness, *invert),
        MaterialIrInstruction::Dissolve {
            source,
            threshold,
            edge_width,
            invert,
        } => dissolve_expression(*source, *threshold, *edge_width, *invert),
        MaterialIrInstruction::DissolveEdge {
            source,
            threshold,
            edge_width,
            invert,
        } => dissolve_edge_expression(*source, *threshold, *edge_width, *invert),
        MaterialIrInstruction::DepthFade {
            scene_depth,
            pixel_depth,
            fade_distance,
            invert,
        } => depth_fade_expression(*scene_depth, *pixel_depth, *fade_distance, *invert),
        MaterialIrInstruction::SoftParticle {
            alpha,
            scene_depth,
            pixel_depth,
            fade_distance,
            invert,
        } => soft_particle_expression(*alpha, *scene_depth, *pixel_depth, *fade_distance, *invert),
        MaterialIrInstruction::PanUv { uv, speed, time } => format!(
            "({} + ({} * {}))",
            value_name(*uv),
            value_name(*speed),
            value_name(*time)
        ),
        MaterialIrInstruction::RotateUv { uv, center, angle } => format!(
            "({center} + (mat2x2<f32>(cos({angle}), sin({angle}), -sin({angle}), cos({angle})) * ({uv} - {center})))",
            uv = value_name(*uv),
            center = value_name(*center),
            angle = value_name(*angle),
        ),
        MaterialIrInstruction::ScaleUv { uv, center, scale } => format!(
            "({center} + (({uv} - {center}) * {scale}))",
            uv = value_name(*uv),
            center = value_name(*center),
            scale = value_name(*scale),
        ),
        MaterialIrInstruction::DerivativeX { value } => {
            format!("dpdx({})", value_name(*value))
        }
        MaterialIrInstruction::DerivativeY { value } => {
            format!("dpdy({})", value_name(*value))
        }
        MaterialIrInstruction::SampleTexture {
            texture,
            uv,
            sampling,
        } => {
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
            match sampling {
                MaterialTextureSamplingMode::ImplicitDerivatives => format!(
                    "textureSample({}, material_sampler_{}, {})",
                    texture_name(parameter),
                    slot.sampler_binding,
                    value_name(*uv)
                ),
                MaterialTextureSamplingMode::ExplicitLod { level } => format!(
                    "textureSampleLevel({}, material_sampler_{}, {}, {})",
                    texture_name(parameter),
                    slot.sampler_binding,
                    value_name(*uv),
                    value_name(*level)
                ),
                MaterialTextureSamplingMode::ExplicitGradient { ddx, ddy } => format!(
                    "textureSampleGrad({}, material_sampler_{}, {}, {}, {})",
                    texture_name(parameter),
                    slot.sampler_binding,
                    value_name(*uv),
                    value_name(*ddx),
                    value_name(*ddy)
                ),
            }
        }
        MaterialIrInstruction::ExtractComponent { value, component } => {
            format!("{}.{}", value_name(*value), component_name(*component))
        }
    };
    Ok(expression)
}

fn namespace_custom_wesl(function: u128, source: &str) -> String {
    let mut names = Vec::new();
    for (offset, _) in source.match_indices("fn ") {
        let rest = &source[offset + 3..];
        let name = rest
            .chars()
            .take_while(|character| *character == '_' || character.is_ascii_alphanumeric())
            .collect::<String>();
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    let mut namespaced = source.to_owned();
    for name in names {
        namespaced =
            replace_wesl_identifier(&namespaced, &name, &custom_wesl_symbol(function, &name));
    }
    namespaced
}

fn custom_wesl_symbol(function: u128, entry_point: &str) -> String {
    format!("aestra_custom_{function:032x}_{entry_point}")
}

fn replace_wesl_identifier(source: &str, from: &str, to: &str) -> String {
    let mut output = String::with_capacity(source.len() + to.len());
    let mut cursor = 0;
    while let Some(relative) = source[cursor..].find(from) {
        let start = cursor + relative;
        let end = start + from.len();
        let before_is_identifier = source[..start]
            .chars()
            .next_back()
            .is_some_and(|character| character == '_' || character.is_ascii_alphanumeric());
        let after_is_identifier = source[end..]
            .chars()
            .next()
            .is_some_and(|character| character == '_' || character.is_ascii_alphanumeric());
        output.push_str(&source[cursor..start]);
        if before_is_identifier || after_is_identifier {
            output.push_str(from);
        } else {
            output.push_str(to);
        }
        cursor = end;
    }
    output.push_str(&source[cursor..]);
    output
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

#[allow(clippy::too_many_arguments)]
fn remap_expression(
    ir: &MaterialIrProgram,
    value: MaterialIrValueId,
    input_min: MaterialIrValueId,
    input_max: MaterialIrValueId,
    output_min: MaterialIrValueId,
    output_max: MaterialIrValueId,
    result_type: MaterialValueType,
) -> String {
    let value = promote_operand(ir, value, result_type);
    let input_min = promote_operand(ir, input_min, result_type);
    let input_max = promote_operand(ir, input_max, result_type);
    let output_min = promote_operand(ir, output_min, result_type);
    let output_max = promote_operand(ir, output_max, result_type);
    let delta = format!("({input_max} - {input_min})");
    let epsilon = numeric_literal(result_type, 0.000_001);
    let one = numeric_literal(result_type, 1.0);
    let safe = format!("(abs({delta}) >= {epsilon})");
    let denominator = format!("select({one}, {delta}, {safe})");
    format!(
        "select({output_min}, ({output_min} + ((({value} - {input_min}) / {denominator}) * ({output_max} - {output_min}))), {safe})"
    )
}

fn smoothstep_expression(
    ir: &MaterialIrProgram,
    edge_min: MaterialIrValueId,
    edge_max: MaterialIrValueId,
    value: MaterialIrValueId,
    result_type: MaterialValueType,
) -> String {
    let edge_min = promote_operand(ir, edge_min, result_type);
    let edge_max = promote_operand(ir, edge_max, result_type);
    let value = promote_operand(ir, value, result_type);
    let delta = format!("({edge_max} - {edge_min})");
    let epsilon = numeric_literal(result_type, 0.000_001);
    let zero = numeric_literal(result_type, 0.0);
    let one = numeric_literal(result_type, 1.0);
    let two = numeric_literal(result_type, 2.0);
    let three = numeric_literal(result_type, 3.0);
    let safe = format!("(abs({delta}) >= {epsilon})");
    let denominator = format!("select({one}, {delta}, {safe})");
    let factor = format!("clamp((({value} - {edge_min}) / {denominator}), {zero}, {one})");
    let curve = format!("(({factor} * {factor}) * ({three} - ({two} * {factor})))");
    let step = format!("select({zero}, {one}, ({value} >= {edge_max}))");
    format!("select({step}, {curve}, {safe})")
}

fn radial_mask_expression(
    uv: MaterialIrValueId,
    center: MaterialIrValueId,
    radius: MaterialIrValueId,
    softness: MaterialIrValueId,
    invert: MaterialIrValueId,
) -> String {
    let uv = value_name(uv);
    let center = value_name(center);
    let radius = format!("max({}, 0.0)", value_name(radius));
    let softness = format!("max({}, 0.0)", value_name(softness));
    let invert = value_name(invert);
    let distance = format!("length({uv} - {center})");
    let safe = format!("({softness} >= 0.000001)");
    let denominator = format!("select(1.0, {softness}, {safe})");
    let factor =
        format!("clamp((({distance} - ({radius} - {softness})) / {denominator}), 0.0, 1.0)");
    let smooth = format!("(({factor} * {factor}) * (3.0 - (2.0 * {factor})))");
    let soft_mask = format!("(1.0 - {smooth})");
    let hard_mask = format!("select(0.0, 1.0, ({distance} <= {radius}))");
    let mask = format!("select({hard_mask}, {soft_mask}, {safe})");
    format!("select({mask}, (1.0 - {mask}), {invert})")
}

fn dissolve_expression(
    source: MaterialIrValueId,
    threshold: MaterialIrValueId,
    edge_width: MaterialIrValueId,
    invert: MaterialIrValueId,
) -> String {
    let source = value_name(source);
    let threshold = value_name(threshold);
    let edge_width = format!("max({}, 0.0)", value_name(edge_width));
    let invert = value_name(invert);
    let safe = format!("({edge_width} >= 0.000001)");
    let denominator = format!("select(1.0, {edge_width}, {safe})");
    let factor =
        format!("clamp((({source} - ({threshold} - {edge_width})) / {denominator}), 0.0, 1.0)");
    let soft_mask = format!("(({factor} * {factor}) * (3.0 - (2.0 * {factor})))");
    let hard_mask = format!("select(0.0, 1.0, ({source} >= {threshold}))");
    let mask = format!("select({hard_mask}, {soft_mask}, {safe})");
    format!("select({mask}, (1.0 - {mask}), {invert})")
}

fn dissolve_edge_expression(
    source: MaterialIrValueId,
    threshold: MaterialIrValueId,
    edge_width: MaterialIrValueId,
    invert: MaterialIrValueId,
) -> String {
    let source = value_name(source);
    let threshold = value_name(threshold);
    let edge_width = format!("max({}, 0.0)", value_name(edge_width));
    let invert = value_name(invert);
    let safe = format!("({edge_width} >= 0.000001)");
    let directed_distance =
        format!("select(({source} - {threshold}), ({threshold} - {source}), {invert})");
    let denominator = format!("select(1.0, {edge_width}, {safe})");
    let factor = format!("clamp(({directed_distance} / {denominator}), 0.0, 1.0)");
    let smooth = format!("(({factor} * {factor}) * (3.0 - (2.0 * {factor})))");
    let inside = format!("(({directed_distance} >= 0.0) && {safe})");
    format!("select(0.0, (1.0 - {smooth}), {inside})")
}

fn depth_fade_expression(
    scene_depth: MaterialIrValueId,
    pixel_depth: MaterialIrValueId,
    fade_distance: MaterialIrValueId,
    invert: MaterialIrValueId,
) -> String {
    let scene_depth = value_name(scene_depth);
    let pixel_depth = value_name(pixel_depth);
    let fade_distance = format!("max({}, 0.0)", value_name(fade_distance));
    let invert = value_name(invert);
    let separation = format!("max(({scene_depth} - {pixel_depth}), 0.0)");
    let safe = format!("({fade_distance} >= 0.000001)");
    let denominator = format!("select(1.0, {fade_distance}, {safe})");
    let soft = format!("clamp(({separation} / {denominator}), 0.0, 1.0)");
    let hard = format!("select(0.0, 1.0, ({separation} > 0.0))");
    let fade = format!("select({hard}, {soft}, {safe})");
    format!("select({fade}, (1.0 - {fade}), {invert})")
}

fn soft_particle_expression(
    alpha: MaterialIrValueId,
    scene_depth: MaterialIrValueId,
    pixel_depth: MaterialIrValueId,
    fade_distance: MaterialIrValueId,
    invert: MaterialIrValueId,
) -> String {
    let fade = depth_fade_expression(scene_depth, pixel_depth, fade_distance, invert);
    format!("({} * {fade})", value_name(alpha))
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

fn input_expression(
    input: MaterialInput,
    varyings: &MaterialVaryingLayout,
) -> Option<&'static str> {
    match input {
        MaterialInput::Normal if varyings.domain == MaterialDomain::Mesh => {
            Some("normalize(input.normal)")
        }
        MaterialInput::ViewDirection if varyings.domain == MaterialDomain::Mesh => {
            Some("normalize(input.view_direction)")
        }
        MaterialInput::WorldPosition if varyings.domain == MaterialDomain::Mesh => {
            Some("input.world_position")
        }
        MaterialInput::LocalPosition if varyings.domain == MaterialDomain::Mesh => {
            Some("input.local_position")
        }
        MaterialInput::Uv0 => Some("input.uv0"),
        MaterialInput::Normal => Some(
            "normalize(vec3<f32>(input.quad_position, sqrt(max(0.0, 1.0 - dot(input.quad_position, input.quad_position)))))",
        ),
        MaterialInput::ViewDirection => Some("vec3<f32>(0.0, 0.0, 1.0)"),
        MaterialInput::ParticleColor => Some("input.particle_color"),
        MaterialInput::ParticleOpacity if varyings.has_color() => Some("input.particle_color.a"),
        MaterialInput::ParticleOpacity => Some("input.particle_opacity"),
        MaterialInput::ParticleNormalizedAge => Some("input.particle_normalized_age"),
        MaterialInput::EffectTime => Some("input.effect_time"),
        MaterialInput::SceneDepth => Some("aestra_scene_depth(input)"),
        MaterialInput::PixelDepth => Some("aestra_pixel_depth(input)"),
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

fn numeric_literal(value_type: MaterialValueType, value: f32) -> String {
    let value = float_literal(value);
    if value_type == MaterialValueType::Float {
        value
    } else {
        format!("{}({value})", wgsl_type(value_type))
    }
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
    varyings: &MaterialVaryingLayout,
) -> MaterialProgramFingerprint {
    let mut fingerprint = FingerprintBuilder::new(b"aestra.material.program");
    fingerprint.u32(MATERIAL_ABI_VERSION);
    fingerprint.u32(MATERIAL_SHADER_GENERATOR_VERSION);
    fingerprint.u32(varyings.slots.len() as u32);
    for slot in &varyings.slots {
        fingerprint.u32(slot.location);
        fingerprint.byte(slot.varying as u8);
    }
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
    fingerprint.u32(ir.outputs.vertex_offset.map_or(u32::MAX, |id| id.0));
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
        MaterialIrInstruction::CustomWeslCall {
            function,
            entry_point,
            source,
            arguments,
        } => {
            fingerprint.byte(22);
            fingerprint.u128(function.as_uuid().as_u128());
            fingerprint.bytes(entry_point.as_bytes());
            fingerprint.bytes(source.as_bytes());
            for argument in arguments {
                fingerprint.u32(argument.0);
            }
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
        MaterialIrInstruction::Select {
            condition,
            if_false,
            if_true,
        } => {
            fingerprint.byte(23);
            fingerprint.u32(condition.0);
            fingerprint.u32(if_false.0);
            fingerprint.u32(if_true.0);
        }
        MaterialIrInstruction::Remap {
            value,
            input_min,
            input_max,
            output_min,
            output_max,
        } => {
            fingerprint.byte(14);
            fingerprint.u32(value.0);
            fingerprint.u32(input_min.0);
            fingerprint.u32(input_max.0);
            fingerprint.u32(output_min.0);
            fingerprint.u32(output_max.0);
        }
        MaterialIrInstruction::Smoothstep {
            edge_min,
            edge_max,
            value,
        } => {
            fingerprint.byte(15);
            fingerprint.u32(edge_min.0);
            fingerprint.u32(edge_max.0);
            fingerprint.u32(value.0);
        }
        MaterialIrInstruction::Fresnel {
            normal,
            view,
            power,
        } => {
            fingerprint.byte(21);
            fingerprint.u32(normal.0);
            fingerprint.u32(view.0);
            fingerprint.u32(power.0);
        }
        MaterialIrInstruction::RadialMask {
            uv,
            center,
            radius,
            softness,
            invert,
        } => {
            fingerprint.byte(16);
            fingerprint.u32(uv.0);
            fingerprint.u32(center.0);
            fingerprint.u32(radius.0);
            fingerprint.u32(softness.0);
            fingerprint.u32(invert.0);
        }
        MaterialIrInstruction::Dissolve {
            source,
            threshold,
            edge_width,
            invert,
        } => {
            fingerprint.byte(17);
            fingerprint.u32(source.0);
            fingerprint.u32(threshold.0);
            fingerprint.u32(edge_width.0);
            fingerprint.u32(invert.0);
        }
        MaterialIrInstruction::DepthFade {
            scene_depth,
            pixel_depth,
            fade_distance,
            invert,
        } => {
            fingerprint.byte(19);
            fingerprint.u32(scene_depth.0);
            fingerprint.u32(pixel_depth.0);
            fingerprint.u32(fade_distance.0);
            fingerprint.u32(invert.0);
        }
        MaterialIrInstruction::SoftParticle {
            alpha,
            scene_depth,
            pixel_depth,
            fade_distance,
            invert,
        } => {
            fingerprint.byte(20);
            fingerprint.u32(alpha.0);
            fingerprint.u32(scene_depth.0);
            fingerprint.u32(pixel_depth.0);
            fingerprint.u32(fade_distance.0);
            fingerprint.u32(invert.0);
        }
        MaterialIrInstruction::DissolveEdge {
            source,
            threshold,
            edge_width,
            invert,
        } => {
            fingerprint.byte(18);
            fingerprint.u32(source.0);
            fingerprint.u32(threshold.0);
            fingerprint.u32(edge_width.0);
            fingerprint.u32(invert.0);
        }
        MaterialIrInstruction::SampleTexture {
            texture,
            uv,
            sampling,
        } => {
            fingerprint.byte(9);
            fingerprint.u32(texture.0);
            fingerprint.u32(uv.0);
            fingerprint.byte(match sampling {
                MaterialTextureSamplingMode::ImplicitDerivatives => 0,
                MaterialTextureSamplingMode::ExplicitLod { .. } => 1,
                MaterialTextureSamplingMode::ExplicitGradient { .. } => 2,
            });
            if let MaterialTextureSamplingMode::ExplicitLod { level } = sampling {
                fingerprint.u32(level.0);
            }
            if let MaterialTextureSamplingMode::ExplicitGradient { ddx, ddy } = sampling {
                fingerprint.u32(ddx.0);
                fingerprint.u32(ddy.0);
            }
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
        MaterialIrInstruction::PanUv { uv, speed, time } => {
            fingerprint.byte(11);
            fingerprint.u32(uv.0);
            fingerprint.u32(speed.0);
            fingerprint.u32(time.0);
        }
        MaterialIrInstruction::RotateUv { uv, center, angle } => {
            fingerprint.byte(12);
            fingerprint.u32(uv.0);
            fingerprint.u32(center.0);
            fingerprint.u32(angle.0);
        }
        MaterialIrInstruction::ScaleUv { uv, center, scale } => {
            fingerprint.byte(13);
            fingerprint.u32(uv.0);
            fingerprint.u32(center.0);
            fingerprint.u32(scale.0);
        }
        MaterialIrInstruction::DerivativeX { value } => {
            fingerprint.byte(24);
            fingerprint.u32(value.0);
        }
        MaterialIrInstruction::DerivativeY { value } => {
            fingerprint.byte(25);
            fingerprint.u32(value.0);
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
