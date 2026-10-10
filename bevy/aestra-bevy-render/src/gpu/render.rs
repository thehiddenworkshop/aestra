pub(super) use super::draw_commands::PreparedMeshDraw;
use super::draw_commands::*;
use super::draw_instance::{GpuDrawInstance, GpuRenderMode};
use super::draw_preparation::{
    prepare_mesh_draws, prepare_render_bind_groups, prepare_scene_depth_bind_groups,
};
use super::draw_resources::{PrepareAlphaSort, TrailCompactionSystems};
use super::pipeline::{
    GpuSpritePipeline, GpuSpritePipelineKey, GpuSpriteViewKey, draw_pipeline_key, material_for_draw,
};
pub(super) use super::view_phases::visible_gpu_draws;
use super::view_phases::{PhaseDraw, reject_sampled_sprite, retire_invisible_draws};
use bevy::{
    app::SubApp,
    camera::MainPassResolutionOverride,
    core_pipeline::{core_2d::Transparent2d, core_3d::Transparent3d},
    pbr::{MeshPipeline, MeshPipelineSystems, ViewKeyCache},
    prelude::*,
    render::{
        Render, RenderStartup, RenderSystems,
        camera::TemporalJitter,
        render_phase::{AddRenderCommand, DrawFunctions, ViewSortedRenderPhases},
        render_resource::{PipelineCache, SpecializedRenderPipelines},
        view::{ExtractedView, RenderVisibleEntities},
    },
    sprite_render::{Mesh2dPipeline, Mesh2dPipelineKey, init_mesh_2d_pipeline},
};

pub const WESL_RENDER_SHADER_PATH: &str =
    "embedded://aestra_bevy_render/shaders/aestra_sprite_render.wesl";
pub const WESL_MESH_WIREFRAME_SHADER_PATH: &str =
    "embedded://aestra_bevy_render/shaders/aestra_mesh_wireframe.wesl";

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
                prepare_render_bind_groups
                    .in_set(RenderSystems::PrepareBindGroups)
                    .after(PrepareAlphaSort)
                    .after(TrailCompactionSystems::Prepare),
                prepare_scene_depth_bind_groups.in_set(RenderSystems::PrepareBindGroups),
                queue_gpu_sprites.in_set(RenderSystems::QueueMeshes),
                queue_gpu_sprites_3d.in_set(RenderSystems::QueueMeshes),
            ),
        );
}

fn init_render_pipeline(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mesh2d: Res<Mesh2dPipeline>,
    mesh3d: Res<MeshPipeline>,
) {
    commands.insert_resource(GpuSpritePipeline::new(
        mesh2d.clone(),
        mesh3d.clone(),
        asset_server.load(WESL_RENDER_SHADER_PATH),
        asset_server.load(WESL_MESH_WIREFRAME_SHADER_PATH),
    ));
}

#[allow(clippy::type_complexity)]
fn queue_gpu_sprites(
    draw_functions: Res<DrawFunctions<Transparent2d>>,
    pipeline: Res<GpuSpritePipeline>,
    mut pipelines: ResMut<SpecializedRenderPipelines<GpuSpritePipeline>>,
    pipeline_cache: Res<PipelineCache>,
    effects: Query<(&GpuDrawInstance, Option<&PreparedMeshDraw>)>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent2d>>,
    views: Query<(
        &RenderVisibleEntities,
        &ExtractedView,
        &Msaa,
        Option<&TemporalJitter>,
        Option<&MainPassResolutionOverride>,
    )>,
) {
    let _span = bevy::log::info_span!("aestra::gpu::queue_sprites").entered();
    let draw_functions = draw_functions.read();
    let legacy_draw_function = draw_functions.id::<DrawGpuSprites>();
    let semantic_draw_function = draw_functions.id::<DrawSemanticGpuSprites>();
    for (visible_entities, view, msaa, jitter, resolution) in &views {
        let sampled_culling = super::sprite_culling::prepare(view, resolution, jitter.is_some());
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        retire_invisible_draws(
            phase,
            visible_entities,
            &[legacy_draw_function, semantic_draw_function],
        );
        let mesh_key = Mesh2dPipelineKey::from_msaa_samples(msaa.samples())
            | Mesh2dPipelineKey::from_target_format(view.target_format);
        for (render_entity, main_entity) in visible_gpu_draws(visible_entities) {
            let Ok((effect, mesh)) = effects.get(render_entity) else {
                phase.remove(render_entity, main_entity);
                continue;
            };
            if reject_sampled_sprite(
                phase,
                effect.sampled_sprite_cull.as_ref(),
                sampled_culling.as_ref(),
                render_entity,
                main_entity,
            ) {
                continue;
            }
            if effect.mesh.is_some()
                && (mesh.is_none()
                    || (effect.semantic_material.is_none()
                        && effect.render_mode == GpuRenderMode::Rendered))
            {
                phase.remove(render_entity, main_entity);
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
                phase.remove(render_entity, main_entity);
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
            phase.add_retained(
                PhaseDraw {
                    entity: (render_entity, main_entity),
                    pipeline: pipeline_id,
                    draw_function: if material_for_draw(effect).is_some() {
                        semantic_draw_function
                    } else {
                        legacy_draw_function
                    },
                    indexed: mesh.is_some_and(|mesh| mesh.index.is_some()),
                }
                .two_d(effect.renderer_order),
            );
        }
    }
}

#[allow(clippy::type_complexity)]
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
    views: Query<(
        &RenderVisibleEntities,
        &ExtractedView,
        Option<&TemporalJitter>,
        Option<&MainPassResolutionOverride>,
    )>,
) {
    let (pipeline, mut pipelines, pipeline_cache, view_key_cache) = pipeline_resources;
    let draw_functions = draw_functions.read();
    let legacy_draw_function = draw_functions.id::<DrawGpuSprites3d>();
    let semantic_draw_function = draw_functions.id::<DrawSemanticGpuSprites3d>();
    let semantic_depth_draw_function = draw_functions.id::<DrawSemanticDepthGpuSprites3d>();
    for (visible_entities, view, jitter, resolution) in &views {
        let sampled_culling = super::sprite_culling::prepare(view, resolution, jitter.is_some());
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        retire_invisible_draws(
            phase,
            visible_entities,
            &[
                legacy_draw_function,
                semantic_draw_function,
                semantic_depth_draw_function,
            ],
        );
        let Some(&mesh_key) = view_key_cache.get(&view.retained_view_entity) else {
            continue;
        };
        for (render_entity, main_entity) in visible_gpu_draws(visible_entities) {
            let Ok((effect, mesh)) = effects.get(render_entity) else {
                phase.remove(render_entity, main_entity);
                continue;
            };
            if reject_sampled_sprite(
                phase,
                effect.sampled_sprite_cull.as_ref(),
                sampled_culling.as_ref(),
                render_entity,
                main_entity,
            ) {
                continue;
            }
            if effect.mesh.is_some()
                && (mesh.is_none()
                    || (effect.semantic_material.is_none()
                        && effect.render_mode == GpuRenderMode::Rendered))
            {
                phase.remove(render_entity, main_entity);
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
            phase.add_retained(
                PhaseDraw {
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
                    indexed: mesh.is_some_and(|mesh| mesh.index.is_some()),
                }
                .three_d(effect.mesh_center, effect.renderer_order),
            );
        }
    }
}
