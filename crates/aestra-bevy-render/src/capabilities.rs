use bevy::prelude::{Component, Resource};
use std::{collections::BTreeSet, fmt};

use crate::{
    BackendCapabilities, CompatibilityReport, CompatibilityTarget, EffectRequirements,
    PresentationMode, RendererCapability,
};
use aestra_gpu::material::MaterialBackendCapabilities;

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
    pub max_sampled_textures_per_shader_stage: u32,
    pub max_samplers_per_shader_stage: u32,
    pub max_uniform_buffer_binding_size: u64,
    pub max_buffer_size: u64,
    pub max_compute_workgroups_per_dimension: u32,
    pub max_compute_invocations_per_workgroup: u32,
    pub max_compute_workgroup_size_x: u32,
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
            max_sampled_textures_per_shader_stage: 0,
            max_samplers_per_shader_stage: 0,
            max_uniform_buffer_binding_size: 0,
            max_buffer_size: 0,
            max_compute_workgroups_per_dimension: 0,
            max_compute_invocations_per_workgroup: 0,
            max_compute_workgroup_size_x: 0,
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

    /// Converts Bevy/WGPU device discovery into the portable Aestra capability contract.
    pub fn backend_capabilities(&self, application_particle_budget: u32) -> BackendCapabilities {
        const REQUIRED_WORKGROUP_SIZE: u32 = 64;
        const REQUIRED_STORAGE_BINDINGS: u32 = 7;
        BackendCapabilities {
            compute_shaders: self.compute_shaders,
            compute_workgroups: self.max_compute_invocations_per_workgroup
                >= REQUIRED_WORKGROUP_SIZE
                && self.max_compute_workgroup_size_x >= REQUIRED_WORKGROUP_SIZE,
            storage_buffers: self.max_storage_buffers_per_shader_stage >= REQUIRED_STORAGE_BINDINGS
                && self.max_bindings_per_bind_group >= REQUIRED_STORAGE_BINDINGS
                && self.max_particles > 0,
            gpu_readback: self.compute_pipeline_supported,
            indirect_draw: self.indirect_execution,
            vertex_storage: self.vertex_storage && self.max_bind_groups >= 2,
            max_particles: self.max_particles.min(application_particle_budget) as usize,
            renderers: BTreeSet::from([
                RendererCapability::SpriteParticles,
                RendererCapability::FlipbookParticles,
                RendererCapability::MeshParticles,
                RendererCapability::RibbonParticles,
            ]),
        }
    }

    /// Converts concrete device limits into the portable semantic-material ABI contract.
    pub fn material_backend_capabilities(&self) -> MaterialBackendCapabilities {
        MaterialBackendCapabilities {
            max_bind_groups: self.max_bind_groups,
            max_bindings_per_bind_group: self.max_bindings_per_bind_group,
            max_sampled_textures_per_shader_stage: self.max_sampled_textures_per_shader_stage,
            max_samplers_per_shader_stage: self.max_samplers_per_shader_stage,
            max_uniform_buffer_binding_size: self.max_uniform_buffer_binding_size,
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
    pub compatibility: CompatibilityReport,
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

    let backend = capabilities.backend_capabilities(capabilities.max_particles);
    let native_report =
        baseline_requirements(true).compatibility_report(&backend, CompatibilityTarget::NativeGpu);
    let readback_report = baseline_requirements(false)
        .compatibility_report(&backend, CompatibilityTarget::GpuReadback);
    let (active, reason) = match requested {
        PresentationMode::CpuReference => (
            ActiveBackend::CpuReference,
            "CPU reference presentation was explicitly requested".into(),
        ),
        PresentationMode::Auto | PresentationMode::Gpu if native_report.is_compatible() => (
            ActiveBackend::Gpu,
            "compute, vertex storage, and indirect drawing are supported".into(),
        ),
        PresentationMode::Auto | PresentationMode::Gpu | PresentationMode::GpuReadback
            if readback_report.is_compatible() =>
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
            incompatible_reason(
                "GPU compute requirements are unavailable; using the CPU reference",
                &readback_report,
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
    requirements: &EffectRequirements,
    capabilities: &BackendCapabilities,
) -> EffectRuntimeStatus {
    let target = compatibility_target(runtime.active);
    let compatibility = requirements.compatibility_report(capabilities, target);
    if !compatibility.is_compatible() {
        EffectRuntimeStatus {
            active: ActiveBackend::CpuReference,
            reason: format!("{}; using the CPU reference", compatibility.summary()),
            compatibility,
        }
    } else {
        EffectRuntimeStatus {
            active: runtime.active,
            reason: runtime.reason.clone(),
            compatibility,
        }
    }
}

fn baseline_requirements(native_gpu_presentation: bool) -> EffectRequirements {
    EffectRequirements {
        max_particles: 1,
        gpu_simulation: true,
        native_gpu_presentation,
        ..EffectRequirements::default()
    }
}

fn compatibility_target(backend: ActiveBackend) -> CompatibilityTarget {
    match backend {
        ActiveBackend::Gpu => CompatibilityTarget::NativeGpu,
        ActiveBackend::GpuReadback => CompatibilityTarget::GpuReadback,
        ActiveBackend::Pending | ActiveBackend::CpuReference => CompatibilityTarget::CpuReference,
    }
}

fn with_limitations(prefix: &str, capabilities: &GpuCapabilities) -> String {
    if capabilities.limitations.is_empty() {
        prefix.into()
    } else {
        format!("{prefix}: {}", capabilities.limitations.join("; "))
    }
}

fn incompatible_reason(
    prefix: &str,
    report: &CompatibilityReport,
    capabilities: &GpuCapabilities,
) -> String {
    with_limitations(&format!("{prefix}: {}", report.summary()), capabilities)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capabilities(compute: bool, native: bool) -> GpuCapabilities {
        GpuCapabilities {
            detected: true,
            compute_shaders: compute,
            indirect_execution: native,
            vertex_storage: native,
            compute_pipeline_supported: compute,
            native_render_supported: native,
            max_bind_groups: if native { 2 } else { 1 },
            max_bindings_per_bind_group: if compute { 7 } else { 0 },
            max_storage_buffers_per_shader_stage: if compute { 7 } else { 0 },
            max_compute_invocations_per_workgroup: if compute { 64 } else { 0 },
            max_compute_workgroup_size_x: if compute { 64 } else { 0 },
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
    fn semantic_material_limits_translate_without_backend_types() {
        let capabilities = GpuCapabilities {
            max_bind_groups: 4,
            max_bindings_per_bind_group: 12,
            max_sampled_textures_per_shader_stage: 8,
            max_samplers_per_shader_stage: 6,
            max_uniform_buffer_binding_size: 32_768,
            ..GpuCapabilities::default()
        };

        let material = capabilities.material_backend_capabilities();

        assert_eq!(material.max_bind_groups, 4);
        assert_eq!(material.max_bindings_per_bind_group, 12);
        assert_eq!(material.max_sampled_textures_per_shader_stage, 8);
        assert_eq!(material.max_samplers_per_shader_stage, 6);
        assert_eq!(material.max_uniform_buffer_binding_size, 32_768);
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
    fn portable_conversion_applies_the_application_budget() {
        let backend = capabilities(true, true).backend_capabilities(128);
        assert_eq!(backend.max_particles, 128);
        assert!(
            backend
                .renderers
                .contains(&RendererCapability::SpriteParticles)
        );
        assert!(
            backend
                .renderers
                .contains(&RendererCapability::FlipbookParticles)
        );
    }

    #[test]
    fn backend_selection_uses_the_structured_storage_contract() {
        let mut capabilities = capabilities(true, true);
        capabilities.max_storage_buffers_per_shader_stage = 6;
        let status = select_backend(PresentationMode::Auto, &capabilities);
        assert_eq!(status.active, ActiveBackend::CpuReference);
        assert!(status.reason.contains("storage-buffer bindings"));
    }

    #[test]
    fn oversized_effect_falls_back_without_changing_the_device_backend() {
        let capabilities = capabilities(true, true);
        let runtime = select_backend(PresentationMode::Auto, &capabilities);
        let effect = select_effect_backend(
            &runtime,
            &EffectRequirements {
                max_particles: 1001,
                gpu_simulation: true,
                native_gpu_presentation: true,
                ..EffectRequirements::default()
            },
            &capabilities.backend_capabilities(1000),
        );
        assert_eq!(runtime.active, ActiveBackend::Gpu);
        assert_eq!(effect.active, ActiveBackend::CpuReference);
        assert!(effect.reason.contains("1001 particles"));
        assert_eq!(
            effect.compatibility.issues[0].code,
            crate::CompatibilityIssueCode::ParticleCapacityExceeded
        );
    }
}
