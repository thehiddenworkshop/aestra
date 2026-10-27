//! View-specific preparation without effect entities, simulation or particle allocation.
use super::{
    draw_instance::{GpuRenderMode, GpuSemanticMaterialBinding},
    pipeline::{
        GpuSpritePipeline, GpuSpritePipelineKey, GpuSpriteViewKey, fragment_list_eligible,
        semantic_pipeline_key,
    },
    preparation::{MaterialShaderCache, prepare_material_shaders},
};
use crate::PresentedEffect;
use aestra_gpu::GpuBlend;
use aestra_runtime::{CompiledEffectProject, RendererPlanKind};
use bevy::{
    extract::{
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        sync_component::SyncComponent,
    },
    pbr::ViewKeyCache,
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        render_resource::{CachedPipelineState, PipelineCache, SpecializedRenderPipelines},
        view::ExtractedView,
    },
    shader::ShaderCacheError,
};
use std::sync::{Arc, Mutex};

/// Result for the most recently extracted active 3D view configuration.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum EffectPipelinePreparationStatus {
    #[default]
    Pending,
    Ready {
        pipelines: usize,
    },
    Failed(String),
}

/// Attach to a `Camera3d` before starting playback. Prepares all supplied sprite,
/// trail and flipbook material pipelines for that camera's actual HDR/MSAA/OIT key.
/// No particles, clip entities, gameplay events or replay history are created.
///
/// This is pipeline preparation, not asset residency or simulation readiness.
/// Meshes, ribbons and extension stages currently fail explicitly. Dynamic material
/// replacement requires a new request; uniform/clip parameter changes do not.
/// Replace this component to invalidate the request. Remove it when no longer needed.
/// Status is asynchronously refreshed by rendering; an inactive/unextracted view
/// cannot certify a new configuration. Hosts must keep the view active while waiting.
#[derive(Component)]
pub struct EffectPipelinePreparation {
    effects: Arc<[PresentedEffect]>,
    descriptors: Option<Arc<[Descriptor]>>,
    status: Arc<Mutex<EffectPipelinePreparationStatus>>,
}

impl EffectPipelinePreparation {
    /// Includes the root and every compiled dependency, including clips that are
    /// inactive at time zero. Clip overrides only change uniforms, not shader keys.
    pub fn for_project(project: &CompiledEffectProject) -> Self {
        Self::new(
            std::iter::once(project.root.clone())
                .chain(project.dependencies.values().cloned())
                .map(PresentedEffect::new),
        )
    }

    /// Supports explicit host-owned semantic bindings as well as automatic ones.
    pub fn new(effects: impl IntoIterator<Item = PresentedEffect>) -> Self {
        Self {
            effects: effects.into_iter().collect(),
            descriptors: None,
            status: default(),
        }
    }

    pub fn status(&self) -> EffectPipelinePreparationStatus {
        self.status
            .lock()
            .expect("pipeline preparation status poisoned")
            .clone()
    }

    fn publish(&self, value: EffectPipelinePreparationStatus) {
        *self
            .status
            .lock()
            .expect("pipeline preparation status poisoned") = value;
    }
}

#[derive(Clone)]
struct Descriptor {
    kind: u32,
    blend: GpuBlend,
    material: Option<GpuSemanticMaterialBinding>,
}

impl SyncComponent<RenderApp> for EffectPipelinePreparation {
    type Target = Self;
}
impl ExtractComponent<RenderApp> for EffectPipelinePreparation {
    type QueryData = &'static Self;
    type QueryFilter = ();
    type Out = Self;
    fn extract_component(
        source: bevy::ecs::query::QueryItem<'_, '_, Self::QueryData>,
    ) -> Option<Self> {
        Some(Self {
            effects: source.effects.clone(),
            descriptors: source.descriptors.clone(),
            status: source.status.clone(),
        })
    }
}

pub(super) fn install(app: &mut App) {
    app.add_plugins(ExtractComponentPlugin::<EffectPipelinePreparation, RenderApp>::default());
    if app.get_sub_app(RenderApp).is_none() {
        app.add_systems(Update, |requests: Query<&EffectPipelinePreparation>| {
            for request in &requests {
                request.publish(EffectPipelinePreparationStatus::Failed(
                    "No render application is available".into(),
                ));
            }
        });
        return;
    }
    app.add_systems(Update, prepare);
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render.add_systems(Render, queue.in_set(RenderSystems::QueueMeshes));
    }
}

fn prepare(
    mut requests: Query<&mut EffectPipelinePreparation, Changed<EffectPipelinePreparation>>,
    mut shaders: ResMut<Assets<Shader>>,
    mut cache: ResMut<MaterialShaderCache>,
) {
    for mut request in &mut requests {
        if request.descriptors.is_some()
            || matches!(request.status(), EffectPipelinePreparationStatus::Failed(_))
        {
            continue;
        }
        match descriptors(&request.effects, &mut shaders, &mut cache) {
            Ok(descriptors) => request.descriptors = Some(descriptors.into()),
            Err(error) => request.publish(EffectPipelinePreparationStatus::Failed(error)),
        }
    }
}

fn descriptors(
    effects: &[PresentedEffect],
    shaders: &mut Assets<Shader>,
    cache: &mut MaterialShaderCache,
) -> Result<Vec<Descriptor>, String> {
    let mut result = Vec::new();
    for effect in effects {
        if effect.effect().all_extension_stages().next().is_some() {
            return Err("Extension pipeline preparation is not supported yet".into());
        }
        for emitter in &effect.effect().emitters {
            for renderer in &emitter.renderers {
                let kind =
                    match renderer.kind {
                        RendererPlanKind::Sprite => 0,
                        RendererPlanKind::Trail { .. } => 1,
                        RendererPlanKind::Flipbook { .. } => 4,
                        _ => return Err(
                            "Pipeline preparation currently supports sprites, trails and flipbooks"
                                .into(),
                        ),
                    };
                let material = effect
                    .material_binding_for_emitter(renderer.material, emitter.source)
                    .map(|binding| {
                        // Validate rather than declaring failed semantic programs to be legacy.
                        binding.prepare().map_err(|error| error.to_string())?;
                        let program = binding.program().clone();
                        let variants = prepare_material_shaders(&program, shaders, cache);
                        Ok::<_, String>(GpuSemanticMaterialBinding {
                            program,
                            render_state: binding.render_state(),
                            shader: variants.single_sampled,
                            multisampled_shader: variants.multisampled,
                            uniforms: Arc::from([]),
                            textures: Vec::new(),
                            fallback_texture: default(),
                        })
                    })
                    .transpose()?;
                if effect
                    .effect()
                    .material_instance(renderer.material)
                    .is_some()
                    && material.is_none()
                {
                    return Err(format!(
                        "Semantic material {} has no valid binding",
                        renderer.material
                    ));
                }
                result.push(Descriptor {
                    kind,
                    blend: match material
                        .as_ref()
                        .map(|m| m.render_state.blend)
                        .or_else(|| effect.effect().material(renderer.material).map(|m| m.blend))
                        .unwrap_or(aestra_core::BlendMode::Alpha)
                    {
                        aestra_core::BlendMode::Alpha => GpuBlend::Alpha,
                        aestra_core::BlendMode::Additive => GpuBlend::Additive,
                        aestra_core::BlendMode::Multiply => GpuBlend::Multiply,
                    },
                    material,
                });
            }
        }
    }
    Ok(result)
}

fn queue(
    views: Query<(
        &EffectPipelinePreparation,
        &ExtractedView,
        Has<crate::experimental_oit::OitApproved>,
    )>,
    pipeline: Res<GpuSpritePipeline>,
    mut pipelines: ResMut<SpecializedRenderPipelines<GpuSpritePipeline>>,
    cache: Res<PipelineCache>,
    keys: Res<ViewKeyCache>,
) {
    for (request, view, oit) in &views {
        let Some(descriptors) = &request.descriptors else {
            continue;
        };
        let Some(&key) = keys.get(&view.retained_view_entity) else {
            request.publish(EffectPipelinePreparationStatus::Pending);
            continue;
        };
        let mut ids = std::collections::HashSet::new();
        for descriptor in descriptors.iter() {
            ids.insert(pipelines.specialize(
                &cache,
                &pipeline,
                GpuSpritePipelineKey {
                    fragment_list_oit: oit
                        && key.contains(bevy::pbr::MeshPipelineKey::OIT_ENABLED)
                        && fragment_list_eligible(
                            descriptor.kind,
                            descriptor.blend,
                            GpuRenderMode::Rendered,
                            descriptor.material.as_ref().map(|m| m.render_state),
                        ),
                    mesh_wireframe: false,
                    mesh_layout: None,
                    view: GpuSpriteViewKey::ThreeD(key),
                    blend: descriptor.blend,
                    render_mode: GpuRenderMode::Rendered,
                    material: semantic_pipeline_key(
                        descriptor.material.as_ref(),
                        view.target_format,
                        key.msaa_samples(),
                        1,
                    ),
                },
            ));
        }
        let mut status = EffectPipelinePreparationStatus::Ready {
            pipelines: ids.len(),
        };
        for id in ids {
            match cache.get_render_pipeline_state(id) {
                CachedPipelineState::Ok(_) => {}
                CachedPipelineState::Err(error) if fatal(error) => {
                    status = EffectPipelinePreparationStatus::Failed(error.to_string());
                    break;
                }
                _ => status = EffectPipelinePreparationStatus::Pending,
            }
        }
        request.publish(status);
    }
}

fn fatal(error: &ShaderCacheError) -> bool {
    !matches!(
        error,
        ShaderCacheError::ShaderNotLoaded(_) | ShaderCacheError::ShaderImportNotYetAvailable
    )
}

#[cfg(test)]
#[path = "tests/pipeline_preparation.rs"]
mod tests;
