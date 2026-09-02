//! Shared Bevy/WGPU presentation backend for Aestra effects.
//!
//! This crate owns rendering infrastructure, not playback lifecycle. Applications feed it
//! [`PresentedEffect`] components; `aestra-bevy` and `aestra-editor` remain independent owners of
//! runtime and preview behavior.

mod capabilities;
mod cpu;
pub mod gpu;
pub mod material;

pub use aestra_runtime::{
    BackendCapabilities, CompatibilityIssue, CompatibilityIssueCode, CompatibilityReport,
    CompatibilityTarget, EffectRequirements, RendererCapability,
};
pub use capabilities::{
    ActiveBackend, AestraRuntimeStatus, DEFAULT_GPU_PARTICLE_BUDGET, EffectRuntimeStatus,
    GpuCapabilities,
};

use crate::material::{
    MaterialBindingContext, MaterialBindingError, MaterialRuntimeBinding, compile_material_program,
};
use aestra_core::{EmitterId, MaterialId, MaterialProgramId};
use aestra_gpu::material::CompiledMaterialProgram;
use aestra_runtime::{CompiledEffect, EffectInstance, ParticleSample};
use bevy::{
    ecs::schedule::IntoScheduleConfigs,
    prelude::{
        App, AssetServer, Component, Entity, Image, Plugin, Res, Resource, Transform, Update,
        Visibility, Without,
    },
};
use std::{collections::BTreeMap, sync::Arc, time::Duration};

/// Selects the presentation path used by [`AestraRenderPlugin`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PresentationMode {
    /// Select the best supported backend and fall back without panicking.
    #[default]
    Auto,
    /// Simulate and render particles entirely on the GPU.
    Gpu,
    /// Use the deterministic CPU interpreter and pooled Bevy sprites.
    CpuReference,
    /// Simulate on the GPU, read particles back, and present them as Bevy sprites.
    GpuReadback,
}

/// Selects how an effect's presentation geometry is shaded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EffectRenderMode {
    #[default]
    Rendered,
    Wireframe,
}

#[derive(Resource, Debug, Clone, Copy)]
pub struct AestraRenderSettings {
    pub presentation: PresentationMode,
    /// Application budget applied in addition to physical device limits.
    pub max_gpu_particles: u32,
}

impl Default for AestraRenderSettings {
    fn default() -> Self {
        Self {
            presentation: PresentationMode::Auto,
            max_gpu_particles: DEFAULT_GPU_PARTICLE_BUDGET,
        }
    }
}

/// Renderer input for one effect instance. Playback clocks and application events live elsewhere.
#[derive(Component, Debug, Clone)]
#[require(Transform, Visibility)]
pub struct PresentedEffect {
    pub instance: EffectInstance,
    render_mode: EffectRenderMode,
    material_bindings: BTreeMap<MaterialId, MaterialRuntimeBinding>,
    compiled_material_programs: BTreeMap<MaterialProgramId, Arc<CompiledMaterialProgram>>,
    automatic_material_bindings: BTreeMap<(EmitterId, MaterialId), MaterialRuntimeBinding>,
    cpu_samples: Vec<ParticleSample>,
    gpu_samples: Vec<ParticleSample>,
    cpu_evaluation_time: Option<Duration>,
}

impl PresentedEffect {
    pub fn new(effect: Arc<CompiledEffect>) -> Self {
        let mut presented = Self {
            instance: EffectInstance::new(effect),
            render_mode: EffectRenderMode::Rendered,
            material_bindings: BTreeMap::new(),
            compiled_material_programs: BTreeMap::new(),
            automatic_material_bindings: BTreeMap::new(),
            cpu_samples: Vec::new(),
            gpu_samples: Vec::new(),
            cpu_evaluation_time: None,
        };
        presented.rebuild_automatic_material_bindings();
        presented
    }

    pub fn effect(&self) -> &Arc<CompiledEffect> {
        self.instance.effect()
    }

    pub fn simulation_time(&self) -> f32 {
        self.instance.time()
    }

    pub fn render_mode(&self) -> EffectRenderMode {
        self.render_mode
    }

    pub fn set_render_mode(&mut self, mode: EffectRenderMode) {
        self.render_mode = mode;
    }

    /// Selects the semantic material program and instance values used by one renderer material.
    ///
    /// Renderers without a binding continue through the legacy compatibility shader.
    pub fn bind_material(&mut self, material: MaterialId, binding: MaterialRuntimeBinding) {
        self.material_bindings.insert(material, binding);
    }

    pub fn unbind_material(&mut self, material: MaterialId) -> Option<MaterialRuntimeBinding> {
        self.material_bindings.remove(&material)
    }

    pub fn material_binding(&self, material: MaterialId) -> Option<&MaterialRuntimeBinding> {
        self.material_bindings.get(&material).or_else(|| {
            self.automatic_material_bindings
                .iter()
                .find_map(|((_, candidate), binding)| (*candidate == material).then_some(binding))
        })
    }

    pub fn material_binding_for_emitter(
        &self,
        material: MaterialId,
        emitter: EmitterId,
    ) -> Option<&MaterialRuntimeBinding> {
        self.material_bindings
            .get(&material)
            .or_else(|| self.automatic_material_bindings.get(&(emitter, material)))
    }

    /// Re-resolves automatic bindings against the current effect/emitter parameter state.
    pub fn refresh_automatic_material_bindings(&mut self) {
        let instance = &self.instance;
        for ((emitter, _), binding) in &mut self.automatic_material_bindings {
            let context = MaterialBindingContext::for_emitter(instance, *emitter);
            if let Err(error) = binding.refresh_dynamic_values(&context) {
                bevy::log::warn!("semantic material binding could not be refreshed: {error}");
            }
        }
    }

    /// Refreshes one bound material after effect/emitter automation or parameter edits.
    ///
    /// This updates only dynamic uniform/resource values. It retains the compiled shader and
    /// pipeline-compatible program already held by the binding.
    pub fn refresh_material_binding(
        &mut self,
        material: MaterialId,
        context: &MaterialBindingContext,
    ) -> Result<(), MaterialBindingError> {
        let binding = self
            .material_bindings
            .get_mut(&material)
            .ok_or(MaterialBindingError::UnknownMaterial(material))?;
        binding.refresh_dynamic_values(context)
    }

    fn rebuild_automatic_material_bindings(&mut self) {
        self.compiled_material_programs.clear();
        self.automatic_material_bindings.clear();
        let effect = Arc::clone(self.effect());
        for program in &effect.material_programs {
            match compile_material_program(program) {
                Ok(compiled) => {
                    self.compiled_material_programs.insert(program.id, compiled);
                }
                Err(error) => bevy::log::warn!(
                    "semantic material program {} could not be compiled: {error}",
                    program.id
                ),
            }
        }
        for emitter in &effect.emitters {
            let context = MaterialBindingContext::for_emitter(&self.instance, emitter.source);
            for renderer in &emitter.renderers {
                let Some(instance) = effect.material_instance(renderer.material) else {
                    continue;
                };
                let Some(program) = self.compiled_material_programs.get(&instance.program.id())
                else {
                    continue;
                };
                match MaterialRuntimeBinding::from_instance_with_context(
                    Arc::clone(program),
                    instance,
                    &context,
                ) {
                    Ok(binding) => {
                        self.automatic_material_bindings
                            .insert((emitter.source, renderer.material), binding);
                    }
                    Err(error) => bevy::log::warn!(
                        "semantic material {} could not be resolved for emitter {}: {error}",
                        renderer.material,
                        emitter.source
                    ),
                }
            }
        }
    }

    pub fn gpu_samples(&self) -> &[ParticleSample] {
        &self.gpu_samples
    }

    pub fn samples(&self) -> &[ParticleSample] {
        if self.gpu_samples.is_empty() {
            &self.cpu_samples
        } else {
            &self.gpu_samples
        }
    }

    /// Duration of the most recent CPU-reference evaluation, when the active backend used one.
    pub fn cpu_evaluation_time(&self) -> Option<Duration> {
        self.cpu_evaluation_time
    }

    pub fn take_gpu_samples(&mut self) -> Vec<ParticleSample> {
        std::mem::take(&mut self.gpu_samples)
    }

    pub fn restore_gpu_samples(&mut self, samples: Vec<ParticleSample>) {
        self.gpu_samples = samples;
    }
}

/// Scheduling point applications use to update [`PresentedEffect`] before rendering consumes it.
#[derive(bevy::ecs::schedule::SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AestraRenderSet {
    Prepare,
}

#[derive(Default)]
pub struct AestraRenderPlugin;

impl Plugin for AestraRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AestraRenderSettings>()
            .init_resource::<GpuCapabilities>()
            .init_resource::<AestraRuntimeStatus>()
            .init_resource::<TextureAssetCache>()
            .add_observer(gpu::receive_readback);
        gpu::install(app);
        app.add_systems(
            Update,
            (
                ensure_aestra_depth_prepass,
                assign_effect_backends,
                cpu::prepare_cpu_effects,
                gpu::prepare_gpu_effects,
                cpu::present_cpu_effects,
            )
                .chain()
                .in_set(AestraRenderSet::Prepare),
        );
    }
}

/// Aestra's semantic scene-depth inputs sample Bevy's separate 3D prepass
/// texture. Enabling it on 3D cameras avoids the invalid feedback loop that
/// would result from sampling the active main-pass depth attachment.
fn ensure_aestra_depth_prepass(
    mut commands: bevy::prelude::Commands,
    cameras: bevy::prelude::Query<
        bevy::prelude::Entity,
        (
            bevy::prelude::With<bevy::prelude::Camera3d>,
            bevy::prelude::Without<bevy::core_pipeline::prepass::DepthPrepass>,
        ),
    >,
) {
    for camera in &cameras {
        commands
            .entity(camera)
            .insert(bevy::core_pipeline::prepass::DepthPrepass);
    }
}

#[derive(Resource, Default)]
pub(crate) struct TextureAssetCache(BTreeMap<String, bevy::prelude::Handle<Image>>);

impl TextureAssetCache {
    pub(crate) fn load(
        &mut self,
        asset_server: &AssetServer,
        path: &str,
    ) -> bevy::prelude::Handle<Image> {
        self.0
            .entry(path.to_owned())
            .or_insert_with(|| asset_server.load(path.to_owned()))
            .clone()
    }
}

#[derive(Component)]
pub(crate) struct GpuPresentationPrepared;

fn assign_effect_backends(
    mut commands: bevy::prelude::Commands,
    settings: Res<AestraRenderSettings>,
    runtime: Res<AestraRuntimeStatus>,
    capabilities: Res<GpuCapabilities>,
    effects: bevy::prelude::Query<(Entity, &PresentedEffect), Without<EffectRuntimeStatus>>,
) {
    if runtime.active == ActiveBackend::Pending {
        return;
    }
    let backend = capabilities.backend_capabilities(settings.max_gpu_particles);
    for (entity, effect) in &effects {
        commands
            .entity(entity)
            .insert(capabilities::select_effect_backend(
                &runtime,
                &effect.effect().requirements,
                &backend,
            ));
    }
}
