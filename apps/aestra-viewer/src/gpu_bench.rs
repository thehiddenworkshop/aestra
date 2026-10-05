//! GPU-timestamp benchmark capture.
//!
//! Runs the viewer for a fixed number of frames, samples the render diagnostics
//! store each frame, and writes percentile statistics for Aestra's GPU passes to
//! JSON. Unlike the eyeballed 1-second averages of `LogDiagnosticsPlugin`, this
//! records a fresh-observation distribution (p50/p95/p99) so the numbers are meaningful
//! above the timestamp noise floor.
//!
//! Requires `RenderDiagnosticsPlugin` and a GPU with timestamp-query support
//! (Vulkan/DX12); diagnostics whose GPU value is unavailable simply produce no
//! samples.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use aestra_bevy::{
    EffectClipInstance, EffectProfiler, EffectRuntimeStatus, GpuCapabilities, PresentedEffect,
    ProfileValue,
    gpu::{GpuEventLinkStatistics, GpuParticleStatistics, GpuSimulationFrame, GpuSimulationTiming},
};
use bevy::app::AppExit;
use bevy::diagnostic::{DiagnosticMeasurement, DiagnosticsStore};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use serde::Serialize;

/// Default frames measured after warm-up, chosen to sit well above the GPU
/// timestamp noise floor.
pub const DEFAULT_GPU_BENCH_FRAMES: usize = 600;
/// Default warm-up frames skipped before sampling (pipeline compilation, clock ramp).
pub const DEFAULT_GPU_BENCH_WARMUP: usize = 120;

/// Drives a fixed-length GPU-timestamp capture and writes JSON on completion.
#[derive(Resource)]
pub struct GpuBenchPlan {
    output: PathBuf,
    effect: String,
    warmup: usize,
    warmup_remaining: usize,
    frames: usize,
    remaining: usize,
    samples: BTreeMap<String, Vec<f64>>,
    work: BTreeMap<String, WorkStats>,
    diagnostic_times: BTreeMap<String, bevy::platform::time::Instant>,
    simulation_sequences: BTreeMap<Entity, u64>,
    simulation_frames: BTreeMap<String, Vec<SimulationFrame>>,
    history_policy: aestra_bevy::PlaybackHistoryPolicy,
    presentation: Option<BenchPresentation>,
    adapter: Option<BenchAdapter>,
    effect_backends: BTreeMap<String, BTreeSet<String>>,
    physical_window_sizes: BTreeSet<[u32; 2]>,
    instances: BTreeMap<String, BenchInstance>,
    peak_active_clip_instances: usize,
    active_clip_instances_at_end: usize,
    project_work: BTreeMap<String, WorkStats>,
    particle_lights: Vec<crate::particle_light_bench::Observation>,
    particle_light_pool: Vec<ParticleLightPoolObservation>,
    particle_light_gpu: Vec<ParticleLightGpuObservation>,
    light_skipped_busy: u64,
    light_overwritten_results: u64,
    cluster_buffers: Vec<crate::particle_light_bench::clusters::Observation>,
    cluster_overwritten_results: u64,
    cluster_main: Vec<crate::particle_light_bench::clusters::MainObservation>,
}

/// Stable authored identity for transient ECS owners, retained after clips expire.
#[derive(Serialize)]
struct BenchInstance {
    root: String,
    clip_path: Vec<String>,
    source_effect: String,
    name: String,
    seed: String,
    particle_capacity: usize,
    history_policy: &'static str,
}

impl BenchInstance {
    fn observed(
        entity: Entity,
        presented: &PresentedEffect,
        clip: Option<&EffectClipInstance>,
    ) -> Self {
        Self {
            root: clip.map_or(entity, |clip| clip.root).to_string(),
            clip_path: clip.map_or_else(Vec::new, |clip| {
                clip.path.iter().map(ToString::to_string).collect()
            }),
            source_effect: presented.instance.effect().source.to_string(),
            name: presented.instance.effect().name.clone(),
            seed: format!("0x{:016x}", presented.instance.seed()),
            particle_capacity: presented.instance.effect().max_particles,
            history_policy: match presented.instance.history_policy() {
                aestra_bevy::PlaybackHistoryPolicy::PlaybackOnly => "playback-only",
                aestra_bevy::PlaybackHistoryPolicy::ReplayEnabled => "replay-enabled",
            },
        }
    }
}

/// Requested viewer setup, not a claim that an unsupported policy was applied by a fallback.
#[derive(Serialize)]
pub struct BenchPresentation {
    seed: String,
    requested_backend: &'static str,
    camera: &'static str,
    render_mode: &'static str,
    transparent_order: &'static str,
    max_gpu_particles: u32,
    quality_tier: String,
    legacy_material_migration: bool,
    raster_probe: Option<crate::fireworks_f4::RasterProbeSetup>,
    fixed_simulation_step_seconds: Option<f64>,
    response: crate::photographic::CaptureResponse,
    particle_light_benchmark_fixture: bool,
    particle_light_allocation_snapshots: bool,
    particle_light_realization: bool,
    particle_light_mode: Option<&'static str>,
    particle_light_gpu_cap: Option<u32>,
    /// Requested initial [Z-slice, index-list] native cluster capacities; may
    /// grow, not a renderer allocation measurement or hard memory limit.
    particle_light_cluster_initial_capacities: Option<[usize; 2]>,
    particle_light_cluster_adaptive_index_target: Option<usize>,
    representative_lights: bool,
    global_particle_light_cap: Option<u32>,
    particle_light_memory_mib: Option<u32>,
    headless_target: Option<[u32; 2]>,
}

impl BenchPresentation {
    pub fn from_config(config: &crate::ViewerConfig) -> Self {
        Self {
            seed: format!("0x{:016x}", config.resolved_seed()),
            requested_backend: match config.presentation {
                aestra_bevy::PresentationMode::Auto => "auto",
                aestra_bevy::PresentationMode::Gpu => "gpu",
                aestra_bevy::PresentationMode::GpuReadback => "gpu-readback",
                aestra_bevy::PresentationMode::CpuReference => "cpu",
            },
            camera: if config
                .capture_mode
                .as_ref()
                .is_some_and(crate::CaptureMode::is_editor_viewport_smoke)
            {
                "editor-viewport-smoke"
            } else if config.fireworks_f0 {
                match config.fireworks_camera {
                    crate::FireworksCamera::Close => "close",
                    crate::FireworksCamera::Audience => "audience",
                    crate::FireworksCamera::Wide => "wide",
                }
            } else if config.view_3d {
                "framed-3d"
            } else {
                "2d"
            },
            render_mode: if config.wireframe {
                "wireframe"
            } else {
                "rendered"
            },
            transparent_order: match config.transparent_order {
                aestra_bevy::TransparentOrderMode::Fast => "fast",
                aestra_bevy::TransparentOrderMode::StableCapture => "stable-capture",
            },
            max_gpu_particles: config.max_gpu_particles,
            quality_tier: config.tier.name.clone(),
            legacy_material_migration: config.semantic_materials,
            raster_probe: match config.fireworks_probe {
                Some(crate::FireworksProbe::Raster(probe)) => Some(probe.setup()),
                _ => None,
            },
            fixed_simulation_step_seconds: config.probe_bench_step().map(|step| step.as_secs_f64()),
            response: crate::photographic::CaptureResponse::new(
                config.photographic,
                config.sprite_minimum_pixels,
            )
            .with_trail_sampling(config.trail_minimum_pixels),
            particle_light_benchmark_fixture: config.particle_light_bench,
            particle_light_allocation_snapshots: config.particle_light_allocations,
            particle_light_realization: config.particle_light_realization,
            particle_light_mode: config.particle_light_realization.then_some(
                match config.particle_light_mode.unwrap_or_default() {
                    aestra_bevy::ParticleLightMode::PortableAsync => "async",
                    aestra_bevy::ParticleLightMode::SameFrameGpu => "gpu",
                },
            ),
            representative_lights: config.transient_lights,
            particle_light_cluster_initial_capacities: (config.particle_light_mode
                == Some(aestra_bevy::ParticleLightMode::SameFrameGpu))
            .then(|| crate::particle_light_bench::cluster_capacities(&config.tier.name)),
            particle_light_cluster_adaptive_index_target: (config.particle_light_mode
                == Some(aestra_bevy::ParticleLightMode::SameFrameGpu))
            .then(|| crate::particle_light_bench::cluster_capacities(&config.tier.name)[1]),
            particle_light_gpu_cap: (config.particle_light_mode
                == Some(aestra_bevy::ParticleLightMode::SameFrameGpu))
            .then(|| {
                config
                    .particle_light_gpu_cap
                    .unwrap_or(config.particle_light_cap.unwrap_or(
                        match config.tier.name.as_str() {
                            "high" => 96,
                            "medium" => 48,
                            _ => 24,
                        },
                    ))
            }),
            global_particle_light_cap: config.particle_light_bench.then(|| {
                config
                    .particle_light_cap
                    .unwrap_or(match config.tier.name.as_str() {
                        "high" => 96,
                        "medium" => 48,
                        _ => 24,
                    })
            }),
            particle_light_memory_mib: config
                .particle_light_bench
                .then_some(config.particle_light_memory_mib),
            headless_target: config
                .headless_bench
                .then_some([crate::VIEW_WIDTH, crate::VIEW_HEIGHT]),
        }
    }
}

#[derive(Serialize)]
struct BenchAdapter {
    name: String,
    backend: String,
    driver: String,
}

impl GpuBenchPlan {
    pub fn new(output: PathBuf, effect: String, warmup: usize, frames: usize) -> Self {
        Self {
            output,
            effect,
            warmup,
            warmup_remaining: warmup,
            frames,
            remaining: frames,
            samples: BTreeMap::new(),
            work: BTreeMap::new(),
            diagnostic_times: BTreeMap::new(),
            simulation_sequences: BTreeMap::new(),
            simulation_frames: BTreeMap::new(),
            history_policy: default(),
            presentation: None,
            adapter: None,
            effect_backends: BTreeMap::new(),
            physical_window_sizes: BTreeSet::new(),
            instances: BTreeMap::new(),
            peak_active_clip_instances: 0,
            active_clip_instances_at_end: 0,
            project_work: BTreeMap::new(),
            particle_lights: Vec::new(),
            particle_light_pool: Vec::new(),
            particle_light_gpu: Vec::new(),
            light_skipped_busy: 0,
            light_overwritten_results: 0,
            cluster_buffers: Vec::new(),
            cluster_overwritten_results: 0,
            cluster_main: Vec::new(),
        }
    }

    pub fn with_history_policy(mut self, policy: aestra_bevy::PlaybackHistoryPolicy) -> Self {
        self.history_policy = policy;
        self
    }

    pub fn with_presentation(mut self, presentation: BenchPresentation) -> Self {
        self.presentation = Some(presentation);
        self
    }

    fn write_report(&self) -> Result<(), String> {
        let metrics = self
            .samples
            .iter()
            .map(|(path, values)| (path.clone(), Stats::from_samples(values)))
            .collect();
        let report = GpuBenchReport {
            particle_lights: &self.particle_lights,
            particle_light_pool: &self.particle_light_pool,
            particle_light_gpu: &self.particle_light_gpu,
            cluster_buffers: &self.cluster_buffers,
            cluster_buffers_scope: crate::particle_light_bench::clusters::SCOPE,
            allocator_scope: crate::particle_light_bench::clusters::allocations::SCOPE,
            cluster_overwritten_results: self.cluster_overwritten_results,
            cluster_main: &self.cluster_main,
            light_skipped_busy: self.light_skipped_busy,
            light_overwritten_results: self.light_overwritten_results,
            presentation: self.presentation.as_ref(),
            adapter: self.adapter.as_ref(),
            effect_backends: &self.effect_backends,
            physical_window_sizes: &self.physical_window_sizes,
            instances: &self.instances,
            peak_active_clip_instances: self.peak_active_clip_instances,
            active_clip_instances_at_end: self.active_clip_instances_at_end,
            project_work: &self.project_work,
            effect: self.effect.clone(),
            history_policy: match self.history_policy {
                aestra_bevy::PlaybackHistoryPolicy::PlaybackOnly => "playback-only",
                aestra_bevy::PlaybackHistoryPolicy::ReplayEnabled => "replay-enabled",
            },
            warmup: self.warmup,
            frames: self.frames,
            metrics,
            work: &self.work,
            simulation_frames: &self.simulation_frames,
            simulation_total: self
                .simulation_frames
                .iter()
                .map(|(owner, frames)| {
                    (
                        owner.clone(),
                        Stats::from_samples(
                            &frames.iter().map(|frame| frame.gpu_ms).collect::<Vec<_>>(),
                        ),
                    )
                })
                .collect(),
            simulation_by_work: self
                .simulation_frames
                .iter()
                .map(|(owner, frames)| {
                    let mut groups = BTreeMap::<String, Vec<f64>>::new();
                    for frame in frames {
                        groups
                            .entry(format!(
                                "ticks={:?},observations={},workgroups={},checkpoint_bytes={:?}",
                                frame.fixed_ticks,
                                frame.trail_observations,
                                frame.trail_workgroups,
                                frame.checkpoint_capture_bytes
                            ))
                            .or_default()
                            .push(frame.gpu_ms);
                    }
                    (
                        owner.clone(),
                        groups
                            .into_iter()
                            .map(|(key, times)| (key, Stats::from_samples(&times)))
                            .collect(),
                    )
                })
                .collect(),
            work_scope: "Asynchronous host observations received during the measured window; late warm-up results can arrive in this window and its final GPU frames can arrive after capture ends. Peaks and event readbacks are not frame-aligned; event totals include warm-up/replay. Missing measurements are null. Simulation frames pair one effect's timestamp window with executed ticks, total history observations/workgroups, and coupled particle/trail checkpoint capture bytes; sequence deduplicates retained results. Workgroups count padded history dispatches, not live particles, and exclude ribbons. Checkpoint bytes exclude domain snapshots/restores; other paths report null rather than zero. Requested time is playback seconds, not proof catch-up reached it. Other diagnostic paths can publish only the last fixed-tick observation; do not sum stage percentiles into whole-frame costs. GPU/CPU durations use milliseconds.",
        };
        let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
        std::fs::write(&self.output, json).map_err(|error| error.to_string())
    }

    fn record_simulation(&mut self, owner: Entity, frame: GpuSimulationFrame, measured: bool) {
        let sequence = self.simulation_sequences.entry(owner).or_default();
        if frame.sequence <= *sequence {
            return;
        }
        *sequence = frame.sequence;
        if measured {
            self.simulation_frames
                .entry(owner.to_string())
                .or_default()
                .push(SimulationFrame {
                    sequence: frame.sequence,
                    gpu_ms: frame.nanoseconds as f64 / 1_000_000.0,
                    requested_time: frame.requested_time,
                    fixed_ticks: frame.work.fixed_ticks,
                    trail_observations: frame.work.trail_observations,
                    trail_workgroups: frame.work.trail_workgroups,
                    checkpoint_capture_bytes: frame.work.checkpoint_capture_bytes,
                });
        }
    }

    fn record_diagnostic(
        &mut self,
        path: &str,
        measurement: &DiagnosticMeasurement,
        measured: bool,
    ) {
        if self
            .diagnostic_times
            .get(path)
            .is_some_and(|time| *time >= measurement.time)
        {
            return;
        }
        self.diagnostic_times
            .insert(path.to_owned(), measurement.time);
        if measured && measurement.value.is_finite() {
            self.samples
                .entry(path.to_owned())
                .or_default()
                .push(measurement.value);
        }
    }
}

#[derive(Serialize)]
struct SimulationFrame {
    sequence: u64,
    gpu_ms: f64,
    requested_time: f32,
    fixed_ticks: Option<u32>,
    trail_observations: u32,
    trail_workgroups: u64,
    checkpoint_capture_bytes: Option<u64>,
}

/// Distribution of one diagnostic across the capture, in the diagnostic's native
/// unit (milliseconds for the render-pass timers).
#[derive(Serialize)]
struct Stats {
    samples: usize,
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
    min: f64,
    mean: f64,
    stddev: f64,
}

impl Stats {
    fn from_samples(values: &[f64]) -> Self {
        if values.is_empty() {
            return Self {
                samples: 0,
                p50: 0.0,
                p95: 0.0,
                p99: 0.0,
                max: 0.0,
                min: 0.0,
                mean: 0.0,
                stddev: 0.0,
            };
        }
        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let count = sorted.len();
        let mean = sorted.iter().sum::<f64>() / count as f64;
        let variance = sorted.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / count as f64;
        Self {
            samples: count,
            p50: percentile(&sorted, 0.50),
            p95: percentile(&sorted, 0.95),
            p99: percentile(&sorted, 0.99),
            max: sorted[count - 1],
            min: sorted[0],
            mean,
            stddev: variance.sqrt(),
        }
    }
}

fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    let rank = (fraction * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

#[derive(Serialize)]
struct GpuBenchReport<'a> {
    cluster_buffers: &'a [crate::particle_light_bench::clusters::Observation],
    cluster_buffers_scope: &'static str,
    allocator_scope: &'static str,
    cluster_overwritten_results: u64,
    cluster_main: &'a [crate::particle_light_bench::clusters::MainObservation],
    /// Counter copies tagged with the originating render frame. No source/selected
    /// records are mapped. Timings are independent fresh render diagnostics, not
    /// frame-paired with these counters; final in-flight results may be absent.
    particle_lights: &'a [crate::particle_light_bench::Observation],
    /// Prior PostUpdate's main-world pool, not frame-paired with render counters
    /// or GPU diagnostics. Readback age/lag refer to the last newly accepted set.
    particle_light_pool: &'a [ParticleLightPoolObservation],
    /// Latest shared render observation plus independent main/extraction CPU
    /// timings. Bounds, NOT GPU active counts; not frame-paired with counters.
    particle_light_gpu: &'a [ParticleLightGpuObservation],
    light_skipped_busy: u64,
    light_overwritten_results: u64,
    instances: &'a BTreeMap<String, BenchInstance>,
    /// ECS presentations, not simultaneously alive particle cohorts.
    peak_active_clip_instances: usize,
    active_clip_instances_at_end: usize,
    /// Concurrent active project totals; never a sum of separate owners' peaks.
    project_work: &'a BTreeMap<String, WorkStats>,
    presentation: Option<&'a BenchPresentation>,
    adapter: Option<&'a BenchAdapter>,
    effect_backends: &'a BTreeMap<String, BTreeSet<String>>,
    /// All physical primary-window sizes observed in the measured window, not logical DPI units.
    physical_window_sizes: &'a BTreeSet<[u32; 2]>,
    effect: String,
    history_policy: &'static str,
    warmup: usize,
    frames: usize,
    metrics: BTreeMap<String, Stats>,
    /// Host-observed asynchronous root/child metrics during the measured window.
    /// Counts can lag. Event totals cover buffer lifetime, including warm-up;
    /// they are not deltas for this window or authoritative replay accounting.
    work: &'a BTreeMap<String, WorkStats>,
    work_scope: &'static str,
    simulation_frames: &'a BTreeMap<String, Vec<SimulationFrame>>,
    simulation_by_work: BTreeMap<String, BTreeMap<String, Stats>>,
    simulation_total: BTreeMap<String, Stats>,
}

#[derive(Serialize)]
struct ParticleLightGpuObservation {
    sample: usize,
    dispatches: u64,
    sequence: u64,
    reserved_slots: u32,
    written_capacity: u32,
    invalid_sources: usize,
    buffer_bytes: u64,
    rejection: Option<String>,
    reserve_cpu_ms: f64,
    authorize_cpu_ms: f64,
}
impl ParticleLightGpuObservation {
    fn new(sample: usize, s: aestra_bevy::ParticleLightGpuObservation) -> Self {
        Self {
            sample,
            dispatches: s.dispatches,
            sequence: s.sequence,
            reserved_slots: s.reserved_slots,
            written_capacity: s.written_capacity,
            invalid_sources: s.invalid_sources,
            buffer_bytes: s.buffer_bytes,
            rejection: s.rejection.map(|e| format!("{e:?}")),
            reserve_cpu_ms: s.reserve_cpu_ms,
            authorize_cpu_ms: s.authorize_cpu_ms,
        }
    }
}

#[derive(Serialize)]
struct ParticleLightPoolObservation {
    sample: usize,
    active: usize,
    allocated: usize,
    sequence: u64,
    accepted_updates: u64,
    copied_bytes: u64,
    update_age_ms: f64,
    frame_lag: u64,
    pool_update_ms: f64,
    pending: usize,
    staging_bytes: u64,
    submitted: u64,
    completed: u64,
    skipped_busy: u64,
    stale_callbacks: u64,
    overwritten: u64,
    failed: u64,
    rejected: u64,
    rejection: Option<String>,
    stale_sources: u64,
    discarded_packets: u64,
    expired: u64,
    representative_active: usize,
    representative_allocated: usize,
}
impl ParticleLightPoolObservation {
    fn new(
        sample: usize,
        s: &aestra_bevy::ParticleLightStatistics,
        representative: Option<&aestra_bevy::TransientLightStatistics>,
    ) -> Self {
        Self {
            sample,
            active: s.active,
            allocated: s.allocated,
            sequence: s.last_sequence,
            accepted_updates: s.updates,
            copied_bytes: s.copied_bytes,
            update_age_ms: s.update_age_seconds * 1000.0,
            frame_lag: s.frame_lag,
            pool_update_ms: s.pool_update_ms,
            pending: s.readback.pending,
            staging_bytes: s.readback.staging_bytes,
            submitted: s.readback.submitted,
            completed: s.readback.completed,
            skipped_busy: s.readback.skipped_busy,
            stale_callbacks: s.readback.stale_callbacks,
            overwritten: s.readback.overwritten,
            failed: s.readback.failed,
            rejected: s.readback.rejected,
            rejection: s.readback.rejection.clone(),
            stale_sources: s.stale_sources,
            discarded_packets: s.discarded_packets,
            expired: s.expired,
            representative_active: representative.map_or(0, |r| r.active),
            representative_allocated: representative.map_or(0, |r| r.allocated),
        }
    }
}

#[derive(Default, Serialize)]
struct WorkStats {
    /// Last asynchronous measured observation before an owner disappears.
    /// Not a frame-aligned guarantee of cleanup at clip end.
    last_live_particles: Option<u32>,
    last_occupied_trails: Option<u32>,
    min_live_particles: Option<u32>,
    peak_live_particles: Option<u32>,
    min_occupied_trails: Option<u32>,
    peak_occupied_trails: Option<u32>,
    peak_retired_trails: Option<u32>,
    max_trail_evictions: Option<u32>,
    max_truncated_trails: Option<u32>,
    estimated_buffer_memory_bytes: Option<u64>,
    peak_estimated_buffer_memory_bytes: Option<u64>,
    event_readback_samples: Option<u64>,
    source_event_overflow: Option<u64>,
    links: Option<Vec<LinkStats>>,
}

#[derive(Serialize)]
struct LinkStats {
    captured_demand: u64,
    expansion_omitted: u64,
    accepted: u64,
    destination_rejected: u64,
}

impl WorkStats {
    fn record(&mut self, profiler: &EffectProfiler, events: Option<&GpuEventLinkStatistics>) {
        self.record_profile(&profiler.0, events);
    }

    fn record_profile(
        &mut self,
        profile: &aestra_bevy::EffectProfile,
        events: Option<&GpuEventLinkStatistics>,
    ) {
        fn peak(target: &mut Option<u32>, value: ProfileValue<u32>) {
            if let ProfileValue::Measured(value) = value {
                *target = Some(target.unwrap_or(0).max(value));
            }
        }
        fn minimum(target: &mut Option<u32>, value: ProfileValue<u32>) {
            if let ProfileValue::Measured(value) = value {
                *target = Some(target.unwrap_or(value).min(value));
            }
        }
        // Peaks alone can hide a cohort retiring during a raster benchmark. Like
        // peaks, these are asynchronous host observations, not per-frame certificates.
        minimum(&mut self.min_live_particles, profile.alive_particles);
        minimum(&mut self.min_occupied_trails, profile.occupied_trails);
        if let ProfileValue::Measured(value) = profile.alive_particles {
            self.last_live_particles = Some(value);
        }
        if let ProfileValue::Measured(value) = profile.occupied_trails {
            self.last_occupied_trails = Some(value);
        }
        peak(&mut self.peak_live_particles, profile.alive_particles);
        peak(&mut self.peak_occupied_trails, profile.occupied_trails);
        peak(&mut self.peak_retired_trails, profile.retired_trails);
        peak(&mut self.max_trail_evictions, profile.trail_evictions);
        peak(&mut self.max_truncated_trails, profile.truncated_trails);
        self.estimated_buffer_memory_bytes = profile.buffer_memory_bytes.value();
        if let Some(bytes) = self.estimated_buffer_memory_bytes {
            self.peak_estimated_buffer_memory_bytes = Some(
                self.peak_estimated_buffer_memory_bytes
                    .unwrap_or(0)
                    .max(bytes),
            );
        }
        if let Some(events) = events.filter(|events| events.readback_samples > 0) {
            self.event_readback_samples = Some(events.readback_samples);
            self.source_event_overflow = Some(events.source_overflow);
            self.links = Some(
                events
                    .links
                    .iter()
                    .map(|link| LinkStats {
                        captured_demand: link.captured_demand,
                        expansion_omitted: link.expansion_omitted,
                        accepted: link.accepted,
                        destination_rejected: link.destination_rejected,
                    })
                    .collect(),
            );
        }
    }
}

/// Samples Aestra's GPU diagnostics each frame and exits once the capture is done.
/// A no-op unless a [`GpuBenchPlan`] resource is present.
#[derive(SystemParam)]
#[allow(clippy::type_complexity)]
pub struct BenchEffects<'w, 's> {
    clusters: Option<Res<'w, crate::particle_light_bench::clusters::Mailbox>>,
    cluster_settings: Option<Res<'w, bevy::light::cluster::GlobalClusterSettings>>,
    cluster_views: Query<
        'w,
        's,
        (
            Entity,
            &'static Camera,
            &'static bevy::light::cluster::Clusters,
        ),
        With<Camera3d>,
    >,
    gpu: Option<Res<'w, aestra_bevy::ParticleLightGpuStatistics>>,
    pool: Option<Res<'w, aestra_bevy::ParticleLightStatistics>>,
    representative: Option<Res<'w, aestra_bevy::TransientLightStatistics>>,
    light_start: Option<Res<'w, crate::particle_light_bench::Start>>,
    lights: Option<Res<'w, crate::particle_light_bench::Mailbox>>,
    roots: Query<
        'w,
        's,
        (
            Entity,
            &'static EffectProfiler,
            Option<&'static GpuEventLinkStatistics>,
            Option<&'static EffectRuntimeStatus>,
        ),
    >,
    instances: Query<
        'w,
        's,
        (
            Entity,
            &'static PresentedEffect,
            Option<&'static EffectClipInstance>,
        ),
    >,
    children: Query<
        'w,
        's,
        (
            Entity,
            &'static EffectClipInstance,
            Option<&'static GpuEventLinkStatistics>,
            Option<&'static EffectRuntimeStatus>,
        ),
    >,
    projects: Query<'w, 's, (Entity, &'static aestra_bevy::ProjectProfiler)>,
}

pub fn drive_gpu_bench(
    plan: Option<ResMut<GpuBenchPlan>>,
    diagnostics: Res<DiagnosticsStore>,
    effects: BenchEffects,
    capabilities: Option<Res<GpuCapabilities>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    timings: Query<(
        Entity,
        &PresentedEffect,
        &GpuParticleStatistics,
        &GpuSimulationTiming,
    )>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut plan) = plan else {
        return;
    };
    if plan.remaining == 0 {
        return;
    }
    if let Some(clusters) = &effects.clusters {
        let (observations, overwritten) = clusters.take();
        plan.cluster_buffers.extend(observations);
        plan.cluster_overwritten_results = overwritten;
    }
    if effects
        .light_start
        .as_ref()
        .is_some_and(|start| !start.ready)
    {
        // Shader preparation must not consume the forward show or warm-up window.
        if let Some(mailbox) = &effects.lights {
            mailbox.take();
        }
        return;
    }
    if let Some(mailbox) = &effects.lights {
        let (observations, busy, overwritten) = mailbox.take();
        plan.light_skipped_busy = busy;
        plan.light_overwritten_results = overwritten;
        plan.particle_lights
            .extend(observations.into_iter().filter(|o| o.tick.measured));
    }
    let measured = plan.warmup_remaining == 0;
    if measured
        && effects.clusters.is_some()
        && let Some(settings) = &effects.cluster_settings
    {
        let sample = plan.frames - plan.remaining;
        let mut views = effects
            .cluster_views
            .iter()
            .filter(|(_, camera, _)| camera.is_active)
            .map(
                |(entity, _, clusters)| crate::particle_light_bench::clusters::MainView {
                    main_entity: entity.to_string(),
                    dimensions: clusters.dimensions.to_array(),
                    last_native_index_demand: clusters.last_frame_total_cluster_index_count,
                },
            )
            .collect::<Vec<_>>();
        views.sort_by(|a, b| a.main_entity.cmp(&b.main_entity));
        plan.cluster_main
            .push(crate::particle_light_bench::clusters::MainObservation {
                sample,
                gpu_clustering_enabled: settings.gpu_clustering.is_some(),
                adaptive_index_target: settings.view_cluster_bindings_max_indices,
                views,
            });
    }
    if measured && let Some(gpu) = &effects.gpu {
        let sample = plan.frames - plan.remaining;
        plan.particle_light_gpu
            .push(ParticleLightGpuObservation::new(sample, gpu.snapshot()));
    }
    if measured && let Some(pool) = &effects.pool {
        let sample = plan.frames - plan.remaining;
        plan.particle_light_pool
            .push(ParticleLightPoolObservation::new(
                sample,
                pool,
                effects.representative.as_deref(),
            ));
    }
    // Capture identity during warm-up too: short-lived children must not vanish
    // from the report merely because their ECS owner was destroyed before measurement.
    let mut active_clips = 0;
    for (entity, presented, clip) in &effects.instances {
        active_clips += usize::from(clip.is_some());
        plan.instances.insert(
            entity.to_string(),
            BenchInstance::observed(entity, presented, clip),
        );
    }
    plan.peak_active_clip_instances = plan.peak_active_clip_instances.max(active_clips);
    plan.active_clip_instances_at_end = active_clips;
    for (owner, effect, context, timing) in &timings {
        if let Some(frame) = timing.frame_sample(&effect.instance, context) {
            plan.record_simulation(owner, frame, measured);
        }
    }
    if !measured {
        // Consume old diagnostic observations too, so warm-up values retained
        // across the boundary are not repeatedly counted as measured frames.
        for diagnostic in diagnostics.iter() {
            if let Some(measurement) = diagnostic.measurement() {
                plan.record_diagnostic(diagnostic.path().as_str(), measurement, false);
            }
        }
        plan.warmup_remaining -= 1;
        return;
    }
    for diagnostic in diagnostics.iter() {
        let path = diagnostic.path().as_str();
        // Aestra's passes, and the transparent passes its particles and volumes draw in.
        let included = [
            "aestra::gpu::simulate",
            "aestra::gpu::particle_lights",
            "aestra::gpu::particle_light_copy",
            "aestra::gpu::particle_light_inject",
            "cluster",
            "aestra::gpu::trail_history",
            "aestra::gpu::trail_particles",
            "aestra::gpu::trail_compaction",
            "aestra::gpu::extension_stages",
            "main_transparent_pass_2d",
            "main_transparent_pass_3d",
        ]
        .iter()
        .any(|pass| path.contains(pass))
            || path.ends_with("aestra::bench::full_frame/elapsed_gpu")
            || path.ends_with("aestra::bench::full_frame/elapsed_cpu");
        if included && let Some(measurement) = diagnostic.measurement() {
            plan.record_diagnostic(path, measurement, true);
        }
    }
    if let Some(capabilities) = capabilities.filter(|caps| caps.detected) {
        plan.adapter = Some(BenchAdapter {
            name: capabilities.adapter_name.clone(),
            backend: capabilities.backend.clone(),
            driver: capabilities.driver.clone(),
        });
    }
    for window in &windows {
        plan.physical_window_sizes
            .insert([window.physical_width(), window.physical_height()]);
    }
    for (entity, profiler, events, runtime) in &effects.roots {
        if let Some(runtime) = runtime {
            plan.effect_backends
                .entry(entity.to_string())
                .or_default()
                .insert(runtime.active.to_string());
        }
        plan.work
            .entry(entity.to_string())
            .or_default()
            .record(profiler, events);
    }
    // EffectProfiler is intentionally root-only. Child observations come from
    // the public project profile; do not accidentally benchmark only the empty carrier.
    for (entity, clip, events, runtime) in &effects.children {
        if let Some(runtime) = runtime {
            plan.effect_backends
                .entry(entity.to_string())
                .or_default()
                .insert(runtime.active.to_string());
        }
        if let Ok((_, project)) = effects.projects.get(clip.root)
            && let Some(entry) = project
                .0
                .instances
                .iter()
                .find(|entry| entry.path == clip.path)
        {
            plan.work
                .entry(entity.to_string())
                .or_default()
                .record_profile(&entry.profile, events);
        }
    }
    for (root, project) in &effects.projects {
        plan.project_work
            .entry(root.to_string())
            .or_default()
            .record_profile(&project.0.total, None);
    }
    plan.remaining -= 1;
    if plan.remaining == 0 {
        match plan.write_report() {
            Ok(()) => {
                info!(
                    "aestra-viewer: wrote GPU benchmark to {}",
                    plan.output.display()
                );
                exit.write(AppExit::Success);
            }
            Err(error) => {
                eprintln!("aestra-viewer: GPU benchmark failed: {error}");
                exit.write(AppExit::error());
            }
        }
    }
}

pub fn publish_light_tick(
    plan: Option<Res<GpuBenchPlan>>,
    time: Res<Time>,
    mut tick: ResMut<crate::particle_light_bench::Tick>,
) {
    if let Some(plan) = plan {
        tick.index = (plan.warmup - plan.warmup_remaining + plan.frames - plan.remaining) as u64;
        tick.measured = plan.warmup_remaining == 0;
        tick.host_elapsed_seconds = time.elapsed_secs();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_report_preserves_bounds_and_rejections_without_inventing_active_counts() {
        let observation = ParticleLightGpuObservation::new(
            7,
            aestra_bevy::ParticleLightGpuObservation {
                sequence: 42,
                reserved_slots: 48,
                written_capacity: 0,
                rejection: Some(aestra_bevy::ParticleLightGpuRejection::ManifestBudget),
                reserve_cpu_ms: 0.01,
                authorize_cpu_ms: 0.02,
                ..default()
            },
        );
        let report = serde_json::to_value(observation).unwrap();
        assert_eq!(report["sample"], 7);
        assert_eq!(report["sequence"], 42);
        assert_eq!(report["reserved_slots"], 48);
        assert_eq!(report["written_capacity"], 0);
        assert_eq!(report["rejection"], "ManifestBudget");
        assert_eq!(report["reserve_cpu_ms"], 0.01);
        assert!(report.get("active").is_none());
    }

    #[test]
    fn benchmark_retains_instance_identity_after_warmup_owner_despawns() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("instances.json");
        let compiled = aestra_bevy::EffectCompiler::default()
            .compile(&aestra_bevy::EffectAsset::new("transient", 1.0))
            .unwrap();
        let source = compiled.source.to_string();
        let mut presented = PresentedEffect::new(compiled.into());
        presented.instance.set_seed(42);
        presented.set_history_policy(aestra_bevy::PlaybackHistoryPolicy::PlaybackOnly);
        let mut app = App::new();
        app.add_message::<AppExit>()
            .init_resource::<DiagnosticsStore>()
            .insert_resource(GpuBenchPlan::new(output.clone(), "fixture".into(), 1, 1))
            .add_systems(Update, drive_gpu_bench);
        let entity = app.world_mut().spawn(presented).id();
        app.update(); // identity is recorded even before the measured window
        app.world_mut().despawn(entity);
        app.update();
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
        let identity = &report["instances"][entity.to_string()];
        assert_eq!(identity["root"], entity.to_string());
        assert_eq!(identity["source_effect"], source);
        assert_eq!(identity["clip_path"], serde_json::json!([]));
        assert_eq!(identity["seed"], "0x000000000000002a");
        assert_eq!(identity["history_policy"], "playback-only");
        assert_eq!(report["active_clip_instances_at_end"], 0);
        assert!(report["work"].as_object().unwrap().is_empty());
    }

    #[test]
    fn project_peaks_use_concurrent_snapshots_and_cleanup_not_sums_of_owner_peaks() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("project.json");
        let mut app = App::new();
        app.add_message::<AppExit>()
            .init_resource::<DiagnosticsStore>()
            .insert_resource(GpuBenchPlan::new(output.clone(), "fixture".into(), 0, 3))
            .add_systems(Update, drive_gpu_bench);
        let root = app
            .world_mut()
            .spawn(aestra_bevy::ProjectProfiler::default())
            .id();
        for (live, memory) in [(100, 2000), (80, 3000), (0, 8)] {
            let mut project = app
                .world_mut()
                .get_mut::<aestra_bevy::ProjectProfiler>(root)
                .unwrap();
            project.0.total.alive_particles = ProfileValue::Measured(live);
            project.0.total.buffer_memory_bytes = ProfileValue::Estimated(memory);
            app.update();
        }
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
        let work = &report["project_work"][root.to_string()];
        assert_eq!(work["peak_live_particles"], 100);
        assert_eq!(work["last_live_particles"], 0);
        assert_eq!(work["peak_estimated_buffer_memory_bytes"], 3000);
        assert_eq!(work["estimated_buffer_memory_bytes"], 8);
        assert!(work["links"].is_null()); // Never invent project-wide admission.
    }

    #[test]
    fn benchmark_records_requested_presentation_and_actual_fallback_and_window_sizes() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("report.json");
        let config = crate::ViewerConfig::from_iter(
            [
                "--fireworks-f0",
                "--camera",
                "wide",
                "--hdr",
                "--sprite-min-pixels",
                "2",
                "--history",
                "playback-only",
                "--seed",
                "0x1234",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        let plan = GpuBenchPlan::new(output.clone(), "fixture".into(), 1, 2)
            .with_history_policy(config.history_policy)
            .with_presentation(BenchPresentation::from_config(&config));
        let mut app = App::new();
        app.add_message::<AppExit>()
            .init_resource::<DiagnosticsStore>()
            .insert_resource(plan)
            .insert_resource(GpuCapabilities {
                detected: true,
                adapter_name: "test adapter".into(),
                backend: "test backend".into(),
                driver: "test driver".into(),
                ..default()
            })
            .add_systems(Update, drive_gpu_bench);
        let compiled = aestra_bevy::EffectCompiler::default()
            .compile(&aestra_bevy::EffectAsset::new("report", 6.0))
            .unwrap();
        let entity = app
            .world_mut()
            .spawn((
                EffectProfiler(aestra_bevy::EffectProfile::from_compiled(&compiled)),
                EffectRuntimeStatus {
                    active: aestra_bevy::ActiveBackend::CpuReference,
                    reason: "budget fallback".into(),
                    compatibility: aestra_bevy::CompatibilityReport::compatible(
                        aestra_bevy::CompatibilityTarget::CpuReference,
                    ),
                },
            ))
            .id();
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution: bevy::window::WindowResolution::new(320, 180),
                    ..default()
                },
                bevy::window::PrimaryWindow,
            ))
            .id();
        app.update(); // warm-up size must not enter the measured set
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .resolution
            .set_physical_resolution(960, 540);
        app.update();
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .resolution
            .set_physical_resolution(1280, 720);
        // Preserve all observed decisions, not only the last backend in a mixed window.
        app.world_mut()
            .get_mut::<EffectRuntimeStatus>(entity)
            .unwrap()
            .active = aestra_bevy::ActiveBackend::Gpu;
        app.update();
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
        assert_eq!(report["presentation"]["camera"], "wide");
        assert_eq!(report["presentation"]["seed"], "0x0000000000001234");
        assert_eq!(report["presentation"]["requested_backend"], "auto");
        assert_eq!(report["presentation"]["response"]["hdr"], true);
        assert_eq!(
            report["presentation"]["response"]["sprite_minimum_pixels"],
            2.0
        );
        assert_eq!(
            report["effect_backends"][entity.to_string()],
            serde_json::json!(["CPU reference", "native GPU"])
        );
        assert_eq!(report["adapter"]["name"], "test adapter");
        assert_eq!(
            report["physical_window_sizes"],
            serde_json::json!([[960, 540], [1280, 720]])
        );
        assert!(
            report["metrics"].as_object().unwrap().is_empty(),
            "missing timings must not masquerade as measured zero"
        );
    }

    #[test]
    fn diagnostics_only_count_fresh_finite_measurements_after_warmup() {
        let mut plan = GpuBenchPlan::new(PathBuf::new(), "fixture".into(), 1, 3);
        let mut measurement = DiagnosticMeasurement {
            time: bevy::platform::time::Instant::now(),
            value: 2.0,
        };
        plan.record_diagnostic("gpu", &measurement, false);
        plan.record_diagnostic("gpu", &measurement, true);
        measurement.time += std::time::Duration::from_millis(1);
        plan.record_diagnostic("gpu", &measurement, true);
        plan.record_diagnostic("gpu", &measurement, true);
        measurement.time += std::time::Duration::from_millis(1);
        measurement.value = f64::NAN;
        plan.record_diagnostic("gpu", &measurement, true);
        assert_eq!(plan.samples["gpu"], [2.0]);
    }

    #[test]
    fn simulation_frames_pair_work_and_time_without_recounting_old_results() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("report.json");
        let mut plan = GpuBenchPlan::new(output.clone(), "fixture".into(), 1, 3);
        let owner = Entity::PLACEHOLDER;
        let mut frame = GpuSimulationFrame {
            sequence: 1,
            nanoseconds: 2_000_000,
            requested_time: 1.0,
            work: aestra_bevy::gpu::GpuSimulationWork {
                fixed_ticks: Some(1),
                trail_observations: 1,
                trail_workgroups: 128,
                checkpoint_capture_bytes: Some(0),
            },
        };
        plan.record_simulation(owner, frame, false);
        plan.record_simulation(owner, frame, true); // retained warm-up result
        frame.sequence = 2;
        plan.record_simulation(owner, frame, true);
        plan.record_simulation(owner, frame, true); // retained measured result
        frame.sequence = 4; // asynchronous maps may skip a frame
        frame.nanoseconds = 4_000_000;
        frame.work.fixed_ticks = Some(2);
        frame.work.trail_observations = 2;
        frame.work.trail_workgroups = 256;
        frame.work.checkpoint_capture_bytes = Some(1024);
        plan.record_simulation(owner, frame, true);
        frame.sequence = 3; // late result must not overwrite/recount
        plan.record_simulation(owner, frame, true);
        plan.write_report().unwrap();
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
        let frames = report["simulation_frames"][owner.to_string()]
            .as_array()
            .unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[1]["sequence"], 4);
        assert_eq!(frames[1]["gpu_ms"], 4.0);
        assert_eq!(frames[1]["requested_time"], 1.0);
        assert_eq!(frames[1]["fixed_ticks"], 2);
        assert_eq!(frames[1]["trail_workgroups"], 256);
        assert_eq!(frames[1]["checkpoint_capture_bytes"], 1024);
        let groups = report["simulation_by_work"][owner.to_string()]
            .as_object()
            .unwrap();
        assert_eq!(groups.len(), 2);
        assert!(groups.values().all(|group| group["samples"] == 1));
        assert_eq!(report["simulation_total"][owner.to_string()]["samples"], 2);
    }

    #[test]
    fn work_report_keeps_missing_measurements_distinct_from_zero_and_counts_admitted_work() {
        let compiled = aestra_bevy::EffectCompiler::default()
            .compile(&aestra_bevy::EffectAsset::new("report", 6.0))
            .unwrap();
        let mut profile = EffectProfiler(aestra_bevy::EffectProfile::from_compiled(&compiled));
        profile.0.alive_particles = ProfileValue::Estimated(9000);
        let mut work = WorkStats::default();
        work.record(&profile, Some(&GpuEventLinkStatistics::default()));
        assert_eq!(work.peak_live_particles, None);
        assert_eq!(work.min_live_particles, None);
        assert_eq!(work.min_occupied_trails, None);
        assert_eq!(work.last_live_particles, None);
        assert!(work.links.is_none());
        profile.0.alive_particles = ProfileValue::Measured(7200);
        profile.0.occupied_trails = ProfileValue::Measured(10400);
        profile.0.retired_trails = ProfileValue::Measured(3200);
        let events = GpuEventLinkStatistics {
            links: vec![aestra_bevy::gpu::GpuEventLinkCounts {
                captured_demand: 12800,
                accepted: 12800,
                ..default()
            }],
            readback_samples: 100,
            ..default()
        };
        work.record(&profile, Some(&events));
        profile.0.alive_particles = ProfileValue::Measured(0);
        work.record(&profile, Some(&events));
        let json = serde_json::to_value(&work).unwrap();
        assert_eq!(json["peak_live_particles"], 7200);
        assert_eq!(json["min_live_particles"], 0);
        assert_eq!(json["last_live_particles"], 0);
        assert_eq!(json["min_occupied_trails"], 10400);
        assert_eq!(json["peak_occupied_trails"], 10400);
        assert_eq!(json["peak_retired_trails"], 3200);
        assert_eq!(json["links"][0]["accepted"], 12800);
        assert_eq!(json["source_event_overflow"], 0);
        assert!(json["max_trail_evictions"].is_null());
    }

    #[test]
    fn report_preserves_configured_warmup_after_it_is_consumed() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("report.json");
        let mut plan = GpuBenchPlan::new(output.clone(), "fixture".into(), 120, 2)
            .with_history_policy(aestra_bevy::PlaybackHistoryPolicy::PlaybackOnly);
        plan.warmup_remaining = 0;
        plan.samples.insert("gpu".into(), vec![0.2, 0.3]);
        plan.write_report().unwrap();
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
        assert_eq!(report["warmup"], 120);
        assert_eq!(report["frames"], 2);
        assert_eq!(report["history_policy"], "playback-only");
        assert!(report["adapter"].is_null());
        assert!(report["presentation"].is_null());
        assert!(report["effect_backends"].as_object().unwrap().is_empty());
        assert!(
            report["physical_window_sizes"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}
