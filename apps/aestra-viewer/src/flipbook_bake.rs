//! Fixed-rate transparent capture and playback export for self-contained sprite effects.
use super::*;
use aestra_bevy::{
    AssetDefinition, AssetKind, BlendMode, ColorKey, CompiledEffect, Curve, CurveKey, Emitter,
    EmitterShape, FlipbookDefinition, FlipbookTimeSource, Gradient, MaterialDefinition,
    ModuleInstance, RendererInstance, RendererPlanKind, RendererProperties, ScalarRange,
    SimulationSeekMode, pixel_render_target,
};
use bevy::{
    app::PluginsState,
    asset::LoadState,
    camera::{RenderTarget, ScalingMode},
    render::renderer::RenderDevice,
    window::ExitCondition,
};
use std::{ffi::OsString, path::Path};

const EDGE: u32 = 64;
const FPS: u32 = 12;
const ATLAS_COLUMNS: u32 = 8;
const TIMEOUT: Duration = Duration::from_secs(60);
const ATLAS_PATH: &str = "textures/atlas.png";
const EFFECT_PATH: &str = "effects/flipbook.aestra.ron";

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

impl BakeView {
    fn parse(mut arguments: impl Iterator<Item = OsString>) -> Result<Self, String> {
        let mut view = Self::default();
        let mut span: Option<f32> = None;
        while let Some(flag) = arguments.next() {
            let flag = flag.to_string_lossy();
            let value = arguments
                .next()
                .ok_or_else(|| format!("{flag} needs a value"))?;
            let value = value.to_string_lossy();
            match flag.as_ref() {
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
        Ok(view)
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
            "usage: aestra-viewer --bake-flipbook INPUT.aestra.ron NEW_OUTPUT_DIRECTORY [--size PIXELS] [--span UNITS] [--center X,Y,Z] [--eye X,Y,Z]".into(),
        );
    };
    let view = BakeView::parse(arguments)?;
    let input = PathBuf::from(input);
    let output = PathBuf::from(output);
    if output.exists() {
        return Err(format!(
            "output directory already exists: {}",
            output.display()
        ));
    }
    let prepared = prepare(&input)?;
    let effect = &prepared.compiled;
    let count = frame_count(effect.duration)?;
    let frames = capture(&prepared, count, view)?;
    export(effect, &frames, &output, view)?;
    println!(
        "Baked {count} frames at {FPS} FPS to {}",
        output.join(EFFECT_PATH).display()
    );
    Ok(())
}

fn prepare(input: &Path) -> Result<PreparedViewer, String> {
    let input = input.canonicalize().map_err(|error| error.to_string())?;
    let config =
        ViewerConfig::from_iter(["--effect".into(), input.to_string_lossy().into_owned()])?;
    let prepared = prepare_viewer(&config).map_err(|failure| failure.message)?;
    let effect = &prepared.compiled;
    if effect.seek_mode != SimulationSeekMode::StatelessDirect
        || effect.playback_mode.is_continuous()
        || !effect.effect_clips.is_empty()
        || !effect.bindings.is_empty()
        || !effect.host_fields.is_empty()
        || !effect.point_lights.is_empty()
        || !effect.event_inputs.is_empty()
        || effect.host_transform_track.is_some()
    {
        return Err(
            "the first bake supports self-contained analytic effects without host inputs".into(),
        );
    }
    let mut visible_renderers = 0;
    for renderer in effect
        .emitters
        .iter()
        .filter(|emitter| emitter.enabled)
        .flat_map(|emitter| &emitter.renderers)
    {
        visible_renderers += 1;
        if !matches!(
            renderer.kind,
            RendererPlanKind::Sprite | RendererPlanKind::Flipbook { .. }
        ) {
            return Err("the first bake supports sprite and flipbook renderers".into());
        }
        let blend = if let Some(instance) = effect.material_instance(renderer.material) {
            let program = effect
                .material_program(instance.program.id())
                .ok_or("missing material program")?;
            let shader = aestra_bevy::compile_material_program(program)
                .map_err(|error| error.to_string())?;
            if shader.requires_scene_lighting() || shader.requires_scene_depth() {
                return Err("baking requires an unlit material without scene inputs".into());
            }
            instance.render_state.blend
        } else {
            effect
                .material(renderer.material)
                .ok_or("missing sprite material")?
                .blend
        };
        if blend != BlendMode::Alpha {
            return Err("baking currently requires alpha blending".into());
        }
    }
    if visible_renderers == 0 {
        return Err("the effect has no enabled renderers".into());
    }
    Ok(prepared)
}

fn frame_count(duration: f32) -> Result<u32, String> {
    let count = (duration * FPS as f32).ceil() as u32;
    if !(1..=aestra_gpu::MAX_FLIPBOOK_FRAMES as u32).contains(&count) {
        return Err(format!(
            "the effect needs {count} frames at {FPS} FPS. Native flipbooks support up to {}",
            aestra_gpu::MAX_FLIPBOOK_FRAMES
        ));
    }
    Ok(count)
}

#[derive(Resource, Default)]
struct Readback(Option<Image>);

fn capture(
    prepared: &PreparedViewer,
    count: u32,
    view: BakeView,
) -> Result<Vec<RgbaImage>, String> {
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
                file_path: prepared.asset_root.to_string_lossy().into_owned(),
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
    let limit = world
        .get_resource::<RenderDevice>()
        .ok_or("no GPU render device is available")?
        .limits()
        .max_texture_dimension_2d;
    let columns = count.min(ATLAS_COLUMNS);
    let atlas_cells = columns.max(count.div_ceil(columns));
    if view.size > limit / atlas_cells {
        return Err(format!(
            "the atlas for {count} frames at {}×{} would exceed the adapter's {limit}-pixel texture limit",
            view.size, view.size
        ));
    }
    let textures = prepared
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
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
        RenderTarget::Image(target.clone().into()),
        Msaa::Off,
    );
    let scaling_mode = ScalingMode::Fixed {
        width: view.span,
        height: view.span,
    };
    if let Some(eye) = view.eye {
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
        ));
    } else {
        world.spawn((
            Camera2d,
            camera,
            Projection::Orthographic(OrthographicProjection {
                scaling_mode,
                ..OrthographicProjection::default_2d()
            }),
            Transform::from_translation(view.center),
        ));
    }
    let mut player = EffectPlayer::from_project(prepared.project.clone());
    player.playing = false;
    player.set_seed(REGRESSION_SEED);
    let owner = world
        .spawn((player, PresentedEffect::new(prepared.compiled.clone())))
        .id();
    let mut frames = Vec::with_capacity(count as usize);
    for index in 0..count {
        app.world_mut()
            .get_mut::<EffectPlayer>(owner)
            .unwrap()
            .seek_frame(u64::from(index) * u64::from(DEFAULT_PLAYBACK_TICK_RATE / FPS));
        wait_for_frame(&mut app, owner, &textures)?;
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
    Ok(frames)
}

fn pump(app: &mut App) {
    app.update();
    std::thread::sleep(Duration::from_millis(1));
}

fn wait_for_frame(app: &mut App, owner: Entity, textures: &[Handle<Image>]) -> Result<(), String> {
    let deadline = Instant::now() + TIMEOUT;
    let mut ready_frames = 0;
    while ready_frames < 4 {
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
        let ready = readiness.ready
            && world
                .get::<EffectRuntimeStatus>(owner)
                .is_some_and(|status| status.active == ActiveBackend::Gpu)
            && textures
                .iter()
                .all(|handle| assets.is_loaded_with_dependencies(handle.id()));
        ready_frames = if ready { ready_frames + 1 } else { 0 };
        if Instant::now() >= deadline {
            return Err(format!(
                "offscreen renderer did not settle: {}",
                readiness.detail
            ));
        }
    }
    Ok(())
}

/// Hardware blend stores premultiplied RGB in linear light. PNG stores straight alpha.
fn straight_pixel(pixel: Rgba<u8>) -> Rgba<u8> {
    let [red, green, blue, alpha] = pixel.0;
    if alpha == 0 {
        return Rgba([0; 4]);
    }
    if alpha == 255 {
        return pixel;
    }
    let color = Color::srgba_u8(red, green, blue, alpha).to_linear();
    let straight = Color::linear_rgb(
        color.red / color.alpha,
        color.green / color.alpha,
        color.blue / color.alpha,
    )
    .to_srgba();
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgba([
        byte(straight.red),
        byte(straight.green),
        byte(straight.blue),
        alpha,
    ])
}

fn export(
    source: &CompiledEffect,
    frames: &[RgbaImage],
    output: &Path,
    view: BakeView,
) -> Result<(), String> {
    let count = frames.len() as u32;
    let columns = count.min(ATLAS_COLUMNS);
    let rows = count.div_ceil(columns);
    let mut atlas = RgbaImage::new(columns * view.size, rows * view.size);
    for (index, frame) in frames.iter().enumerate() {
        let x = index as u32 % columns * view.size;
        let y = index as u32 / columns * view.size;
        for (column, row, pixel) in frame.enumerate_pixels() {
            atlas.put_pixel(x + column, y + row, straight_pixel(*pixel));
        }
    }
    if !atlas.pixels().any(|pixel| pixel[3] != 0) {
        return Err("all captured frames are transparent within the selected camera".into());
    }
    let effect = build_playback(source, count, columns, rows, view.span);
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let staging = tempfile::Builder::new()
        .prefix(".aestra-bake-")
        .tempdir_in(parent)
        .map_err(|error| error.to_string())?;
    let directory = staging.path();
    fs::create_dir(directory.join("textures")).map_err(|error| error.to_string())?;
    fs::create_dir(directory.join("effects")).map_err(|error| error.to_string())?;
    atlas
        .save(directory.join(ATLAS_PATH))
        .map_err(|error| error.to_string())?;
    effect
        .save_ron(directory.join(EFFECT_PATH))
        .map_err(|error| error.to_string())?;
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
    count: u32,
    columns: u32,
    rows: u32,
    span: f32,
) -> EffectAsset {
    let duration = count as f32 / FPS as f32;
    let mut effect = EffectAsset::new(format!("{} · Baked Flipbook", source.name), duration);
    effect.playback_mode = source.playback_mode;
    let atlas = AssetDefinition::texture("Baked atlas", ATLAS_PATH);
    let mut flipbook =
        FlipbookDefinition::grid("Baked frames", atlas.id, columns, rows, FPS as f32);
    flipbook.frames.truncate(count as usize);
    flipbook.looping = source.playback_mode.is_looping();
    let material = MaterialDefinition::sprite("Baked alpha", BlendMode::Alpha, 0.0);
    let mut emitter = Emitter::basic_sprite("Baked frames", duration);
    emitter.max_particles = 1;
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
            Curve::new(vec![CurveKey::new(0.0, span)]),
            Curve::new(vec![CurveKey::new(0.0, 1.0)]),
            Gradient::new(vec![ColorKey::new(0.0, [1.0; 4])]),
        ),
    ];
    let mut renderer = RendererInstance::flipbook(material.id, flipbook.id);
    if let RendererProperties::Flipbook { time_source, .. } = &mut renderer.properties {
        *time_source = FlipbookTimeSource::EffectTime;
    }
    emitter.renderers = vec![renderer];
    effect.assets.push(atlas);
    effect.flipbooks.push(flipbook);
    effect.materials = vec![material];
    effect.emitters.push(emitter);
    effect
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bake_view_defaults_follow_pixel_size_and_reject_invalid_framing() {
        let parse = |args: &[&str]| BakeView::parse(args.iter().copied().map(OsString::from));
        assert_eq!(parse(&[]), Ok(BakeView::default()));
        assert_eq!(parse(&["--size", "96"]).unwrap().span, 96.0);
        let args = [
            "--size",
            "32",
            "--span",
            "64",
            "--center",
            "-16,0,0",
            "--eye",
            "-16,24,96",
        ];
        assert_eq!(
            parse(&args),
            Ok(BakeView {
                size: 32,
                span: 64.0,
                center: Vec3::new(-16.0, 0.0, 0.0),
                eye: Some(Vec3::new(-16.0, 24.0, 96.0)),
            }),
        );
        for args in [
            &["--size", "0"][..],
            &["--span", "NaN"][..],
            &["--center", "1,2"][..],
            &["--eye", "0,0,0"][..],
        ] {
            assert!(parse(args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn sampling_excludes_the_loop_endpoint_and_enforces_the_native_limit() {
        assert_eq!(frame_count(0.125), Ok(2));
        for frames in 1..=aestra_gpu::MAX_FLIPBOOK_FRAMES as u32 {
            assert_eq!(frame_count(frames as f32 / FPS as f32), Ok(frames));
        }
        assert!(frame_count(6.0).is_err());
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
        let xy = BakeView::default();
        let mut source_asset = build_playback(&seed_effect, 9, 2, 5, xy.span);
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
        let source = capture(&prepared, 9, xy).unwrap();
        assert_eq!(source, capture(&prepared, 9, xy).unwrap());
        let mut previous_red = 0;
        for frame in &source {
            let pixel = frame.get_pixel(10, 8);
            assert!(
                pixel[0] > previous_red && pixel[3].abs_diff(128) <= 1,
                "unexpected colored pixel {pixel:?}"
            );
            assert_eq!(frame.get_pixel(10, EDGE - 9).0, [0; 4]);
            previous_red = pixel[0];
        }
        let output = temp.path().join("baked");
        export(&prepared.compiled, &source, &output, xy).unwrap();
        let imported = prepare(&output.join(EFFECT_PATH)).unwrap();
        assert_eq!(imported.compiled.flipbooks[0].frames.len(), 9);
        assert_eq!(imported.compiled.flipbooks[0].frames[8].min, [0.0, 0.5]);
        let playback = capture(&imported, 9, xy).unwrap();
        for (expected, actual) in source.iter().zip(&playback) {
            assert!(
                expected
                    .as_raw()
                    .iter()
                    .zip(actual.as_raw())
                    .all(|(a, b)| a.abs_diff(*b) <= 2)
            );
        }

        let framed = BakeView {
            size: 32,
            span: 48.0,
            center: Vec3::new(-16.0, 0.0, 0.0),
            eye: Some(Vec3::new(-16.0, 24.0, 96.0)),
        };
        let frames = capture(&prepared, 9, framed).unwrap();
        assert_eq!(frames[0].dimensions(), (32, 32));
        // Only the tilted 3D view projects the emitter's Z offset down into row 4.
        assert!(frames[0].get_pixel(12, 4)[3].abs_diff(128) <= 1);
        assert_eq!(frames[0].get_pixel(5, 4).0, [0; 4]);
        let output = temp.path().join("framed");
        export(&prepared.compiled, &frames, &output, framed).unwrap();
        assert_eq!(
            image::open(output.join(ATLAS_PATH))
                .unwrap()
                .into_rgba8()
                .dimensions(),
            (256, 64)
        );
        let imported = prepare(&output.join(EFFECT_PATH)).unwrap();
        let runtime_view = BakeView {
            size: framed.size,
            span: framed.span,
            ..xy
        };
        let playback = capture(&imported, 9, runtime_view).unwrap();
        assert!(
            frames.iter().zip(&playback).all(|(expected, actual)| {
                expected
                    .as_raw()
                    .iter()
                    .zip(actual.as_raw())
                    .all(|(a, b)| a.abs_diff(*b) <= 2)
            }),
            "a 3D capture must replay as a 2D flipbook of the same apparent size",
        );
    }
}
