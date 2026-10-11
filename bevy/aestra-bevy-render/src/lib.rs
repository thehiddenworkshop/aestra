//! Shared Bevy/WGPU presentation backend for Aestra effects.
//!
//! This crate owns rendering infrastructure, not playback lifecycle. Applications feed it
//! [`PresentedEffect`] components; `aestra-bevy` and `aestra-editor` remain independent owners of
//! runtime and preview behavior.

mod capabilities;
mod cpu;
pub mod execution;
pub mod gpu;
mod host_transform;
pub mod material;
mod material_layout;
mod presented_effect;
mod project_assets;
pub use presented_effect::{EffectRenderMode, PresentedEffect};
pub use project_assets::AestraTextureRoot;
pub(crate) use project_assets::{ProjectAssetCache, sync_texture_root};
pub mod preview;
mod render_set;
mod render_settings;
pub use render_set::AestraRenderSet;
pub mod sampling;
pub use render_settings::{AestraRenderSettings, PresentationMode, TransparentOrderMode};

pub use aestra_runtime::{
    BackendCapabilities, CompatibilityIssue, CompatibilityIssueCode, CompatibilityReport,
    CompatibilityTarget, EffectRequirements, PlaybackHistoryPolicy, RendererCapability,
};
pub use capabilities::{
    ActiveBackend, AestraRuntimeStatus, DEFAULT_GPU_PARTICLE_BUDGET, EffectRuntimeStatus,
    GpuCapabilities,
};

use bevy::{
    ecs::schedule::IntoScheduleConfigs,
    prelude::{App, Component, Entity, Plugin, Res, Update, Without},
};

#[derive(Default)]
pub struct AestraRenderPlugin;

impl Plugin for AestraRenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            bevy::prelude::PostUpdate,
            cpu::sync_particle_globals
                .after(bevy::transform::TransformSystems::Propagate)
                .before(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
        );
        app.init_resource::<AestraRenderSettings>()
            .init_resource::<sampling::SpriteSampling>()
            .init_resource::<sampling::TrailRasterSampling>()
            .init_resource::<AestraTextureRoot>()
            .init_resource::<GpuCapabilities>()
            .init_resource::<AestraRuntimeStatus>()
            .init_resource::<ProjectAssetCache>()
            .add_observer(gpu::receive_readback);
        gpu::install(app);
        app.add_systems(
            Update,
            (
                ensure_aestra_depth_prepass,
                sync_texture_root,
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
