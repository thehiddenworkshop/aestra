//! One isolated native-renderer capture at a time. Never touches EditorSession or viewport entities.
use super::*;
mod capture;
mod displacement;
mod framing;
use aestra_bevy_render::{
    ActiveBackend, EffectRuntimeStatus, PresentedEffect, gpu::GpuParticleStatistics,
};
use aestra_compiler::EffectCompiler;
use aestra_core::{AssetKind, EffectAssetRef, material::MaterialExpressionKind};
use aestra_project::{ProjectAssetId, ProjectContent, ResolvedEffectProject};
use aestra_runtime::RendererPlanKind;
use bevy::{
    camera::{RenderTarget, ScalingMode, visibility::RenderLayers},
    ecs::system::SystemParam,
    render::{
        ExtractSchedule, MainWorld, RenderApp,
        render_asset::RenderAssets,
        render_resource::{CachedPipelineState, PipelineCache, TextureUsages},
        texture::GpuImage,
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
};
use capture::CaptureHarness;
use std::time::{Duration, Instant};

const LAYER: usize = 30; // Viewport = 0, gizmos = 15, editor UI = 31.
const LIVE_LAYER: usize = 29; // Hover live-preview render, isolated from the static capture layer.
const SEED: u64 = 0xAE57_0009;
const PARTICLES: u64 = 4096;
const INSTANCES: usize = 16;
const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Resource)]
pub(super) struct Enabled;
#[derive(Resource, Default)]
pub(super) struct Readiness(bool);

pub(super) fn register(app: &mut App) {
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render.add_systems(ExtractSchedule, publish_readiness);
        app.insert_resource(Enabled);
    }
}

fn publish_readiness(
    pipelines: Res<PipelineCache>,
    images: Res<RenderAssets<GpuImage>>,
    meshes: Res<RenderAssets<bevy::render::mesh::RenderMesh>>,
    mut main: ResMut<MainWorld>,
) {
    if main
        .get_resource::<ThumbnailCache>()
        .is_none_or(|cache| cache.gpu.is_none())
    {
        return;
    }
    // Conservative: a compiling viewport pipeline can delay a thumbnail capture.
    let mut count = 0;
    let ready = pipelines.pipelines().all(|pipeline| {
        count += 1;
        matches!(pipeline.state, CachedPipelineState::Ok(_))
    });
    let textures_ready = main
        .resource::<ThumbnailCache>()
        .gpu
        .as_ref()
        .is_some_and(|job| {
            job.textures
                .iter()
                .all(|handle| images.get(handle).is_some())
                && job.meshes.iter().all(|handle| meshes.get(handle).is_some())
        });
    main.insert_resource(Readiness(ready && count > 0 && textures_ready));
}

#[derive(SystemParam)]
pub(super) struct Context<'w, 's> {
    pub(super) meshes: Option<ResMut<'w, Assets<Mesh>>>,
    enabled: Option<Res<'w, Enabled>>,
    readiness: Option<Res<'w, Readiness>>,
    players: Query<
        'w,
        's,
        (
            &'static PresentedEffect,
            Option<&'static EffectRuntimeStatus>,
            Option<&'static GpuParticleStatistics>,
        ),
    >,
}
impl Context<'_, '_> {
    pub fn enabled(&self) -> bool {
        self.enabled.is_some()
    }
}

pub(super) fn saved(
    content: &ProjectContent,
    source: ProjectSourceId,
) -> Result<ResolvedEffectProject, String> {
    if content
        .source(source)
        .and_then(|entry| entry.metadata.as_ref())
        .is_some_and(|m| m.bytes > FILE_LIMIT)
    {
        return Err("Preview limit: 16 MiB per effect source".into());
    }
    let Some(ProjectAssetId::Effect(id)) = content.asset_for_source(source) else {
        return Err("Effect source could not be parsed".into());
    };
    let effect = content
        .cached_effect(EffectAssetRef::new(id))
        .map_err(|e| e.to_string())?;
    content
        .cached_effect_project_with_materials(&effect, BTreeMap::new())
        .map_err(|_| "Saved effect dependencies could not be resolved".into())
}

pub(super) struct Prepared {
    players: Vec<PresentedEffect>,
    center: Vec3,
    radius: f32,
    textures: Vec<(PathBuf, Image)>,
    meshes: Vec<(PathBuf, Mesh)>,
}

fn sample_time(duration: f32) -> Result<f32, String> {
    if !duration.is_finite() || duration <= 0.0 {
        return Err("Effect has no positive finite duration".into());
    }
    Ok((duration * 0.5).min(2.0))
}

pub(super) fn prepare(
    mut saved: ResolvedEffectProject,
    root: &Path,
    cancelled: &AtomicBool,
) -> Result<Prepared, String> {
    check_cancelled(cancelled)?;
    // The project resolver includes the entire function library, including unrelated WESL.
    // Calls are rejected below for this slice, so none of those definitions are required.
    saved.material_functions.clear();
    if saved.dependencies.len() >= INSTANCES || saved.material_programs.len() > 32 {
        return Err("Preview limit: 16 effects and 32 materials".into());
    }
    // Bound the expanded instance tree before the scheduler can allocate it.
    let mut pending = vec![&saved.root];
    let mut visits = 0;
    while let Some(effect) = pending.pop() {
        visits += 1;
        if visits > INSTANCES {
            return Err("Preview limit: 16 expanded effect instances".into());
        }
        for clip in &effect.effect_clips {
            if let Some(child) = saved.effect(clip.source.id) {
                pending.push(child);
            }
        }
    }
    // Bound authoring inputs before compiling/inlining. Custom code is not run in background previews.
    let effects = std::iter::once(&saved.root).chain(saved.dependencies.values());
    for effect in effects {
        if effect.emitters.len() > 32
            || effect.effect_clips.len() > 16
            || effect.assets.len() > 8
            || effect.emitters.iter().any(|e| {
                e.modules.len() > 32
                    || e.renderers.len() > 8
                    || u64::from(e.max_particles) > PARTICLES
            })
        {
            return Err("Effect exceeds the bounded thumbnail complexity limit".into());
        }
    }
    for expressions in saved.material_programs.values().map(|p| &p.expressions) {
        if expressions.len() > 256
            || expressions.iter().any(|e| {
                matches!(
                    e.kind,
                    MaterialExpressionKind::CustomWeslCall { .. }
                        | MaterialExpressionKind::FunctionCall { .. }
                )
            })
        {
            return Err(
                "Preview limit: 256 expressions; material function calls are not yet previewed"
                    .into(),
            );
        }
    }
    let project = EffectCompiler::default()
        .compile_resolved_project(&saved)
        .map_err(|e| e.to_string())?;
    check_cancelled(cancelled)?;
    let time = sample_time(project.root.duration)?;
    let scheduled = project.instances(time, SEED);
    if scheduled.len() > INSTANCES
        || scheduled
            .iter()
            .map(|i| i.effect.max_particles as u64)
            .sum::<u64>()
            > PARTICLES
    {
        return Err("Preview limit: 16 scheduled instances and 4096 particles".into());
    }
    let mut textures = BTreeSet::new();
    let mut players = Vec::new();
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    let mut trail_points = 0u64;
    let mut meshes = BTreeMap::<PathBuf, (Mesh, f32)>::new();
    for instance in scheduled {
        check_cancelled(cancelled)?;
        if !instance.time.is_finite() || instance.time < 0.0 || instance.time > 4.0 {
            return Err("Preview limit: four seconds of local simulation history".into());
        }
        let mut width = 0.0f32;
        let mut geometry_radius = 1.0f32;
        let mut has_renderer = false;
        for emitter in instance.effect.emitters.iter().filter(|e| e.enabled) {
            for renderer in &emitter.renderers {
                has_renderer = true;
                match renderer.kind {
                    RendererPlanKind::Trail {
                        width: w,
                        max_points,
                        max_trails,
                        ..
                    } => {
                        width = width.max(w);
                        trail_points += u64::from(max_points)
                            * u64::from(if max_trails == 0 {
                                emitter.max_particles
                            } else {
                                max_trails
                            });
                    }
                    RendererPlanKind::Ribbon { width: w, .. } => width = width.max(w),
                    RendererPlanKind::Mesh { asset } => {
                        let asset = instance
                            .effect
                            .assets
                            .iter()
                            .find(|a| a.source == asset && a.kind == AssetKind::Mesh)
                            .ok_or("Missing mesh resource")?;
                        let path = root.join(&asset.path);
                        if !meshes.contains_key(&path) {
                            if meshes.len() >= 8 {
                                return Err("Preview limit: eight mesh primitives".into());
                            }
                            meshes.insert(
                                path.clone(),
                                mesh::load_primitive(root, &asset.path, cancelled)?,
                            );
                        }
                        let mut radius = meshes[&path].1;
                        if let Some(material) = instance.effect.material_instance(renderer.material)
                            && let Some(program) =
                                instance.effect.material_program(material.program.id())
                        {
                            radius = displacement::radius(program, material, radius)?;
                        }
                        geometry_radius = geometry_radius.max(
                            radius
                                * Vec3::from_array(emitter.transform.scale)
                                    .abs()
                                    .max_element(),
                        );
                    }
                    _ => {}
                }
                if !matches!(renderer.kind, RendererPlanKind::Mesh { .. })
                    && instance
                        .effect
                        .material_instance(renderer.material)
                        .and_then(|material| {
                            instance.effect.material_program(material.program.id())
                        })
                        .is_some_and(|program| program.outputs.vertex_offset.is_some())
                {
                    return Err("Non-mesh displacement needs shader-aware thumbnail bounds".into());
                }
            }
        }
        if trail_points > 32_768 {
            return Err("Preview limit: 32768 trail history points".into());
        }
        if !has_renderer || instance.effect.max_particles == 0 {
            continue;
        }
        for asset in &instance.effect.assets {
            if asset.kind == AssetKind::Mesh {
                continue;
            }
            textures.insert(root.join(&asset.path));
        }
        let mut player = PresentedEffect::new(instance.effect);
        player.instance.set_seed(instance.seed);
        player
            .instance
            .apply_compiled_parameter_overrides(&instance.parameter_overrides);
        player
            .instance
            .set_inherited_host_transform(instance.inherited);
        // Include the complete bounded prefix: trail history and retired particles can extend
        // far beyond the live particle head at the capture time.
        let frames = (instance.time * 60.0).ceil() as u32;
        let mut samples = Vec::new();
        for frame in 0..=frames {
            check_cancelled(cancelled)?;
            let t = (frame as f32 / 60.0).min(instance.time);
            player.instance.set_playback_time(t);
            player.instance.evaluate(&mut samples);
            let matrix =
                Mat4::from_cols_array(&player.instance.host_transform_context().matrix_at(t));
            let scale = matrix
                .x_axis
                .truncate()
                .length()
                .max(matrix.y_axis.truncate().length())
                .max(matrix.z_axis.truncate().length());
            for sample in &samples {
                let p = matrix.transform_point3(Vec3::from_array(sample.position));
                let extent =
                    Vec3::splat((sample.size.abs() * geometry_radius + width.abs()) * scale);
                if !p.is_finite() || !extent.is_finite() {
                    return Err("Non-finite particle bounds".into());
                }
                min = min.min(p - extent);
                max = max.max(p + extent);
            }
        }
        player.instance.seek(instance.time);
        players.push(player);
    }
    if players.is_empty() || !min.is_finite() || !max.is_finite() {
        return Err("No particles in the sampled effect interval".into());
    }
    if textures.len() > 8 {
        return Err("Preview limit: eight textures".into());
    }
    let mut pixels = 0u64;
    let mut decoded = Vec::new();
    for path in &textures {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "Texture is outside the project")?;
        let bytes = read_source(root, relative, cancelled)?;
        let mut reader = image::ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())?;
        if reader.format().is_none()
            && path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("tga"))
        {
            reader.set_format(image::ImageFormat::Tga);
        }
        if !matches!(
            reader.format(),
            Some(
                image::ImageFormat::Png
                    | image::ImageFormat::Jpeg
                    | image::ImageFormat::WebP
                    | image::ImageFormat::Bmp
                    | image::ImageFormat::Tga
            )
        ) {
            return Err("Effect thumbnails support PNG, JPEG, WebP, BMP and TGA textures".into());
        }
        let format = reader.format().unwrap();
        let (w, h) = reader.into_dimensions().map_err(|e| e.to_string())?;
        pixels += u64::from(w) * u64::from(h);
        if w > 4096 || h > 4096 || pixels > 16 * 1024 * 1024 {
            return Err("Preview texture limit: 4096 per edge and 16 million pixels total".into());
        }
        let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(DECODE_LIMIT);
        reader.limits(limits);
        let mut image = Image::from_dynamic(
            reader.decode().map_err(|e| e.to_string())?,
            true,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.sampler = ImageSampler::linear();
        decoded.push((path.clone(), image));
        check_cancelled(cancelled)?;
    }
    let center = (min + max) * 0.5;
    let radius = (max - min).length().max(0.1) * 0.55;
    if !center.is_finite() || !radius.is_finite() || radius > 1.0e6 {
        return Err("Effect bounds exceed thumbnail framing limits".into());
    }
    check_cancelled(cancelled)?;
    Ok(Prepared {
        players,
        center,
        radius,
        textures: decoded,
        meshes: meshes
            .into_iter()
            .map(|(path, (mesh, _))| (path, mesh))
            .collect(),
    })
}

pub(super) struct GpuJob {
    pub source: ProjectSourceId,
    pub epoch: Epoch,
    players: Vec<Entity>,
    textures: Vec<Handle<Image>>,
    meshes: Vec<Handle<Mesh>>,
    harness: CaptureHarness,
}

impl GpuJob {
    pub fn start(
        prepared: Prepared,
        source: ProjectSourceId,
        epoch: Epoch,
        commands: &mut Commands,
        images: &mut Assets<Image>,
        context: &mut Context,
    ) -> Self {
        let harness = CaptureHarness::new(
            prepared.center,
            prepared.radius,
            LAYER,
            -10,
            commands,
            images,
        );
        let layer = harness.layer();
        let textures: BTreeMap<_, _> = prepared
            .textures
            .into_iter()
            .map(|(path, image)| (path, images.add(image)))
            .collect();
        let meshes: BTreeMap<_, _> = prepared
            .meshes
            .into_iter()
            .map(|(path, mesh)| {
                (
                    path,
                    context
                        .meshes
                        .as_deref_mut()
                        .expect("native renderer provides mesh assets")
                        .add(mesh),
                )
            })
            .collect();
        let players: Vec<_> = prepared
            .players
            .into_iter()
            .map(|player| {
                let overrides = player
                    .effect()
                    .assets
                    .iter()
                    .filter_map(|asset| {
                        textures
                            .get(&epoch.0.join(&asset.path))
                            .map(|handle| (asset.source, handle.clone()))
                    })
                    .collect();
                commands
                    .spawn((
                        {
                            let mesh_overrides = player
                                .effect()
                                .assets
                                .iter()
                                .filter_map(|asset| {
                                    meshes
                                        .get(&epoch.0.join(&asset.path))
                                        .map(|handle| (asset.source, handle.clone()))
                                })
                                .collect();
                            player
                                .with_texture_overrides(overrides)
                                .with_mesh_overrides(mesh_overrides)
                        },
                        RenderLayers::layer(layer),
                    ))
                    .id()
            })
            .collect();
        Self {
            source,
            epoch,
            players,
            textures: textures.into_values().collect(),
            meshes: meshes.into_values().collect(),
            harness,
        }
    }

    /// The camera and every spawned instance, for cleanup verification.
    #[cfg(test)]
    pub(super) fn spawned_entities(&self) -> Vec<Entity> {
        std::iter::once(self.harness.camera())
            .chain(self.players.iter().copied())
            .collect()
    }

    /// The render target image id, for cleanup verification.
    #[cfg(test)]
    pub(super) fn target_id(&self) -> bevy::asset::AssetId<Image> {
        self.harness.target().id()
    }

    pub fn poll(
        &mut self,
        commands: &mut Commands,
        context: &Context,
    ) -> Option<Result<Vec<u8>, String>> {
        // The harness owns the capture loop (reframe, timeout budget, in-flight
        // wait); this producer only decides when the effect has actually settled.
        match self.harness.advance(commands) {
            capture::Poll::Done(result) => return Some(result),
            capture::Poll::TimedOut => return Some(Err(self.timeout_message(context))),
            capture::Poll::Pending {
                evaluate_ready: false,
            } => return None,
            capture::Poll::Pending {
                evaluate_ready: true,
            } => {}
        }
        let mut ready = context.readiness.as_ref().is_some_and(|r| r.0);
        for entity in &self.players {
            let Ok((player, status, stats)) = context.players.get(*entity) else {
                ready = false;
                continue;
            };
            match status.map(|s| s.active) {
                Some(ActiveBackend::CpuReference | ActiveBackend::GpuReadback) => {
                    return Some(Err(
                        "Effect thumbnail requires native GPU presentation".into()
                    ));
                }
                Some(ActiveBackend::Gpu) => {}
                _ => {
                    ready = false;
                }
            }
            ready &= stats
                .and_then(|s| s.observation(&player.instance))
                .is_some_and(|(time, _)| (time - player.simulation_time()).abs() < 0.0001);
        }
        self.harness.settle(ready, commands);
        None
    }

    /// The effect-specific diagnostic for a capture that never settled in time.
    fn timeout_message(&self, context: &Context) -> String {
        let players = self
            .players
            .iter()
            .filter_map(|entity| context.players.get(*entity).ok())
            .map(|(player, status, stats)| {
                format!(
                    "{:?}: {:?}/{}",
                    status.map(|s| s.active),
                    stats
                        .and_then(|s| s.observation(&player.instance))
                        .map(|(t, _)| t),
                    player.simulation_time()
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "Effect preview timed out. Pipelines/textures ready: {}; GPU observations: {players}",
            context.readiness.as_ref().is_some_and(|r| r.0)
        )
    }

    /// The final refined camera framing, so a live hover preview can match the
    /// static thumbnail's tight fit instead of the conservative history bounds.
    pub fn framing(&self) -> (Transform, OrthographicProjection) {
        self.harness.framing()
    }

    pub fn cleanup(
        self,
        commands: &mut Commands,
        images: &mut Assets<Image>,
        meshes: Option<&mut Assets<Mesh>>,
    ) {
        for entity in self.players {
            commands.entity(entity).try_despawn();
        }
        self.harness.cleanup(commands, images);
        for handle in self.textures {
            images.remove(handle.id());
        }
        if let Some(meshes) = meshes {
            for handle in self.meshes {
                meshes.remove(handle.id());
            }
        }
    }
}

/// A continuously-rendered effect preview for the hovered thumbnail. Unlike
/// [`GpuJob`], it is never screenshotted: its render target image is displayed
/// live and its instances are advanced every frame. Isolated on [`LIVE_LAYER`]
/// so it never interferes with the static capture on [`LAYER`].
pub(super) struct LivePreview {
    pub target: Handle<Image>,
    entities: Vec<Entity>,
    players: Vec<Entity>,
    textures: Vec<Handle<Image>>,
    meshes: Vec<Handle<Mesh>>,
    duration: f32,
    settled: u8,
    /// The GPU has presented at least a few settled frames — safe to crossfade in.
    pub ready: bool,
}

impl LivePreview {
    pub fn start(
        prepared: Prepared,
        epoch: &Epoch,
        framing: Option<(Transform, OrthographicProjection)>,
        commands: &mut Commands,
        images: &mut Assets<Image>,
        meshes_assets: &mut Assets<Mesh>,
    ) -> Self {
        let target = images.add(Image::new_target_texture(
            EDGE,
            EDGE,
            TextureFormat::Rgba8UnormSrgb,
            None,
        ));
        // Prefer the static capture's refined framing so the live animation lines
        // up with the thumbnail; fall back to the conservative history bounds.
        let (camera_transform, projection) =
            framing.unwrap_or_else(|| capture::default_framing(prepared.center, prepared.radius));
        let camera = commands
            .spawn((
                Camera3d::default(),
                Camera {
                    order: -9,
                    clear_color: ClearColorConfig::Custom(Color::srgb(0.025, 0.028, 0.035)),
                    ..default()
                },
                RenderTarget::Image(target.clone().into()),
                Projection::Orthographic(projection),
                camera_transform,
                RenderLayers::layer(LIVE_LAYER),
                Msaa::Off,
            ))
            .id();
        let textures: BTreeMap<_, _> = prepared
            .textures
            .into_iter()
            .map(|(path, image)| (path, images.add(image)))
            .collect();
        let meshes: BTreeMap<_, _> = prepared
            .meshes
            .into_iter()
            .map(|(path, mesh)| (path, meshes_assets.add(mesh)))
            .collect();
        let duration = prepared
            .players
            .iter()
            .map(|player| player.effect().duration)
            .fold(0.0f32, f32::max)
            .max(0.1);
        let players: Vec<_> = prepared
            .players
            .into_iter()
            .map(|mut player| {
                let overrides = player
                    .effect()
                    .assets
                    .iter()
                    .filter_map(|asset| {
                        textures
                            .get(&epoch.0.join(&asset.path))
                            .map(|handle| (asset.source, handle.clone()))
                    })
                    .collect();
                let mesh_overrides = player
                    .effect()
                    .assets
                    .iter()
                    .filter_map(|asset| {
                        meshes
                            .get(&epoch.0.join(&asset.path))
                            .map(|handle| (asset.source, handle.clone()))
                    })
                    .collect();
                // Restart the timeline so the preview loops from the beginning.
                player.instance.set_playback_time(0.0);
                commands
                    .spawn((
                        player
                            .with_texture_overrides(overrides)
                            .with_mesh_overrides(mesh_overrides),
                        RenderLayers::layer(LIVE_LAYER),
                    ))
                    .id()
            })
            .collect();
        Self {
            target,
            entities: std::iter::once(camera)
                .chain(players.iter().copied())
                .collect(),
            players,
            textures: textures.into_values().collect(),
            meshes: meshes.into_values().collect(),
            duration,
            settled: 0,
            ready: false,
        }
    }

    /// Advances every instance by `dt`, looping over the effect duration, and
    /// tracks when the GPU presentation has caught up (so the crossfade can start).
    pub fn advance(
        &mut self,
        players: &mut Query<(
            &mut PresentedEffect,
            Option<&EffectRuntimeStatus>,
            Option<&GpuParticleStatistics>,
        )>,
        dt: f32,
    ) {
        let mut all_settled = !self.players.is_empty();
        for entity in &self.players {
            let Ok((mut player, status, stats)) = players.get_mut(*entity) else {
                all_settled = false;
                continue;
            };
            let mut time = player.instance.time() + dt.clamp(0.0, 0.1);
            if time >= self.duration {
                player.instance.restart();
                time = 0.0;
            }
            player.instance.set_playback_time(time);
            let presenting = matches!(status.map(|s| s.active), Some(ActiveBackend::Gpu));
            let observed = stats
                .and_then(|s| s.observation(&player.instance))
                .is_some();
            all_settled &= presenting && observed;
        }
        self.settled = if all_settled {
            self.settled.saturating_add(1)
        } else {
            0
        };
        if self.settled >= 3 {
            self.ready = true;
        }
    }

    pub fn cleanup(
        self,
        commands: &mut Commands,
        images: &mut Assets<Image>,
        meshes: &mut Assets<Mesh>,
    ) {
        for entity in self.entities {
            commands.entity(entity).try_despawn();
        }
        images.remove(self.target.id());
        for handle in self.textures {
            images.remove(handle.id());
        }
        for handle in self.meshes {
            meshes.remove(handle.id());
        }
    }
}

fn receive(event: On<ScreenshotCaptured>, mut cache: ResMut<ThumbnailCache>) {
    let Some(job) = cache
        .gpu
        .as_mut()
        .filter(|job| job.harness.matches_capture(event.event_target()))
    else {
        return;
    };
    job.harness.store_capture(
        event
            .image
            .clone()
            .try_into_dynamic()
            .map_err(|e| e.to_string())
            .and_then(|image| {
                if image.width() != EDGE || image.height() != EDGE {
                    return Err("Unexpected effect capture dimensions".into());
                }
                Ok(image.to_rgba8().into_raw())
            }),
    );
}

#[cfg(test)]
mod tests;
