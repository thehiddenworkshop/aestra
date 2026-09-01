//! Shared Bevy/WGPU presentation backend for Aestra effects.
//!
//! This crate owns rendering infrastructure, not playback lifecycle. Applications feed it
//! [`PresentedEffect`] components; `aestra-bevy` and `aestra-editor` remain independent owners of
//! runtime and preview behavior.

mod capabilities;
mod cpu;
pub mod gpu;

pub use capabilities::{
    ActiveBackend, AestraRuntimeStatus, DEFAULT_GPU_PARTICLE_BUDGET, EffectRuntimeStatus,
    GpuCapabilities,
};

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
    cpu_samples: Vec<ParticleSample>,
    gpu_samples: Vec<ParticleSample>,
    cpu_evaluation_time: Option<Duration>,
}

impl PresentedEffect {
    pub fn new(effect: Arc<CompiledEffect>) -> Self {
        Self {
            instance: EffectInstance::new(effect),
            render_mode: EffectRenderMode::Rendered,
            cpu_samples: Vec::new(),
            gpu_samples: Vec::new(),
            cpu_evaluation_time: None,
        }
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
    let particle_budget = capabilities.max_particles.min(settings.max_gpu_particles) as usize;
    for (entity, effect) in &effects {
        commands
            .entity(entity)
            .insert(capabilities::select_effect_backend(
                &runtime,
                effect.effect().max_particles,
                particle_budget,
            ));
    }
}
