mod gpu_bench;
mod preview_report;
mod visual_regression;

use aestra_authoring::{MaterialAuthoringDocument, migrate_legacy_sprite_materials};
use aestra_bevy::material::MaterialProgram;
use aestra_bevy::{
    ActiveBackend, AestraPlugin, AestraRuntimeStatus, AestraSettings, DEFAULT_GPU_PARTICLE_BUDGET,
    DEFAULT_PLAYBACK_TICK_RATE, EffectAsset, EffectCompiler, EffectPlayer, EffectProfiler,
    EffectRuntimeStatus, GpuCapabilities, PlaybackClock, PresentationMode, PresentedEffect,
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
    window::WindowResolution,
};
use image::{Rgba, RgbaImage, imageops};
use std::{collections::BTreeMap, env, fs, path::PathBuf, sync::Arc};

use preview_report::{
    CompilerPreviewData, PreviewCaptureData, PreviewRuntimeData, write_preview_failure_report,
    write_preview_report,
};
use visual_regression::{ComparisonReport, compare_capture};

const SAMPLE_SOURCE: &str = include_str!("../../../assets/effects/prism_bloom.aestra.ron");
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
    let config = ViewerConfig::from_args().unwrap_or_else(|error| {
        eprintln!("aestra-viewer: {error}");
        eprintln!("usage: aestra-viewer [--effect file.aestra.ron] [--semantic-materials] [--wireframe] [--diagnostics] [--gpu-bench output.json] [--backend auto|gpu|gpu-readback|cpu] [--seed number] [--max-gpu-particles count] [--frames 8 | --sample-frames 0,30,60 | --sample-times 0,0.5,1] [--capture output-dir | --approve-visual-reference reference-dir | --visual-test reference-dir output-dir | --editor-viewport-smoke output-dir]");
        std::process::exit(2);
    });
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
    let gpu_bench_output = config.gpu_bench.clone();
    let gpu_bench_effect = config
        .effect_path
        .as_ref()
        .and_then(|path| path.file_stem())
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "prism_bloom".to_owned());

    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb(0.009, 0.012, 0.024)))
        .insert_resource(AestraSettings {
            presentation: config.presentation,
            max_gpu_particles: config.max_gpu_particles,
        })
        .insert_resource(prepared)
        .insert_resource(config)
        .add_plugins((
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: "../../assets".into(),
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
                viewer_controls,
                update_hud,
                drive_capture.after(update_hud),
                gpu_bench::drive_gpu_bench,
            ),
        );
    if let Some(capture) = capture {
        app.insert_resource(capture);
    }
    if let Some(output) = gpu_bench_output {
        app.insert_resource(gpu_bench::GpuBenchPlan::new(
            output,
            gpu_bench_effect,
            gpu_bench::DEFAULT_GPU_BENCH_WARMUP,
            gpu_bench::DEFAULT_GPU_BENCH_FRAMES,
        ));
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
    compiler: CompilerPreviewData,
}

struct PreparationFailure {
    message: String,
    diagnostics: Vec<aestra_bevy::Diagnostic>,
}

#[derive(Resource)]
struct ViewerConfig {
    effect_path: Option<PathBuf>,
    semantic_materials: bool,
    wireframe: bool,
    capture_mode: Option<CaptureMode>,
    capture_sampling: CaptureSampling,
    presentation: PresentationMode,
    max_gpu_particles: u32,
    preview_seed: Option<u64>,
    diagnostics: bool,
    gpu_bench: Option<PathBuf>,
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
        let mut effect_path = None;
        let mut semantic_materials = false;
        let mut wireframe = false;
        let mut capture_mode = None;
        let mut capture_sampling = CaptureSampling::EvenlySpaced(8);
        let mut capture_sampling_was_set = false;
        let mut presentation = PresentationMode::Auto;
        let mut max_gpu_particles = DEFAULT_GPU_PARTICLE_BUDGET;
        let mut preview_seed = None;
        let mut diagnostics = false;
        let mut gpu_bench = None;
        let mut args = env::args().skip(1);
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--effect" => {
                    effect_path = Some(PathBuf::from(
                        args.next().ok_or("--effect requires a file path")?,
                    ));
                }
                "--semantic-materials" => semantic_materials = true,
                "--wireframe" => wireframe = true,
                "--diagnostics" => diagnostics = true,
                "--gpu-bench" => {
                    gpu_bench = Some(PathBuf::from(
                        args.next()
                            .ok_or("--gpu-bench requires an output JSON path")?,
                    ));
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
        Ok(Self {
            effect_path,
            semantic_materials,
            wireframe,
            capture_mode,
            capture_sampling,
            presentation,
            max_gpu_particles,
            preview_seed,
            diagnostics,
            gpu_bench,
        })
    }

    fn resolved_seed(&self) -> u64 {
        self.preview_seed.unwrap_or_else(|| {
            if self
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
    let mut effect = config
        .effect_path
        .as_ref()
        .map_or_else(
            || EffectAsset::from_ron(SAMPLE_SOURCE),
            EffectAsset::load_ron,
        )
        .map_err(|error| PreparationFailure {
            message: format!("could not load viewer effect: {error}"),
            diagnostics: Vec::new(),
        })?;
    let material_programs = load_viewer_material_programs(&effect, config.effect_path.as_deref())
        .map_err(|message| PreparationFailure {
        message,
        diagnostics: Vec::new(),
    })?;
    let material_programs = if config.semantic_materials {
        migrate_viewer_materials(&mut effect, material_programs).map_err(|error| {
            PreparationFailure {
                message: format!("could not migrate viewer materials: {error}"),
                diagnostics: Vec::new(),
            }
        })?
    } else {
        material_programs
    };
    let mut diagnostics = effect.validation_report().diagnostics;
    diagnostics.extend(
        material_programs
            .iter()
            .flat_map(|program| program.validation_report().diagnostics),
    );
    diagnostics.sort();
    diagnostics.dedup();
    let material_programs = material_programs
        .into_iter()
        .map(|program| (program.id, program))
        .collect::<BTreeMap<_, _>>();
    let compiled = EffectCompiler::default()
        .compile_with_material_programs(&effect, &material_programs)
        .map_err(|error| PreparationFailure {
            message: format!("could not compile viewer effect: {error}"),
            diagnostics: error.report().diagnostics.clone(),
        })?;
    let material_program_fingerprints = compiled
        .material_programs
        .iter()
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
    let compiler = CompilerPreviewData::new(&compiled, diagnostics, material_program_fingerprints);
    Ok(PreparedViewer {
        compiled: Arc::new(compiled),
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

fn setup(mut commands: Commands, config: Res<ViewerConfig>, prepared: Res<PreparedViewer>) {
    let effect_name = prepared.compiled.name.clone();
    let regression_scene = config
        .capture_mode
        .as_ref()
        .is_some_and(CaptureMode::is_regression);
    let editor_viewport_smoke = config
        .capture_mode
        .as_ref()
        .is_some_and(CaptureMode::is_editor_viewport_smoke);

    let mut player = EffectPlayer::from_compiled(Arc::clone(&prepared.compiled));
    if config.wireframe {
        player.set_render_mode(aestra_bevy::EffectRenderMode::Wireframe);
    }
    player.set_seed(config.resolved_seed());
    let presentation = PresentedEffect::new(player.effect().clone());
    if editor_viewport_smoke {
        spawn_editor_viewport_smoke_scene(&mut commands);
        commands.spawn((player, presentation, RenderLayers::layer(0)));
        return;
    }

    commands.spawn(Camera2d);
    commands.spawn((player, presentation));

    if regression_scene {
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

fn load_viewer_material_programs(
    effect: &EffectAsset,
    path: Option<&std::path::Path>,
) -> Result<Vec<MaterialProgram>, String> {
    if effect.material_instances.is_empty() {
        return Ok(Vec::new());
    }
    let default_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    let parent = path
        .and_then(std::path::Path::parent)
        .unwrap_or(&default_root);
    let root = if parent.file_name().is_some_and(|name| name == "effects") {
        parent.parent().unwrap_or(parent)
    } else {
        parent
    };
    let index = aestra_project::ProjectAssetIndex::scan(root);
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
    *effect = document.effect;
    Ok(document.programs)
}

fn spawn_editor_viewport_smoke_scene(commands: &mut Commands) {
    let preview_viewport = Viewport {
        physical_position: UVec2::new(EDITOR_PREVIEW_X, EDITOR_PREVIEW_Y),
        physical_size: UVec2::new(EDITOR_PREVIEW_WIDTH, EDITOR_PREVIEW_HEIGHT),
        ..default()
    };
    commands.spawn((
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
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
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

fn editor_preview_camera_transform() -> Transform {
    let orbit = Quat::from_rotation_x(-0.35);
    Transform::from_translation(orbit * Vec3::Z * 140.0).looking_at(Vec3::ZERO, Vec3::Y)
}

fn viewer_controls(
    keys: Res<ButtonInput<KeyCode>>,
    mut players: Query<&mut EffectPlayer>,
    mut commands: Commands,
    mut screenshot_index: Local<u32>,
) {
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
            let seed = player.instance.seed().wrapping_sub(1);
            player.set_seed(seed);
        }
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        for mut player in &mut players {
            let seed = player.instance.seed().wrapping_add(1);
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
        player.instance.seed(),
        if player.playing { "PLAYING" } else { "PAUSED" },
        runtime.active,
    );
}

fn drive_capture(
    capture: Option<ResMut<CapturePlan>>,
    mut players: Query<&mut EffectPlayer>,
    mut commands: Commands,
) {
    let Some(mut capture) = capture else {
        return;
    };
    if capture.pending || capture.next_frame >= capture.frame_count() {
        return;
    }
    if capture.settle_frames > 0 {
        capture.settle_frames -= 1;
        return;
    }

    if !capture.positioned {
        for mut player in &mut players {
            let sample_frame = capture.sample_frames[capture.next_frame];
            let has_trails = player
                .effect()
                .emitters
                .iter()
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
                        .instance
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
    runtime: Res<'w, AestraRuntimeStatus>,
    settings: Res<'w, AestraSettings>,
    capabilities: Res<'w, GpuCapabilities>,
    prepared: Res<'w, PreparedViewer>,
    effects: Query<'w, 's, (&'static EffectRuntimeStatus, &'static EffectProfiler)>,
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
        let effect_runtime = effect_status.map(|(runtime, _)| runtime);
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
            },
            &report.prepared.compiler,
            PreviewRuntimeData {
                runtime: &report.runtime,
                effect_runtime,
                settings: &report.settings,
                capabilities: &report.capabilities,
                profile: effect_status.map(|(_, profile)| &profile.0),
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
    fn capture_sampling_selects_exact_evenly_spaced_frames() {
        let frames = (0..4)
            .map(|index| capture_frame(120, index, 4))
            .collect::<Vec<_>>();
        assert_eq!(frames, vec![15, 45, 75, 105]);
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
    fn viewer_resolves_existing_mesh_materials_before_legacy_migration() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/effects/mesh_material_lab.aestra.ron");
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
