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

use bevy::prelude::Component;

mod render_plugin;
pub use render_plugin::AestraRenderPlugin;
#[derive(Component)]
pub(crate) struct GpuPresentationPrepared;
