mod fireworks_f0;
mod fireworks_f3;
mod gpu_bench;
mod photographic;
mod preview_report;
mod velocity_f2;
mod visual_regression;

use aestra_authoring::{MaterialAuthoringDocument, migrate_legacy_sprite_materials};
use aestra_bevy::material::MaterialProgram;
use aestra_bevy::{
    ActiveBackend, AestraPlugin, AestraRuntimeStatus, AestraSettings, DEFAULT_GPU_PARTICLE_BUDGET,
    DEFAULT_PLAYBACK_TICK_RATE, EffectAsset, EffectCompiler, EffectPlayer, EffectProfiler,
    EffectRuntimeStatus, GpuCapabilities, PlaybackClock, PresentationMode, PresentedEffect,
    TransparentOrderMode,
};
use bevy::{
    app::AppExit,
    asset::AssetPlugin,
    camera::{Viewport, visibility::RenderLayers},
    diagnostic::LogDiagnosticsPlugin,
    ecs::system::SystemParam,
    prelude::*,
    render::diagnostic::RenderDiagnosticsPlugin,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
    render::{
        ExtractSchedule, MainWorld, RenderApp,
        render_resource::{CachedPipelineState, PipelineCache},
    },
    window::WindowResolution,
};
use image::{Rgba, RgbaImage, imageops};
#[cfg(test)]
use std::collections::BTreeMap;
use std::{
    env, fs,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use preview_report::{
    CompilerPreviewData, PreviewCaptureData, PreviewRuntimeData, write_preview_failure_report,
    write_preview_report,
};
use visual_regression::{ComparisonReport, compare_capture};

const SAMPLE_SOURCE: &str = include_str!("../../../assets/test/effects/prism_bloom.aestra.ron");
const VIEW_WIDTH: u32 = 960;
const VIEW_HEIGHT: u32 = 540;
const REGRESSION_SEED: u64 = 0xa357_2a11_5eed_0001;
const EDITOR_PREVIEW_X: u32 = 96;
const EDITOR_PREVIEW_Y: u32 = 64;
const EDITOR_PREVIEW_WIDTH: u32 = 640;
const EDITOR_PREVIEW_HEIGHT: u32 = 412;
const OVERLAY_PROBE_X: u32 = 784;
const OVERLAY_PROBE_Y: u32 = 96;
const OVERLAY_PROBE_SIZE: u32 = 144;

fn main() {
    // Linked extensions (extensible-stages M10) register before any effect is compiled.
    aestra_example_extension::link();
    aestra_fluid::link();
    let config = ViewerConfig::from_args().unwrap_or_else(|error| {
        eprintln!("aestra-viewer: {error}");
        eprintln!("usage: aestra-viewer [--effect file.aestra.ron | --fireworks-f0 [--fireworks-f0-probe event|event-hero|trail|trail-hero|event-trail|event-trail-large|event-trail-volley|event-trail-sparse]] [--camera close|audience|wide] [--semantic-materials] [--wireframe] [--diagnostics] [--view3d] [--gpu-bench output.json] [--backend auto|gpu|gpu-readback|cpu] [--history playback-only|replay-enabled] [--stable-transparency] [--seed number] [--tier high|medium|low] [--max-gpu-particles count] [--frames 8 | --sample-frames 0,30,60 | --sample-times 0,0.5,1] [--capture output-dir | --approve-visual-reference reference-dir | --visual-test reference-dir | --editor-viewport-smoke output-dir]");
        eprintln!("F2 distribution probes: f2-peony | f2-ring | f2-palm | f2-hemisphere-fan | f2-double-ring (with --fireworks-f0 --fireworks-f0-probe).");
        eprintln!("F3 shell prototypes: f3-peony | f3-chrysanthemum | f3-pistil | f3-willow (with --fireworks-f0 --fireworks-f0-probe; not production budget certification).");
        eprintln!("Photographic preview (opt-in HDR): --hdr [--exposure -8..8] [--tonemapping tony|aces|reinhard] [--bloom 0..1]. Any photographic option enables HDR. Exposure is fixed relative stops; 0 bloom disables glow.");
        eprintln!("Native GPU additive sprites: --sprite-min-pixels 0..8 (default 0; try 2). Expands tiny quads with inverse-area alpha attenuation; does not enable HDR.");
        std::process::exit(2);
    });
    // Packaged extensions (extensible-stages M12) installed in the effect's project.
    if let Some(effect_path) = &config.effect_path {
        let report = aestra_extension::host::link_packages(
            &aestra_extension::host::project_extension_dirs(effect_path),
            &Default::default(),
        );
        for package in report.problems() {
            eprintln!(
                "aestra-viewer: extension {} ({}): {}",
                package.id.as_ref().map_or("<unknown>", |id| id.as_str()),
                package.root.display(),
                package.status.describe()
            );
        }
    }
    let preview_seed = config.resolved_seed();
    let prepared = prepare_viewer(&config).unwrap_or_else(|failure| {
        report_preparation_failure(&config, &failure);
        eprintln!("aestra-viewer: {}", failure.message);
        std::process::exit(1);
    });
    let capture = config.capture_mode.clone().map(|mode| {
        CapturePlan::new(
            mode,
            &config.capture_sampling,
            preview_seed,
            prepared.compiled.duration,
        )
        .unwrap_or_else(|message| {
            let failure = PreparationFailure {
                message,
                diagnostics: prepared.compiler.diagnostics.clone(),
            };
            report_preparation_failure(&config, &failure);
            eprintln!("aestra-viewer: {}", failure.message);
            std::process::exit(2);
        })
    });
    let log_diagnostics = config.diagnostics;
    let asset_root = prepared.asset_root.to_string_lossy().into_owned();
    let gpu_bench_output = config.gpu_bench.clone();
    let gpu_bench_presentation = gpu_bench::BenchPresentation::from_config(&config);
    let history_policy = config.history_policy;
    let gpu_bench_effect = if config.fireworks_f0 {
        match config.fireworks_probe {
            Some(FireworksProbe::Event) => "fireworks_f0_event",
            Some(FireworksProbe::EventHero) => "fireworks_f1_event_hero",
            Some(FireworksProbe::Trail) => "fireworks_f0_trail",
            Some(FireworksProbe::TrailHero) => "fireworks_f1b_trail_hero",
            Some(FireworksProbe::EventTrail) => "fireworks_f1b_event_trail",
            Some(FireworksProbe::EventTrailLarge) => "fireworks_f1b_event_trail_large",
            Some(FireworksProbe::EventTrailVolley) => "fireworks_f1b_event_trail_volley",
            Some(FireworksProbe::EventTrailSparse) => "fireworks_f1b_event_trail_sparse",
            Some(FireworksProbe::Velocity(probe)) => probe.name(),
            Some(FireworksProbe::Shell(probe)) => probe.name(),
            None => "fireworks_f0",
        }
        .to_owned()
    } else {
        config
            .effect_path
            .as_ref()
            .and_then(|path| path.file_stem())
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "prism_bloom".to_owned())
    };

    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb(0.009, 0.012, 0.024)))
        .insert_resource(AestraSettings {
            presentation: config.presentation,
            max_gpu_particles: config.max_gpu_particles,
            transparent_order: config.transparent_order,
        })
        .insert_resource(aestra_bevy::SpriteSampling {
            minimum_pixels: config.sprite_minimum_pixels,
        })
        .insert_resource(prepared)
        .insert_resource(config)
        // Show a slice through plugin grid fields (a fluid's density) (fluid F1).
        .insert_resource(aestra_bevy::gpu::AestraDebugViews { field_slices: true })
        .add_plugins((
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: asset_root,
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Aestra Viewer".into(),
                        resolution: WindowResolution::new(VIEW_WIDTH, VIEW_HEIGHT),
                        resizable: true,
                        ..default()
                    }),
                    ..default()
                }),
            AestraPlugin,
            // Records GPU timestamps for Aestra's simulation pass (the
            // `aestra::gpu::simulate` span) and Bevy's transparent passes on
            // Vulkan/DX12. Pass `--diagnostics` to print them to the console via
            // `LogDiagnosticsPlugin`.
            RenderDiagnosticsPlugin,
        ))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                viewer_controls.before(aestra_bevy::AestraSet::Playback),
                update_hud,
                drive_capture
                    .after(update_hud)
                    .before(aestra_bevy::AestraSet::Playback),
                gpu_bench::drive_gpu_bench,
            ),
        );
    if let Some(capture) = capture {
        app.insert_resource(capture)
            .init_resource::<CaptureRenderReadiness>()
            // Every captured frame is the exact tick it names: a seek catches up in full, not paced
            // for responsiveness as in the editor.
            .insert_resource(aestra_bevy::gpu::AestraCatchupPacing { paced: false });
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app.add_systems(ExtractSchedule, publish_capture_render_readiness);
        }
    }
    if let Some(output) = gpu_bench_output {
        app.insert_resource(
            gpu_bench::GpuBenchPlan::new(
                output,
                gpu_bench_effect,
                gpu_bench::DEFAULT_GPU_BENCH_WARMUP,
                gpu_bench::DEFAULT_GPU_BENCH_FRAMES,
            )
            .with_history_policy(history_policy)
            .with_presentation(gpu_bench_presentation),
        );
    }
    if log_diagnostics {
        // Prints the diagnostics store (whole-frame CPU time and, on Vulkan/DX12,
        // GPU pass timings including `aestra::gpu::simulate`) to the console.
        app.add_plugins(LogDiagnosticsPlugin::default());
    }
    if let AppExit::Error(code) = app.run() {
        std::process::exit(i32::from(code.get()));
    }
}

#[derive(Resource)]
struct PreparedViewer {
    compiled: Arc<aestra_bevy::CompiledEffect>,
    project: Arc<aestra_bevy::CompiledEffectProject>,
    asset_root: PathBuf,
    compiler: CompilerPreviewData,
}

struct PreparationFailure {
    message: String,
    diagnostics: Vec<aestra_bevy::Diagnostic>,
}

#[derive(Resource)]
struct ViewerConfig {
    effect_path: Option<PathBuf>,
    fireworks_f0: bool,
    fireworks_probe: Option<FireworksProbe>,
    fireworks_camera: FireworksCamera,
    semantic_materials: bool,
    wireframe: bool,
    capture_mode: Option<CaptureMode>,
    capture_sampling: CaptureSampling,
    presentation: PresentationMode,
    transparent_order: TransparentOrderMode,
    max_gpu_particles: u32,
    preview_seed: Option<u64>,
    diagnostics: bool,
    gpu_bench: Option<PathBuf>,
    history_policy: aestra_bevy::PlaybackHistoryPolicy,
    /// View through a 3-D camera framing the effect's simulation domains (fluid F3): volumes need
    /// one. Otherwise the viewer is 2-D.
    view_3d: bool,
    /// The quality tier the effect is compiled for (fluid F12); `high` is the authored effect.
    tier: aestra_bevy::QualityTier,
    photographic: Option<photographic::PhotographicPreview>,
    sprite_minimum_pixels: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FireworksCamera {
    Close,
    Audience,
    Wide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FireworksProbe {
    Event,
    EventHero,
    Trail,
    TrailHero,
    EventTrail,
    EventTrailLarge,
    EventTrailVolley,
    EventTrailSparse,
    Velocity(velocity_f2::Probe),
    Shell(fireworks_f3::Probe),
}

impl FireworksProbe {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "event" => Some(Self::Event),
            "event-hero" => Some(Self::EventHero),
            "trail" => Some(Self::Trail),
            "trail-hero" => Some(Self::TrailHero),
            "event-trail" => Some(Self::EventTrail),
            "event-trail-large" => Some(Self::EventTrailLarge),
            "event-trail-volley" => Some(Self::EventTrailVolley),
            "event-trail-sparse" => Some(Self::EventTrailSparse),
            _ => velocity_f2::Probe::parse(value)
                .map(Self::Velocity)
                .or_else(|| fireworks_f3::Probe::parse(value).map(Self::Shell)),
        }
    }
}

impl FireworksCamera {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "close" => Some(Self::Close),
            "audience" => Some(Self::Audience),
            "wide" => Some(Self::Wide),
            _ => None,
        }
    }

    fn transform(self) -> Transform {
        let (eye, target) = match self {
            Self::Close => (Vec3::new(0.0, 24.0, 48.0), Vec3::new(0.0, 24.0, 0.0)),
            Self::Audience => (Vec3::new(0.0, 22.0, 85.0), Vec3::new(0.0, 22.0, 0.0)),
            Self::Wide => (Vec3::new(0.0, 45.0, 165.0), Vec3::new(0.0, 25.0, 0.0)),
        };
        Transform::from_translation(eye).looking_at(target, Vec3::Y)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum CaptureSampling {
    EvenlySpaced(usize),
    ExplicitFrames(Vec<u64>),
    ExplicitTimes(Vec<f32>),
}

#[derive(Clone)]
enum CaptureMode {
    Standard { output: PathBuf },
    Approve { reference: PathBuf },
    Compare { reference: PathBuf, output: PathBuf },
    EditorViewportSmoke { output: PathBuf },
}

impl CaptureMode {
    fn output_directory(&self) -> &PathBuf {
        match self {
            Self::Standard { output }
            | Self::Compare { output, .. }
            | Self::EditorViewportSmoke { output } => output,
            Self::Approve { reference } => reference,
        }
    }

    fn is_regression(&self) -> bool {
        !matches!(self, Self::Standard { .. })
    }

    fn is_editor_viewport_smoke(&self) -> bool {
        matches!(self, Self::EditorViewportSmoke { .. })
    }
}

impl ViewerConfig {
    fn from_args() -> Result<Self, String> {
        Self::from_iter(env::args().skip(1))
    }

    fn from_iter(arguments: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut effect_path = None;
        let mut fireworks_f0 = false;
        let mut fireworks_probe = None;
        let mut fireworks_camera = FireworksCamera::Audience;
        let mut camera_was_set = false;
        let mut semantic_materials = false;
        let mut wireframe = false;
        let mut capture_mode = None;
        let mut capture_sampling = CaptureSampling::EvenlySpaced(8);
        let mut capture_sampling_was_set = false;
        let mut presentation = PresentationMode::Auto;
        let mut transparent_order = TransparentOrderMode::Fast;
        let mut max_gpu_particles = DEFAULT_GPU_PARTICLE_BUDGET;
        let mut preview_seed = None;
        let mut diagnostics = false;
        let mut gpu_bench = None;
        let mut history_policy = aestra_bevy::PlaybackHistoryPolicy::default();
        let mut view_3d = false;
        let mut tier = aestra_bevy::QualityTier::default();
        let mut photographic = None;
        let mut sprite_minimum_pixels = 0.0;
        let mut args = arguments.into_iter();
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--effect" => {
                    effect_path = Some(PathBuf::from(
                        args.next().ok_or("--effect requires a file path")?,
                    ));
                }
                "--fireworks-f0" => fireworks_f0 = true,
                "--fireworks-f0-probe" => {
                    let value = args.next().ok_or(
                        "--fireworks-f0-probe requires an event/trail probe, f2 distribution probe or f3 shell probe (peony/chrysanthemum/pistil/willow)",
                    )?;
                    fireworks_probe = Some(FireworksProbe::parse(&value).ok_or(
                        "--fireworks-f0-probe requires an event/trail probe, f2 distribution probe or f3 shell probe (peony/chrysanthemum/pistil/willow)",
                    )?);
                }
                "--camera" => {
                    let value = args
                        .next()
                        .ok_or("--camera requires close, audience or wide")?;
                    fireworks_camera = FireworksCamera::parse(&value)
                        .ok_or("--camera requires close, audience or wide")?;
                    camera_was_set = true;
                }
                "--semantic-materials" => semantic_materials = true,
                "--wireframe" => wireframe = true,
                "--stable-transparency" => transparent_order = TransparentOrderMode::StableCapture,
                "--diagnostics" => diagnostics = true,
                "--view3d" => view_3d = true,
                "--sprite-min-pixels" => {
                    let value = args.next().ok_or("--sprite-min-pixels requires a value")?;
                    sprite_minimum_pixels =
                        photographic::bounded_number(&value, "--sprite-min-pixels", 0.0, 8.0)?;
                }
                "--hdr" => {
                    photographic.get_or_insert_with(photographic::PhotographicPreview::default);
                }
                "--exposure" => {
                    let value = args.next().ok_or("--exposure requires relative stops")?;
                    photographic
                        .get_or_insert_with(photographic::PhotographicPreview::default)
                        .exposure_stops =
                        photographic::bounded_number(&value, "--exposure", -8.0, 8.0)?;
                }
                "--tonemapping" => {
                    let value = args
                        .next()
                        .ok_or("--tonemapping requires tony, aces or reinhard")?;
                    photographic
                        .get_or_insert_with(photographic::PhotographicPreview::default)
                        .tonemapping = photographic::DisplayTransform::parse(&value)?;
                }
                "--bloom" => {
                    let value = args.next().ok_or("--bloom requires a strength")?;
                    photographic
                        .get_or_insert_with(photographic::PhotographicPreview::default)
                        .bloom_intensity =
                        photographic::bounded_number(&value, "--bloom", 0.0, 1.0)?;
                }
                "--gpu-bench" => {
                    gpu_bench = Some(PathBuf::from(
                        args.next()
                            .ok_or("--gpu-bench requires an output JSON path")?,
                    ));
                }
                "--history" => {
                    history_policy = match args.next().as_deref() {
                        Some("playback-only") => aestra_bevy::PlaybackHistoryPolicy::PlaybackOnly,
                        Some("replay-enabled") => aestra_bevy::PlaybackHistoryPolicy::ReplayEnabled,
                        _ => {
                            return Err("--history requires playback-only or replay-enabled".into());
                        }
                    };
                }
                "--capture" => {
                    set_capture_mode(
                        &mut capture_mode,
                        CaptureMode::Standard {
                            output: PathBuf::from(
                                args.next().ok_or("--capture requires a directory")?,
                            ),
                        },
                    )?;
                }
                "--approve-visual-reference" => {
                    set_capture_mode(
                        &mut capture_mode,
                        CaptureMode::Approve {
                            reference: PathBuf::from(
                                args.next()
                                    .ok_or("--approve-visual-reference requires a directory")?,
                            ),
                        },
                    )?;
                }
                "--visual-test" => {
                    set_capture_mode(
                        &mut capture_mode,
                        CaptureMode::Compare {
                            reference: PathBuf::from(
                                args.next()
                                    .ok_or("--visual-test requires a reference directory")?,
                            ),
                            output: PathBuf::from(
                                args.next()
                                    .ok_or("--visual-test requires an output directory")?,
                            ),
                        },
                    )?;
                }
                "--editor-viewport-smoke" => {
                    set_capture_mode(
                        &mut capture_mode,
                        CaptureMode::EditorViewportSmoke {
                            output: PathBuf::from(
                                args.next().ok_or(
                                    "--editor-viewport-smoke requires an output directory",
                                )?,
                            ),
                        },
                    )?;
                }
                "--frames" => {
                    let frame_count = args
                        .next()
                        .ok_or("--frames requires a number")?
                        .parse::<usize>()
                        .map_err(|_| "--frames must be a positive integer")?;
                    if frame_count == 0 || frame_count > 64 {
                        return Err("--frames must be between 1 and 64".into());
                    }
                    set_capture_sampling(
                        &mut capture_sampling,
                        &mut capture_sampling_was_set,
                        CaptureSampling::EvenlySpaced(frame_count),
                    )?;
                }
                "--sample-frames" => {
                    let values = args
                        .next()
                        .ok_or("--sample-frames requires a comma-separated frame list")?;
                    set_capture_sampling(
                        &mut capture_sampling,
                        &mut capture_sampling_was_set,
                        CaptureSampling::ExplicitFrames(parse_sample_frames(&values)?),
                    )?;
                }
                "--sample-times" => {
                    let values = args
                        .next()
                        .ok_or("--sample-times requires a comma-separated seconds list")?;
                    set_capture_sampling(
                        &mut capture_sampling,
                        &mut capture_sampling_was_set,
                        CaptureSampling::ExplicitTimes(parse_sample_times(&values)?),
                    )?;
                }
                "--backend" => {
                    presentation = match args
                        .next()
                        .ok_or("--backend requires auto, gpu, gpu-readback, or cpu")?
                        .as_str()
                    {
                        "auto" => PresentationMode::Auto,
                        "gpu" => PresentationMode::Gpu,
                        "gpu-readback" => PresentationMode::GpuReadback,
                        "cpu" => PresentationMode::CpuReference,
                        value => return Err(format!("unknown backend '{value}'")),
                    };
                }
                "--max-gpu-particles" => {
                    max_gpu_particles = args
                        .next()
                        .ok_or("--max-gpu-particles requires a count")?
                        .parse::<u32>()
                        .map_err(|_| "--max-gpu-particles must be a positive integer")?;
                    if max_gpu_particles == 0 {
                        return Err("--max-gpu-particles must be greater than zero".into());
                    }
                }
                "--tier" => {
                    let name = args.next().ok_or("--tier requires high, medium or low")?;
                    tier = aestra_bevy::QualityTier::preset(&name)
                        .ok_or_else(|| format!("unknown tier '{name}'"))?;
                }
                "--seed" => {
                    let value = args.next().ok_or("--seed requires an integer")?;
                    preview_seed = Some(parse_seed(&value)?);
                }
                "--help" | "-h" => {
                    return Err("help requested".into());
                }
                unknown => return Err(format!("unknown argument '{unknown}'")),
            }
        }
        if fireworks_f0 && effect_path.is_some() {
            return Err("--fireworks-f0 and --effect are mutually exclusive".into());
        }
        if fireworks_probe.is_some() && !fireworks_f0 {
            return Err("--fireworks-f0-probe requires --fireworks-f0".into());
        }
        if camera_was_set && !fireworks_f0 {
            return Err("--camera is only available with --fireworks-f0".into());
        }
        if fireworks_f0 {
            view_3d = true;
        }
        if sprite_minimum_pixels > 0.0
            && matches!(
                presentation,
                PresentationMode::CpuReference | PresentationMode::GpuReadback
            )
        {
            return Err(
                "--sprite-min-pixels requires native GPU presentation (auto or gpu)".into(),
            );
        }
        Ok(Self {
            effect_path,
            fireworks_f0,
            fireworks_probe,
            fireworks_camera,
            semantic_materials,
            wireframe,
            capture_mode,
            capture_sampling,
            presentation,
            transparent_order,
            max_gpu_particles,
            preview_seed,
            diagnostics,
            gpu_bench,
            history_policy,
            view_3d,
            tier,
            photographic,
            sprite_minimum_pixels,
        })
    }

    fn resolved_seed(&self) -> u64 {
        self.preview_seed.unwrap_or_else(|| {
            if self.fireworks_f0 {
                fireworks_f0::SEED
            } else if self
                .capture_mode
                .as_ref()
                .is_some_and(CaptureMode::is_regression)
            {
                REGRESSION_SEED
            } else {
                0
            }
        })
    }
}

fn set_capture_sampling(
    target: &mut CaptureSampling,
    was_set: &mut bool,
    sampling: CaptureSampling,
) -> Result<(), String> {
    if *was_set {
        return Err("--frames, --sample-frames, and --sample-times are mutually exclusive".into());
    }
    *target = sampling;
    *was_set = true;
    Ok(())
}

fn parse_sample_frames(value: &str) -> Result<Vec<u64>, String> {
    parse_sample_list(value, "--sample-frames", |item| {
        item.parse::<u64>()
            .map_err(|_| "frame values must be non-negative integers".to_owned())
    })
}

fn parse_sample_times(value: &str) -> Result<Vec<f32>, String> {
    parse_sample_list(value, "--sample-times", |item| {
        let seconds = item
            .parse::<f32>()
            .map_err(|_| "time values must be finite non-negative seconds".to_owned())?;
        if !seconds.is_finite() || seconds < 0.0 {
            return Err("time values must be finite non-negative seconds".to_owned());
        }
        Ok(seconds)
    })
}

fn parse_sample_list<T: Copy + PartialOrd>(
    value: &str,
    option: &str,
    parse: impl Fn(&str) -> Result<T, String>,
) -> Result<Vec<T>, String> {
    let values = value
        .split(',')
        .map(str::trim)
        .map(|item| {
            if item.is_empty() {
                Err(format!("{option} contains an empty value"))
            } else {
                parse(item).map_err(|error| format!("{option}: {error}"))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    if values.is_empty() || values.len() > 64 {
        return Err(format!("{option} must contain between 1 and 64 values"));
    }
    if values.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(format!("{option} values must be strictly increasing"));
    }
    Ok(values)
}

fn parse_seed(value: &str) -> Result<u64, String> {
    value
        .strip_prefix("0x")
        .map_or_else(|| value.parse::<u64>(), |hex| u64::from_str_radix(hex, 16))
        .map_err(|_| "--seed must be a decimal or 0x-prefixed integer".into())
}

fn set_capture_mode(target: &mut Option<CaptureMode>, mode: CaptureMode) -> Result<(), String> {
    if target.is_some() {
        return Err(
            "capture, approval, visual-test, and viewport-smoke modes are mutually exclusive"
                .into(),
        );
    }
    *target = Some(mode);
    Ok(())
}

#[derive(Resource)]
struct CapturePlan {
    waiting_since: Option<Instant>,
    history_frame: Option<u64>,
    mode: CaptureMode,
    sample_frames: Vec<u64>,
    next_frame: usize,
    settle_frames: u8,
    positioned: bool,
    pending: bool,
    images: Vec<RgbaImage>,
    seed: u64,
    sampled_frames: Vec<u64>,
}

impl CapturePlan {
    fn new(
        mode: CaptureMode,
        sampling: &CaptureSampling,
        seed: u64,
        effect_duration: f32,
    ) -> Result<Self, String> {
        let maximum_frame = PlaybackClock::default().maximum_frame(effect_duration);
        let sample_frames = resolve_sample_frames(sampling, maximum_frame)?;
        let frame_count = sample_frames.len();
        Ok(Self {
            waiting_since: None,
            mode,
            history_frame: None,
            sample_frames,
            next_frame: 0,
            // Let the window, glyph atlas, sprite pipelines, and particle pool reach the render
            // world before the first capture. Later samples only need a short seek settle.
            settle_frames: 20,
            positioned: false,
            pending: false,
            images: Vec::with_capacity(frame_count),
            seed,
            sampled_frames: Vec::with_capacity(frame_count),
        })
    }

    fn frame_count(&self) -> usize {
        self.sample_frames.len()
    }
}

fn resolve_sample_frames(
    sampling: &CaptureSampling,
    maximum_frame: u64,
) -> Result<Vec<u64>, String> {
    let frames = match sampling {
        CaptureSampling::EvenlySpaced(frame_count) => (0..*frame_count)
            .map(|index| capture_frame(maximum_frame, index, *frame_count))
            .collect(),
        CaptureSampling::ExplicitFrames(frames) => frames.clone(),
        CaptureSampling::ExplicitTimes(times) => times
            .iter()
            .map(|seconds| (f64::from(*seconds) * f64::from(DEFAULT_PLAYBACK_TICK_RATE)).round())
            .map(|frame| frame.clamp(0.0, u64::MAX as f64) as u64)
            .collect(),
    };
    if frames.windows(2).any(|pair| pair[0] >= pair[1]) {
        return Err(
            "sample times resolve to duplicate or unordered simulation frames at 60 Hz".into(),
        );
    }
    if let Some(frame) = frames.iter().find(|frame| **frame > maximum_frame) {
        return Err(format!(
            "sample frame {frame} exceeds the effect's final frame {maximum_frame}"
        ));
    }
    Ok(frames)
}

#[derive(Component)]
struct ViewerHud;

fn prepare_viewer(config: &ViewerConfig) -> Result<PreparedViewer, PreparationFailure> {
    let effect = if config.fireworks_f0 {
        Ok(match config.fireworks_probe {
            Some(FireworksProbe::Event) => fireworks_f0::event_probe(),
            Some(FireworksProbe::EventHero) => fireworks_f0::hero_event_probe(),
            Some(FireworksProbe::Trail) => fireworks_f0::trail_probe(),
            Some(FireworksProbe::TrailHero) => fireworks_f0::hero_trail_probe(),
            Some(FireworksProbe::EventTrail) => fireworks_f0::event_trail_probe(),
            Some(FireworksProbe::EventTrailLarge) => fireworks_f0::large_event_trail_probe(),
            Some(FireworksProbe::EventTrailVolley) => fireworks_f0::event_trail_volley_probe(),
            Some(FireworksProbe::EventTrailSparse) => {
                fireworks_f0::sparse_event_trail_volley_probe()
            }
            Some(FireworksProbe::Velocity(probe)) => velocity_f2::effect(probe),
            Some(FireworksProbe::Shell(probe)) => fireworks_f3::effect(probe),
            None => fireworks_f0::effect(),
        })
    } else {
        config.effect_path.as_ref().map_or_else(
            || EffectAsset::from_ron(SAMPLE_SOURCE),
            EffectAsset::load_ron,
        )
    }
    .map_err(|error| PreparationFailure {
        message: format!("could not load viewer effect: {error}"),
        diagnostics: Vec::new(),
    })?;
    let asset_root = viewer_asset_root(config.effect_path.as_deref())
        .canonicalize()
        .map_err(|error| PreparationFailure {
            message: format!("could not locate viewer assets: {error}"),
            diagnostics: vec![],
        })?;
    let index = aestra_project::ProjectAssetIndex::scan(&asset_root);
    let mut resolved =
        index
            .resolve_effect_project(&effect)
            .map_err(|error| PreparationFailure {
                message: format!("could not resolve viewer project: {error}"),
                diagnostics: vec![],
            })?;
    if config.semantic_materials {
        for effect in std::iter::once(&mut resolved.root).chain(resolved.dependencies.values_mut())
        {
            let programs = migrate_viewer_materials(
                effect,
                resolved.material_programs.values().cloned().collect(),
            )
            .map_err(|error| PreparationFailure {
                message: format!("could not migrate viewer materials: {error}"),
                diagnostics: vec![],
            })?;
            resolved
                .material_programs
                .extend(programs.into_iter().map(|p| (p.id, p)));
        }
    }
    let mut diagnostics = std::iter::once(&resolved.root)
        .chain(resolved.dependencies.values())
        .flat_map(|effect| effect.validation_report().diagnostics)
        .collect::<Vec<_>>();
    diagnostics.extend(
        resolved
            .material_programs
            .values()
            .flat_map(|program| program.validation_report().diagnostics),
    );
    diagnostics.sort();
    diagnostics.dedup();
    let project = EffectCompiler::default()
        .with_tier(config.tier.clone())
        .compile_resolved_project(&resolved)
        .map_err(|error| PreparationFailure {
            message: format!("could not compile viewer project: {error}"),
            diagnostics: match &error {
                aestra_bevy::ProjectCompileError::Effect { source, .. } => {
                    source.report().diagnostics.clone()
                }
                _ => diagnostics.clone(),
            },
        })?;
    let compiled = project.root.clone();
    let mut material_program_fingerprints = std::iter::once(&project.root)
        .chain(project.dependencies.values())
        .flat_map(|effect| &effect.material_programs)
        .map(|program| {
            aestra_bevy::compile_material_program(program)
                .map(|compiled| {
                    (
                        program.id.to_string(),
                        compiled.program_fingerprint.to_string(),
                    )
                })
                .map_err(|error| PreparationFailure {
                    message: format!(
                        "could not compile semantic material program {}: {error}",
                        program.id
                    ),
                    diagnostics: diagnostics.clone(),
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    material_program_fingerprints.sort();
    material_program_fingerprints.dedup();
    let compiler = CompilerPreviewData::new(&compiled, diagnostics, material_program_fingerprints);
    Ok(PreparedViewer {
        compiled,
        project: Arc::new(project),
        asset_root,
        compiler,
    })
}

fn report_preparation_failure(config: &ViewerConfig, failure: &PreparationFailure) {
    let Some(mode) = &config.capture_mode else {
        return;
    };
    if let Err(error) = write_preview_failure_report(
        mode.output_directory(),
        &failure.message,
        &failure.diagnostics,
    ) {
        eprintln!("aestra-viewer: could not write preview failure report: {error}");
    }
}

fn setup(
    mut commands: Commands,
    config: Res<ViewerConfig>,
    prepared: Res<PreparedViewer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let effect_name = prepared.compiled.name.clone();
    let regression_scene = config
        .capture_mode
        .as_ref()
        .is_some_and(CaptureMode::is_regression);
    let editor_viewport_smoke = config
        .capture_mode
        .as_ref()
        .is_some_and(CaptureMode::is_editor_viewport_smoke);

    let mut player = EffectPlayer::from_project(Arc::clone(&prepared.project))
        .with_history_policy(config.history_policy);
    if config.wireframe {
        player.set_render_mode(aestra_bevy::EffectRenderMode::Wireframe);
    }
    player.set_seed(config.resolved_seed());
    let presentation = PresentedEffect::new(player.effect().clone());
    if editor_viewport_smoke {
        spawn_editor_viewport_smoke_scene(&mut commands, config.photographic);
        commands.spawn((player, presentation, RenderLayers::layer(0)));
        return;
    }

    if config.view_3d {
        let camera_transform = if config.fireworks_f0 {
            config.fireworks_camera.transform()
        } else {
            framing_transform(&prepared.compiled)
        };
        let mut camera = commands.spawn((
            Camera3d::default(),
            Camera {
                clear_color: ClearColorConfig::Custom(Color::srgb(0.009, 0.012, 0.024)),
                ..default()
            },
            camera_transform,
        ));
        if let Some(settings) = config.photographic {
            settings.apply(&mut camera);
        }
    } else {
        let mut camera = commands.spawn(Camera2d);
        if let Some(settings) = config.photographic {
            settings.apply(&mut camera);
        }
    }
    commands.spawn((player, presentation));

    if config.fireworks_f0 {
        spawn_fireworks_validation_scene(&mut commands, &mut meshes, &mut materials);
    }

    // The 2-D grid and HUD are sprites and UI for the 2-D view; regression scenes stay bare.
    if regression_scene || config.view_3d {
        return;
    }

    // A quiet reference grid makes motion and scale legible without becoming part of the effect.
    for x in (-480..=480).step_by(80) {
        commands.spawn((
            Sprite::from_color(Color::srgba(0.25, 0.30, 0.45, 0.10), Vec2::new(1.0, 540.0)),
            Transform::from_xyz(x as f32, 0.0, -10.0),
        ));
    }
    for y in (-240..=240).step_by(80) {
        commands.spawn((
            Sprite::from_color(Color::srgba(0.25, 0.30, 0.45, 0.10), Vec2::new(960.0, 1.0)),
            Transform::from_xyz(0.0, y as f32, -10.0),
        ));
    }

    commands.spawn((
        Text::new(format!("AESTRA VIEWER  |  {effect_name}")),
        TextFont {
            font_size: FontSize::Px(13.0),
            ..default()
        },
        TextColor(Color::srgb(0.75, 0.70, 1.0)),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(18.0),
            top: Val::Px(16.0),
            ..default()
        },
    ));
    commands.spawn((
        ViewerHud,
        Text::new("00.000 / 00.000  |  SPACE Pause  |  R Restart  |  S Screenshot"),
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
        TextColor(Color::srgb(0.47, 0.50, 0.61)),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(18.0),
            bottom: Val::Px(16.0),
            ..default()
        },
    ));
}

fn spawn_fireworks_validation_scene(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    commands.spawn((
        DirectionalLight {
            illuminance: 120.0,
            ..default()
        },
        Transform::from_xyz(-30.0, 80.0, 60.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(220.0, 220.0))),
        MeshMaterial3d(materials.add(Color::srgb(0.025, 0.03, 0.045))),
    ));
    let marker_mesh = meshes.add(Cuboid::new(2.0, 8.0, 2.0));
    let marker_material = materials.add(Color::srgb(0.10, 0.12, 0.16));
    for x in [-24.0, 24.0] {
        commands.spawn((
            Mesh3d(marker_mesh.clone()),
            MeshMaterial3d(marker_material.clone()),
            Transform::from_xyz(x, 4.0, 0.0),
        ));
    }
}

fn viewer_asset_root(path: Option<&std::path::Path>) -> PathBuf {
    let default_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/test");
    let parent = path
        .and_then(std::path::Path::parent)
        .unwrap_or(&default_root);
    let root = if parent.file_name().is_some_and(|name| name == "effects") {
        parent.parent().unwrap_or(parent)
    } else {
        parent
    };
    root.to_path_buf()
}

#[cfg(test)]
fn load_viewer_material_programs(
    effect: &EffectAsset,
    path: Option<&std::path::Path>,
) -> Result<Vec<MaterialProgram>, String> {
    let index = aestra_project::ProjectAssetIndex::scan(viewer_asset_root(path));
    effect
        .material_instances
        .iter()
        .map(|instance| {
            let entry = index
                .resolve_material_program(instance.program)
                .map_err(|error| error.to_string())?;
            MaterialProgram::load_ron(&entry.path).map_err(|error| error.to_string())
        })
        .collect()
}

fn migrate_viewer_materials(
    effect: &mut EffectAsset,
    programs: Vec<MaterialProgram>,
) -> Result<Vec<MaterialProgram>, String> {
    let mut document = MaterialAuthoringDocument::new(effect.clone(), programs);
    migrate_legacy_sprite_materials(&mut document).map_err(|error| error.to_string())?;
    *effect = document
        .effect
        .expect("migration retains its effect context");
    Ok(document.programs)
}

fn spawn_editor_viewport_smoke_scene(
    commands: &mut Commands,
    photographic: Option<photographic::PhotographicPreview>,
) {
    let preview_viewport = Viewport {
        physical_position: UVec2::new(EDITOR_PREVIEW_X, EDITOR_PREVIEW_Y),
        physical_size: UVec2::new(EDITOR_PREVIEW_WIDTH, EDITOR_PREVIEW_HEIGHT),
        ..default()
    };
    let mut preview_camera = commands.spawn((
        Camera3d::default(),
        Camera {
            order: -2,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.009, 0.012, 0.024)),
            viewport: Some(preview_viewport.clone()),
            ..default()
        },
        editor_preview_camera_transform(),
        RenderLayers::layer(0),
    ));
    if let Some(profile) = photographic {
        profile.apply(&mut preview_camera);
    }
    if photographic.is_some() {
        // Model the editor's separate LDR UI camera too: it must not erase the HDR viewport.
        commands.spawn((
            Camera2d,
            IsDefaultUiCamera,
            RenderLayers::layer(31),
            Camera {
                order: 0,
                clear_color: ClearColorConfig::Custom(Color::NONE),
                output_mode: bevy::camera::CameraOutputMode::Write {
                    blend_state: Some(bevy::render::render_resource::BlendState::ALPHA_BLENDING),
                    clear_color: ClearColorConfig::None,
                },
                ..default()
            },
        ));
    }
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: 1,
            clear_color: if photographic.is_some() {
                ClearColorConfig::Custom(Color::NONE)
            } else {
                ClearColorConfig::None
            },
            output_mode: if photographic.is_some() {
                bevy::camera::CameraOutputMode::Write {
                    blend_state: Some(bevy::render::render_resource::BlendState::ALPHA_BLENDING),
                    clear_color: ClearColorConfig::None,
                }
            } else {
                Default::default()
            },
            viewport: Some(preview_viewport),
            ..default()
        },
        editor_preview_camera_transform(),
        RenderLayers::layer(15),
    ));
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: 2,
            clear_color: ClearColorConfig::Custom(Color::srgb(0.018, 0.024, 0.036)),
            viewport: Some(Viewport {
                physical_position: UVec2::new(OVERLAY_PROBE_X, OVERLAY_PROBE_Y),
                physical_size: UVec2::splat(OVERLAY_PROBE_SIZE),
                ..default()
            }),
            ..default()
        },
        editor_preview_camera_transform(),
        RenderLayers::layer(15),
    ));
}

/// A 3-D view of the effect: its simulation domains' boxes framed whole from slightly above, else the
/// editor preview's view of the origin.
fn framing_transform(effect: &aestra_bevy::CompiledEffect) -> Transform {
    let bounds = effect
        .all_extension_stages()
        .flat_map(|stage| stage.block.fields.iter())
        .map(|field| {
            let min = Vec3::from(field.origin);
            let size = Vec3::from(field.dims.map(|cells| cells as f32 * field.cell_size));
            (min, min + size)
        })
        .reduce(|(min_a, max_a), (min_b, max_b)| (min_a.min(min_b), max_a.max(max_b)));
    let Some((min, max)) = bounds else {
        return editor_preview_camera_transform();
    };
    let center = (min + max) * 0.5;
    // The box's front face fills the default 45° vertical field of view.
    let half = (max - min) * 0.5;
    let distance = half.x.max(half.y) / std::f32::consts::FRAC_PI_8.tan() + half.z;
    let orbit = Quat::from_rotation_x(-0.25);
    Transform::from_translation(center + orbit * Vec3::Z * distance).looking_at(center, Vec3::Y)
}

fn editor_preview_camera_transform() -> Transform {
    let orbit = Quat::from_rotation_x(-0.35);
    Transform::from_translation(orbit * Vec3::Z * 140.0).looking_at(Vec3::ZERO, Vec3::Y)
}

fn viewer_controls(
    benchmark: Option<Res<gpu_bench::GpuBenchPlan>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut players: Query<&mut EffectPlayer>,
    mut commands: Commands,
    mut screenshot_index: Local<u32>,
) {
    // Benchmark setup must not silently change via play/seek/seed/wireframe hotkeys.
    if benchmark.is_some() {
        return;
    }
    if keys.just_pressed(KeyCode::Space) {
        for mut player in &mut players {
            player.playing = !player.playing;
        }
    }
    if keys.just_pressed(KeyCode::KeyR) {
        for mut player in &mut players {
            player.restart();
        }
    }
    if keys.just_pressed(KeyCode::KeyW) {
        for mut player in &mut players {
            let mode = match player.render_mode() {
                aestra_bevy::EffectRenderMode::Rendered => aestra_bevy::EffectRenderMode::Wireframe,
                aestra_bevy::EffectRenderMode::Wireframe => aestra_bevy::EffectRenderMode::Rendered,
            };
            player.set_render_mode(mode);
        }
    }
    if keys.just_pressed(KeyCode::ArrowLeft) {
        for mut player in &mut players {
            player.step_back();
        }
    }
    if keys.just_pressed(KeyCode::ArrowRight) {
        for mut player in &mut players {
            player.step_forward();
        }
    }
    if keys.just_pressed(KeyCode::BracketLeft) {
        for mut player in &mut players {
            let seed = player.instance().seed().wrapping_sub(1);
            player.set_seed(seed);
        }
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        for mut player in &mut players {
            let seed = player.instance().seed().wrapping_add(1);
            player.set_seed(seed);
        }
    }
    if keys.just_pressed(KeyCode::KeyS) {
        let path = format!("aestra-viewer-{:03}.png", *screenshot_index);
        *screenshot_index += 1;
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
}

fn update_hud(
    players: Query<(&EffectPlayer, &EffectRuntimeStatus)>,
    mut hud: Query<&mut Text, With<ViewerHud>>,
) {
    let (Ok((player, runtime)), Ok(mut text)) = (players.single(), hud.single_mut()) else {
        return;
    };
    text.0 = format!(
        "F{:05} @ {} Hz  |  {:06.3} / {:06.3}  |  seed {:016x}  |  {}  |  {}  |  ←/→ Step  |  [/] Seed  |  W Wireframe",
        player.frame(),
        player.tick_rate(),
        player.elapsed(),
        player.effect().duration,
        player.instance().seed(),
        if player.playing { "PLAYING" } else { "PAUSED" },
        runtime.active,
    );
}

#[derive(Resource, Default)]
struct CaptureRenderReadiness {
    ready: bool,
    detail: String,
}

fn publish_capture_render_readiness(cache: Res<PipelineCache>, mut main_world: ResMut<MainWorld>) {
    let mut total = 0;
    let mut pending = 0;
    let mut error = None;
    for pipeline in cache.pipelines() {
        total += 1;
        if !matches!(pipeline.state, CachedPipelineState::Ok(_)) {
            pending += 1;
        }
        if let CachedPipelineState::Err(reason) = &pipeline.state {
            error.get_or_insert_with(|| reason.to_string());
        }
    }
    main_world.insert_resource(CaptureRenderReadiness {
        ready: total > 0 && pending == 0,
        detail: error.unwrap_or_else(|| format!("{pending} of {total} render pipelines pending")),
    });
}

fn drive_capture(
    capture: Option<ResMut<CapturePlan>>,
    mut players: Query<&mut EffectPlayer>,
    mut commands: Commands,
    readiness: Option<Res<CaptureRenderReadiness>>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut capture) = capture else {
        return;
    };
    if capture.pending || capture.next_frame >= capture.frame_count() {
        return;
    }
    if let Some(readiness) = readiness.as_ref().filter(|readiness| !readiness.ready) {
        let waiting_since = capture.waiting_since.get_or_insert_with(Instant::now);
        if waiting_since.elapsed() >= Duration::from_secs(60) {
            let message = format!(
                "capture timed out waiting for render pipelines: {}",
                readiness.detail
            );
            eprintln!("aestra-viewer: {message}");
            if let Err(error) =
                write_preview_failure_report(capture.mode.output_directory(), &message, &[])
            {
                eprintln!("aestra-viewer: could not write capture failure report: {error}");
            }
            capture.pending = true;
            exit.write(AppExit::error());
        }
        return;
    }
    capture.waiting_since = None;
    if capture.settle_frames > 0 {
        capture.settle_frames -= 1;
        return;
    }

    if !capture.positioned {
        for mut player in &mut players {
            let sample_frame = capture.sample_frames[capture.next_frame];
            let has_trails = std::iter::once(player.effect())
                .chain(
                    player
                        .project()
                        .into_iter()
                        .flat_map(|p| p.dependencies.values()),
                )
                .flat_map(|effect| &effect.emitters)
                .filter(|e| e.enabled)
                .any(|e| {
                    e.renderers
                        .iter()
                        .any(|r| matches!(r.kind, aestra_bevy::RendererPlanKind::Trail { .. }))
                });
            if has_trails {
                // History cannot be captured by a stateless seek: replay one 60 Hz
                // observation per rendered frame, keeping the real GPU tail alive.
                let Some(frame) = capture.history_frame.filter(|frame| *frame <= sample_frame)
                else {
                    player.restart();
                    player.playing = false;
                    capture.history_frame = Some(0);
                    return;
                };
                if frame < sample_frame {
                    player
                        .set_playback_time((frame + 1) as f32 / DEFAULT_PLAYBACK_TICK_RATE as f32);
                    player.playing = false;
                    capture.history_frame = Some(frame + 1);
                    return;
                }
            } else {
                player.seek_frame(sample_frame);
            }
            player.playing = false;
            capture.sampled_frames.push(sample_frame);
        }
        capture.positioned = true;
        capture.settle_frames = 2;
        return;
    }

    capture.pending = true;
    capture.positioned = false;
    commands
        .spawn(Screenshot::primary_window())
        .observe(receive_capture);
}

fn capture_frame(maximum_frame: u64, index: usize, frame_count: usize) -> u64 {
    let numerator = u128::from(maximum_frame) * (2 * index as u128 + 1);
    let denominator = 2 * frame_count.max(1) as u128;
    ((numerator + denominator / 2) / denominator) as u64
}

#[derive(SystemParam)]
struct CaptureReportContext<'w, 's> {
    config: Res<'w, ViewerConfig>,
    runtime: Res<'w, AestraRuntimeStatus>,
    settings: Res<'w, AestraSettings>,
    capabilities: Res<'w, GpuCapabilities>,
    prepared: Res<'w, PreparedViewer>,
    effects: Query<
        'w,
        's,
        (
            &'static EffectRuntimeStatus,
            &'static EffectProfiler,
            Option<&'static aestra_bevy::ProjectProfiler>,
        ),
    >,
}

fn receive_capture(
    event: On<ScreenshotCaptured>,
    mut capture: ResMut<CapturePlan>,
    report: CaptureReportContext,
    mut exit: MessageWriter<AppExit>,
) {
    let output_directory = capture.mode.output_directory().clone();
    fs::create_dir_all(&output_directory)
        .unwrap_or_else(|error| panic!("could not create capture directory: {error}"));
    let frame = event
        .image
        .clone()
        .try_into_dynamic()
        .expect("the primary-window screenshot must use a convertible pixel format")
        .to_rgba8();
    let frame_path = output_directory.join(format!("frame-{:03}.png", capture.next_frame));
    frame
        .save(&frame_path)
        .unwrap_or_else(|error| panic!("could not save {}: {error}", frame_path.display()));
    capture.images.push(frame);
    capture.next_frame += 1;
    capture.pending = false;
    capture.settle_frames = 1;

    if capture.next_frame == capture.frame_count() {
        let effect_status = report.effects.single().ok();
        let effect_runtime = effect_status.map(|(runtime, _, _)| runtime);
        write_contact_sheet(
            &capture,
            &report.runtime,
            effect_runtime,
            &report.settings,
            &report.capabilities,
        );
        let completion = finish_capture(
            &capture,
            &report.runtime,
            effect_runtime,
            &report.capabilities,
        );
        let frame_count = capture.frame_count();
        let columns = (frame_count as f32).sqrt().ceil() as u32;
        let rows = (frame_count as u32).div_ceil(columns);
        let report_result = write_preview_report(
            capture.mode.output_directory(),
            PreviewCaptureData {
                sampled_frames: &capture.sampled_frames,
                seed: capture.seed,
                width: VIEW_WIDTH,
                height: VIEW_HEIGHT,
                columns,
                rows,
                tick_rate: DEFAULT_PLAYBACK_TICK_RATE,
                response: photographic::CaptureResponse::new(
                    report.config.photographic,
                    report.config.sprite_minimum_pixels,
                ),
            },
            &report.prepared.compiler,
            PreviewRuntimeData {
                runtime: &report.runtime,
                effect_runtime,
                settings: &report.settings,
                capabilities: &report.capabilities,
                profile: effect_status
                    .map(|(_, profile, project)| project.map_or(&profile.0, |p| &p.0.total)),
                project: effect_status.and_then(|(_, _, project)| project.map(|p| &p.0)),
            },
            completion.comparison.as_ref(),
            completion.result.as_ref().err().map(String::as_str),
        );
        let result = completion.result.and(report_result);
        exit.write(if result.is_ok() {
            AppExit::Success
        } else {
            eprintln!(
                "aestra-viewer: {}",
                result.expect_err("failed regression must contain a reason")
            );
            AppExit::error()
        });
    }
}

fn write_contact_sheet(
    capture: &CapturePlan,
    runtime: &AestraRuntimeStatus,
    effect_runtime: Option<&EffectRuntimeStatus>,
    settings: &AestraSettings,
    capabilities: &GpuCapabilities,
) {
    let frame_count = capture.frame_count();
    let columns = (frame_count as f32).sqrt().ceil() as u32;
    let rows = (frame_count as u32).div_ceil(columns);
    let mut sheet = RgbaImage::from_pixel(
        VIEW_WIDTH * columns,
        VIEW_HEIGHT * rows,
        Rgba([3, 4, 9, 255]),
    );
    for (index, frame) in capture.images.iter().enumerate() {
        let x = index as u32 % columns * VIEW_WIDTH;
        let y = index as u32 / columns * VIEW_HEIGHT;
        imageops::replace(&mut sheet, frame, i64::from(x), i64::from(y));
    }
    let output_directory = capture.mode.output_directory();
    let path = output_directory.join("contact-sheet.png");
    sheet
        .save(&path)
        .unwrap_or_else(|error| panic!("could not save {}: {error}", path.display()));

    let active = effect_runtime.map_or(runtime.active, |status| status.active);
    let reason = effect_runtime.map_or(runtime.reason.as_str(), |status| status.reason.as_str());
    let manifest = format!(
        "# Aestra visual capture\n\n- Frames: {}\n- Frame size: {} x {}\n- Contact sheet: {} columns x {} rows\n- Seed: `{:#018x}`\n- Sampling: exact {} Hz simulation frames {:?}\n- Requested backend: {:?}\n- Active backend: {}\n- Selection reason: {}\n- Adapter: {} ({}, {})\n- Driver: {}\n- Physical GPU particle capacity: {}\n- Configured GPU particle budget: {}\n- Effective GPU particle budget: {}\n",
        frame_count,
        VIEW_WIDTH,
        VIEW_HEIGHT,
        columns,
        rows,
        capture.seed,
        DEFAULT_PLAYBACK_TICK_RATE,
        capture.sampled_frames,
        runtime.requested,
        active,
        reason,
        capabilities.adapter_name,
        capabilities.backend,
        capabilities.device_type,
        capabilities.driver,
        capabilities.max_particles,
        settings.max_gpu_particles,
        capabilities.max_particles.min(settings.max_gpu_particles),
    );
    fs::write(output_directory.join("capture-manifest.md"), manifest)
        .expect("capture manifest should be writable");
}

struct CaptureCompletion {
    result: Result<(), String>,
    comparison: Option<ComparisonReport>,
}

impl CaptureCompletion {
    fn result(result: Result<(), String>) -> Self {
        Self {
            result,
            comparison: None,
        }
    }
}

fn finish_capture(
    capture: &CapturePlan,
    runtime: &AestraRuntimeStatus,
    effect_runtime: Option<&EffectRuntimeStatus>,
    capabilities: &GpuCapabilities,
) -> CaptureCompletion {
    let active = effect_runtime.map_or(runtime.active, |status| status.active);
    if capture.mode.is_regression() && active != ActiveBackend::Gpu {
        return CaptureCompletion::result(Err(format!(
            "visual regression requires the native GPU backend, but {active} was selected"
        )));
    }
    let result = match &capture.mode {
        CaptureMode::Standard { output } => {
            println!("capture written to {}", output.display());
            Ok(())
        }
        CaptureMode::Approve { reference } => {
            let result = fs::write(
                reference.join("visual-reference.md"),
                format!(
                    "# Aestra visual reference\n\n- Frames: {}\n- Frame size: {} x {}\n- Seed: `{:#018x}`\n- Scene: effect only, fixed camera and background\n- Sampling: exact {} Hz simulation frames {:?}\n- Backend: {}\n- Adapter: {} ({})\n",
                    capture.frame_count(),
                    VIEW_WIDTH,
                    VIEW_HEIGHT,
                    capture.seed,
                    DEFAULT_PLAYBACK_TICK_RATE,
                    capture.sampled_frames,
                    active,
                    capabilities.adapter_name,
                    capabilities.backend,
                ),
            )
            .map_err(|error| format!("could not write visual reference metadata: {error}"));
            if result.is_ok() {
                println!("visual reference approved at {}", reference.display());
            }
            result
        }
        CaptureMode::Compare { reference, output } => {
            return match compare_capture(reference, output, capture.frame_count()) {
                Ok(report) => {
                    let result = match report.failure_message(output) {
                        Some(error) => Err(error),
                        None => {
                            println!(
                                "visual regression passed: {} frames, worst RMSE {:.4}",
                                report.frames.len(),
                                report
                                    .frames
                                    .iter()
                                    .map(|frame| frame.foreground_rmse)
                                    .fold(0.0, f32::max)
                            );
                            Ok(())
                        }
                    };
                    CaptureCompletion {
                        result,
                        comparison: Some(report),
                    }
                }
                Err(error) => CaptureCompletion::result(Err(error)),
            };
        }
        CaptureMode::EditorViewportSmoke { output } => {
            let result = validate_editor_viewport_smoke(&capture.images);
            if result.is_ok() {
                println!(
                    "editor viewport GPU smoke passed: {} frames written to {}",
                    capture.frame_count(),
                    output.display()
                );
            }
            result
        }
    };
    CaptureCompletion::result(result)
}

fn validate_editor_viewport_smoke(images: &[RgbaImage]) -> Result<(), String> {
    let preview_counts = images
        .iter()
        .map(|image| luminous_pixels_in_columns(image, EDITOR_PREVIEW_X, EDITOR_PREVIEW_WIDTH))
        .collect::<Vec<_>>();
    if preview_counts.iter().all(|count| *count < 8) {
        return Err(format!(
            "editor viewport smoke found no visible GPU particles in the preview viewport (luminous pixels per frame: {preview_counts:?})"
        ));
    }

    let probe_counts = images
        .iter()
        .map(|image| luminous_pixels_in_columns(image, OVERLAY_PROBE_X, OVERLAY_PROBE_SIZE))
        .collect::<Vec<_>>();
    if probe_counts.iter().any(|count| *count >= 8) {
        return Err(format!(
            "editor viewport smoke detected GPU particles in the layer-15 overlay probe (luminous pixels per frame: {probe_counts:?})"
        ));
    }
    Ok(())
}

fn luminous_pixels_in_columns(image: &RgbaImage, start_x: u32, width: u32) -> usize {
    let end_x = start_x.saturating_add(width).min(image.width());
    (start_x.min(image.width())..end_x)
        .flat_map(|x| (0..image.height()).map(move |y| (x, y)))
        .filter(|&(x, y)| {
            let [red, green, blue, alpha] = image.get_pixel(x, y).0;
            alpha > 0 && red.max(green).max(blue) >= 80
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn benchmark_hotkeys_cannot_mutate_recorded_playback_setup() {
        use bevy::ecs::system::RunSystemOnce;
        let mut world = World::new();
        let mut keys = ButtonInput::<KeyCode>::default();
        for key in [KeyCode::Space, KeyCode::BracketRight, KeyCode::KeyW] {
            keys.press(key);
        }
        world.insert_resource(keys);
        world.insert_resource(gpu_bench::GpuBenchPlan::new(
            PathBuf::new(),
            "controls".into(),
            1,
            1,
        ));
        let mut player = EffectPlayer::new(&aestra_bevy::EffectAsset::new("controls", 1.0));
        player.set_seed(1234);
        let entity = world.spawn(player).id();
        world.run_system_once(viewer_controls).unwrap();
        let player = world.get::<EffectPlayer>(entity).unwrap();
        assert_eq!(player.instance().seed(), 1234);
        assert!(player.playing);
        assert_eq!(
            player.render_mode(),
            aestra_bevy::EffectRenderMode::Rendered
        );
        world.remove_resource::<gpu_bench::GpuBenchPlan>();
        world.run_system_once(viewer_controls).unwrap();
        let player = world.get::<EffectPlayer>(entity).unwrap();
        assert_eq!(player.instance().seed(), 1235);
        assert!(!player.playing);
        assert_eq!(
            player.render_mode(),
            aestra_bevy::EffectRenderMode::Wireframe
        );
    }

    #[test]
    fn viewer_history_policy_is_explicit_and_defaults_to_replay() {
        use aestra_bevy::PlaybackHistoryPolicy;
        assert_eq!(
            ViewerConfig::from_iter(std::iter::empty())
                .unwrap()
                .history_policy,
            PlaybackHistoryPolicy::ReplayEnabled
        );
        for (value, expected) in [
            ("playback-only", PlaybackHistoryPolicy::PlaybackOnly),
            ("replay-enabled", PlaybackHistoryPolicy::ReplayEnabled),
        ] {
            assert_eq!(
                ViewerConfig::from_iter(["--history", value].into_iter().map(str::to_owned))
                    .unwrap()
                    .history_policy,
                expected
            );
        }
        for arguments in [vec!["--history"], vec!["--history", "other"]] {
            assert!(ViewerConfig::from_iter(arguments.into_iter().map(str::to_owned)).is_err());
        }
    }

    #[test]
    fn sprite_sampling_is_opt_in_native_only_and_does_not_change_playback() {
        let baseline = ViewerConfig::from_iter(std::iter::empty()).unwrap();
        assert_eq!(baseline.sprite_minimum_pixels, 0.0);
        for value in ["0", "2", "8"] {
            let config = ViewerConfig::from_iter(
                ["--sprite-min-pixels", value]
                    .into_iter()
                    .map(str::to_owned),
            )
            .unwrap();
            assert_eq!(config.sprite_minimum_pixels, value.parse::<f32>().unwrap());
            assert!(config.photographic.is_none());
            assert_eq!(config.resolved_seed(), baseline.resolved_seed());
            assert_eq!(config.history_policy, baseline.history_policy);
            assert_eq!(config.capture_sampling, baseline.capture_sampling);
            assert_eq!(
                prepare_viewer(&config)
                    .unwrap_or_else(|e| panic!("{}", e.message))
                    .compiled,
                prepare_viewer(&baseline)
                    .unwrap_or_else(|e| panic!("{}", e.message))
                    .compiled
            );
        }
        for arguments in [
            vec!["--sprite-min-pixels"],
            vec!["--sprite-min-pixels", "NaN"],
            vec!["--sprite-min-pixels", "inf"],
            vec!["--sprite-min-pixels", "-1"],
            vec!["--sprite-min-pixels", "9"],
            vec!["--sprite-min-pixels", "2", "--backend", "cpu"],
            vec!["--backend", "cpu", "--sprite-min-pixels", "2"],
            vec!["--backend", "gpu-readback", "--sprite-min-pixels", "2"],
        ] {
            assert!(ViewerConfig::from_iter(arguments.into_iter().map(str::to_owned)).is_err());
        }
    }

    #[test]
    fn photographic_preview_is_opt_in_order_independent_and_does_not_change_playback() {
        assert!(
            ViewerConfig::from_iter(std::iter::empty())
                .unwrap()
                .photographic
                .is_none()
        );
        let baseline = ViewerConfig::from_iter(
            ["--fireworks-f0", "--fireworks-f0-probe", "f3-chrysanthemum"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap();
        for options in [
            [
                "--hdr",
                "--exposure",
                "2",
                "--tonemapping",
                "aces",
                "--bloom",
                "0",
            ],
            [
                "--exposure",
                "2",
                "--tonemapping",
                "aces",
                "--bloom",
                "0",
                "--hdr",
            ],
        ] {
            let config = ViewerConfig::from_iter(
                ["--fireworks-f0", "--fireworks-f0-probe", "f3-chrysanthemum"]
                    .into_iter()
                    .chain(options)
                    .map(str::to_owned),
            )
            .unwrap();
            assert_eq!(
                config.photographic,
                Some(photographic::PhotographicPreview {
                    exposure_stops: 2.0,
                    tonemapping: photographic::DisplayTransform::Aces,
                    bloom_intensity: 0.0,
                })
            );
            assert_eq!(config.resolved_seed(), baseline.resolved_seed());
            assert_eq!(config.history_policy, baseline.history_policy);
            assert_eq!(config.capture_sampling, baseline.capture_sampling);
            assert_eq!(
                prepare_viewer(&config)
                    .unwrap_or_else(|e| panic!("{}", e.message))
                    .compiled,
                prepare_viewer(&baseline)
                    .unwrap_or_else(|e| panic!("{}", e.message))
                    .compiled
            );
        }
    }

    #[test]
    fn photographic_arguments_reject_nonfinite_out_of_range_and_unsupported_settings() {
        for arguments in [
            vec!["--exposure"],
            vec!["--exposure", "NaN"],
            vec!["--exposure", "inf"],
            vec!["--exposure", "-9"],
            vec!["--exposure", "9"],
            vec!["--bloom"],
            vec!["--bloom", "NaN"],
            vec!["--bloom", "-0.1"],
            vec!["--bloom", "1.1"],
            vec!["--tonemapping"],
            vec!["--tonemapping", "none"],
        ] {
            assert!(ViewerConfig::from_iter(arguments.into_iter().map(str::to_owned)).is_err());
        }
        for arguments in [
            vec!["--exposure", "-8"],
            vec!["--exposure", "8"],
            vec!["--bloom", "1"],
            vec!["--tonemapping", "reinhard"],
        ] {
            assert!(
                ViewerConfig::from_iter(arguments.into_iter().map(str::to_owned))
                    .unwrap()
                    .photographic
                    .is_some()
            );
        }
    }

    #[test]
    fn photographic_viewport_smoke_profiles_only_the_effect_camera() {
        use bevy::{camera::Hdr, post_process::bloom::Bloom};
        let config = ViewerConfig::from_iter(
            ["--hdr", "--editor-viewport-smoke", "unused"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap();
        let mut world = World::new();
        spawn_editor_viewport_smoke_scene(&mut world.commands(), config.photographic);
        world.flush();
        let mut cameras = world.query::<(&Camera, &RenderLayers, Has<Hdr>, Has<Bloom>)>();
        let cameras = cameras.iter(&world).collect::<Vec<_>>();
        assert_eq!(cameras.len(), 4);
        for (camera, layers, hdr, bloom) in cameras {
            if layers == &RenderLayers::layer(0) {
                assert_eq!(camera.order, -2);
                assert!(hdr && bloom);
            } else {
                assert!(!hdr && !bloom);
            }
        }
    }

    #[test]
    fn fireworks_f0_selects_fixed_camera_and_seed() {
        let config = ViewerConfig::from_iter(
            [
                "--fireworks-f0",
                "--fireworks-f0-probe",
                "trail",
                "--camera",
                "wide",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert!(config.view_3d);
        assert_eq!(config.fireworks_probe, Some(FireworksProbe::Trail));
        assert_eq!(config.fireworks_camera, FireworksCamera::Wide);
        assert_eq!(config.resolved_seed(), fireworks_f0::SEED);
    }

    #[test]
    fn sparse_trail_probe_is_selected_by_the_cli() {
        let config = ViewerConfig::from_iter(
            [
                "--fireworks-f0",
                "--fireworks-f0-probe",
                "event-trail-sparse",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(
            config.fireworks_probe,
            Some(FireworksProbe::EventTrailSparse)
        );
    }

    #[test]
    fn fireworks_camera_requires_the_fixture_and_a_valid_preset() {
        for arguments in [
            vec!["--camera", "close"],
            vec!["--fireworks-f0", "--camera", "side"],
            vec!["--fireworks-f0", "--effect", "other.aestra.ron"],
            vec!["--fireworks-f0-probe", "event"],
            vec!["--fireworks-f0", "--fireworks-f0-probe", "other"],
        ] {
            assert!(ViewerConfig::from_iter(arguments.into_iter().map(str::to_owned)).is_err());
        }
    }

    #[test]
    fn transparent_order_defaults_to_fast_and_can_be_opted_into_for_capture() {
        let fast = ViewerConfig::from_iter(std::iter::empty()).unwrap();
        let stable = ViewerConfig::from_iter(["--stable-transparency".to_owned()]).unwrap();
        assert_eq!(fast.transparent_order, TransparentOrderMode::Fast);
        assert_eq!(
            stable.transparent_order,
            TransparentOrderMode::StableCapture
        );
    }

    #[test]
    fn capture_sampling_selects_exact_evenly_spaced_frames() {
        let frames = (0..4)
            .map(|index| capture_frame(120, index, 4))
            .collect::<Vec<_>>();
        assert_eq!(frames, vec![15, 45, 75, 105]);
    }

    #[test]
    fn capture_waits_for_render_pipelines_before_positioning() {
        let mut capture = CapturePlan::new(
            CaptureMode::Standard {
                output: PathBuf::from("unused-capture-test"),
            },
            &CaptureSampling::EvenlySpaced(1),
            1,
            1.0,
        )
        .unwrap();
        capture.settle_frames = 0;
        let mut app = App::new();
        app.insert_resource(capture)
            .init_resource::<CaptureRenderReadiness>()
            .add_message::<AppExit>()
            .add_systems(Update, drive_capture);
        app.update();
        assert!(!app.world().resource::<CapturePlan>().positioned);
        assert!(!app.world().resource::<CapturePlan>().pending);
        app.world_mut()
            .resource_mut::<CaptureRenderReadiness>()
            .ready = true;
        app.update();
        assert!(app.world().resource::<CapturePlan>().positioned);
    }

    #[test]
    fn explicit_capture_frames_are_preserved_exactly() {
        let frames =
            resolve_sample_frames(&CaptureSampling::ExplicitFrames(vec![0, 17, 120]), 120).unwrap();
        assert_eq!(frames, vec![0, 17, 120]);
    }

    #[test]
    fn capture_times_resolve_to_deterministic_simulation_frames() {
        let frames =
            resolve_sample_frames(&CaptureSampling::ExplicitTimes(vec![0.0, 0.5, 1.25]), 120)
                .unwrap();
        assert_eq!(frames, vec![0, 30, 75]);
    }

    #[test]
    fn explicit_capture_sampling_rejects_ambiguous_or_out_of_range_frames() {
        assert!(
            resolve_sample_frames(&CaptureSampling::ExplicitTimes(vec![0.01, 0.02]), 120,).is_err()
        );
        assert!(
            resolve_sample_frames(&CaptureSampling::ExplicitFrames(vec![0, 121]), 120).is_err()
        );
        assert!(parse_sample_frames("10,2").is_err());
        assert!(parse_sample_times("0,nan").is_err());
    }

    #[test]
    fn viewport_smoke_pixel_scan_is_limited_to_the_requested_columns() {
        let mut image = RgbaImage::from_pixel(8, 4, Rgba([3, 4, 9, 255]));
        image.put_pixel(2, 1, Rgba([180, 120, 220, 255]));
        image.put_pixel(6, 2, Rgba([220, 180, 240, 255]));

        assert_eq!(luminous_pixels_in_columns(&image, 0, 4), 1);
        assert_eq!(luminous_pixels_in_columns(&image, 4, 4), 1);
        assert_eq!(luminous_pixels_in_columns(&image, 3, 3), 0);
    }

    #[test]
    fn viewer_seed_parser_supports_decimal_and_hex() {
        assert_eq!(parse_seed("42").unwrap(), 42);
        assert_eq!(parse_seed("0x2a").unwrap(), 42);
        assert!(parse_seed("seed").is_err());
    }

    #[test]
    fn viewer_prepares_nested_projects_and_reports_missing_children() {
        let config = |path| ViewerConfig {
            effect_path: Some(path),
            fireworks_f0: false,
            fireworks_probe: None,
            fireworks_camera: FireworksCamera::Audience,
            semantic_materials: true,
            wireframe: false,
            capture_mode: None,
            capture_sampling: CaptureSampling::EvenlySpaced(8),
            presentation: PresentationMode::Auto,
            transparent_order: TransparentOrderMode::Fast,
            max_gpu_particles: DEFAULT_GPU_PARTICLE_BUDGET,
            preview_seed: None,
            diagnostics: false,
            gpu_bench: None,
            history_policy: aestra_bevy::PlaybackHistoryPolicy::default(),
            view_3d: false,
            tier: aestra_bevy::QualityTier::default(),
            photographic: None,
            sprite_minimum_pixels: 0.0,
        };
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/test/effects/nested_moving_trail_lab.aestra.ron");
        let prepared = prepare_viewer(&config(path)).unwrap_or_else(|e| panic!("{}", e.message));
        assert_eq!(prepared.project.dependencies.len(), 2);
        assert!(Arc::ptr_eq(&prepared.compiled, &prepared.project.root));
        let scheduled = prepared.project.instances(1.0, 23);
        let leaf = scheduled.iter().find(|i| i.path.len() == 2).unwrap();
        assert!(!leaf.effect.material_programs.is_empty());
        assert!(
            leaf.effect
                .emitters
                .iter()
                .flat_map(|e| &e.renderers)
                .any(|r| matches!(r.kind, aestra_bevy::RendererPlanKind::Trail { .. }))
        );
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("missing.aestra.ron");
        let mut effect = EffectAsset::new("Missing child", 2.0);
        effect.effect_clips.push(aestra_bevy::EffectClip::new(
            aestra_bevy::EffectId::new(),
            0.0,
            1.0,
        ));
        effect.save_ron(&path).unwrap();
        let error = prepare_viewer(&config(path))
            .err()
            .expect("must not silently render only the root");
        assert!(error.message.contains("resolve viewer project"));
    }

    #[test]
    fn viewer_resolves_existing_mesh_materials_before_legacy_migration() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/test/effects/mesh_material_lab.aestra.ron");
        let mut effect = EffectAsset::load_ron(&path).unwrap();
        let programs = load_viewer_material_programs(&effect, Some(&path)).unwrap();
        assert_eq!(programs.len(), 1);
        let programs = migrate_viewer_materials(&mut effect, programs).unwrap();
        let compiled = EffectCompiler::default()
            .compile_with_material_programs(
                &effect,
                &programs
                    .into_iter()
                    .map(|program| (program.id, program))
                    .collect(),
            )
            .unwrap();
        assert!(
            compiled
                .requirements
                .renderers
                .contains(&aestra_bevy::RendererCapability::MeshParticles)
        );
    }

    #[test]
    fn semantic_viewer_mode_builds_live_bindings_without_rewriting_the_source() {
        let original = EffectAsset::from_ron(SAMPLE_SOURCE).unwrap();
        let mut migrated = original.clone();
        let programs = migrate_viewer_materials(&mut migrated, Vec::new()).unwrap();
        let programs = programs
            .into_iter()
            .map(|program| (program.id, program))
            .collect::<BTreeMap<_, _>>();
        let compiled = Arc::new(
            EffectCompiler::default()
                .compile_with_material_programs(&migrated, &programs)
                .unwrap(),
        );
        let presented = PresentedEffect::new(compiled);

        assert!(!programs.is_empty());
        assert_eq!(migrated.materials, original.materials);
        assert!(
            migrated
                .emitters
                .iter()
                .flat_map(|emitter| &emitter.renderers)
                .all(|renderer| presented.material_binding(renderer.material).is_some())
        );
    }
}
