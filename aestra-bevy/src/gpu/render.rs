use super::{
    GpuBlend, GpuDrawInstance, GpuEmitter, GpuParticle, GpuRenderGlobals, WESL_RENDER_SHADER_PATH,
};
use bevy::{
    app::SubApp,
    core_pipeline::core_2d::{CORE_2D_DEPTH_FORMAT, Transparent2d},
    ecs::{
        query::ROQueryItem,
        system::{SystemParamItem, lifetimeless::*},
    },
    math::FloatOrd,
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
            RenderPipelineDescriptor, ShaderStages, SpecializedRenderPipeline,
            SpecializedRenderPipelines, StencilFaceState, StencilState, VertexState,
            binding_types::storage_buffer_read_only,
        },
        renderer::RenderDevice,
        storage::GpuShaderBuffer,
        view::{ExtractedView, RenderVisibleEntities},
    },
    sprite_render::{
        Mesh2dPipeline, Mesh2dPipelineKey, SetMesh2dViewBindGroup, init_mesh_2d_pipeline,
    },
};

pub(super) fn install(render_app: &mut SubApp) {
    render_app
        .add_render_command::<Transparent2d, DrawGpuSprites>()
        .init_resource::<SpecializedRenderPipelines<GpuSpritePipeline>>()
        .add_systems(
            RenderStartup,
            init_render_pipeline.after(init_mesh_2d_pipeline),
        )
        .add_systems(
            Render,
            (
                prepare_render_bind_groups.in_set(RenderSystems::PrepareBindGroups),
                queue_gpu_sprites.in_set(RenderSystems::QueueMeshes),
            ),
        );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct GpuSpritePipelineKey {
    mesh: Mesh2dPipelineKey,
    blend: GpuBlend,
}

#[derive(Resource)]
struct GpuSpritePipeline {
    mesh2d: Mesh2dPipeline,
    effect_layout: BindGroupLayoutDescriptor,
    shader: Handle<Shader>,
}

fn init_render_pipeline(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mesh2d: Res<Mesh2dPipeline>,
) {
    let effect_layout = BindGroupLayoutDescriptor::new(
        "aestra_gpu_sprite",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::VERTEX,
            (
                storage_buffer_read_only::<Vec<GpuEmitter>>(false),
                storage_buffer_read_only::<Vec<GpuParticle>>(false),
                storage_buffer_read_only::<Vec<u32>>(false),
                storage_buffer_read_only::<GpuRenderGlobals>(false),
            ),
        ),
    );
    commands.insert_resource(GpuSpritePipeline {
        mesh2d: mesh2d.clone(),
        effect_layout,
        shader: asset_server.load(WESL_RENDER_SHADER_PATH),
    });
}

impl SpecializedRenderPipeline for GpuSpritePipeline {
    type Key = GpuSpritePipelineKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        let blend = match key.blend {
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
        };
        RenderPipelineDescriptor {
            label: Some("aestra gpu sprite".into()),
            layout: vec![self.mesh2d.view_layout.clone(), self.effect_layout.clone()],
            vertex: VertexState {
                shader: self.shader.clone(),
                entry_point: Some("vertex".into()),
                buffers: Vec::new(),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: self.shader.clone(),
                entry_point: Some(
                    match key.blend {
                        GpuBlend::Alpha => "fragment_alpha",
                        GpuBlend::Additive => "fragment_additive",
                    }
                    .into(),
                ),
                targets: vec![Some(ColorTargetState {
                    format: key.mesh.target_format(),
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
                format: CORE_2D_DEPTH_FORMAT,
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
                count: key.mesh.msaa_samples(),
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
    effects: Query<(Entity, &GpuDrawInstance)>,
) {
    for (entity, effect) in &effects {
        let Some(emitters) = buffers.get(&effect.emitters) else {
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
        let bind_group = render_device.create_bind_group(
            Some("aestra gpu sprite"),
            &pipeline_cache.get_bind_group_layout(&pipeline.effect_layout),
            &BindGroupEntries::sequential((
                emitters.buffer.as_entire_buffer_binding(),
                particles.buffer.as_entire_buffer_binding(),
                alive.buffer.as_entire_buffer_binding(),
                globals.buffer.as_entire_buffer_binding(),
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
                    mesh: mesh_key,
                    blend: effect.blend,
                },
            );
            phase.add_retained(Transparent2d {
                sort_key: FloatOrd(0.0),
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

type DrawGpuSprites = (
    SetItemPipeline,
    SetMesh2dViewBindGroup<0>,
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
