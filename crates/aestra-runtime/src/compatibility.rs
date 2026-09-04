use std::{collections::BTreeSet, fmt};

/// Renderer capabilities referenced by an engine-neutral compiled effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RendererCapability {
    SpriteParticles,
    FlipbookParticles,
    MeshParticles,
}

impl fmt::Display for RendererCapability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::MeshParticles => "mesh particles",
            Self::SpriteParticles => "sprite particles",
            Self::FlipbookParticles => "flipbook particles",
        })
    }
}

/// Portable compiler output describing the presentation resources an effect may require.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffectRequirements {
    pub max_particles: usize,
    pub renderers: BTreeSet<RendererCapability>,
    pub gpu_simulation: bool,
    pub native_gpu_presentation: bool,
}

/// Engine-neutral view of the capabilities supplied by one concrete rendering backend.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BackendCapabilities {
    pub compute_shaders: bool,
    pub compute_workgroups: bool,
    pub storage_buffers: bool,
    pub gpu_readback: bool,
    pub indirect_draw: bool,
    pub vertex_storage: bool,
    pub max_particles: usize,
    pub renderers: BTreeSet<RendererCapability>,
}

/// Presentation path evaluated by a [`CompatibilityReport`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatibilityTarget {
    CpuReference,
    GpuReadback,
    NativeGpu,
}

impl fmt::Display for CompatibilityTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::CpuReference => "CPU reference",
            Self::GpuReadback => "GPU readback",
            Self::NativeGpu => "native GPU",
        })
    }
}

/// Stable category for one compatibility failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompatibilityIssueCode {
    ComputeShadersUnavailable,
    ComputeWorkgroupsUnavailable,
    StorageBuffersUnavailable,
    GpuReadbackUnavailable,
    IndirectDrawUnavailable,
    VertexStorageUnavailable,
    ParticleCapacityExceeded,
    RendererUnsupported,
    BackendRejected,
}

/// One actionable incompatibility between a compiled effect and a backend target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatibilityIssue {
    pub code: CompatibilityIssueCode,
    pub message: String,
}

impl CompatibilityIssue {
    pub fn new(code: CompatibilityIssueCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Structured result of checking compiler-derived requirements against backend capabilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatibilityReport {
    pub target: CompatibilityTarget,
    pub issues: Vec<CompatibilityIssue>,
}

impl CompatibilityReport {
    pub fn compatible(target: CompatibilityTarget) -> Self {
        Self {
            target,
            issues: Vec::new(),
        }
    }

    pub fn from_issues(
        target: CompatibilityTarget,
        issues: impl IntoIterator<Item = CompatibilityIssue>,
    ) -> Self {
        Self {
            target,
            issues: issues.into_iter().collect(),
        }
    }

    pub fn is_compatible(&self) -> bool {
        self.issues.is_empty()
    }

    pub fn summary(&self) -> String {
        if self.is_compatible() {
            format!("effect is compatible with {} presentation", self.target)
        } else {
            self.issues
                .iter()
                .map(|issue| issue.message.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        }
    }
}

impl EffectRequirements {
    pub fn compatibility_report(
        &self,
        capabilities: &BackendCapabilities,
        target: CompatibilityTarget,
    ) -> CompatibilityReport {
        if target != CompatibilityTarget::NativeGpu
            && self.renderers.contains(&RendererCapability::MeshParticles)
        {
            return CompatibilityReport::from_issues(
                target,
                [CompatibilityIssue::new(
                    CompatibilityIssueCode::RendererUnsupported,
                    "mesh particles require native GPU presentation",
                )],
            );
        }
        if target == CompatibilityTarget::CpuReference {
            return CompatibilityReport::compatible(target);
        }

        let mut issues = Vec::new();
        if self.gpu_simulation {
            require(
                capabilities.compute_shaders,
                CompatibilityIssueCode::ComputeShadersUnavailable,
                "compute shaders are unavailable",
                &mut issues,
            );
            require(
                capabilities.compute_workgroups,
                CompatibilityIssueCode::ComputeWorkgroupsUnavailable,
                "required compute workgroup sizes are unavailable",
                &mut issues,
            );
            require(
                capabilities.storage_buffers,
                CompatibilityIssueCode::StorageBuffersUnavailable,
                "required storage-buffer bindings are unavailable",
                &mut issues,
            );
            if self.max_particles > capabilities.max_particles {
                issues.push(CompatibilityIssue::new(
                    CompatibilityIssueCode::ParticleCapacityExceeded,
                    format!(
                        "effect requests {} particles but the backend budget is {}",
                        self.max_particles, capabilities.max_particles
                    ),
                ));
            }
        }

        if target == CompatibilityTarget::GpuReadback {
            require(
                capabilities.gpu_readback,
                CompatibilityIssueCode::GpuReadbackUnavailable,
                "GPU readback presentation is unavailable",
                &mut issues,
            );
        } else if self.native_gpu_presentation {
            require(
                capabilities.indirect_draw,
                CompatibilityIssueCode::IndirectDrawUnavailable,
                "indirect drawing is unavailable",
                &mut issues,
            );
            require(
                capabilities.vertex_storage,
                CompatibilityIssueCode::VertexStorageUnavailable,
                "vertex-stage storage buffers are unavailable",
                &mut issues,
            );
        }

        for renderer in self.renderers.difference(&capabilities.renderers) {
            issues.push(CompatibilityIssue::new(
                CompatibilityIssueCode::RendererUnsupported,
                format!("{renderer} are unsupported by this backend"),
            ));
        }
        CompatibilityReport::from_issues(target, issues)
    }
}

fn require(
    available: bool,
    code: CompatibilityIssueCode,
    message: &'static str,
    issues: &mut Vec<CompatibilityIssue>,
) {
    if !available {
        issues.push(CompatibilityIssue::new(code, message));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requirements() -> EffectRequirements {
        EffectRequirements {
            max_particles: 128,
            renderers: BTreeSet::from([RendererCapability::SpriteParticles]),
            gpu_simulation: true,
            native_gpu_presentation: true,
        }
    }

    fn capabilities() -> BackendCapabilities {
        BackendCapabilities {
            compute_shaders: true,
            compute_workgroups: true,
            storage_buffers: true,
            gpu_readback: true,
            indirect_draw: true,
            vertex_storage: true,
            max_particles: 256,
            renderers: BTreeSet::from([RendererCapability::SpriteParticles]),
        }
    }

    #[test]
    fn compatible_native_backend_produces_an_empty_report() {
        let report =
            requirements().compatibility_report(&capabilities(), CompatibilityTarget::NativeGpu);
        assert!(report.is_compatible());
    }

    #[test]
    fn reports_compute_storage_indirect_and_capacity_failures_structurally() {
        let mut capabilities = capabilities();
        capabilities.compute_shaders = false;
        capabilities.storage_buffers = false;
        capabilities.indirect_draw = false;
        capabilities.max_particles = 64;
        let report =
            requirements().compatibility_report(&capabilities, CompatibilityTarget::NativeGpu);
        let codes = report
            .issues
            .iter()
            .map(|issue| issue.code)
            .collect::<Vec<_>>();
        assert!(codes.contains(&CompatibilityIssueCode::ComputeShadersUnavailable));
        assert!(codes.contains(&CompatibilityIssueCode::StorageBuffersUnavailable));
        assert!(codes.contains(&CompatibilityIssueCode::IndirectDrawUnavailable));
        assert!(codes.contains(&CompatibilityIssueCode::ParticleCapacityExceeded));
    }

    #[test]
    fn cpu_reference_is_always_a_valid_portable_fallback() {
        let report = requirements().compatibility_report(
            &BackendCapabilities::default(),
            CompatibilityTarget::CpuReference,
        );
        assert!(report.is_compatible());
    }

    #[test]
    fn gpu_readback_does_not_require_native_draw_features() {
        let mut capabilities = capabilities();
        capabilities.indirect_draw = false;
        capabilities.vertex_storage = false;
        let report =
            requirements().compatibility_report(&capabilities, CompatibilityTarget::GpuReadback);
        assert!(report.is_compatible());
    }

    #[test]
    fn unsupported_renderer_is_reported_by_stable_code() {
        let mut capabilities = capabilities();
        capabilities.renderers.clear();
        let report =
            requirements().compatibility_report(&capabilities, CompatibilityTarget::NativeGpu);
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.code == CompatibilityIssueCode::RendererUnsupported)
        );
    }
}
