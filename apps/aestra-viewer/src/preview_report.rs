use aestra_bevy::{
    AestraRuntimeStatus, AestraSettings, CompiledEffect, Diagnostic, EffectProfile,
    EffectRuntimeStatus, GpuCapabilities, ProfileValue, ProfileValueSource,
};
use serde::Serialize;
use std::{fs, path::Path};

pub const PREVIEW_REPORT_FILE: &str = "preview-report.json";
const PREVIEW_REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct CompilerPreviewData {
    pub diagnostics: Vec<Diagnostic>,
    effect_id: String,
    effect_name: String,
    effect_duration_seconds: f32,
    playback_mode: String,
    seek_mode: String,
    emitter_count: usize,
    material_count: usize,
    material_program_count: usize,
    max_particles: usize,
    optimizations: PreviewOptimizations,
    material_program_fingerprints: Vec<MaterialProgramFingerprint>,
}

impl CompilerPreviewData {
    pub fn new(
        effect: &CompiledEffect,
        diagnostics: Vec<Diagnostic>,
        material_program_fingerprints: Vec<(String, String)>,
    ) -> Self {
        Self {
            diagnostics,
            effect_id: effect.source.to_string(),
            effect_name: effect.name.clone(),
            effect_duration_seconds: effect.duration,
            playback_mode: format!("{:?}", effect.playback_mode),
            seek_mode: format!("{:?}", effect.seek_mode),
            emitter_count: effect.emitters.len(),
            material_count: effect.materials.len(),
            material_program_count: effect.material_programs.len(),
            max_particles: effect.max_particles,
            optimizations: PreviewOptimizations {
                constant_expressions: effect.optimizations.constant_expressions,
                runtime_parameter_reads: effect.optimizations.runtime_parameter_reads,
                eliminated_attributes: effect.optimizations.eliminated_attributes,
            },
            material_program_fingerprints: material_program_fingerprints
                .into_iter()
                .map(|(program_id, fingerprint)| MaterialProgramFingerprint {
                    program_id,
                    fingerprint,
                })
                .collect(),
        }
    }
}

pub struct PreviewCaptureData<'a> {
    pub sampled_frames: &'a [u64],
    pub seed: u64,
    pub width: u32,
    pub height: u32,
    pub columns: u32,
    pub rows: u32,
    pub tick_rate: u32,
}

pub struct PreviewRuntimeData<'a> {
    pub runtime: &'a AestraRuntimeStatus,
    pub effect_runtime: Option<&'a EffectRuntimeStatus>,
    pub settings: &'a AestraSettings,
    pub capabilities: &'a GpuCapabilities,
    pub profile: Option<&'a EffectProfile>,
}

pub fn write_preview_report(
    output_directory: &Path,
    capture: PreviewCaptureData<'_>,
    compiler: &CompilerPreviewData,
    runtime: PreviewRuntimeData<'_>,
    capture_error: Option<&str>,
) -> Result<(), String> {
    fs::create_dir_all(output_directory)
        .map_err(|error| format!("could not create preview report directory: {error}"))?;
    let active_backend = runtime
        .effect_runtime
        .map_or(runtime.runtime.active, |status| status.active);
    let selection_reason = runtime
        .effect_runtime
        .map_or(&runtime.runtime.reason, |status| &status.reason);
    let compatibility = runtime.effect_runtime.map(|status| PreviewCompatibility {
        target: status.compatibility.target.to_string(),
        compatible: status.compatibility.is_compatible(),
        issues: status
            .compatibility
            .issues
            .iter()
            .map(|issue| PreviewCompatibilityIssue {
                code: format!("{:?}", issue.code),
                message: issue.message.clone(),
            })
            .collect(),
    });
    let report = PreviewReport {
        schema_version: PREVIEW_REPORT_SCHEMA_VERSION,
        status: if capture_error.is_some() {
            PreviewStatus::Failed
        } else {
            PreviewStatus::Succeeded
        },
        errors: capture_error.into_iter().map(str::to_owned).collect(),
        effect: Some(PreviewEffect {
            id: compiler.effect_id.clone(),
            name: compiler.effect_name.clone(),
            duration_seconds: compiler.effect_duration_seconds,
            playback_mode: compiler.playback_mode.clone(),
            seek_mode: compiler.seek_mode.clone(),
            emitter_count: compiler.emitter_count,
            material_count: compiler.material_count,
            material_program_count: compiler.material_program_count,
            max_particles: compiler.max_particles,
        }),
        capture: Some(PreviewCapture {
            tick_rate: capture.tick_rate,
            seed: format!("0x{:016x}", capture.seed),
            frame_width: capture.width,
            frame_height: capture.height,
            contact_sheet_columns: capture.columns,
            contact_sheet_rows: capture.rows,
            contact_sheet: "contact-sheet.png".to_owned(),
            frames: capture
                .sampled_frames
                .iter()
                .enumerate()
                .map(|(index, frame)| PreviewFrame {
                    index,
                    simulation_frame: *frame,
                    time_seconds: *frame as f64 / f64::from(capture.tick_rate),
                    image: format!("frame-{index:03}.png"),
                })
                .collect(),
        }),
        compiler: PreviewCompiler {
            diagnostics: compiler.diagnostics.clone(),
            optimizations: Some(compiler.optimizations.clone()),
            material_program_fingerprints: compiler.material_program_fingerprints.clone(),
        },
        runtime: Some(PreviewRuntime {
            requested_backend: format!("{:?}", runtime.runtime.requested),
            active_backend: active_backend.to_string(),
            selection_reason: selection_reason.clone(),
            compatibility,
            adapter: PreviewAdapter {
                detected: runtime.capabilities.detected,
                name: runtime.capabilities.adapter_name.clone(),
                backend: runtime.capabilities.backend.clone(),
                device_type: runtime.capabilities.device_type.clone(),
                driver: runtime.capabilities.driver.clone(),
                limitations: runtime.capabilities.limitations.clone(),
            },
            physical_gpu_particle_capacity: runtime.capabilities.max_particles,
            configured_gpu_particle_budget: runtime.settings.max_gpu_particles,
            effective_gpu_particle_budget: runtime
                .capabilities
                .max_particles
                .min(runtime.settings.max_gpu_particles),
        }),
        metrics: runtime.profile.map(PreviewMetrics::from),
    };
    write_json(output_directory, &report)
}

pub fn write_preview_failure_report(
    output_directory: &Path,
    error: &str,
    diagnostics: &[Diagnostic],
) -> Result<(), String> {
    fs::create_dir_all(output_directory)
        .map_err(|source| format!("could not create preview report directory: {source}"))?;
    let report = PreviewReport {
        schema_version: PREVIEW_REPORT_SCHEMA_VERSION,
        status: PreviewStatus::Failed,
        errors: vec![error.to_owned()],
        effect: None,
        capture: None,
        compiler: PreviewCompiler {
            diagnostics: diagnostics.to_vec(),
            optimizations: None,
            material_program_fingerprints: Vec::new(),
        },
        runtime: None,
        metrics: None,
    };
    write_json(output_directory, &report)
}

fn write_json(output_directory: &Path, report: &PreviewReport) -> Result<(), String> {
    let json = serde_json::to_string_pretty(report)
        .map_err(|error| format!("could not serialize preview report: {error}"))?;
    fs::write(
        output_directory.join(PREVIEW_REPORT_FILE),
        format!("{json}\n"),
    )
    .map_err(|error| format!("could not write preview report: {error}"))
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum PreviewStatus {
    Succeeded,
    Failed,
}

#[derive(Serialize)]
struct PreviewReport {
    schema_version: u32,
    status: PreviewStatus,
    errors: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    effect: Option<PreviewEffect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    capture: Option<PreviewCapture>,
    compiler: PreviewCompiler,
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime: Option<PreviewRuntime>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metrics: Option<PreviewMetrics>,
}

#[derive(Serialize)]
struct PreviewEffect {
    id: String,
    name: String,
    duration_seconds: f32,
    playback_mode: String,
    seek_mode: String,
    emitter_count: usize,
    material_count: usize,
    material_program_count: usize,
    max_particles: usize,
}

#[derive(Serialize)]
struct PreviewCapture {
    tick_rate: u32,
    seed: String,
    frame_width: u32,
    frame_height: u32,
    contact_sheet_columns: u32,
    contact_sheet_rows: u32,
    contact_sheet: String,
    frames: Vec<PreviewFrame>,
}

#[derive(Serialize)]
struct PreviewFrame {
    index: usize,
    simulation_frame: u64,
    time_seconds: f64,
    image: String,
}

#[derive(Serialize)]
struct PreviewCompiler {
    diagnostics: Vec<Diagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    optimizations: Option<PreviewOptimizations>,
    material_program_fingerprints: Vec<MaterialProgramFingerprint>,
}

#[derive(Debug, Clone, Serialize)]
struct PreviewOptimizations {
    constant_expressions: usize,
    runtime_parameter_reads: usize,
    eliminated_attributes: usize,
}

#[derive(Debug, Clone, Serialize)]
struct MaterialProgramFingerprint {
    program_id: String,
    fingerprint: String,
}

#[derive(Serialize)]
struct PreviewRuntime {
    requested_backend: String,
    active_backend: String,
    selection_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    compatibility: Option<PreviewCompatibility>,
    adapter: PreviewAdapter,
    physical_gpu_particle_capacity: u32,
    configured_gpu_particle_budget: u32,
    effective_gpu_particle_budget: u32,
}

#[derive(Serialize)]
struct PreviewCompatibility {
    target: String,
    compatible: bool,
    issues: Vec<PreviewCompatibilityIssue>,
}

#[derive(Serialize)]
struct PreviewCompatibilityIssue {
    code: String,
    message: String,
}

#[derive(Serialize)]
struct PreviewAdapter {
    detected: bool,
    name: String,
    backend: String,
    device_type: String,
    driver: String,
    limitations: Vec<String>,
}

#[derive(Serialize)]
struct PreviewMetrics {
    cpu_time_ns: PreviewMetric<u64>,
    gpu_time_ns: PreviewMetric<u64>,
    alive_particles: PreviewMetric<u32>,
    submitted_instances: PreviewMetric<u32>,
    peak_particles: PreviewMetric<u32>,
    particle_capacity: PreviewMetric<u32>,
    emitter_count: PreviewMetric<u32>,
    draw_calls: PreviewMetric<u32>,
    dispatch_count: PreviewMetric<u32>,
    estimated_overdraw: PreviewMetric<f32>,
    texture_sample_count: PreviewMetric<u32>,
    buffer_memory_bytes: PreviewMetric<u64>,
    texture_memory_bytes: PreviewMetric<u64>,
    collision_time_ns: PreviewMetric<u64>,
    emitters: Vec<PreviewEmitterMetrics>,
    platform_warnings: Vec<String>,
}

impl From<&EffectProfile> for PreviewMetrics {
    fn from(profile: &EffectProfile) -> Self {
        Self {
            cpu_time_ns: profile.cpu_time_ns.into(),
            gpu_time_ns: profile.gpu_time_ns.into(),
            alive_particles: profile.alive_particles.into(),
            submitted_instances: profile.submitted_instances.into(),
            peak_particles: profile.peak_particles.into(),
            particle_capacity: profile.particle_capacity.into(),
            emitter_count: profile.emitter_count.into(),
            draw_calls: profile.draw_calls.into(),
            dispatch_count: profile.dispatch_count.into(),
            estimated_overdraw: profile.estimated_overdraw.into(),
            texture_sample_count: profile.texture_sample_count.into(),
            buffer_memory_bytes: profile.buffer_memory_bytes.into(),
            texture_memory_bytes: profile.texture_memory_bytes.into(),
            collision_time_ns: profile.collision_time_ns.into(),
            emitters: profile
                .emitters
                .iter()
                .map(|emitter| PreviewEmitterMetrics {
                    id: emitter.source.to_string(),
                    name: emitter.name.clone(),
                    alive_particles: emitter.alive_particles,
                    peak_particles: emitter.peak_particles,
                    particle_capacity: emitter.particle_capacity,
                })
                .collect(),
            platform_warnings: profile.platform_warnings.clone(),
        }
    }
}

#[derive(Serialize)]
struct PreviewEmitterMetrics {
    id: String,
    name: String,
    alive_particles: u32,
    peak_particles: u32,
    particle_capacity: u32,
}

#[derive(Serialize)]
struct PreviewMetric<T> {
    source: PreviewMetricSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<T>,
}

impl<T: Copy> From<ProfileValue<T>> for PreviewMetric<T> {
    fn from(value: ProfileValue<T>) -> Self {
        Self {
            source: value.source().into(),
            value: value.value(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum PreviewMetricSource {
    Measured,
    Estimated,
    Unavailable,
}

impl From<ProfileValueSource> for PreviewMetricSource {
    fn from(source: ProfileValueSource) -> Self {
        match source {
            ProfileValueSource::Measured => Self::Measured,
            ProfileValueSource::Estimated => Self::Estimated,
            ProfileValueSource::Unavailable => Self::Unavailable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_bevy::{EffectAsset, EffectCompiler, Emitter};

    #[test]
    fn failure_report_is_machine_readable_and_versioned() {
        let directory = tempfile::tempdir().unwrap();
        write_preview_failure_report(directory.path(), "compile failed", &[]).unwrap();

        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.path().join(PREVIEW_REPORT_FILE)).unwrap())
                .unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["status"], "failed");
        assert_eq!(value["errors"][0], "compile failed");
        assert!(value.get("runtime").is_none());
    }

    #[test]
    fn compiler_report_keeps_effect_metrics_and_optimization_data() {
        let mut effect = EffectAsset::new("Preview", 2.0);
        effect.emitters.push(Emitter::basic_sprite("Sparks", 2.0));
        let compiled = EffectCompiler::default().compile(&effect).unwrap();
        let metadata = CompilerPreviewData::new(&compiled, Vec::new(), Vec::new());

        assert_eq!(metadata.effect_name, "Preview");
        assert_eq!(metadata.emitter_count, 1);
        assert_eq!(metadata.max_particles, compiled.max_particles);
    }

    #[test]
    fn successful_report_contains_exact_frames_artifacts_and_metric_provenance() {
        let directory = tempfile::tempdir().unwrap();
        let mut effect = EffectAsset::new("Preview", 2.0);
        effect.emitters.push(Emitter::basic_sprite("Sparks", 2.0));
        let compiled = EffectCompiler::default().compile(&effect).unwrap();
        let compiler = CompilerPreviewData::new(&compiled, Vec::new(), Vec::new());
        let profile = EffectProfile::from_compiled(&compiled);

        write_preview_report(
            directory.path(),
            PreviewCaptureData {
                sampled_frames: &[0, 30, 120],
                seed: 42,
                width: 960,
                height: 540,
                columns: 2,
                rows: 2,
                tick_rate: 60,
            },
            &compiler,
            PreviewRuntimeData {
                runtime: &AestraRuntimeStatus::default(),
                effect_runtime: None,
                settings: &AestraSettings::default(),
                capabilities: &GpuCapabilities::default(),
                profile: Some(&profile),
            },
            None,
        )
        .unwrap();

        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.path().join(PREVIEW_REPORT_FILE)).unwrap())
                .unwrap();
        assert_eq!(value["status"], "succeeded");
        assert_eq!(value["capture"]["frames"][1]["simulation_frame"], 30);
        assert_eq!(value["capture"]["frames"][1]["time_seconds"], 0.5);
        assert_eq!(value["capture"]["frames"][2]["image"], "frame-002.png");
        assert_eq!(value["metrics"]["emitter_count"]["source"], "measured");
    }
}
