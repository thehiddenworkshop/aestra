use super::{
    GpuBlend, GpuDrawInstance, GpuParticle, GpuRenderGlobals, GpuRenderMode, GpuRenderParams,
    GpuRenderer, WESL_RENDER_SHADER_PATH,
};
use bevy::{
    app::SubApp,
    core_pipeline::{
        core_2d::{CORE_2D_DEPTH_FORMAT, Transparent2d},
        core_3d::{CORE_3D_DEPTH_FORMAT, Transparent3d, TransparentSortingInfo3d},
    },
    ecs::{
        query::ROQueryItem,
        system::{SystemParamItem, lifetimeless::*},
    },
    math::FloatOrd,
    pbr::{MeshPipeline, MeshPipelineKey, MeshPipelineSystems, SetMeshViewBindGroup, ViewKeyCache},
    prelude::*,
    render::{
        Render, RenderStartup, RenderSystems,
        render_asset::RenderAssets,
        render_phase::{
            AddRenderCommand, DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand,
            RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
        },
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries,
            BlendComponent, BlendFactor, BlendOperation, BlendState, ColorTargetState, ColorWrites,
            CompareFunction, DepthBiasState, DepthStencilState, Face, FragmentState,
            MultisampleState, PipelineCache, PrimitiveState, PrimitiveTopology,
            RenderPipelineDescriptor, SamplerBindingType, ShaderStages, SpecializedRenderPipeline,
            SpecializedRenderPipelines, StencilFaceState, StencilState, TextureSampleType,
            VertexState,
            binding_types::{sampler, storage_buffer_read_only, texture_2d},
        },
        renderer::RenderDevice,
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
        .add_render_command::<Transparent3d, DrawGpuSprites3d>()
        .init_resource::<SpecializedRenderPipelines<GpuSpritePipeline>>()
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
                queue_gpu_sprites.in_set(RenderSystems::QueueMeshes),
                queue_gpu_sprites_3d.in_set(RenderSystems::QueueMeshes),
            ),
        );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct GpuSpritePipelineKey {
    view: GpuSpriteViewKey,
    blend: GpuBlend,
    render_mode: GpuRenderMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum GpuSpriteViewKey {
    TwoD(Mesh2dPipelineKey),
    ThreeD(MeshPipelineKey),
}

#[derive(Resource)]
struct GpuSpritePipeline {
    mesh2d: Mesh2dPipeline,
    mesh3d: MeshPipeline,
    effect_layout: BindGroupLayoutDescriptor,
    shader: Handle<Shader>,
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
    commands.insert_resource(GpuSpritePipeline {
        mesh2d: mesh2d.clone(),
        mesh3d: mesh3d.clone(),
        effect_layout,
        shader: asset_server.load(WESL_RENDER_SHADER_PATH),
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
        let blend = match key.render_mode {
            GpuRenderMode::Wireframe => BlendState::ALPHA_BLENDING,
            GpuRenderMode::Rendered => match key.blend {
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
        RenderPipelineDescriptor {
            label: Some("aestra gpu sprite".into()),
            layout: vec![view_layout, self.effect_layout.clone()],
            vertex: VertexState {
                shader: self.shader.clone(),
                entry_point: Some("vertex".into()),
                buffers: Vec::new(),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                entry_point: Some(
                    match key.render_mode {
                        GpuRenderMode::Wireframe => "fragment_wireframe",
                        GpuRenderMode::Rendered => match key.blend {
                            GpuBlend::Alpha => "fragment_alpha",
                            GpuBlend::Additive => "fragment_additive",
                            GpuBlend::Multiply => "fragment_multiply",
                        },
                    }
                    .into(),
                ),
                targets: vec![Some(ColorTargetState {
                    format: target_format,
                    blend: Some(blend),
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                cull_mode: Some(Face::Back),
                ..default()
            },
            depth_stencil: Some(DepthStencilState {
                format: depth_format,
                depth_write_enabled: Some(false),
                depth_compare: Some(CompareFunction::GreaterEqual),
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

#[derive(Component)]
struct GpuRenderBindGroup(BindGroup);

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
    }
}

fn queue_gpu_sprites(
    draw_functions: Res<DrawFunctions<Transparent2d>>,
    pipeline: Res<GpuSpritePipeline>,
    mut pipelines: ResMut<SpecializedRenderPipelines<GpuSpritePipeline>>,
    pipeline_cache: Res<PipelineCache>,
    effects: Query<&GpuDrawInstance>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent2d>>,
    views: Query<(&RenderVisibleEntities, &ExtractedView, &Msaa)>,
) {
    let draw_function = draw_functions.read().id::<DrawGpuSprites>();
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
            let Ok(effect) = effects.get(*render_entity) else {
                continue;
            };
            let pipeline_id = pipelines.specialize(
                &pipeline_cache,
                &pipeline,
                GpuSpritePipelineKey {
                    view: GpuSpriteViewKey::TwoD(mesh_key),
                    blend: effect.blend,
                    render_mode: effect.render_mode,
                },
            );
            phase.add_retained(Transparent2d {
                sort_key: FloatOrd(effect.renderer_order as f32),
                entity: (*render_entity, *main_entity),
                pipeline: pipeline_id,
                draw_function,
                batch_range: 0..1,
                extracted_index: usize::MAX,
                extra_index: PhaseItemExtraIndex::None,
                indexed: false,
            });
        }
    }
}

fn queue_gpu_sprites_3d(
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    pipeline: Res<GpuSpritePipeline>,
    mut pipelines: ResMut<SpecializedRenderPipelines<GpuSpritePipeline>>,
    pipeline_cache: Res<PipelineCache>,
    effects: Query<(Entity, &MainEntity, &GpuDrawInstance)>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    views: Query<&ExtractedView>,
    view_key_cache: Res<ViewKeyCache>,
) {
    let draw_function = draw_functions.read().id::<DrawGpuSprites3d>();
    for view in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Some(&mesh_key) = view_key_cache.get(&view.retained_view_entity) else {
            continue;
        };
        for (render_entity, main_entity, effect) in &effects {
            let pipeline_id = pipelines.specialize(
                &pipeline_cache,
                &pipeline,
                GpuSpritePipelineKey {
                    view: GpuSpriteViewKey::ThreeD(mesh_key),
                    blend: effect.blend,
                    render_mode: effect.render_mode,
                },
            );
            phase.add_retained(Transparent3d {
                sorting_info: TransparentSortingInfo3d::Sorted {
                    mesh_center: Vec3::ZERO,
                    depth_bias: effect.renderer_order as f32 * -0.0001,
                },
                entity: (render_entity, *main_entity),
                pipeline: pipeline_id,
                draw_function,
                distance: 0.0,
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::None,
                indexed: false,
            });
        }
    }
}

type DrawGpuSprites = (
    SetItemPipeline,
    SetMesh2dViewBindGroup<0>,
    SetGpuRenderBindGroup<1>,
    DrawGpuSpritesIndirect,
);

type DrawGpuSprites3d = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetGpuRenderBindGroup<1>,
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

struct DrawGpuSpritesIndirect;

impl<P: PhaseItem> RenderCommand<P> for DrawGpuSpritesIndirect {
    type Param = SRes<RenderAssets<GpuShaderBuffer>>;
    type ViewQuery = ();
    type ItemQuery = Read<GpuDrawInstance>;

    fn render<'w>(
        _item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        effect: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        buffers: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(effect) = effect else {
            return RenderCommandResult::Skip;
        };
        let Some(indirect) = buffers.into_inner().get(&effect.indirect) else {
            return RenderCommandResult::Skip;
        };
        pass.draw_indirect(&indirect.buffer, 0);
        RenderCommandResult::Success
    }
}
