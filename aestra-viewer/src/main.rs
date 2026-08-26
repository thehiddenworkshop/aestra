mod visual_regression;

use aestra_bevy::{
    ActiveBackend, AestraPlugin, AestraRuntimeStatus, AestraSettings, DEFAULT_GPU_PARTICLE_BUDGET,
    DEFAULT_PLAYBACK_TICK_RATE, EffectAsset, EffectPlayer, EffectRuntimeStatus, GpuCapabilities,
    PlaybackClock, PresentationMode,
};
use bevy::{
    app::AppExit,
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
    window::WindowResolution,
};
use image::{Rgba, RgbaImage, imageops};
use std::{env, fs, path::PathBuf};

use visual_regression::compare_capture;

const SAMPLE_SOURCE: &str = include_str!("../../assets/effects/prism_bloom.aestra.ron");
const VIEW_WIDTH: u32 = 960;
const VIEW_HEIGHT: u32 = 540;
const REGRESSION_SEED: u64 = 0xa357_2a11_5eed_0001;

fn main() {
    let config = ViewerConfig::from_args().unwrap_or_else(|error| {
        eprintln!("aestra-viewer: {error}");
        eprintln!("usage: aestra-viewer [--effect file.aestra.ron] [--backend auto|gpu|gpu-readback|cpu] [--seed number] [--max-gpu-particles count] [--frames 8] [--capture output-dir | --approve-visual-reference reference-dir | --visual-test reference-dir output-dir]");
        std::process::exit(2);
    });
    let preview_seed = config.resolved_seed();
    let capture = config
        .capture_mode
        .clone()
        .map(|mode| CapturePlan::new(mode, config.capture_frames, preview_seed));

    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb(0.009, 0.012, 0.024)))
        .insert_resource(AestraSettings {
            presentation: config.presentation,
            max_gpu_particles: config.max_gpu_particles,
        })
        .insert_resource(config)
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Aestra Viewer".into(),
                    resolution: WindowResolution::new(VIEW_WIDTH, VIEW_HEIGHT),
                    resizable: true,
                    ..default()
                }),
                ..default()
            }),
            AestraPlugin,
        ))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (viewer_controls, update_hud, drive_capture.after(update_hud)),
        );
    if let Some(capture) = capture {
        app.insert_resource(capture);
    }
    if let AppExit::Error(code) = app.run() {
        std::process::exit(i32::from(code.get()));
    }
}

#[derive(Resource)]
struct ViewerConfig {
    effect_path: Option<PathBuf>,
    capture_mode: Option<CaptureMode>,
    capture_frames: usize,
    presentation: PresentationMode,
    max_gpu_particles: u32,
    preview_seed: Option<u64>,
}

#[derive(Clone)]
enum CaptureMode {
    Standard { output: PathBuf },
    Approve { reference: PathBuf },
    Compare { reference: PathBuf, output: PathBuf },
}

impl CaptureMode {
    fn output_directory(&self) -> &PathBuf {
        match self {
            Self::Standard { output } | Self::Compare { output, .. } => output,
            Self::Approve { reference } => reference,
        }
    }

    fn is_regression(&self) -> bool {
        !matches!(self, Self::Standard { .. })
    }
}

impl ViewerConfig {
    fn from_args() -> Result<Self, String> {
        let mut effect_path = None;
        let mut capture_mode = None;
        let mut capture_frames = 8usize;
        let mut presentation = PresentationMode::Auto;
        let mut max_gpu_particles = DEFAULT_GPU_PARTICLE_BUDGET;
        let mut preview_seed = None;
        let mut args = env::args().skip(1);
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--effect" => {
                    effect_path = Some(PathBuf::from(
                        args.next().ok_or("--effect requires a file path")?,
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
                "--frames" => {
                    capture_frames = args
                        .next()
                        .ok_or("--frames requires a number")?
                        .parse::<usize>()
                        .map_err(|_| "--frames must be a positive integer")?;
                    if capture_frames == 0 || capture_frames > 64 {
                        return Err("--frames must be between 1 and 64".into());
                    }
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
            capture_mode,
            capture_frames,
            presentation,
            max_gpu_particles,
            preview_seed,
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

fn parse_seed(value: &str) -> Result<u64, String> {
    value
        .strip_prefix("0x")
        .map_or_else(|| value.parse::<u64>(), |hex| u64::from_str_radix(hex, 16))
        .map_err(|_| "--seed must be a decimal or 0x-prefixed integer".into())
}

fn set_capture_mode(target: &mut Option<CaptureMode>, mode: CaptureMode) -> Result<(), String> {
    if target.is_some() {
        return Err("capture, approval, and visual-test modes are mutually exclusive".into());
    }
    *target = Some(mode);
    Ok(())
}

#[derive(Resource)]
struct CapturePlan {
    mode: CaptureMode,
    frame_count: usize,
    next_frame: usize,
    settle_frames: u8,
    positioned: bool,
    pending: bool,
    images: Vec<RgbaImage>,
    seed: u64,
    sampled_frames: Vec<u64>,
}

impl CapturePlan {
    fn new(mode: CaptureMode, frame_count: usize, seed: u64) -> Self {
        Self {
            mode,
            frame_count,
            next_frame: 0,
            // Let the window, glyph atlas, sprite pipelines, and particle pool reach the render
            // world before the first capture. Later samples only need a short seek settle.
            settle_frames: 20,
            positioned: false,
            pending: false,
            images: Vec::with_capacity(frame_count),
            seed,
            sampled_frames: Vec::with_capacity(frame_count),
        }
    }
}

#[derive(Component)]
struct ViewerHud;

fn setup(mut commands: Commands, config: Res<ViewerConfig>) {
    let effect = config
        .effect_path
        .as_ref()
        .map_or_else(
            || EffectAsset::from_ron(SAMPLE_SOURCE),
            EffectAsset::load_ron,
        )
        .unwrap_or_else(|error| panic!("could not load viewer effect: {error}"));
    let effect_name = effect.name.clone();
    let regression_scene = config
        .capture_mode
        .as_ref()
        .is_some_and(CaptureMode::is_regression);

    commands.spawn(Camera2d);
    let mut player = EffectPlayer::new(&effect);
    player.set_seed(config.resolved_seed());
    commands.spawn(player);

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
        "F{:05} @ {} Hz  |  {:06.3} / {:06.3}  |  seed {:016x}  |  {}  |  {}  |  ←/→ Step  |  [/] Seed",
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
    if capture.pending || capture.next_frame >= capture.frame_count {
        return;
    }
    if capture.settle_frames > 0 {
        capture.settle_frames -= 1;
        return;
    }

    if !capture.positioned {
        for mut player in &mut players {
            let maximum = PlaybackClock::default().maximum_frame(player.effect().duration);
            let sample_frame = capture_frame(maximum, capture.next_frame, capture.frame_count);
            player.seek_frame(sample_frame);
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

fn receive_capture(
    event: On<ScreenshotCaptured>,
    mut capture: ResMut<CapturePlan>,
    runtime: Res<AestraRuntimeStatus>,
    settings: Res<AestraSettings>,
    capabilities: Res<GpuCapabilities>,
    effects: Query<&EffectRuntimeStatus>,
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

    if capture.next_frame == capture.frame_count {
        let effect_runtime = effects.single().ok();
        write_contact_sheet(&capture, &runtime, effect_runtime, &settings, &capabilities);
        let result = finish_capture(&capture, &runtime, effect_runtime, &capabilities);
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
    let columns = (capture.frame_count as f32).sqrt().ceil() as u32;
    let rows = (capture.frame_count as u32).div_ceil(columns);
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
        capture.frame_count,
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

fn finish_capture(
    capture: &CapturePlan,
    runtime: &AestraRuntimeStatus,
    effect_runtime: Option<&EffectRuntimeStatus>,
    capabilities: &GpuCapabilities,
) -> Result<(), String> {
    let active = effect_runtime.map_or(runtime.active, |status| status.active);
    if capture.mode.is_regression() && active != ActiveBackend::Gpu {
        return Err(format!(
            "visual regression requires the native GPU backend, but {active} was selected"
        ));
    }
    match &capture.mode {
        CaptureMode::Standard { output } => {
            println!("capture written to {}", output.display());
            Ok(())
        }
        CaptureMode::Approve { reference } => {
            fs::write(
                reference.join("visual-reference.md"),
                format!(
                    "# Aestra visual reference\n\n- Frames: {}\n- Frame size: {} x {}\n- Seed: `{:#018x}`\n- Scene: effect only, fixed camera and background\n- Sampling: exact {} Hz simulation frames {:?}\n- Backend: {}\n- Adapter: {} ({})\n",
                    capture.frame_count,
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
            .map_err(|error| format!("could not write visual reference metadata: {error}"))?;
            println!("visual reference approved at {}", reference.display());
            Ok(())
        }
        CaptureMode::Compare { reference, output } => {
            let report = compare_capture(reference, output, capture.frame_count)?;
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
    }
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
    fn viewer_seed_parser_supports_decimal_and_hex() {
        assert_eq!(parse_seed("42").unwrap(), 42);
        assert_eq!(parse_seed("0x2a").unwrap(), 42);
        assert!(parse_seed("seed").is_err());
    }
}
