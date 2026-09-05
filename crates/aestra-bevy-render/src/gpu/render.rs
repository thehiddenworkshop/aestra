use super::{
    GpuBlend, GpuDrawInstance, GpuParticle, GpuRenderGlobals, GpuRenderMode, GpuRenderParams,
    GpuRenderer, GpuSemanticMaterialBinding, WESL_MESH_WIREFRAME_SHADER_PATH,
    WESL_RENDER_SHADER_PATH,
};
use crate::material::{bevy_sampler_descriptor, material_bind_group_layout};
use aestra_core::material::{MaterialCullMode, MaterialDepthTest};
use aestra_gpu::material::{
    MaterialColorTargetFormat, MaterialPipelineKey, MaterialPipelineVariant, MaterialResourceLayout,
};
use bevy::{
    app::SubApp,
    core_pipeline::{
        core_2d::{CORE_2D_DEPTH_FORMAT, Transparent2d},
        core_3d::{CORE_3D_DEPTH_FORMAT, Transparent3d, TransparentSortingInfo3d},
        prepass::ViewPrepassTextures,
    },
    ecs::{
        query::ROQueryItem,
        system::{SystemParamItem, lifetimeless::*},
    },
    math::FloatOrd,
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MeshPipeline, MeshPipelineKey, MeshPipelineSystems, SetMeshViewBindGroup, ViewKeyCache},
    prelude::*,
    render::{
        Render, RenderStartup, RenderSystems,
        mesh::{RenderMesh, RenderMeshBufferInfo, allocator::MeshAllocator},
        render_asset::RenderAssets,
        render_phase::{
            AddRenderCommand, DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand,
            RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
        },
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntries, BindingResource, BlendComponent, BlendFactor, BlendOperation,
            BlendState, Buffer, BufferInitDescriptor, BufferUsages, ColorTargetState, ColorWrites,
            CompareFunction, DepthBiasState, DepthStencilState, Face, FragmentState, IndexFormat,
            MultisampleState, PipelineCache, PrimitiveState, PrimitiveTopology,
            RenderPipelineDescriptor, Sampler, SamplerBindingType, ShaderStages, ShaderType,
            SpecializedRenderPipeline, SpecializedRenderPipelines, StencilFaceState, StencilState,
            TextureFormat, TextureSampleType, VertexState,
            binding_types::{
                sampler, storage_buffer_read_only, texture_2d, texture_depth_2d,
                texture_depth_2d_multisampled, uniform_buffer,
            },
            encase::UniformBuffer,
        },
        renderer::{RenderDevice, RenderQueue},
        storage::GpuShaderBuffer,
        sync_world::MainEntity,
        texture::GpuImage,
        view::{ExtractedView, RenderVisibleEntities},
    },
    sprite_render::{
        Mesh2dPipeline, Mesh2dPipelineKey, SetMesh2dViewBindGroup, init_mesh_2d_pipeline,
    },
};

pub(super) fn install(render_app: &mut SubApp) {
    render_app
        .add_render_command::<Transparent2d, DrawGpuSprites>()
        .add_render_command::<Transparent2d, DrawSemanticGpuSprites>()
        .add_render_command::<Transparent3d, DrawGpuSprites3d>()
        .add_render_command::<Transparent3d, DrawSemanticGpuSprites3d>()
        .add_render_command::<Transparent3d, DrawSemanticDepthGpuSprites3d>()
        .init_resource::<SpecializedRenderPipelines<GpuSpritePipeline>>()
        .add_systems(
            Render,
            prepare_mesh_draws
                .after(RenderSystems::PrepareMeshes)
                .before(RenderSystems::Queue),
        )
        .add_systems(
            RenderStartup,
            init_render_pipeline
                .after(init_mesh_2d_pipeline)
                .after(MeshPipelineSystems),
        )
        .add_systems(
            Render,
            (
                prepare_render_bind_groups.in_set(RenderSystems::PrepareBindGroups),
                prepare_scene_depth_bind_groups.in_set(RenderSystems::PrepareBindGroups),
                queue_gpu_sprites.in_set(RenderSystems::QueueMeshes),
                queue_gpu_sprites_3d.in_set(RenderSystems::QueueMeshes),
            ),
        );
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct GpuSpritePipelineKey {
    mesh_wireframe: bool,
    mesh_layout: Option<MeshVertexBufferLayoutRef>,
    view: GpuSpriteViewKey,
    blend: GpuBlend,
    render_mode: GpuRenderMode,
    material: Option<GpuSemanticPipelineKey>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum GpuSpriteViewKey {
    TwoD(Mesh2dPipelineKey),
    ThreeD(MeshPipelineKey),
}

#[derive(Clone, Debug)]
struct GpuSemanticPipelineKey {
    key: MaterialPipelineKey,
    shader: Handle<Shader>,
    layout: std::sync::Arc<MaterialResourceLayout>,
    requires_scene_depth: bool,
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
struct GpuSpritePipeline {
    mesh2d: Mesh2dPipeline,
    mesh3d: MeshPipeline,
    effect_layout: BindGroupLayoutDescriptor,
    scene_depth_layout: BindGroupLayoutDescriptor,
    multisampled_scene_depth_layout: BindGroupLayoutDescriptor,
    shader: Handle<Shader>,
    mesh_wireframe_shader: Handle<Shader>,
}

#[derive(Clone, Copy, ShaderType)]
struct MaterialSceneUniforms {
    view_from_clip: Mat4,
    viewport: Vec4,
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

fn init_render_pipeline(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mesh2d: Res<Mesh2dPipeline>,
    mesh3d: Res<MeshPipeline>,
) {
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
            ),
        ),
    );
    let scene_depth_layout = scene_depth_bind_group_layout(false);
    let multisampled_scene_depth_layout = scene_depth_bind_group_layout(true);
    commands.insert_resource(GpuSpritePipeline {
        mesh2d: mesh2d.clone(),
        mesh3d: mesh3d.clone(),
        effect_layout,
        scene_depth_layout,
        multisampled_scene_depth_layout,
        shader: asset_server.load(WESL_RENDER_SHADER_PATH),
        mesh_wireframe_shader: asset_server.load(WESL_MESH_WIREFRAME_SHADER_PATH),
    });
}

impl SpecializedRenderPipeline for GpuSpritePipeline {
    type Key = GpuSpritePipelineKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
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
        RenderPipelineDescriptor {
            label: Some("aestra gpu sprite".into()),
            layout,
            vertex: VertexState {
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
                        array_stride: 32,
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
                        ],
                    }]
                } else {
                    key.mesh_layout
                        .as_ref()
                        .map(|layout| {
                            layout
                                .0
                                .get_layout(&[
                                    Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
                                    Mesh::ATTRIBUTE_NORMAL.at_shader_location(1),
                                    Mesh::ATTRIBUTE_UV_0.at_shader_location(2),
                                ])
                                .expect("mesh attributes validated before queueing")
                        })
                        .into_iter()
                        .collect()
                },
                ..default()
            },
            fragment: Some(FragmentState {
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
                topology: if key.mesh_wireframe {
                    PrimitiveTopology::LineList
                } else {
                    PrimitiveTopology::TriangleList
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

/// Renderer-local geometry command; only its instance count comes from simulation.
#[derive(Component)]
pub(super) struct PreparedMeshDraw {
    pub(super) indirect: Buffer,
    vertex: Buffer,
    index: Option<(Buffer, IndexFormat)>,
    layout: Option<MeshVertexBufferLayoutRef>,
    wireframe: Option<std::sync::Arc<super::wireframe::WireframeGeometry>>,
}

fn prepare_mesh_draws(
    mut commands: Commands,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    meshes: Res<RenderAssets<RenderMesh>>,
    allocator: Res<MeshAllocator>,
    draws: Query<(Entity, &GpuDrawInstance, Option<&PreparedMeshDraw>)>,
) {
    for (entity, draw, previous) in &draws {
        let Some(handle) = &draw.mesh else {
            continue;
        };
        if draw.render_mode == GpuRenderMode::Wireframe {
            let Some(geometry) = &draw.wireframe_geometry else {
                commands.entity(entity).remove::<PreparedMeshDraw>();
                continue;
            };
            if previous
                .and_then(|previous| previous.wireframe.as_ref())
                .is_some_and(|previous| std::sync::Arc::ptr_eq(previous, geometry))
            {
                continue;
            }
            let positions = geometry
                .vertices
                .iter()
                .flatten()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>();
            let indices = geometry
                .indices
                .iter()
                .flat_map(|i| i.to_le_bytes())
                .collect::<Vec<_>>();
            let words = [geometry.indices.len() as u32, 0, 0, 0, 0];
            let indirect = device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("aestra mesh wireframe indirect"),
                contents: &words
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
                usage: BufferUsages::INDIRECT | BufferUsages::COPY_DST,
            });
            commands.entity(entity).insert(PreparedMeshDraw {
                indirect,
                vertex: device.create_buffer_with_data(&BufferInitDescriptor {
                    label: Some("aestra mesh wireframe positions"),
                    contents: &positions,
                    usage: BufferUsages::VERTEX,
                }),
                index: Some((
                    device.create_buffer_with_data(&BufferInitDescriptor {
                        label: Some("aestra mesh wireframe edges"),
                        contents: &indices,
                        usage: BufferUsages::INDEX,
                    }),
                    IndexFormat::Uint32,
                )),
                layout: None,
                wireframe: Some(geometry.clone()),
            });
            continue;
        }
        let Some(mesh) = meshes.get(handle) else {
            commands.entity(entity).remove::<PreparedMeshDraw>();
            continue;
        };
        if mesh.primitive_topology() != PrimitiveTopology::TriangleList
            || mesh
                .layout
                .0
                .get_layout(&[
                    Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
                    Mesh::ATTRIBUTE_NORMAL.at_shader_location(1),
                    Mesh::ATTRIBUTE_UV_0.at_shader_location(2),
                ])
                .is_err()
        {
            bevy::log::warn_once!(
                "Aestra mesh particles require TriangleList geometry with position, normal and UV0 attributes"
            );
            commands.entity(entity).remove::<PreparedMeshDraw>();
            continue;
        }
        let Some(vertices) = allocator.mesh_vertex_slice(&handle.id()) else {
            commands.entity(entity).remove::<PreparedMeshDraw>();
            continue;
        };
        let (words, index) = match mesh.buffer_info {
            RenderMeshBufferInfo::Indexed {
                count,
                index_format,
            } => {
                let Some(indices) = allocator.mesh_index_slice(&handle.id()) else {
                    commands.entity(entity).remove::<PreparedMeshDraw>();
                    continue;
                };
                (
                    [count, 0, indices.range.start, vertices.range.start, 0],
                    Some((indices.buffer.clone(), index_format)),
                )
            }
            RenderMeshBufferInfo::NonIndexed => (
                [vertices.range.len() as u32, 0, vertices.range.start, 0, 0],
                None,
            ),
        };
        let bytes = words
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        let indirect = if let Some(previous) = previous {
            queue.write_buffer(&previous.indirect, 0, &bytes);
            previous.indirect.clone()
        } else {
            device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("aestra mesh indirect"),
                contents: &bytes,
                usage: BufferUsages::INDIRECT | BufferUsages::COPY_DST,
            })
        };
        commands.entity(entity).insert(PreparedMeshDraw {
            indirect,
            vertex: vertices.buffer.clone(),
            index,
            layout: Some(mesh.layout.clone()),
            wireframe: None,
        });
    }
}

#[derive(Component)]
struct GpuRenderBindGroup(BindGroup);

#[derive(Component)]
struct GpuMaterialBindGroup(BindGroup);

#[derive(Component)]
struct GpuSceneDepthBindGroup(BindGroup);

fn prepare_scene_depth_bind_groups(
    mut commands: Commands,
    pipeline: Res<GpuSpritePipeline>,
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    views: Query<(Entity, &ExtractedView, &ViewPrepassTextures, &Msaa)>,
) {
    for (entity, view, prepass, msaa) in &views {
        let Some(depth_view) = prepass.depth_view() else {
            commands.entity(entity).remove::<GpuSceneDepthBindGroup>();
            continue;
        };
        let mut encoded = UniformBuffer::new(Vec::new());
        encoded
            .write(&MaterialSceneUniforms {
                view_from_clip: view.clip_from_view.inverse(),
                viewport: view.viewport.as_vec4(),
            })
            .expect("material scene uniforms have a fixed valid layout");
        let buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("aestra material scene uniforms"),
            contents: &encoded.into_inner(),
            usage: BufferUsages::UNIFORM,
        });
        let descriptor = if msaa.samples() > 1 {
            &pipeline.multisampled_scene_depth_layout
        } else {
            &pipeline.scene_depth_layout
        };
        let bind_group = render_device.create_bind_group(
            Some("aestra material scene depth"),
            &pipeline_cache.get_bind_group_layout(descriptor),
            &BindGroupEntries::sequential((buffer.as_entire_buffer_binding(), depth_view)),
        );
        commands
            .entity(entity)
            .insert(GpuSceneDepthBindGroup(bind_group));
    }
}

fn prepare_render_bind_groups(
    mut commands: Commands,
    pipeline: Res<GpuSpritePipeline>,
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    buffers: Res<RenderAssets<GpuShaderBuffer>>,
    images: Res<RenderAssets<GpuImage>>,
    effects: Query<(Entity, &GpuDrawInstance)>,
) {
    for (entity, effect) in &effects {
        let Some(renderers) = buffers.get(&effect.renderers) else {
            continue;
        };
        let Some(particles) = buffers.get(&effect.particles) else {
            continue;
        };
        let Some(alive) = buffers.get(&effect.alive) else {
            continue;
        };
        let Some(globals) = buffers.get(&effect.render_globals) else {
            continue;
        };
        let Some(params) = buffers.get(&effect.render_params) else {
            continue;
        };
        let Some(image) = images
            .get(&effect.texture)
            .or_else(|| images.get(&effect.fallback_texture))
        else {
            continue;
        };
        let bind_group = render_device.create_bind_group(
            Some("aestra gpu sprite"),
            &pipeline_cache.get_bind_group_layout(&pipeline.effect_layout),
            &BindGroupEntries::sequential((
                renderers.buffer.as_entire_buffer_binding(),
                particles.buffer.as_entire_buffer_binding(),
                alive.buffer.as_entire_buffer_binding(),
                globals.buffer.as_entire_buffer_binding(),
                params.buffer.as_entire_buffer_binding(),
                &image.texture_view,
                &image.sampler,
            )),
        );
        commands
            .entity(entity)
            .insert(GpuRenderBindGroup(bind_group));
        let Some(material) = &effect.semantic_material else {
            commands.entity(entity).remove::<GpuMaterialBindGroup>();
            continue;
        };
        let descriptor = material_bind_group_layout(&material.program.resource_layout);
        let uniform_buffer = (!material.uniforms.is_empty()).then(|| {
            render_device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("aestra semantic material uniforms"),
                contents: &material.uniforms,
                usage: BufferUsages::UNIFORM,
            })
        });
        let resolved_images = material
            .textures
            .iter()
            .map(|texture| {
                images
                    .get(texture)
                    .or_else(|| images.get(&material.fallback_texture))
            })
            .collect::<Option<Vec<_>>>();
        let Some(resolved_images) = resolved_images else {
            continue;
        };
        let samplers = material
            .program
            .resource_layout
            .samplers
            .iter()
            .map(|slot| render_device.create_sampler(&bevy_sampler_descriptor(slot.descriptor)))
            .collect::<Vec<Sampler>>();
        let mut entries = Vec::with_capacity(descriptor.entries.len());
        if let (Some(binding), Some(buffer)) = (
            material.program.resource_layout.uniforms.binding,
            uniform_buffer.as_ref(),
        ) {
            entries.push(BindGroupEntry {
                binding,
                resource: BindingResource::Buffer(buffer.as_entire_buffer_binding()),
            });
        }
        for (slot, image) in material
            .program
            .resource_layout
            .textures
            .iter()
            .zip(&resolved_images)
        {
            entries.push(BindGroupEntry {
                binding: slot.binding,
                resource: BindingResource::TextureView(&image.texture_view),
            });
        }
        for (slot, sampler) in material
            .program
            .resource_layout
            .samplers
            .iter()
            .zip(&samplers)
        {
            entries.push(BindGroupEntry {
                binding: slot.binding,
                resource: BindingResource::Sampler(sampler),
            });
        }
        entries.sort_by_key(|entry| entry.binding);
        let bind_group = render_device.create_bind_group(
            Some("aestra semantic material"),
            &pipeline_cache.get_bind_group_layout(&descriptor),
            &entries,
        );
        commands
            .entity(entity)
            .insert(GpuMaterialBindGroup(bind_group));
    }
}

fn queue_gpu_sprites(
    draw_functions: Res<DrawFunctions<Transparent2d>>,
    pipeline: Res<GpuSpritePipeline>,
    mut pipelines: ResMut<SpecializedRenderPipelines<GpuSpritePipeline>>,
    pipeline_cache: Res<PipelineCache>,
    effects: Query<(&GpuDrawInstance, Option<&PreparedMeshDraw>)>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent2d>>,
    views: Query<(&RenderVisibleEntities, &ExtractedView, &Msaa)>,
) {
    let _span = tracing::info_span!("aestra::gpu::queue_sprites").entered();
    let draw_functions = draw_functions.read();
    let legacy_draw_function = draw_functions.id::<DrawGpuSprites>();
    let semantic_draw_function = draw_functions.id::<DrawSemanticGpuSprites>();
    for (visible_entities, view, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Some(visible_entities) = visible_entities.get::<GpuDrawInstance>() else {
            continue;
        };
        let mesh_key = Mesh2dPipelineKey::from_msaa_samples(msaa.samples())
            | Mesh2dPipelineKey::from_target_format(view.target_format);
        for (render_entity, main_entity) in visible_entities.iter_visible() {
            let Ok((effect, mesh)) = effects.get(*render_entity) else {
                continue;
            };
            if effect.mesh.is_some()
                && (mesh.is_none()
                    || (effect.semantic_material.is_none()
                        && effect.render_mode == GpuRenderMode::Rendered))
            {
                continue;
            }
            let material = draw_pipeline_key(effect, view.target_format, msaa.samples(), 0);
            // Scene-depth materials require the 3D depth prepass. Keeping this
            // unsupported in 2D is preferable to sampling the active depth
            // attachment, which is invalid on portable WebGPU backends.
            if material
                .as_ref()
                .is_some_and(|material| material.requires_scene_depth)
            {
                continue;
            }
            let pipeline_id = pipelines.specialize(
                &pipeline_cache,
                &pipeline,
                GpuSpritePipelineKey {
                    mesh_wireframe: effect.mesh.is_some()
                        && effect.render_mode == GpuRenderMode::Wireframe,
                    mesh_layout: mesh.and_then(|mesh| mesh.layout.clone()),
                    view: GpuSpriteViewKey::TwoD(mesh_key),
                    blend: effect.blend,
                    render_mode: effect.render_mode,
                    material,
                },
            );
            phase.add_retained(Transparent2d {
                sort_key: FloatOrd(effect.renderer_order as f32),
                entity: (*render_entity, *main_entity),
                pipeline: pipeline_id,
                draw_function: if material_for_draw(effect).is_some() {
                    semantic_draw_function
                } else {
                    legacy_draw_function
                },
                batch_range: 0..1,
                extracted_index: usize::MAX,
                extra_index: PhaseItemExtraIndex::None,
                indexed: mesh.is_some_and(|mesh| mesh.index.is_some()),
            });
        }
    }
}

fn queue_gpu_sprites_3d(
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    pipeline_resources: (
        Res<GpuSpritePipeline>,
        ResMut<SpecializedRenderPipelines<GpuSpritePipeline>>,
        Res<PipelineCache>,
        Res<ViewKeyCache>,
    ),
    effects: Query<(&GpuDrawInstance, Option<&PreparedMeshDraw>)>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    views: Query<(&RenderVisibleEntities, &ExtractedView)>,
) {
    let (pipeline, mut pipelines, pipeline_cache, view_key_cache) = pipeline_resources;
    let draw_functions = draw_functions.read();
    let legacy_draw_function = draw_functions.id::<DrawGpuSprites3d>();
    let semantic_draw_function = draw_functions.id::<DrawSemanticGpuSprites3d>();
    let semantic_depth_draw_function = draw_functions.id::<DrawSemanticDepthGpuSprites3d>();
    for (visible_entities, view) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Some(&mesh_key) = view_key_cache.get(&view.retained_view_entity) else {
            continue;
        };
        for (render_entity, main_entity) in visible_gpu_draws(visible_entities) {
            let Ok((effect, mesh)) = effects.get(render_entity) else {
                continue;
            };
            if effect.mesh.is_some()
                && (mesh.is_none()
                    || (effect.semantic_material.is_none()
                        && effect.render_mode == GpuRenderMode::Rendered))
            {
                continue;
            }
            let material =
                draw_pipeline_key(effect, view.target_format, mesh_key.msaa_samples(), 1);
            let requires_scene_depth = material
                .as_ref()
                .is_some_and(|material| material.requires_scene_depth);
            let pipeline_id = pipelines.specialize(
                &pipeline_cache,
                &pipeline,
                GpuSpritePipelineKey {
                    mesh_wireframe: effect.mesh.is_some()
                        && effect.render_mode == GpuRenderMode::Wireframe,
                    mesh_layout: mesh.and_then(|mesh| mesh.layout.clone()),
                    view: GpuSpriteViewKey::ThreeD(mesh_key),
                    blend: effect.blend,
                    render_mode: effect.render_mode,
                    material,
                },
            );
            phase.add_retained(Transparent3d {
                sorting_info: gpu_draw_sorting_info(effect.mesh_center, effect.renderer_order),
                entity: (render_entity, main_entity),
                pipeline: pipeline_id,
                draw_function: if requires_scene_depth
                    && effect.render_mode == GpuRenderMode::Rendered
                {
                    semantic_depth_draw_function
                } else if material_for_draw(effect).is_some() {
                    semantic_draw_function
                } else {
                    legacy_draw_function
                },
                distance: 0.0,
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::None,
                indexed: mesh.is_some_and(|mesh| mesh.index.is_some()),
            });
        }
    }
}

fn material_for_draw(effect: &GpuDrawInstance) -> Option<&GpuSemanticMaterialBinding> {
    effect.semantic_material.as_ref().filter(|material| {
        effect.render_mode == GpuRenderMode::Rendered
            || (effect.mesh.is_some() && material.program.has_vertex_offset)
    })
}

fn draw_pipeline_key(
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

fn semantic_pipeline_key(
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
    })
}

const fn portable_target_format(format: TextureFormat) -> MaterialColorTargetFormat {
    match format {
        TextureFormat::Rgba8UnormSrgb => MaterialColorTargetFormat::Rgba8UnormSrgb,
        TextureFormat::Bgra8UnormSrgb => MaterialColorTargetFormat::Bgra8UnormSrgb,
        TextureFormat::Rgba16Float => MaterialColorTargetFormat::Rgba16Float,
        _ => MaterialColorTargetFormat::Other(0),
    }
}

const RENDERER_ORDER_DEPTH_BIAS: f32 = 0.0001;

fn visible_gpu_draws(
    visible_entities: &RenderVisibleEntities,
) -> impl Iterator<Item = (Entity, MainEntity)> + '_ {
    visible_entities
        .get::<GpuDrawInstance>()
        .into_iter()
        .flat_map(|class| class.iter_visible())
        .map(|(render_entity, main_entity)| (*render_entity, *main_entity))
}

fn gpu_draw_sorting_info(mesh_center: Vec3, renderer_order: u32) -> TransparentSortingInfo3d {
    TransparentSortingInfo3d::Sorted {
        mesh_center,
        depth_bias: renderer_order as f32 * RENDERER_ORDER_DEPTH_BIAS,
    }
}

type DrawGpuSprites = (
    SetItemPipeline,
    SetMesh2dViewBindGroup<0>,
    SetGpuRenderBindGroup<1>,
    DrawGpuSpritesIndirect,
);

type DrawSemanticGpuSprites = (
    SetItemPipeline,
    SetMesh2dViewBindGroup<0>,
    SetGpuRenderBindGroup<1>,
    SetGpuMaterialBindGroup<2>,
    DrawGpuSpritesIndirect,
);

type DrawGpuSprites3d = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetGpuRenderBindGroup<1>,
    DrawGpuSpritesIndirect,
);

type DrawSemanticGpuSprites3d = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetGpuRenderBindGroup<1>,
    SetGpuMaterialBindGroup<2>,
    DrawGpuSpritesIndirect,
);

type DrawSemanticDepthGpuSprites3d = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetGpuRenderBindGroup<1>,
    SetGpuMaterialBindGroup<2>,
    SetGpuSceneDepthBindGroup<3>,
    DrawGpuSpritesIndirect,
);

struct SetGpuRenderBindGroup<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetGpuRenderBindGroup<I> {
    type Param = ();
    type ViewQuery = ();
    type ItemQuery = Read<GpuRenderBindGroup>;

    fn render<'w>(
        _item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        bind_group: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        _param: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(bind_group) = bind_group else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(I, &bind_group.0, &[]);
        RenderCommandResult::Success
    }
}

struct SetGpuMaterialBindGroup<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetGpuMaterialBindGroup<I> {
    type Param = ();
    type ViewQuery = ();
    type ItemQuery = Read<GpuMaterialBindGroup>;

    fn render<'w>(
        _item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        bind_group: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        _param: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(bind_group) = bind_group else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(I, &bind_group.0, &[]);
        RenderCommandResult::Success
    }
}

struct SetGpuSceneDepthBindGroup<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetGpuSceneDepthBindGroup<I> {
    type Param = ();
    type ViewQuery = Read<GpuSceneDepthBindGroup>;
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        bind_group: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        _param: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        pass.set_bind_group(I, &bind_group.0, &[]);
        RenderCommandResult::Success
    }
}

struct DrawGpuSpritesIndirect;

impl<P: PhaseItem> RenderCommand<P> for DrawGpuSpritesIndirect {
    type Param = SRes<RenderAssets<GpuShaderBuffer>>;
    type ViewQuery = ();
    type ItemQuery = (Read<GpuDrawInstance>, Option<Read<PreparedMeshDraw>>);

    fn render<'w>(
        _item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        effect: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        buffers: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some((effect, mesh)) = effect else {
            return RenderCommandResult::Skip;
        };
        if let Some(mesh) = mesh {
            pass.set_vertex_buffer(0, mesh.vertex.slice(..));
            if let Some((index, format)) = &mesh.index {
                pass.set_index_buffer(index.slice(..), *format);
                pass.draw_indexed_indirect(&mesh.indirect, 0);
            } else {
                pass.draw_indirect(&mesh.indirect, 0);
            }
            return RenderCommandResult::Success;
        }
        if effect.mesh.is_some() {
            return RenderCommandResult::Skip;
        }
        let Some(indirect) = buffers.into_inner().get(&effect.indirect) else {
            return RenderCommandResult::Skip;
        };
        pass.draw_indirect(&indirect.buffer, effect.indirect_offset);
        RenderCommandResult::Success
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_compiler::MaterialCompiler;
    use aestra_core::material::{MaterialProgram, MaterialRenderState};
    use aestra_gpu::material::{MaterialBackendCapabilities, MaterialShaderCompiler};
    use bevy::{
        math::Affine3A,
        render::{render_phase::ViewRangefinder3d, view::RenderVisibleEntitiesClass},
    };
    use std::any::TypeId;

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

    #[test]
    fn three_dimensional_draw_selection_is_specific_to_each_view() {
        let mut world = World::new();
        let render_a = world.spawn_empty().id();
        let render_b = world.spawn_empty().id();
        let main_a = MainEntity::from(world.spawn_empty().id());
        let main_b = MainEntity::from(world.spawn_empty().id());
        let mut first_view = RenderVisibleEntities::default();
        first_view.classes.insert(
            TypeId::of::<GpuDrawInstance>(),
            RenderVisibleEntitiesClass {
                entities_cpu_culling: vec![(render_a, main_a)],
                ..default()
            },
        );
        let mut second_view = RenderVisibleEntities::default();
        second_view.classes.insert(
            TypeId::of::<GpuDrawInstance>(),
            RenderVisibleEntitiesClass {
                entities_cpu_culling: vec![(render_b, main_b)],
                ..default()
            },
        );

        assert_eq!(
            visible_gpu_draws(&first_view).collect::<Vec<_>>(),
            vec![(render_a, main_a)]
        );
        assert_eq!(
            visible_gpu_draws(&second_view).collect::<Vec<_>>(),
            vec![(render_b, main_b)]
        );
    }

    #[test]
    fn three_dimensional_draw_sorting_uses_world_center_and_renderer_order() {
        let rangefinder = ViewRangefinder3d::from_world_from_view(&Affine3A::IDENTITY);
        let near = gpu_draw_sorting_info(Vec3::new(0.0, 0.0, -2.0), 0);
        let far = gpu_draw_sorting_info(Vec3::new(0.0, 0.0, -8.0), 0);
        let first_renderer = gpu_draw_sorting_info(Vec3::new(0.0, 0.0, -4.0), 0);
        let second_renderer = gpu_draw_sorting_info(Vec3::new(0.0, 0.0, -4.0), 1);

        assert!(far.sort_distance(&rangefinder) < near.sort_distance(&rangefinder));
        assert!(
            first_renderer.sort_distance(&rangefinder)
                < second_renderer.sort_distance(&rangefinder)
        );
    }
}
