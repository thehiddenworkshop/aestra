//! Actual sprite/mesh pipeline specialization, shared across engine graphs.
use super::{
    draw_instance::{GpuDrawInstance, GpuRenderMode, GpuSemanticMaterialBinding},
    mesh_inputs::MeshInputs,
};
use crate::material_layout::material_bind_group_layout;
use aestra_core::material::{MaterialCullMode, MaterialDepthTest};
use aestra_gpu::{
    GpuBlend, GpuParticle, GpuRenderGlobals, GpuRenderParams, GpuRenderer,
    material::{
        MaterialColorTargetFormat, MaterialPipelineKey, MaterialPipelineVariant,
        MaterialResourceLayout,
    },
};
use bevy::{
    core_pipeline::{core_2d::CORE_2D_DEPTH_FORMAT, core_3d::CORE_3D_DEPTH_FORMAT},
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MeshPipeline, MeshPipelineKey},
    prelude::*,
    render::render_resource::{
        BindGroupLayoutDescriptor, BindGroupLayoutEntries, BlendComponent, BlendFactor,
        BlendOperation, BlendState, ColorTargetState, ColorWrites, CompareFunction, DepthBiasState,
        DepthStencilState, Face, FragmentState, MultisampleState, PrimitiveState,
        PrimitiveTopology, RenderPipelineDescriptor, SamplerBindingType, ShaderStages, ShaderType,
        SpecializedRenderPipeline, StencilFaceState, StencilState, TextureFormat,
        TextureSampleType, VertexState,
        binding_types::{
            sampler, storage_buffer_read_only, texture_2d, texture_depth_2d,
            texture_depth_2d_multisampled, uniform_buffer,
        },
    },
    sprite_render::{Mesh2dPipeline, Mesh2dPipelineKey},
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct GpuSpritePipelineKey {
    pub(super) mesh_wireframe: bool,
    pub(super) mesh_layout: Option<MeshVertexBufferLayoutRef>,
    pub(super) view: GpuSpriteViewKey,
    pub(super) blend: GpuBlend,
    pub(super) render_mode: GpuRenderMode,
    pub(super) material: Option<GpuSemanticPipelineKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum GpuSpriteViewKey {
    TwoD(Mesh2dPipelineKey),
    ThreeD(MeshPipelineKey),
}

#[derive(Clone, Debug)]
pub(super) struct GpuSemanticPipelineKey {
    pub(super) key: MaterialPipelineKey,
    pub(super) shader: Handle<Shader>,
    pub(super) layout: std::sync::Arc<MaterialResourceLayout>,
    pub(super) requires_scene_depth: bool,
    pub(super) requires_scene_lighting: bool,
    pub(super) mesh_inputs: MeshInputs,
}

impl PartialEq for GpuSemanticPipelineKey {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && self.shader == other.shader
    }
}

impl Eq for GpuSemanticPipelineKey {}

impl std::hash::Hash for GpuSemanticPipelineKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::hash::Hash::hash(&self.key, state);
        std::hash::Hash::hash(&self.shader, state);
    }
}

#[derive(Resource)]
pub(super) struct GpuSpritePipeline {
    mesh2d: Mesh2dPipeline,
    mesh3d: MeshPipeline,
    pub(super) effect_layout: BindGroupLayoutDescriptor,
    pub(super) scene_depth_layout: BindGroupLayoutDescriptor,
    pub(super) multisampled_scene_depth_layout: BindGroupLayoutDescriptor,
    pub(super) shader: Handle<Shader>,
    mesh_wireframe_shader: Handle<Shader>,
}

#[derive(Clone, Copy, ShaderType)]
pub(super) struct MaterialSceneUniforms {
    pub(super) view_from_clip: Mat4,
    pub(super) viewport: Vec4,
}

fn scene_depth_bind_group_layout(multisampled: bool) -> BindGroupLayoutDescriptor {
    let depth = if multisampled {
        texture_depth_2d_multisampled()
    } else {
        texture_depth_2d()
    };
    BindGroupLayoutDescriptor::new(
        if multisampled {
            "aestra material multisampled scene depth"
        } else {
            "aestra material scene depth"
        },
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (uniform_buffer::<MaterialSceneUniforms>(false), depth),
        ),
    )
}

impl GpuSpritePipeline {
    pub(super) fn new(
        mesh2d: Mesh2dPipeline,
        mesh3d: MeshPipeline,
        shader: Handle<Shader>,
        mesh_wireframe_shader: Handle<Shader>,
    ) -> Self {
        let effect_layout = BindGroupLayoutDescriptor::new(
            "aestra_gpu_sprite",
            &BindGroupLayoutEntries::sequential(
                ShaderStages::VERTEX_FRAGMENT,
                (
                    storage_buffer_read_only::<Vec<GpuRenderer>>(false),
                    storage_buffer_read_only::<Vec<GpuParticle>>(false),
                    storage_buffer_read_only::<Vec<u32>>(false),
                    storage_buffer_read_only::<GpuRenderGlobals>(false),
                    storage_buffer_read_only::<GpuRenderParams>(false),
                    texture_2d(TextureSampleType::Float { filterable: true }),
                    sampler(SamplerBindingType::Filtering),
                    storage_buffer_read_only::<Vec<u32>>(false),
                ),
            ),
        );
        let scene_depth_layout = scene_depth_bind_group_layout(false);
        let multisampled_scene_depth_layout = scene_depth_bind_group_layout(true);
        Self {
            mesh2d,
            mesh3d,
            effect_layout,
            scene_depth_layout,
            multisampled_scene_depth_layout,
            shader,
            mesh_wireframe_shader,
        }
    }
}

impl SpecializedRenderPipeline for GpuSpritePipeline {
    type Key = GpuSpritePipelineKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        let shader_defs = if matches!(key.view, GpuSpriteViewKey::ThreeD(_))
            && key
                .material
                .as_ref()
                .is_some_and(|material| material.requires_scene_lighting)
        {
            vec!["AESTRA_SCENE_POINT_LIGHTING".into()]
        } else {
            vec![]
        };
        let (view_layout, target_format, depth_format, msaa_samples) = match key.view {
            GpuSpriteViewKey::TwoD(mesh) => (
                self.mesh2d.view_layout.clone(),
                mesh.target_format(),
                CORE_2D_DEPTH_FORMAT,
                mesh.msaa_samples(),
            ),
            GpuSpriteViewKey::ThreeD(mesh) => (
                self.mesh3d.get_view_layout(mesh.into()).main_layout,
                mesh.target_format(),
                CORE_3D_DEPTH_FORMAT,
                mesh.msaa_samples(),
            ),
        };
        let render_state = key
            .material
            .as_ref()
            .map(|material| material.key.render_state);
        let blend_mode = render_state.map_or(key.blend, |state| match state.blend {
            aestra_core::BlendMode::Alpha => GpuBlend::Alpha,
            aestra_core::BlendMode::Additive => GpuBlend::Additive,
            aestra_core::BlendMode::Multiply => GpuBlend::Multiply,
        });
        let blend = match key.render_mode {
            GpuRenderMode::Wireframe => BlendState::ALPHA_BLENDING,
            GpuRenderMode::Rendered => match blend_mode {
                GpuBlend::Alpha => BlendState::ALPHA_BLENDING,
                GpuBlend::Additive => BlendState {
                    color: BlendComponent {
                        src_factor: BlendFactor::SrcAlpha,
                        dst_factor: BlendFactor::One,
                        operation: BlendOperation::Add,
                    },
                    alpha: BlendComponent {
                        src_factor: BlendFactor::One,
                        dst_factor: BlendFactor::One,
                        operation: BlendOperation::Add,
                    },
                },
                GpuBlend::Multiply => BlendState {
                    color: BlendComponent {
                        src_factor: BlendFactor::Dst,
                        dst_factor: BlendFactor::Zero,
                        operation: BlendOperation::Add,
                    },
                    alpha: BlendComponent::OVER,
                },
            },
        };
        let mut layout = vec![view_layout, self.effect_layout.clone()];
        if let Some(material) = &key.material {
            layout.push(material_bind_group_layout(&material.layout));
            if material.requires_scene_depth {
                layout.push(if msaa_samples > 1 {
                    self.multisampled_scene_depth_layout.clone()
                } else {
                    self.scene_depth_layout.clone()
                });
            }
        }
        let fragment_shader = if key.mesh_wireframe {
            key.material.as_ref().map_or_else(
                || self.mesh_wireframe_shader.clone(),
                |material| material.shader.clone(),
            )
        } else {
            key.material
                .as_ref()
                .filter(|_| key.render_mode == GpuRenderMode::Rendered)
                .map_or_else(|| self.shader.clone(), |material| material.shader.clone())
        };
        let fragment_entry = if key.mesh_wireframe {
            "fragment_mesh_wireframe"
        } else if key.material.is_some() && key.render_mode == GpuRenderMode::Rendered {
            aestra_gpu::material::MATERIAL_FRAGMENT_ENTRY_POINT
        } else {
            match key.render_mode {
                GpuRenderMode::Wireframe => "fragment_wireframe",
                GpuRenderMode::Rendered => match blend_mode {
                    GpuBlend::Alpha => "fragment_alpha",
                    GpuBlend::Additive => "fragment_additive",
                    GpuBlend::Multiply => "fragment_multiply",
                },
            }
        };
        // Bevy 0.20 adds per-stage pipeline constants; keep their defaults on both graphs.
        #[allow(
            clippy::needless_update,
            reason = "shared with Bevy 0.20's extra stage fields"
        )]
        RenderPipelineDescriptor {
            label: Some("aestra gpu sprite".into()),
            layout,
            vertex: VertexState {
                shader_defs: shader_defs.clone(),
                // Semantic modules contain both stages with one matching varying layout.
                shader: fragment_shader.clone(),
                entry_point: Some(
                    if key.mesh_wireframe {
                        "vertex_mesh_wireframe"
                    } else {
                        "vertex"
                    }
                    .into(),
                ),
                buffers: if key.mesh_wireframe {
                    vec![bevy::mesh::VertexBufferLayout {
                        array_stride: 56,
                        step_mode: bevy::render::render_resource::VertexStepMode::Vertex,
                        attributes: vec![
                            bevy::render::render_resource::VertexAttribute {
                                format: bevy::render::render_resource::VertexFormat::Float32x3,
                                offset: 0,
                                shader_location: 0,
                            },
                            bevy::render::render_resource::VertexAttribute {
                                format: bevy::render::render_resource::VertexFormat::Float32x3,
                                offset: 12,
                                shader_location: 1,
                            },
                            bevy::render::render_resource::VertexAttribute {
                                format: bevy::render::render_resource::VertexFormat::Float32x2,
                                offset: 24,
                                shader_location: 2,
                            },
                            bevy::render::render_resource::VertexAttribute {
                                format: bevy::render::render_resource::VertexFormat::Float32x2,
                                offset: 32,
                                shader_location: 3,
                            },
                            bevy::render::render_resource::VertexAttribute {
                                format: bevy::render::render_resource::VertexFormat::Float32x4,
                                offset: 40,
                                shader_location: 4,
                            },
                        ],
                    }]
                } else {
                    key.mesh_layout
                        .as_ref()
                        .map(|layout| {
                            layout
                                .0
                                .get_layout(
                                    &key.material
                                        .as_ref()
                                        .map_or_else(MeshInputs::default, |material| {
                                            material.mesh_inputs
                                        })
                                        .attributes(),
                                )
                                .expect("mesh attributes validated before queueing")
                        })
                        .into_iter()
                        .collect()
                },
                ..default()
            },
            fragment: Some(FragmentState {
                shader_defs,
                shader: fragment_shader,
                entry_point: Some(fragment_entry.into()),
                targets: vec![Some(ColorTargetState {
                    format: target_format,
                    blend: Some(blend),
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            primitive: PrimitiveState {
                // Billboards (sprite/ribbon/trail — no mesh layout) draw as a
                // four-vertex triangle strip, so the GPU runs one vertex invocation
                // per quad corner instead of six; host meshes keep their indexed
                // TriangleList geometry.
                topology: if key.mesh_wireframe {
                    PrimitiveTopology::LineList
                } else if key.mesh_layout.is_some() {
                    PrimitiveTopology::TriangleList
                } else {
                    PrimitiveTopology::TriangleStrip
                },
                cull_mode: if key.mesh_wireframe {
                    None
                } else {
                    render_state.map_or(Some(Face::Back), |state| match state.cull_mode {
                        MaterialCullMode::None => None,
                        MaterialCullMode::Front => Some(Face::Front),
                        MaterialCullMode::Back => Some(Face::Back),
                    })
                },
                ..default()
            },
            depth_stencil: Some(DepthStencilState {
                format: depth_format,
                depth_write_enabled: Some(render_state.is_some_and(|state| state.depth_write)),
                depth_compare: Some(render_state.map_or(CompareFunction::GreaterEqual, |state| {
                    match state.depth_test {
                        MaterialDepthTest::Disabled | MaterialDepthTest::Always => {
                            CompareFunction::Always
                        }
                        // Bevy's main view uses reverse-Z depth.
                        MaterialDepthTest::Less => CompareFunction::Greater,
                        MaterialDepthTest::LessEqual => CompareFunction::GreaterEqual,
                    }
                })),
                stencil: StencilState {
                    front: StencilFaceState::IGNORE,
                    back: StencilFaceState::IGNORE,
                    read_mask: 0,
                    write_mask: 0,
                },
                bias: DepthBiasState::default(),
            }),
            multisample: MultisampleState {
                count: msaa_samples,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            ..default()
        }
    }
}

pub(super) fn material_for_draw(effect: &GpuDrawInstance) -> Option<&GpuSemanticMaterialBinding> {
    effect.semantic_material.as_ref().filter(|material| {
        effect.render_mode == GpuRenderMode::Rendered
            || (effect.mesh.is_some() && material.program.has_vertex_offset)
    })
}

pub(super) fn draw_pipeline_key(
    effect: &GpuDrawInstance,
    target_format: TextureFormat,
    sample_count: u32,
    feature_bits: u64,
) -> Option<GpuSemanticPipelineKey> {
    let mut key = semantic_pipeline_key(
        material_for_draw(effect),
        target_format,
        sample_count,
        feature_bits,
    )?;
    if effect.render_mode == GpuRenderMode::Wireframe {
        // The diagnostic fragment never samples scene depth, even when the rendered fragment does.
        key.requires_scene_depth = false;
    }
    Some(key)
}

pub(super) fn semantic_pipeline_key(
    binding: Option<&GpuSemanticMaterialBinding>,
    target_format: TextureFormat,
    sample_count: u32,
    feature_bits: u64,
) -> Option<GpuSemanticPipelineKey> {
    let binding = binding?;
    let variant = MaterialPipelineVariant {
        target_format: portable_target_format(target_format),
        sample_count,
        feature_bits,
    };
    let key = binding
        .program
        .pipeline_key(binding.render_state, variant)
        .expect("runtime bindings validate their material render state");
    let requires_scene_depth = binding.program.requires_scene_depth();
    Some(GpuSemanticPipelineKey {
        key,
        shader: if requires_scene_depth && sample_count > 1 {
            binding.multisampled_shader.clone()
        } else {
            binding.shader.clone()
        },
        layout: std::sync::Arc::new(binding.program.resource_layout.clone()),
        requires_scene_depth,
        requires_scene_lighting: binding.program.requires_scene_lighting(),
        mesh_inputs: MeshInputs::for_program(&binding.program),
    })
}

pub(super) const fn portable_target_format(format: TextureFormat) -> MaterialColorTargetFormat {
    match format {
        TextureFormat::Rgba8UnormSrgb => MaterialColorTargetFormat::Rgba8UnormSrgb,
        TextureFormat::Bgra8UnormSrgb => MaterialColorTargetFormat::Bgra8UnormSrgb,
        TextureFormat::Rgba16Float => MaterialColorTargetFormat::Rgba16Float,
        _ => MaterialColorTargetFormat::Other(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_compiler::MaterialCompiler;
    use aestra_core::material::{MaterialProgram, MaterialRenderState};
    use aestra_gpu::material::{MaterialBackendCapabilities, MaterialShaderCompiler};

    #[test]
    fn semantic_pipeline_identity_ignores_instance_uniform_bytes() {
        let program = MaterialProgram::additive_sprite("Pipeline identity");
        let ir = MaterialCompiler.compile(&program).unwrap();
        let program = std::sync::Arc::new(
            MaterialShaderCompiler
                .compile(&ir, &MaterialBackendCapabilities::portable_minimum())
                .unwrap(),
        );
        let binding = |uniforms: &[u8]| GpuSemanticMaterialBinding {
            program: program.clone(),
            render_state: MaterialRenderState::additive_sprite(),
            shader: Handle::default(),
            multisampled_shader: Handle::default(),
            uniforms: std::sync::Arc::from(uniforms),
            textures: Vec::new(),
            fallback_texture: Handle::default(),
        };
        let first = binding(&[0; 16]);
        let second = binding(&[255; 16]);

        assert_eq!(
            semantic_pipeline_key(Some(&first), TextureFormat::Bgra8UnormSrgb, 4, 0),
            semantic_pipeline_key(Some(&second), TextureFormat::Bgra8UnormSrgb, 4, 0),
        );
    }
}
