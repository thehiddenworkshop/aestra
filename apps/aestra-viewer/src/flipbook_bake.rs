//! Fixed-rate transparent capture and playback export for self-contained sprite effects.
pub(super) mod loop_seam;

use super::*;
use aestra_bevy::{
    AssetDefinition, AssetId, AssetKind, BlendMode, CollisionInputSource, ColorKey, CompiledEffect,
    Curve, CurveKey, EffectDimension, Emitter, EmitterShape, FlipbookDefinition,
    FlipbookTimeSource, Gradient, MaterialDefinition, ModuleInstance, PlaybackClock,
    PlaybackHistoryPolicy, PresentationCadence, RendererInstance, RendererPlanKind,
    RendererProperties, ScalarRange, SimulationSeekMode, UvRect,
    gpu::{AestraCaptureOpacity, GpuPlaybackDiagnostics},
    pixel_render_target,
};
use bevy::{
    app::PluginsState,
    asset::LoadState,
    camera::{RenderTarget, ScalingMode},
    render::renderer::RenderDevice,
    window::ExitCondition,
};
use std::{ffi::OsString, num::NonZeroU32, path::Path};

const EDGE: u32 = 64;
const TICK_RATE: NonZeroU32 = NonZeroU32::new(DEFAULT_PLAYBACK_TICK_RATE).unwrap();
const CAPTURE_SETTLE_FRAMES: u32 = 4;
const ATLAS_COLUMNS: u32 = 8;
const TIMEOUT: Duration = Duration::from_secs(60);
const ATLAS_PATH: &str = "textures/atlas.png";
const ADDITIVE_ATLAS_PATH: &str = "textures/atlas_additive.png";
const EFFECT_PATH: &str = "effects/flipbook.aestra.ron";

#[derive(Clone, Copy, PartialEq, Eq)]
enum CapturePurpose {
    Flipbook,
    LoopSeam,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct BakeView {
    size: u32,
    span: f32,
    center: Vec3,
    eye: Option<Vec3>,
}

impl Default for BakeView {
    fn default() -> Self {
        Self {
            size: EDGE,
            span: EDGE as f32,
            center: Vec3::ZERO,
            eye: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct BakeOptions {
    view: BakeView,
    cadence: PresentationCadence,
    crop: bool,
    padding: u32,
}

impl Default for BakeOptions {
    fn default() -> Self {
        Self {
            view: BakeView::default(),
            cadence: PresentationCadence::FPS_12,
            crop: false,
            padding: 0,
        }
    }
}

impl BakeOptions {
    fn parse(arguments: impl Iterator<Item = OsString>) -> Result<Self, String> {
        Self::parse_for(arguments, CapturePurpose::Flipbook)
    }

    fn parse_for(
        mut arguments: impl Iterator<Item = OsString>,
        purpose: CapturePurpose,
    ) -> Result<Self, String> {
        let mut options = Self::default();
        let view = &mut options.view;
        let mut span: Option<f32> = None;
        while let Some(flag) = arguments.next() {
            let flag = flag.to_string_lossy();
            if flag == "--crop" && purpose == CapturePurpose::Flipbook {
                options.crop = true;
                continue;
            }
            let value = arguments
                .next()
                .ok_or_else(|| format!("{flag} needs a value"))?;
            let value = value.to_string_lossy();
            match flag.as_ref() {
                "--fps" if purpose == CapturePurpose::Flipbook => {
                    let fps = value
                        .parse::<NonZeroU32>()
                        .ok()
                        .filter(|fps| *fps <= TICK_RATE)
                        .ok_or_else(|| format!("--fps must be an integer from 1 to {TICK_RATE}"))?;
                    options.cadence = PresentationCadence::new(fps);
                }
                "--size" => {
                    view.size = value
                        .parse()
                        .ok()
                        .filter(|size| *size > 0)
                        .ok_or("--size must be a positive integer")?
                }
                "--span" => {
                    span = Some(
                        value
                            .parse()
                            .map_err(|_| "--span must be a positive finite distance")?,
                    )
                }
                "--center" => view.center = parse_position(&flag, &value)?,
                "--eye" => view.eye = Some(parse_position(&flag, &value)?),
                "--padding" if purpose == CapturePurpose::Flipbook => {
                    options.padding = value
                        .parse()
                        .map_err(|_| "--padding must be a non-negative integer")?;
                }
                _ => return Err(format!("unknown bake option {flag}")),
            }
        }
        view.span = span.unwrap_or(view.size as f32);
        if view.span <= 0.0 || !(view.span * 2.0).is_finite() || !(2.0 / view.span).is_finite() {
            return Err("--span must be a positive finite distance".into());
        }
        if let Some(eye) = view.eye {
            let distance = eye.distance(view.center);
            if distance == 0.0 || !(distance + 2.0 * view.span).is_finite() {
                return Err("--eye must be a finite distance away from --center".into());
            }
        }
        Ok(options)
    }
}

fn parse_position(flag: &str, value: &str) -> Result<Vec3, String> {
    let components = value.split(',').map(|value| value.trim().parse::<f32>());
    let Ok(position) = components.collect::<Result<Vec<_>, _>>() else {
        return Err(format!("{flag} needs three finite coordinates as X,Y,Z"));
    };
    match position.as_slice() {
        [x, y, z] if position.iter().all(|value| value.is_finite()) => Ok(Vec3::new(*x, *y, *z)),
        _ => Err(format!("{flag} needs three finite coordinates as X,Y,Z")),
    }
}

pub(super) fn run(mut arguments: impl Iterator<Item = OsString>) -> Result<(), String> {
    let (Some(input), Some(output)) = (arguments.next(), arguments.next()) else {
        return Err(
            "usage: aestra-viewer --bake-flipbook INPUT.aestra.ron NEW_OUTPUT_DIRECTORY [--fps FPS] [--size PIXELS] [--span UNITS] [--center X,Y,Z] [--eye X,Y,Z] [--crop] [--padding PIXELS]".into(),
        );
    };
    let options = BakeOptions::parse(arguments)?;
    let input = PathBuf::from(input);
    let output = PathBuf::from(output);
    if output.exists() {
        return Err(format!(
            "output directory already exists: {}",
            output.display()
        ));
    }
    let prepared = prepare(&input)?;
    let effect = &prepared.viewer.compiled;
    let count = frame_count(effect.duration, options.cadence)?;
    let capture = capture(&prepared, count, options, Color::NONE)?;
    export(effect, &capture, &output, options)?;
    println!(
        "Baked {count} frames at {} FPS to {}",
        options.cadence.frames_per_second(),
        output.join(EFFECT_PATH).display()
    );
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BakeBlend {
    Alpha,
    Additive,
    Mixed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BakeLayer {
    Alpha,
    Additive,
}

impl BakeBlend {
    fn has_content(self, pixel: &Rgba<u8>) -> bool {
        pixel[3] != 0 || (self != Self::Alpha && pixel.0[..3] != [0; 3])
    }

    fn layers(self) -> &'static [(BakeLayer, &'static str)] {
        match self {
            Self::Alpha => &[(BakeLayer::Alpha, ATLAS_PATH)],
            Self::Additive => &[(BakeLayer::Additive, ATLAS_PATH)],
            Self::Mixed => &[
                (BakeLayer::Alpha, ATLAS_PATH),
                (BakeLayer::Additive, ADDITIVE_ATLAS_PATH),
            ],
        }
    }
}

struct PreparedBake {
    viewer: PreparedViewer,
    blend: BakeBlend,
}

fn prepare(input: &Path) -> Result<PreparedBake, String> {
    prepare_capture(input, CapturePurpose::Flipbook)
}

fn prepare_capture(input: &Path, purpose: CapturePurpose) -> Result<PreparedBake, String> {
    let input = input.canonicalize().map_err(|error| error.to_string())?;
    let config =
        ViewerConfig::from_iter(["--effect".into(), input.to_string_lossy().into_owned()])?;
    let prepared = prepare_viewer(&config).map_err(|failure| failure.message)?;
    let effect = &prepared.compiled;
    if purpose == CapturePurpose::Flipbook && effect.playback_mode.is_continuous() {
        return Err("baking requires one-shot or restart-loop playback".into());
    }
    if !effect.effect_clips.is_empty()
        || !effect.bindings.is_empty()
        || !effect.host_fields.is_empty()
        || !effect.point_lights.is_empty()
        || !effect.event_inputs.is_empty()
        || effect.host_transform_track.is_some()
        || effect
            .collision_inputs()
            .sources()
            .iter()
            .any(|source| *source != CollisionInputSource::AuthoredColliders)
    {
        return Err("capture requires a self-contained effect without host or scene inputs".into());
    }
    if effect.all_extension_stages().next().is_some() {
        return Err("capture supports the built-in particle simulation stages".into());
    }
    let mut bake_blend = None;
    for renderer in effect
        .emitters
        .iter()
        .filter(|emitter| emitter.enabled)
        .flat_map(|emitter| &emitter.renderers)
    {
        if !matches!(
            renderer.kind,
            RendererPlanKind::Sprite | RendererPlanKind::Flipbook { .. }
        ) {
            return Err("capture supports sprite and flipbook renderers".into());
        }
        let blend = if let Some(instance) = effect.material_instance(renderer.material) {
            let program = effect
                .material_program(instance.program.id())
                .ok_or("missing material program")?;
            let shader = aestra_bevy::compile_material_program(program)
                .map_err(|error| error.to_string())?;
            if shader.requires_scene_lighting() || shader.requires_scene_depth() {
                return Err("capture requires an unlit material without scene inputs".into());
            }
            instance.render_state.blend
        } else {
            effect
                .material(renderer.material)
                .ok_or("missing sprite material")?
                .blend
        };
        let blend = match blend {
            BlendMode::Alpha => BakeBlend::Alpha,
            BlendMode::Additive => BakeBlend::Additive,
            BlendMode::Multiply => {
                return Err("capture supports alpha and additive blending".into());
            }
        };
        bake_blend = Some(if bake_blend.is_some_and(|previous| previous != blend) {
            BakeBlend::Mixed
        } else {
            blend
        });
    }
    let blend = bake_blend.ok_or("the effect has no enabled renderers")?;
    Ok(PreparedBake {
        viewer: prepared,
        blend,
    })
}

fn frame_count(duration: f32, cadence: PresentationCadence) -> Result<u32, String> {
    let limit = aestra_gpu::MAX_FLIPBOOK_FRAMES as u32;
    let clock = PlaybackClock::default();
    let count = (0..=limit)
        .take_while(|&frame| {
            clock.time_for_frame(cadence.tick_for_frame(frame, TICK_RATE), f32::MAX) < duration
        })
        .count() as u32;
    if count == 0 {
        return Err("there are no simulation samples before the effect ends".into());
    }
    if count > limit {
        return Err(format!(
            "the effect exceeds the {limit}-frame flipbook limit at {} FPS. Choose a lower --fps or shorten the effect",
            cadence.frames_per_second(),
        ));
    }
    Ok(count)
}

#[derive(Resource, Default)]
struct Readback(Option<Image>);

struct CapturedFrames {
    frames: Vec<RgbaImage>,
    texture_limit: u32,
    blend: BakeBlend,
}

fn capture(
    prepared: &PreparedBake,
    count: u32,
    options: BakeOptions,
    clear_color: Color,
) -> Result<CapturedFrames, String> {
    let ticks = (0..count).map(|index| options.cadence.tick_for_frame(index, TICK_RATE));
    capture_ticks(
        prepared,
        ticks,
        options,
        clear_color,
        CapturePurpose::Flipbook,
    )
}

fn capture_ticks(
    prepared: &PreparedBake,
    ticks: impl ExactSizeIterator<Item = u64>,
    options: BakeOptions,
    clear_color: Color,
    purpose: CapturePurpose,
) -> Result<CapturedFrames, String> {
    let view = options.view;
    let count = ticks.len() as u32;
    let mut app = App::new();
    app.insert_resource(AestraSettings {
        presentation: PresentationMode::Gpu,
        transparent_order: TransparentOrderMode::StableCapture,
        ..default()
    })
    .init_resource::<Readback>()
    .init_resource::<CaptureRenderReadiness>()
    .add_plugins((
        DefaultPlugins
            .set(AssetPlugin {
                file_path: prepared.viewer.asset_root.to_string_lossy().into_owned(),
                ..default()
            })
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::log::LogPlugin>(),
        AestraPlugin,
    ));
    app.get_sub_app_mut(RenderApp)
        .ok_or("the render app could not start")?
        .add_systems(ExtractSchedule, publish_capture_render_readiness);
    let deadline = Instant::now() + TIMEOUT;
    while app.plugins_state() != PluginsState::Ready {
        if Instant::now() >= deadline {
            return Err("GPU initialization timed out".into());
        }
        bevy::tasks::tick_global_task_pools_on_main_thread();
        std::thread::sleep(Duration::from_millis(1));
    }
    app.finish();
    app.cleanup();
    let world = app.world_mut();
    let effect = &prepared.viewer.compiled;
    let stateful = effect.seek_mode != SimulationSeekMode::StatelessDirect;
    let limit = world
        .get_resource::<RenderDevice>()
        .ok_or("no GPU render device is available")?
        .limits()
        .max_texture_dimension_2d;
    if stateful {
        world
            .get_resource::<GpuPlaybackDiagnostics>()
            .ok_or("GPU tick diagnostics are required for stateful capture")?
            .set_enabled(true);
    }
    if purpose == CapturePurpose::Flipbook {
        let columns = count.min(ATLAS_COLUMNS);
        let atlas_cells = columns.max(count.div_ceil(columns));
        if view.size > limit / atlas_cells {
            return Err(format!(
                "the atlas for {count} frames at {}×{} would exceed the adapter's {limit}-pixel texture limit",
                view.size, view.size
            ));
        }
        if options.padding > 0 {
            let minimum_edge = if options.crop { 1 } else { view.size };
            AtlasLayout::new(
                count,
                columns,
                UVec2::ZERO,
                minimum_edge,
                options.padding,
                limit,
            )?;
        }
    } else if view.size > limit {
        return Err(format!(
            "capture size exceeds the adapter's {limit}-pixel texture limit"
        ));
    }
    let textures = prepared
        .viewer
        .compiled
        .assets
        .iter()
        .filter(|asset| asset.kind == AssetKind::Texture)
        .map(|asset| {
            world
                .resource::<AssetServer>()
                .load::<Image>(asset.path.clone())
        })
        .collect::<Vec<_>>();
    let image = pixel_render_target(UVec2::splat(view.size)).ok_or("invalid render-target size")?;
    let target = world.resource_mut::<Assets<Image>>().add(image);
    let camera = (
        Camera {
            clear_color: ClearColorConfig::Custom(clear_color),
            ..default()
        },
        RenderTarget::Image(target.clone().into()),
        Msaa::Off,
    );
    let scaling_mode = ScalingMode::Fixed {
        width: view.span,
        height: view.span,
    };
    let mut camera = if let Some(eye) = view.eye {
        let direction = (view.center - eye).normalize();
        let up = if direction.y.abs() > 0.999 {
            Vec3::Z
        } else {
            Vec3::Y
        };
        let defaults = OrthographicProjection::default_3d();
        world.spawn((
            Camera3d::default(),
            camera,
            Projection::Orthographic(OrthographicProjection {
                scaling_mode,
                far: defaults
                    .far
                    .max(eye.distance(view.center) + 2.0 * view.span),
                ..defaults
            }),
            Transform::from_translation(eye).looking_at(view.center, up),
        ))
    } else {
        world.spawn((
            Camera2d,
            camera,
            Projection::Orthographic(OrthographicProjection {
                scaling_mode,
                ..OrthographicProjection::default_2d()
            }),
            Transform::from_translation(view.center),
        ))
    };
    if prepared.blend == BakeBlend::Mixed {
        camera.insert(AestraCaptureOpacity);
    }
    let mut player = EffectPlayer::from_project(prepared.viewer.project.clone())
        .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly);
    if !stateful && purpose == CapturePurpose::Flipbook {
        player.set_presentation_cadence(Some(options.cadence));
    }
    player.playing = false;
    player.set_seed(match purpose {
        CapturePurpose::Flipbook => REGRESSION_SEED,
        CapturePurpose::LoopSeam => loop_seam::SEED,
    });
    let owner = world
        .spawn((player, PresentedEffect::new(effect.clone())))
        .id();
    let mut frames = Vec::with_capacity(count as usize);
    let clock = PlaybackClock::default();
    let mut next_tick = 0;
    for sample_tick in ticks {
        if stateful {
            // Advance the live simulation once per tick, including ticks between atlas frames.
            for tick in next_tick..=sample_tick {
                let time = clock.time_for_frame(tick, effect.duration);
                app.world_mut()
                    .get_mut::<EffectPlayer>(owner)
                    .unwrap()
                    .set_playback_time(time);
                let settle_frames = if tick == sample_tick {
                    CAPTURE_SETTLE_FRAMES
                } else {
                    1
                };
                wait_for_frame(&mut app, owner, &textures, Some(time), settle_frames)?;
            }
            next_tick = sample_tick + 1;
        } else {
            app.world_mut()
                .get_mut::<EffectPlayer>(owner)
                .unwrap()
                .seek_frame(sample_tick);
            wait_for_frame(&mut app, owner, &textures, None, CAPTURE_SETTLE_FRAMES)?;
        }
        app.world_mut()
            .spawn(Screenshot::image(target.clone()))
            .observe(
                |event: On<ScreenshotCaptured>, mut readback: ResMut<Readback>| {
                    readback.0 = Some(event.image.clone());
                },
            );
        let deadline = Instant::now() + TIMEOUT;
        let frame = loop {
            pump(&mut app);
            if let Some(frame) = app.world_mut().resource_mut::<Readback>().0.take() {
                break frame;
            }
            if Instant::now() >= deadline {
                return Err("offscreen frame readback timed out".into());
            }
        };
        frames.push(
            frame
                .try_into_dynamic()
                .map_err(|error| error.to_string())?
                .to_rgba8(),
        );
    }
    Ok(CapturedFrames {
        frames,
        texture_limit: limit,
        blend: prepared.blend,
    })
}

fn pump(app: &mut App) {
    app.update();
    std::thread::sleep(Duration::from_millis(1));
}

fn wait_for_frame(
    app: &mut App,
    owner: Entity,
    textures: &[Handle<Image>],
    expected_time: Option<f32>,
    settle_frames: u32,
) -> Result<(), String> {
    let deadline = Instant::now() + TIMEOUT;
    let mut ready_frames = 0;
    while ready_frames < settle_frames {
        pump(app);
        let world = app.world();
        if let Some(status) = world.get::<EffectRuntimeStatus>(owner)
            && status.active == ActiveBackend::CpuReference
        {
            return Err(format!("native rendering is required: {}", status.reason));
        }
        let assets = world.resource::<AssetServer>();
        if let Some(error) =
            textures
                .iter()
                .find_map(|handle| match assets.get_load_state(handle.id()) {
                    Some(LoadState::Failed(error)) => Some(error),
                    _ => None,
                })
        {
            return Err(format!("could not load bake texture: {error}"));
        }
        let readiness = world.resource::<CaptureRenderReadiness>();
        let encoded_time = expected_time.and_then(|time| {
            world
                .resource::<GpuPlaybackDiagnostics>()
                .snapshot()
                .samples
                .into_iter()
                .find(|sample| sample.owner == owner && sample.requested_time == time)
                .and_then(|sample| sample.encoded_time)
        });
        let ready = readiness.ready
            && encoded_time == expected_time
            && world
                .get::<EffectRuntimeStatus>(owner)
                .is_some_and(|status| status.active == ActiveBackend::Gpu)
            && textures
                .iter()
                .all(|handle| assets.is_loaded_with_dependencies(handle.id()));
        ready_frames = if ready { ready_frames + 1 } else { 0 };
        if ready_frames < settle_frames && Instant::now() >= deadline {
            if let Some(time) = expected_time {
                return Err(format!(
                    "offscreen renderer did not settle at {time:.6}s (encoded: {encoded_time:?}): {}",
                    readiness.detail
                ));
            }
            return Err(format!(
                "offscreen renderer did not settle: {}",
                readiness.detail
            ));
        }
    }
    Ok(())
}

/// Encode the linear-light contribution as straight RGBA for the playback renderer.
fn straight_pixel(pixel: Rgba<u8>, blend: BakeBlend, layer: BakeLayer) -> Rgba<u8> {
    let [red, green, blue, alpha] = pixel.0;
    if !blend.has_content(&pixel) {
        return Rgba([0; 4]);
    }
    if alpha == 255 && blend != BakeBlend::Mixed {
        return pixel;
    }
    let color = Color::srgba_u8(red, green, blue, alpha).to_linear();
    let mut rgb = Vec3::new(color.red, color.green, color.blue);
    let mut alpha = alpha;
    if blend == BakeBlend::Mixed {
        // The capture stores C and A in C + (1 - A) * background. Fit as much
        // of C as possible into straight alpha and add the remaining light on top.
        let covered = rgb.min(Vec3::splat(color.alpha));
        rgb = match layer {
            BakeLayer::Alpha => covered,
            BakeLayer::Additive => rgb - covered,
        };
        if layer == BakeLayer::Additive {
            alpha = 0;
        }
    }
    if layer == BakeLayer::Additive {
        // An emissive source can add more RGB than its alpha can represent. Raise
        // alpha only as far as necessary to encode the captured light in a PNG.
        alpha = alpha.max((rgb.max_element() * 255.0).ceil() as u8);
    }
    if alpha == 0 {
        return Rgba([0; 4]);
    }
    let straight = rgb / (f32::from(alpha) / 255.0);
    let straight = Color::linear_rgb(straight.x, straight.y, straight.z).to_srgba();
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgba([
        byte(straight.red),
        byte(straight.green),
        byte(straight.blue),
        alpha,
    ])
}

#[derive(Debug, Clone, Copy)]
struct AtlasLayout {
    count: u32,
    columns: u32,
    source_origin: UVec2,
    frame_edge: u32,
    padding: u32,
    tile_edge: u32,
    size: UVec2,
}

impl AtlasLayout {
    fn for_capture(capture: &CapturedFrames, options: BakeOptions) -> Result<Self, String> {
        let frames = &capture.frames;
        let count = frames.len() as u32;
        let size = options.view.size;
        let empty = "all captured frames are transparent within the selected camera";
        let (source_origin, frame_edge) = if options.crop {
            let mut min = UVec2::splat(size);
            let mut max = UVec2::ZERO;
            for (x, y, pixel) in frames.iter().flat_map(RgbaImage::enumerate_pixels) {
                if capture.blend.has_content(pixel) {
                    let position = UVec2::new(x, y);
                    min = min.min(position);
                    max = max.max(position + UVec2::ONE);
                }
            }
            if max == UVec2::ZERO {
                return Err(empty.into());
            }
            let extent = max - min;
            let edge = extent.max_element();
            // Every frame uses one square crop so the flipbook retains its original scale.
            let margin = (UVec2::splat(edge) - extent) / 2;
            let origin = min.saturating_sub(margin).min(UVec2::splat(size - edge));
            (origin, edge)
        } else {
            if !frames
                .iter()
                .any(|frame| frame.pixels().any(|pixel| capture.blend.has_content(pixel)))
            {
                return Err(empty.into());
            }
            (UVec2::ZERO, size)
        };
        Self::new(
            count,
            count.min(ATLAS_COLUMNS),
            source_origin,
            frame_edge,
            options.padding,
            capture.texture_limit,
        )
    }

    fn new(
        count: u32,
        columns: u32,
        source_origin: UVec2,
        frame_edge: u32,
        padding: u32,
        texture_limit: u32,
    ) -> Result<Self, String> {
        let rows = count.div_ceil(columns);
        let tile_edge = padding
            .checked_mul(2)
            .and_then(|gutter| frame_edge.checked_add(gutter))
            .ok_or("bake padding overflows the atlas tile dimensions")?;
        if tile_edge > texture_limit / columns.max(rows) {
            return Err(format!(
                "the atlas for {count} frames with {frame_edge}×{frame_edge} content and {padding} pixels of padding would exceed the adapter's {texture_limit}-pixel texture limit"
            ));
        }
        let size = UVec2::new(columns, rows) * tile_edge;
        size.x
            .checked_mul(size.y)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or("the packed RGBA atlas is too large")?;
        Ok(Self {
            count,
            columns,
            source_origin,
            frame_edge,
            padding,
            tile_edge,
            size,
        })
    }

    fn frame_origin(self, index: u32) -> UVec2 {
        UVec2::new(index % self.columns, index / self.columns) * self.tile_edge
            + UVec2::splat(self.padding)
    }

    fn frame_uv(self, index: u32) -> UvRect {
        let origin = self.frame_origin(index);
        UvRect {
            min: (origin.as_vec2() / self.size.as_vec2()).to_array(),
            max: ((origin + UVec2::splat(self.frame_edge)).as_vec2() / self.size.as_vec2())
                .to_array(),
        }
    }

    fn pack(self, capture: &CapturedFrames, layer: BakeLayer) -> RgbaImage {
        let mut atlas = RgbaImage::new(self.size.x, self.size.y);
        for (index, frame) in capture.frames.iter().enumerate() {
            let origin = self.frame_origin(index as u32);
            for y in 0..self.frame_edge {
                for x in 0..self.frame_edge {
                    let pixel = frame.get_pixel(self.source_origin.x + x, self.source_origin.y + y);
                    atlas.put_pixel(
                        origin.x + x,
                        origin.y + y,
                        straight_pixel(*pixel, capture.blend, layer),
                    );
                }
            }
        }
        atlas
    }
}

fn export(
    source: &CompiledEffect,
    capture: &CapturedFrames,
    output: &Path,
    options: BakeOptions,
) -> Result<(), String> {
    let layout = AtlasLayout::for_capture(capture, options)?;
    let effect = build_playback(source, options, layout, capture.blend);
    write_new_directory(output, |directory| {
        fs::create_dir(directory.join("textures")).map_err(|error| error.to_string())?;
        fs::create_dir(directory.join("effects")).map_err(|error| error.to_string())?;
        for &(layer, path) in capture.blend.layers() {
            layout
                .pack(capture, layer)
                .save(directory.join(path))
                .map_err(|error| error.to_string())?;
        }
        effect
            .save_ron(directory.join(EFFECT_PATH))
            .map_err(|error| error.to_string())
    })
}

fn write_new_directory(
    output: &Path,
    write: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let staging = tempfile::Builder::new()
        .prefix(".aestra-capture-")
        .tempdir_in(parent)
        .map_err(|error| error.to_string())?;
    let directory = staging.path();
    write(directory)?;
    if output.exists() {
        return Err(format!(
            "output directory already exists: {}",
            output.display()
        ));
    }
    fs::rename(directory, output).map_err(|error| error.to_string())
}

fn build_playback(
    source: &CompiledEffect,
    options: BakeOptions,
    layout: AtlasLayout,
    blend: BakeBlend,
) -> EffectAsset {
    let view = options.view;
    let fps = options.cadence.frames_per_second().get() as f32;
    let duration = layout.count as f32 / fps;
    let mut effect = EffectAsset::new(format!("{} · Baked Flipbook", source.name), duration);
    effect.playback_mode = source.playback_mode;
    effect.metadata.insert(
        EffectDimension::METADATA_KEY.into(),
        EffectDimension::TwoD.metadata_value().into(),
    );
    let mut emitter = Emitter::basic_sprite("Baked frames", duration);
    emitter.max_particles = 1;
    let world_per_pixel = view.span / view.size as f32;
    let offset = layout.source_origin.as_vec2()
        + Vec2::splat((layout.frame_edge as f32 - view.size as f32) * 0.5);
    // Preserve the camera center as the playback pivot. Texture rows run downward.
    emitter.transform.translation = [offset.x * world_per_pixel, -offset.y * world_per_pixel, 0.0];
    emitter.modules = vec![
        ModuleInstance::emission(0.0, 1),
        ModuleInstance::shape(EmitterShape::Point),
        ModuleInstance::initialize(
            ScalarRange::new(duration, duration),
            ScalarRange::new(0.0, 0.0),
            [0.0, 1.0, 0.0],
            0.0,
            ScalarRange::new(0.0, 0.0),
        ),
        ModuleInstance::motion([0.0; 3], 0.0, 0.0),
        ModuleInstance::appearance(
            Curve::new(vec![CurveKey::new(
                0.0,
                view.span * (layout.frame_edge as f32 / view.size as f32),
            )]),
            Curve::new(vec![CurveKey::new(0.0, 1.0)]),
            Gradient::new(vec![ColorKey::new(0.0, [1.0; 4])]),
        ),
    ];
    emitter.renderers.clear();
    for &(layer, path) in blend.layers() {
        let (name, blend) = match layer {
            BakeLayer::Alpha => ("Baked alpha", BlendMode::Alpha),
            BakeLayer::Additive => ("Baked additive", BlendMode::Additive),
        };
        let atlas = AssetDefinition::texture(name, path);
        let flipbook = FlipbookDefinition {
            id: AssetId::new(),
            name: name.into(),
            texture: atlas.id,
            frames: (0..layout.count)
                .map(|index| layout.frame_uv(index))
                .collect(),
            frame_rate: fps,
            looping: source.playback_mode.is_looping(),
        };
        let material = MaterialDefinition::sprite(name, blend, 0.0);
        let mut renderer = RendererInstance::flipbook(material.id, flipbook.id);
        if let RendererProperties::Flipbook { time_source, .. } = &mut renderer.properties {
            *time_source = FlipbookTimeSource::EffectTime;
        }
        effect.assets.push(atlas);
        effect.flipbooks.push(flipbook);
        effect.materials.push(material);
        emitter.renderers.push(renderer);
    }
    effect.emitters.push(emitter);
    effect
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bake_view_defaults_follow_pixel_size_and_reject_invalid_framing() {
        let parse = |args: &[&str]| BakeOptions::parse(args.iter().copied().map(OsString::from));
        assert_eq!(parse(&[]), Ok(BakeOptions::default()));
        assert_eq!(
            parse(&["--fps", "24"])
                .unwrap()
                .cadence
                .frames_per_second()
                .get(),
            24
        );
        assert_eq!(parse(&["--size", "96"]).unwrap().view.span, 96.0);
        let args = [
            "--size",
            "32",
            "--span",
            "64",
            "--center",
            "-16,0,0",
            "--eye",
            "-16,24,96",
            "--crop",
            "--padding",
            "2",
        ];
        assert_eq!(
            parse(&args),
            Ok(BakeOptions {
                view: BakeView {
                    size: 32,
                    span: 64.0,
                    center: Vec3::new(-16.0, 0.0, 0.0),
                    eye: Some(Vec3::new(-16.0, 24.0, 96.0)),
                },
                crop: true,
                padding: 2,
                ..BakeOptions::default()
            }),
        );
        for args in [
            &["--size", "0"][..],
            &["--span", "NaN"][..],
            &["--center", "1,2"][..],
            &["--eye", "0,0,0"][..],
            &["--padding", "-1"][..],
            &["--padding"][..],
            &["--fps", "0"][..],
            &["--fps", "61"][..],
            &["--fps", "23.976"][..],
        ] {
            assert!(parse(args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn crop_uses_all_visible_frames_and_keeps_gutters_outside_frame_uvs() {
        let mut capture = CapturedFrames {
            frames: vec![RgbaImage::new(8, 8); 3],
            texture_limit: 64,
            blend: BakeBlend::Alpha,
        };
        let color = Rgba([70, 120, 240, 255]);
        capture.frames[0].put_pixel(7, 1, color);
        capture.frames[2].put_pixel(6, 4, color);
        let options = BakeOptions::parse(
            ["--size", "8", "--crop", "--padding", "1"]
                .map(OsString::from)
                .into_iter(),
        )
        .unwrap();
        let layout = AtlasLayout::for_capture(&capture, options).unwrap();
        assert_eq!(
            (layout.source_origin, layout.frame_edge),
            (UVec2::new(4, 1), 4)
        );
        assert_eq!(layout.size, UVec2::new(18, 6));
        let atlas = layout.pack(&capture, BakeLayer::Alpha);
        assert_eq!(*atlas.get_pixel(4, 1), color);
        assert_eq!(*atlas.get_pixel(15, 4), color);
        assert_eq!(atlas.pixels().filter(|pixel| pixel[3] != 0).count(), 2);
        let uv = layout.frame_uv(2);
        assert_eq!(uv.min, [13.0 / 18.0, 1.0 / 6.0]);
        assert_eq!(uv.max, [17.0 / 18.0, 5.0 / 6.0]);
        assert!(
            AtlasLayout::for_capture(
                &capture,
                BakeOptions {
                    padding: u32::MAX,
                    ..options
                }
            )
            .is_err()
        );
        capture.texture_limit = 17;
        assert!(AtlasLayout::for_capture(&capture, options).is_err());
    }

    #[test]
    fn sampling_excludes_the_loop_endpoint_and_enforces_the_native_limit() {
        let default = PresentationCadence::FPS_12;
        assert_eq!(frame_count(0.125, default), Ok(2));
        for frames in 1..=aestra_gpu::MAX_FLIPBOOK_FRAMES as u32 {
            assert_eq!(frame_count(frames as f32 / 12.0, default), Ok(frames));
        }
        assert!(frame_count(6.0, default).is_err());
        let cadence = PresentationCadence::new(NonZeroU32::new(24).unwrap());
        // The second frame falls at tick three, even though 1/24 s is earlier.
        assert_eq!(frame_count(0.049, cadence), Ok(1));
        assert_eq!(frame_count(0.05, cadence), Ok(1));
        assert_eq!(frame_count(0.05_f32.next_up(), cadence), Ok(2));
    }

    #[test]
    #[ignore = "requires native GPU and offscreen readback"]
    fn native_flipbook_at_simulation_rate_displays_its_last_frame() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("effects")).unwrap();
        fs::create_dir(directory.path().join("textures")).unwrap();
        let options = BakeOptions::parse(
            ["--size", "8", "--fps", "60"]
                .map(OsString::from)
                .into_iter(),
        )
        .unwrap();
        let seed = EffectCompiler::default()
            .compile(&EffectAsset::new("Timing", 2.0))
            .unwrap();
        let layout = AtlasLayout::new(64, 8, UVec2::ZERO, options.view.size, 0, 1024).unwrap();
        let effect = build_playback(&seed, options, layout, BakeBlend::Alpha);
        let input = directory.path().join(EFFECT_PATH);
        effect.save_ron(&input).unwrap();
        let earlier = Rgba([5, 25, 70, 255]);
        let last = Rgba([180, 80, 15, 255]);
        let mut image = RgbaImage::from_pixel(layout.size.x, layout.size.y, earlier);
        let origin = layout.frame_origin(63);
        for x in origin.x..layout.size.x {
            for y in origin.y..layout.size.y {
                image.put_pixel(x, y, last);
            }
        }
        image.save(directory.path().join(ATLAS_PATH)).unwrap();
        let prepared = prepare(&input).unwrap();
        let count = frame_count(prepared.viewer.compiled.duration, options.cadence).unwrap();
        assert_eq!(count, 64);
        let captured = capture(&prepared, count, options, Color::NONE).unwrap();
        assert_eq!(*captured.frames[62].get_pixel(3, 3), earlier);
        assert_eq!(*captured.frames[63].get_pixel(3, 3), last);
    }

    #[test]
    #[ignore = "requires native GPU and offscreen readback"]
    fn native_bake_is_repeatable_and_reimports_an_asymmetric_multirow_atlas() {
        let temp = tempfile::tempdir().unwrap();
        let seed_effect = Arc::new(
            EffectCompiler::default()
                .compile(&EffectAsset::new("Frames", 1.0))
                .unwrap(),
        );
        let source_dir = temp.path().join("source");
        fs::create_dir_all(source_dir.join("effects")).unwrap();
        fs::create_dir_all(source_dir.join("textures")).unwrap();
        let xy = BakeOptions::default();
        let grid = AtlasLayout::new(9, 2, UVec2::ZERO, EDGE, 0, 1024).unwrap();
        let mut source_asset = build_playback(&seed_effect, xy, grid, BakeBlend::Alpha);
        source_asset.emitters[0].transform.translation[2] = 16.0;
        source_asset.save_ron(source_dir.join(EFFECT_PATH)).unwrap();
        let mut atlas = RgbaImage::new(2 * EDGE, 5 * EDGE);
        for frame in 0..9 {
            let x = frame % 2 * EDGE;
            let y = frame / 2 * EDGE;
            for row in 4..12 {
                for column in 7..14 {
                    atlas.put_pixel(
                        x + column,
                        y + row,
                        Rgba([20 + frame as u8 * 24, 120, 245, 128]),
                    );
                }
            }
        }
        atlas.save(source_dir.join(ATLAS_PATH)).unwrap();
        let prepared = prepare(&source_dir.join(EFFECT_PATH)).unwrap();
        let source = capture(&prepared, 9, xy, Color::NONE).unwrap();
        assert_eq!(
            source.frames,
            capture(&prepared, 9, xy, Color::NONE).unwrap().frames
        );
        let mut previous_red = 0;
        for frame in &source.frames {
            let pixel = frame.get_pixel(10, 8);
            assert!(
                pixel[0] > previous_red && pixel[3].abs_diff(128) <= 1,
                "unexpected colored pixel {pixel:?}"
            );
            assert_eq!(frame.get_pixel(10, EDGE - 9).0, [0; 4]);
            previous_red = pixel[0];
        }
        let output = temp.path().join("baked");
        export(&prepared.viewer.compiled, &source, &output, xy).unwrap();
        let imported = prepare(&output.join(EFFECT_PATH)).unwrap();
        assert_eq!(imported.viewer.compiled.flipbooks[0].frames.len(), 9);
        assert_eq!(
            imported.viewer.compiled.flipbooks[0].frames[8].min,
            [0.0, 0.5]
        );
        let playback = capture(&imported, 9, xy, Color::NONE).unwrap();
        for (expected, actual) in source.frames.iter().zip(&playback.frames) {
            assert!(
                expected
                    .as_raw()
                    .iter()
                    .zip(actual.as_raw())
                    .all(|(a, b)| a.abs_diff(*b) <= 2)
            );
        }

        let framed = BakeOptions {
            view: BakeView {
                size: 32,
                span: 48.0,
                center: Vec3::new(-16.0, 0.0, 0.0),
                eye: Some(Vec3::new(-16.0, 24.0, 96.0)),
            },
            crop: true,
            padding: 2,
            ..xy
        };
        let frames = capture(&prepared, 9, framed, Color::NONE).unwrap();
        assert_eq!(frames.frames[0].dimensions(), (32, 32));
        // Only the tilted 3D view projects the emitter's Z offset down into row 4.
        assert!(frames.frames[0].get_pixel(12, 4)[3].abs_diff(128) <= 1);
        assert_eq!(frames.frames[0].get_pixel(5, 4).0, [0; 4]);
        let output = temp.path().join("framed");
        export(&prepared.viewer.compiled, &frames, &output, framed).unwrap();
        let atlas = image::open(output.join(ATLAS_PATH)).unwrap().into_rgba8();
        assert!(atlas.width() < 256 && atlas.height() < 64);
        let layout = AtlasLayout::for_capture(&frames, framed).unwrap();
        assert_eq!(atlas.get_pixel(0, 0).0, [0; 4]);
        assert_eq!(atlas.dimensions(), (layout.size.x, layout.size.y));
        let saved = EffectAsset::load_ron(output.join(EFFECT_PATH)).unwrap();
        assert_eq!(saved.dimension(), EffectDimension::TwoD);
        assert_ne!(saved.emitters[0].transform.translation, [0.0; 3]);
        let imported = prepare(&output.join(EFFECT_PATH)).unwrap();
        let runtime_view = BakeOptions {
            view: BakeView {
                size: framed.view.size,
                span: framed.view.span,
                ..xy.view
            },
            ..xy
        };
        let playback = capture(&imported, 9, runtime_view, Color::NONE).unwrap();
        assert!(
            frames
                .frames
                .iter()
                .zip(&playback.frames)
                .all(|(expected, actual)| {
                    expected
                        .as_raw()
                        .iter()
                        .zip(actual.as_raw())
                        .all(|(a, b)| a.abs_diff(*b) <= 2)
                }),
            "a cropped 3D capture must replay at the same apparent size and position in XY",
        );
    }

    #[test]
    #[ignore = "requires native GPU and offscreen readback"]
    fn native_stateful_bake_round_trips_distance_spawned_stamps() {
        let input = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../sample-project/effects/pixel_stamp_trail.aestra.ron");
        let prepared = prepare(&input).unwrap();
        let effect = &prepared.viewer.compiled;
        assert_ne!(effect.seek_mode, SimulationSeekMode::StatelessDirect);
        let options = BakeOptions {
            view: BakeView {
                size: 192,
                span: 192.0,
                ..Default::default()
            },
            cadence: PresentationCadence::new(NonZeroU32::new(24).unwrap()),
            crop: true,
            padding: 2,
        };
        let count = frame_count(effect.duration, options.cadence).unwrap();
        assert_eq!(count, 58);
        let source = capture(&prepared, count, options, Color::NONE).unwrap();
        let stamp_columns = |frame: &RgbaImage| {
            (0..frame.width())
                .map(|x| {
                    (0..frame.height()).any(|y| {
                        let pixel = frame.get_pixel(x, y);
                        pixel[3] != 0
                            && pixel.0[..3]
                                .iter()
                                .zip([248, 70, 146])
                                .all(|(actual, expected)| actual.abs_diff(expected) <= 3)
                    })
                })
                .collect::<Vec<_>>()
        };
        assert!(!stamp_columns(&source.frames[0]).contains(&true));
        let columns = stamp_columns(&source.frames[22]);
        let stamp_runs = std::iter::once(false)
            .chain(columns.iter().copied())
            .zip(columns.iter().copied())
            .filter(|&(before, after)| !before && after)
            .count();
        assert!(
            stamp_runs >= 8,
            "expected separate stamps, got {stamp_runs}"
        );
        assert_eq!(
            source.frames,
            capture(&prepared, count, options, Color::NONE)
                .unwrap()
                .frames
        );

        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("baked");
        export(effect, &source, &output, options).unwrap();
        let imported = prepare(&output.join(EFFECT_PATH)).unwrap();
        assert_eq!(imported.viewer.compiled.playback_mode, effect.playback_mode);
        assert_eq!(imported.viewer.compiled.flipbooks[0].frame_rate, 24.0);
        let playback = capture(&imported, count, options, Color::NONE).unwrap();
        for (index, (expected, actual)) in source.frames.iter().zip(&playback.frames).enumerate() {
            assert!(
                expected
                    .as_raw()
                    .iter()
                    .zip(actual.as_raw())
                    .all(|(a, b)| a.abs_diff(*b) <= 2),
                "baked playback differed at frame {index}"
            );
        }
    }

    #[test]
    fn additive_encoding_retains_emissive_rgb_above_the_recorded_alpha() {
        let mut capture = CapturedFrames {
            frames: vec![RgbaImage::new(8, 8)],
            texture_limit: 64,
            blend: BakeBlend::Additive,
        };
        // A low-opacity emissive pixel can still add light when alpha rounds to zero.
        let pixel = Rgba([175, 40, 211, 0]);
        capture.frames[0].put_pixel(6, 7, pixel);
        let options =
            BakeOptions::parse(["--size", "8", "--crop"].map(OsString::from).into_iter()).unwrap();
        let layout = AtlasLayout::for_capture(&capture, options).unwrap();
        assert_eq!(
            (layout.source_origin, layout.frame_edge),
            (UVec2::new(6, 7), 1)
        );
        let atlas = layout.pack(&capture, BakeLayer::Additive);
        let [r, g, b, a] = atlas.get_pixel(0, 0).0;
        let actual = Color::srgba_u8(r, g, b, a).to_linear();
        let original = Color::srgba_u8(pixel[0], pixel[1], pixel[2], pixel[3]).to_linear();
        for (stored, captured) in [actual.red, actual.green, actual.blue].into_iter().zip([
            original.red,
            original.green,
            original.blue,
        ]) {
            assert!((stored * actual.alpha - captured).abs() < 0.003);
        }
        capture.blend = BakeBlend::Alpha;
        assert!(AtlasLayout::for_capture(&capture, options).is_err());
    }

    #[test]
    #[ignore = "requires native GPU and offscreen readback"]
    fn native_additive_bake_replays_accumulated_light_over_a_different_background() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        fs::create_dir_all(source.join("effects")).unwrap();
        fs::create_dir_all(source.join("textures")).unwrap();
        let empty = EffectCompiler::default()
            .compile(&EffectAsset::new("Glows", 0.25))
            .unwrap();
        let options = BakeOptions {
            view: BakeView {
                size: 32,
                span: 32.0,
                ..Default::default()
            },
            crop: true,
            padding: 2,
            ..BakeOptions::default()
        };
        let grid = AtlasLayout::new(3, 3, UVec2::ZERO, options.view.size, 0, 1024).unwrap();
        let mut effect = build_playback(&empty, options, grid, BakeBlend::Additive);
        let emitter = &mut effect.emitters[0];
        emitter.max_particles = 2;
        emitter.modules[0] = ModuleInstance::emission(0.0, 2);
        emitter.transform.translation = [5.0, -3.0, 0.0];
        let mut atlas = RgbaImage::new(grid.size.x, grid.size.y);
        for index in 0..3 {
            for x in 4..12 {
                for y in 8..13 {
                    atlas.put_pixel(
                        index * 32 + x,
                        y,
                        Rgba([72 + index as u8 * 30, 110, 168, 77]),
                    );
                }
            }
        }
        atlas.save(source.join(ATLAS_PATH)).unwrap();
        effect.save_ron(source.join(EFFECT_PATH)).unwrap();
        let original = prepare(&source.join(EFFECT_PATH)).unwrap();
        let captured = capture(&original, 3, options, Color::NONE).unwrap();
        assert!(captured.frames[0].get_pixel(11, 13)[3].abs_diff(154) <= 1);
        assert_eq!(captured.frames[0].get_pixel(0, 0).0, [0; 4]);

        let output = directory.path().join("baked");
        export(&original.viewer.compiled, &captured, &output, options).unwrap();
        let imported = prepare(&output.join(EFFECT_PATH)).unwrap();
        assert_eq!(imported.blend, BakeBlend::Additive);
        let colored = Color::srgb_u8(18, 88, 52);
        for background in [Color::NONE, colored] {
            let expected = capture(&original, 3, options, background).unwrap();
            let actual = capture(&imported, 3, options, background).unwrap();
            assert!(
                expected
                    .frames
                    .iter()
                    .zip(&actual.frames)
                    .all(|(source, baked)| {
                        source
                            .as_raw()
                            .iter()
                            .zip(baked.as_raw())
                            .all(|(a, b)| a.abs_diff(*b) <= 2)
                    }),
                "additive bake changed playback over {background:?}"
            );
        }
    }

    #[test]
    #[ignore = "requires native GPU and offscreen readback"]
    fn native_mixed_bake_preserves_occluded_glow_over_new_backgrounds() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        fs::create_dir_all(source.join("effects")).unwrap();
        fs::create_dir_all(source.join("textures")).unwrap();
        let empty = EffectCompiler::default()
            .compile(&EffectAsset::new("Glow behind veil", 0.25))
            .unwrap();
        let options = BakeOptions {
            view: BakeView {
                size: 32,
                span: 32.0,
                ..Default::default()
            },
            crop: true,
            padding: 2,
            ..BakeOptions::default()
        };
        let grid = AtlasLayout::new(3, 3, UVec2::ZERO, options.view.size, 0, 1024).unwrap();
        let mut effect = build_playback(&empty, options, grid, BakeBlend::Mixed);
        effect.emitters[0].renderers.swap(0, 1);
        let mut alpha = RgbaImage::new(grid.size.x, grid.size.y);
        let mut additive = RgbaImage::new(grid.size.x, grid.size.y);
        for index in 0..3 {
            let start = index * grid.frame_edge + 2 * index;
            for y in 7..13 {
                for x in 7..14 {
                    alpha.put_pixel(start + x, y, Rgba([30, 80, 172, 64]));
                }
                for x in 4..11 {
                    additive.put_pixel(start + x, y, Rgba([235, 125, 35, 180]));
                }
            }
        }
        alpha.save(source.join(ATLAS_PATH)).unwrap();
        additive.save(source.join(ADDITIVE_ATLAS_PATH)).unwrap();
        effect.save_ron(source.join(EFFECT_PATH)).unwrap();
        let original = prepare(&source.join(EFFECT_PATH)).unwrap();
        assert_eq!(original.blend, BakeBlend::Mixed);
        let captured = capture(&original, 3, options, Color::NONE).unwrap();
        let glow = captured.frames[0].get_pixel(5, 9);
        let overlap = captured.frames[0].get_pixel(8, 9);
        assert_eq!(glow[3], 0);
        assert_eq!(overlap[3], 64);
        assert!(glow[0] > overlap[0] + 15 && overlap[0] > 150);

        let output = directory.path().join("baked");
        let tilted = BakeOptions {
            view: BakeView {
                eye: Some(Vec3::new(0.0, 16.0, 96.0)),
                ..options.view
            },
            ..options
        };
        run([
            source.join(EFFECT_PATH).into_os_string(),
            output.clone().into_os_string(),
        ]
        .into_iter()
        .chain(
            [
                "--size",
                "32",
                "--crop",
                "--padding",
                "2",
                "--eye",
                "0,16,96",
            ]
            .map(OsString::from),
        ))
        .unwrap();
        let imported = prepare(&output.join(EFFECT_PATH)).unwrap();
        assert_eq!(imported.blend, BakeBlend::Mixed);
        let flipbooks = &imported.viewer.compiled.flipbooks;
        assert_eq!(flipbooks[0].frames, flipbooks[1].frames);
        assert!(output.join(ADDITIVE_ATLAS_PATH).is_file());
        for background in [
            Color::NONE,
            Color::srgb_u8(14, 98, 55),
            Color::srgb_u8(82, 32, 98),
        ] {
            let expected = capture(&original, 3, tilted, background).unwrap();
            let actual = capture(&imported, 3, options, background).unwrap();
            for (index, (source, baked)) in expected.frames.iter().zip(&actual.frames).enumerate() {
                assert!(
                    source
                        .as_raw()
                        .iter()
                        .zip(baked.as_raw())
                        .all(|(a, b)| a.abs_diff(*b) <= 3),
                    "mixed bake changed playback at frame {index} over {background:?}"
                );
            }
        }
    }
}
