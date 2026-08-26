use bevy::prelude::{Component, Resource};
use std::fmt;

use crate::PresentationMode;

pub const DEFAULT_GPU_PARTICLE_BUDGET: u32 = 262_144;

/// GPU limits and downlevel features required by Aestra's current WESL backend.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct GpuCapabilities {
    pub detected: bool,
    pub adapter_name: String,
    pub backend: String,
    pub device_type: String,
    pub driver: String,
    pub compute_shaders: bool,
    pub indirect_execution: bool,
    pub vertex_storage: bool,
    pub compute_pipeline_supported: bool,
    pub native_render_supported: bool,
    pub max_bind_groups: u32,
    pub max_bindings_per_bind_group: u32,
    pub max_storage_buffers_per_shader_stage: u32,
    pub max_storage_buffer_binding_size: u64,
    pub max_buffer_size: u64,
    pub max_compute_workgroups_per_dimension: u32,
    pub max_particles: u32,
    pub limitations: Vec<String>,
}

impl Default for GpuCapabilities {
    fn default() -> Self {
        Self {
            detected: false,
            adapter_name: "detecting".into(),
            backend: "unknown".into(),
            device_type: "unknown".into(),
            driver: "unknown".into(),
            compute_shaders: false,
            indirect_execution: false,
            vertex_storage: false,
            compute_pipeline_supported: false,
            native_render_supported: false,
            max_bind_groups: 0,
            max_bindings_per_bind_group: 0,
            max_storage_buffers_per_shader_stage: 0,
            max_storage_buffer_binding_size: 0,
            max_buffer_size: 0,
            max_compute_workgroups_per_dimension: 0,
            max_particles: 0,
            limitations: Vec::new(),
        }
    }
}

impl GpuCapabilities {
    pub(crate) fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            detected: true,
            adapter_name: "unavailable".into(),
            limitations: vec![reason.into()],
            ..Self::default()
        }
    }
}

/// Runtime backend selected after applying the requested mode to device capabilities.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ActiveBackend {
    #[default]
    Pending,
    Gpu,
    GpuReadback,
    CpuReference,
}

impl fmt::Display for ActiveBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Pending => "pending",
            Self::Gpu => "native GPU",
            Self::GpuReadback => "GPU readback",
            Self::CpuReference => "CPU reference",
        })
    }
}

/// Global device decision. Individual effects may still fall back when they exceed budgets.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct AestraRuntimeStatus {
    pub requested: PresentationMode,
    pub active: ActiveBackend,
    pub reason: String,
}

impl Default for AestraRuntimeStatus {
    fn default() -> Self {
        Self {
            requested: PresentationMode::Auto,
            active: ActiveBackend::Pending,
            reason: "waiting for render-device capability detection".into(),
        }
    }
}

/// Backend and diagnostic reason selected for one [`crate::EffectPlayer`].
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct EffectRuntimeStatus {
    pub active: ActiveBackend,
    pub reason: String,
}

pub(crate) fn select_backend(
    requested: PresentationMode,
    capabilities: &GpuCapabilities,
) -> AestraRuntimeStatus {
    if !capabilities.detected {
        return AestraRuntimeStatus {
            requested,
            ..AestraRuntimeStatus::default()
        };
    }

    let (active, reason) = match requested {
        PresentationMode::CpuReference => (
            ActiveBackend::CpuReference,
            "CPU reference presentation was explicitly requested".into(),
        ),
        PresentationMode::Auto | PresentationMode::Gpu if capabilities.native_render_supported => (
            ActiveBackend::Gpu,
            "compute, vertex storage, and indirect drawing are supported".into(),
        ),
        PresentationMode::Auto | PresentationMode::Gpu | PresentationMode::GpuReadback
            if capabilities.compute_pipeline_supported =>
        {
            let prefix = if requested == PresentationMode::GpuReadback {
                "GPU readback presentation was explicitly requested"
            } else {
                "native drawing is unavailable; using GPU simulation with readback"
            };
            (
                ActiveBackend::GpuReadback,
                with_limitations(prefix, capabilities),
            )
        }
        _ => (
            ActiveBackend::CpuReference,
            with_limitations(
                "GPU compute requirements are unavailable; using the CPU reference",
                capabilities,
            ),
        ),
    };
    AestraRuntimeStatus {
        requested,
        active,
        reason,
    }
}

pub(crate) fn select_effect_backend(
    runtime: &AestraRuntimeStatus,
    requested_particles: usize,
    particle_budget: usize,
) -> EffectRuntimeStatus {
    if matches!(
        runtime.active,
        ActiveBackend::Gpu | ActiveBackend::GpuReadback
    ) && requested_particles > particle_budget
    {
        EffectRuntimeStatus {
            active: ActiveBackend::CpuReference,
            reason: format!(
                "effect requests {requested_particles} particles but the GPU budget is {particle_budget}; using the CPU reference"
            ),
        }
    } else {
        EffectRuntimeStatus {
            active: runtime.active,
            reason: runtime.reason.clone(),
        }
    }
}

fn with_limitations(prefix: &str, capabilities: &GpuCapabilities) -> String {
    if capabilities.limitations.is_empty() {
        prefix.into()
    } else {
        format!("{prefix}: {}", capabilities.limitations.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capabilities(compute: bool, native: bool) -> GpuCapabilities {
        GpuCapabilities {
            detected: true,
            compute_pipeline_supported: compute,
            native_render_supported: native,
            max_particles: 1000,
            limitations: if native {
                Vec::new()
            } else {
                vec!["indirect execution is unavailable".into()]
            },
            ..GpuCapabilities::default()
        }
    }

    #[test]
    fn auto_prefers_native_gpu() {
        assert_eq!(
            select_backend(PresentationMode::Auto, &capabilities(true, true)).active,
            ActiveBackend::Gpu
        );
    }

    #[test]
    fn auto_uses_readback_when_only_compute_is_available() {
        assert_eq!(
            select_backend(PresentationMode::Auto, &capabilities(true, false)).active,
            ActiveBackend::GpuReadback
        );
    }

    #[test]
    fn auto_uses_cpu_without_compute() {
        assert_eq!(
            select_backend(PresentationMode::Auto, &capabilities(false, false)).active,
            ActiveBackend::CpuReference
        );
    }

    #[test]
    fn forced_gpu_degrades_safely() {
        assert_eq!(
            select_backend(PresentationMode::Gpu, &capabilities(false, false)).active,
            ActiveBackend::CpuReference
        );
    }

    #[test]
    fn forced_cpu_ignores_gpu_support() {
        assert_eq!(
            select_backend(PresentationMode::CpuReference, &capabilities(true, true)).active,
            ActiveBackend::CpuReference
        );
    }

    #[test]
    fn forced_readback_is_honored_on_native_hardware() {
        assert_eq!(
            select_backend(PresentationMode::GpuReadback, &capabilities(true, true)).active,
            ActiveBackend::GpuReadback
        );
    }

    #[test]
    fn oversized_effect_falls_back_without_changing_the_device_backend() {
        let runtime = select_backend(PresentationMode::Auto, &capabilities(true, true));
        let effect = select_effect_backend(&runtime, 1001, 1000);
        assert_eq!(runtime.active, ActiveBackend::Gpu);
        assert_eq!(effect.active, ActiveBackend::CpuReference);
        assert!(effect.reason.contains("1001 particles"));
    }
}
