//! Read-only, bounded previews of the current project page. No AssetServer full-size loads.
mod effect;
mod mesh;
use super::{
    panel,
    state::{AssetBrowserState, Kind, SourceScope},
};
use crate::*;
use aestra_bevy_render::{EffectRuntimeStatus, PresentedEffect, gpu::GpuParticleStatistics};
use aestra_core::material::MaterialProgram;
use aestra_project::{ProjectContentVersion, ProjectSourceId};
use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    tasks::{IoTaskPool, Task, futures_lite::future},
    ui::RelativeCursorPosition,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Cursor, Read},
    path::{Component as PathComponent, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const EDGE: u32 = 128;
const CAPACITY: usize = 128; // 8 MiB RGBA per CPU/GPU copy; greater than one browser page.
const WORKERS: usize = 2;
const FILE_LIMIT: u64 = 16 * 1024 * 1024;
const DECODE_LIMIT: u64 = 64 * 1024 * 1024;
type Epoch = (PathBuf, ProjectContentVersion);

pub(super) fn register(app: &mut App) {
    effect::register(app);
    app.init_resource::<ThumbnailCache>().add_systems(
        Update,
        (update, hover_preview)
            .chain()
            .after(panel::sync_panel)
            .before(AestraFeathersSet::Sync),
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Preview {
    Loading,
    Ready(Handle<Image>),
    Failed(String),
}
struct Entry {
    preview: Preview,
    touched: u64,
}
struct Job {
    source: ProjectSourceId,
    epoch: Epoch,
    cancelled: Arc<AtomicBool>,
    task: Task<Result<Work, String>>,
    effect: bool,
}
enum Work {
    Pixels(Vec<u8>),
    Effect(Box<effect::Prepared>),
}
/// One live hover preview at a time. Starts as an async `prepare` task, then
/// becomes a continuously-rendered `LivePreview` displayed over the static image.
struct Hover {
    source: ProjectSourceId,
    /// The thumbnail host entity currently showing (or fading) the preview.
    entity: Entity,
    epoch: Epoch,
    /// Crossfade level, 0.0 (static image) .. 1.0 (live animation).
    fade: f32,
    /// The pointer has left; fade out and then tear down.
    fading_out: bool,
    stage: HoverStage,
}
enum HoverStage {
    Preparing {
        cancelled: Arc<AtomicBool>,
        task: Task<Result<effect::Prepared, String>>,
    },
    Live(effect::LivePreview),
}

#[derive(Resource, Default)]
struct ThumbnailCache {
    epoch: Option<Epoch>,
    observed: Option<(Epoch, aestra_project::ProjectTreeStamp)>,
    entries: BTreeMap<ProjectSourceId, Entry>,
    jobs: Vec<Job>,
    gpu: Option<effect::GpuJob>,
    /// Refined camera framing captured from each effect's static thumbnail, so the
    /// hover live preview matches its size. Cleared with the rest on epoch change.
    effect_framing: BTreeMap<ProjectSourceId, (Transform, bevy::camera::OrthographicProjection)>,
    hover: Option<Hover>,
    tick: u64,
}
impl ThumbnailCache {
    fn content_epoch(&mut self, catalog: &ProjectEffectCatalog) -> Epoch {
        let publication = (catalog.root().to_owned(), catalog.content_revision());
        if self
            .observed
            .as_ref()
            .is_some_and(|(previous, _)| *previous == publication)
        {
            return self.epoch.clone().unwrap_or(publication);
        }
        let stamp = catalog.content_stamp();
        let unchanged = self
            .observed
            .as_ref()
            .is_some_and(|(_, previous)| same_thumbnail_content(previous, stamp));
        self.observed = Some((publication.clone(), stamp.clone()));
        if unchanged {
            self.epoch.clone().unwrap_or(publication)
        } else {
            publication
        }
    }

    fn reset(&mut self, epoch: Epoch, images: &mut Assets<Image>) {
        if self.epoch.as_ref() == Some(&epoch) {
            return;
        }
        for entry in self.entries.values() {
            if let Preview::Ready(handle) = &entry.preview {
                images.remove(handle.id());
            }
        }
        self.entries.clear();
        self.effect_framing.clear();
        for job in &self.jobs {
            job.cancelled.store(true, Ordering::Relaxed);
        }
        // Keep cancelled jobs in the worker budget until decoding actually stops.
        self.epoch = Some(epoch);
    }

    fn room(&mut self, wanted: &BTreeSet<ProjectSourceId>, images: &mut Assets<Image>) -> bool {
        if self.entries.len() < CAPACITY {
            return true;
        }
        let victim = self
            .entries
            .iter()
            .filter(|(id, _)| {
                !wanted.contains(id) && !self.jobs.iter().any(|job| job.source == **id)
            })
            .min_by_key(|(_, entry)| entry.touched)
            .map(|(id, _)| *id);
        if let Some(id) = victim {
            if let Preview::Ready(handle) = self.entries.remove(&id).unwrap().preview {
                images.remove(handle.id());
            }
            true
        } else {
            false
        }
    }

    fn accept(
        &mut self,
        source: ProjectSourceId,
        epoch: &Epoch,
        result: Result<Vec<u8>, String>,
        images: &mut Assets<Image>,
    ) {
        if self.epoch.as_ref() != Some(epoch) {
            return;
        }
        let Some(entry) = self.entries.get_mut(&source) else {
            return;
        };
        entry.preview = match result {
            Ok(bytes) => {
                let mut image = Image::new(
                    Extent3d {
                        width: EDGE,
                        height: EDGE,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    bytes,
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
                );
                image.sampler = ImageSampler::linear();
                Preview::Ready(images.add(image))
            }
            Err(error) => Preview::Failed(error),
        };
    }
}

fn same_thumbnail_content(
    a: &aestra_project::ProjectTreeStamp,
    b: &aestra_project::ProjectTreeStamp,
) -> bool {
    a.root == b.root
        && a.availability == b.availability
        && a.sources.len() == b.sources.len()
        && a.sources.iter().all(|(path, old)| {
            b.sources.get(path).is_some_and(|new| {
                old.kind == new.kind
                    && old.error == new.error
                    && old.fingerprint == new.fingerprint
                    // Preferences/recovery can touch directory timestamps without changing assets.
                    // Identical file bytes also need no rerender after a metadata-only save.
                    && (matches!(old.kind, aestra_project::ProjectSourceKind::Directory)
                        || matches!(old.fingerprint, Some(Ok(_)))
                        || old.metadata == new.metadata)
            })
        })
}

#[derive(Component)]
pub(super) struct ThumbnailBadge;

#[derive(Component)]
struct Thumbnail {
    source: ProjectSourceId,
    kind: Kind,
    fallback: Entity,
    image: Entity,
    /// Overlay `ImageNode` used to crossfade the live hover animation over the
    /// static preview. Alpha driven by the active [`Hover`].
    live_image: Entity,
    badge: Entity,
    rendered: Option<Preview>,
}

pub(super) fn spawn(
    parent: &mut ChildSpawnerCommands,
    assets: &AssetServer,
    source: ProjectSourceId,
    kind: Kind,
) -> Entity {
    let mut host = parent.spawn((
        Node::default(),
        Pickable {
            should_block_lower: false,
            is_hoverable: true,
        },
        // Tracks whether the pointer is over this thumbnail, for the hover preview.
        RelativeCursorPosition::default(),
    ));
    let entity = host.id();
    let mut thumbnail = Thumbnail {
        source,
        kind,
        fallback: Entity::PLACEHOLDER,
        image: Entity::PLACEHOLDER,
        live_image: Entity::PLACEHOLDER,
        badge: Entity::PLACEHOLDER,
        rendered: None,
    };
    host.with_children(|root| {
        thumbnail.fallback = panel::icon(root, assets, kind.icon(), 22.0);
        root.commands().entity(thumbnail.fallback).insert((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            bevy_resvg::prelude::SvgColor(panel::kind_color(kind)),
        ));
        thumbnail.image = root
            .spawn((
                Node {
                    display: Display::None,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .id();
        // Live hover-preview overlay: sits on top of the static image and fades in.
        thumbnail.live_image = root
            .spawn((
                Node {
                    display: Display::None,
                    position_type: PositionType::Absolute,
                    top: Val::Px(0.0),
                    left: Val::Px(0.0),
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .id();
        thumbnail.badge = root
            .spawn((
                ThumbnailBadge,
                Text::new("…"),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(theme::TEXT),
                BackgroundColor(theme::PANEL),
                Pickable::IGNORE,
                Node {
                    position_type: PositionType::Absolute,
                    right: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    ..default()
                },
            ))
            .id();
    });
    host.insert(thumbnail);
    entity
}

#[allow(clippy::too_many_arguments)]
fn update(
    mut commands: Commands,
    mut cache: ResMut<ThumbnailCache>,
    catalog: Res<ProjectEffectCatalog>,
    state: Res<AssetBrowserState>,
    images: Option<ResMut<Assets<Image>>>,
    mut thumbnails: Query<(Entity, &mut Thumbnail)>,
    nodes: Query<&Node>,
    parents: Query<&ChildOf>,
    locale: Res<Localizer>,
    mut render: effect::Context,
) {
    let Some(mut images) = images else {
        return;
    };
    // Opening/reselecting an effect can publish a new catalog generation without changing
    // any source bytes. Keep completed images AND in-flight captures in that case.
    let epoch = cache.content_epoch(&catalog);
    if cache.gpu.as_ref().is_some_and(|job| job.epoch != epoch) {
        cache
            .gpu
            .take()
            .unwrap()
            .cleanup(&mut commands, &mut images, render.meshes.as_deref_mut());
    }
    cache.reset(epoch.clone(), &mut images);
    cache.tick = cache.tick.wrapping_add(1);
    let wanted = thumbnails
        .iter()
        .filter(|(entity, _)| {
            state.scope == SourceScope::Project
                && std::iter::once(*entity)
                    .chain(parents.iter_ancestors(*entity))
                    .all(|ancestor| {
                        nodes
                            .get(ancestor)
                            .is_ok_and(|node| node.display != Display::None)
                    })
        })
        .map(|(_, thumbnail)| thumbnail.source)
        .collect::<BTreeSet<_>>();
    if let Some(mut job) = cache.gpu.take() {
        if !wanted.contains(&job.source) {
            cache.entries.remove(&job.source);
            job.cleanup(&mut commands, &mut images, render.meshes.as_deref_mut());
        } else if let Some(result) = job.poll(&mut commands, &render) {
            if result.is_ok() {
                cache.effect_framing.insert(job.source, job.framing());
            }
            cache.accept(job.source, &job.epoch, result, &mut images);
            job.cleanup(&mut commands, &mut images, render.meshes.as_deref_mut());
        } else {
            cache.gpu = Some(job);
        }
    }
    for job in &cache.jobs {
        if !wanted.contains(&job.source) {
            job.cancelled.store(true, Ordering::Relaxed);
        }
    }
    for index in (0..cache.jobs.len()).rev() {
        if let Some(result) = future::block_on(future::poll_once(&mut cache.jobs[index].task)) {
            let job = cache.jobs.swap_remove(index);
            if !job.cancelled.load(Ordering::Relaxed) && wanted.contains(&job.source) {
                match result {
                    Ok(Work::Effect(prepared)) => {
                        cache.gpu = Some(effect::GpuJob::start(
                            *prepared,
                            job.source,
                            job.epoch,
                            &mut commands,
                            &mut images,
                            &mut render,
                        ));
                    }
                    result => cache.accept(
                        job.source,
                        &job.epoch,
                        result.and_then(|work| match work {
                            Work::Pixels(bytes) => Ok(bytes),
                            Work::Effect(_) => unreachable!(),
                        }),
                        &mut images,
                    ),
                }
            } else if cache.epoch.as_ref() == Some(&job.epoch) {
                cache.entries.remove(&job.source);
            }
        }
    }
    for source in &wanted {
        let tick = cache.tick;
        if let Some(entry) = cache.entries.get_mut(source) {
            entry.touched = tick;
            continue;
        }
        if cache.jobs.len() + usize::from(cache.gpu.is_some()) >= WORKERS
            || !cache.room(&wanted, &mut images)
        {
            continue;
        }
        let Some(entry) = catalog.content().source(*source).filter(|entry| {
            matches!(
                Kind::of(entry),
                Kind::Texture | Kind::Material | Kind::Mesh | Kind::Effect
            )
        }) else {
            continue;
        };
        let is_effect = Kind::of(entry) == Kind::Effect;
        if is_effect && (cache.gpu.is_some() || cache.jobs.iter().any(|job| job.effect)) {
            continue;
        }
        let root = catalog.root().to_owned();
        let relative = entry.relative_path.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let task = if is_effect {
            let saved = if render.enabled() {
                effect::saved(catalog.content(), *source)
            } else {
                Err("Effect thumbnails require the native GPU renderer".into())
            };
            IoTaskPool::get().spawn(async move {
                effect::prepare(saved?, &root, &flag)
                    .map(|prepared| Work::Effect(Box::new(prepared)))
            })
        } else if Kind::of(entry) == Kind::Material {
            // Resolve the saved snapshot, not working drafts or a second disk read. Ambiguous
            // identities fail instead of previewing another source's material.
            let program = saved_material(catalog.content(), *source);
            IoTaskPool::get().spawn(async move {
                let program = program?;
                crate::material_graph::render_material_asset_preview(&program, EDGE, || {
                    flag.load(Ordering::Relaxed)
                })
                .map(Work::Pixels)
            })
        } else if Kind::of(entry) == Kind::Mesh {
            IoTaskPool::get()
                .spawn(async move { mesh::render(&root, &relative, &flag).map(Work::Pixels) })
        } else {
            IoTaskPool::get()
                .spawn(async move { decode(&root, &relative, &flag).map(Work::Pixels) })
        };
        cache.jobs.push(Job {
            source: *source,
            epoch: epoch.clone(),
            cancelled,
            task,
            effect: is_effect,
        });
        cache.entries.insert(
            *source,
            Entry {
                preview: Preview::Loading,
                touched: tick,
            },
        );
    }
    for (entity, mut thumbnail) in &mut thumbnails {
        let preview = if wanted.contains(&thumbnail.source) {
            cache
                .entries
                .get(&thumbnail.source)
                .map(|entry| entry.preview.clone())
                .unwrap_or(Preview::Loading)
        } else {
            Preview::Loading
        };
        if thumbnail.rendered.as_ref() == Some(&preview) && !locale.is_changed() {
            continue;
        }
        let ready = matches!(preview, Preview::Ready(_));
        commands.entity(thumbnail.fallback).insert(Node {
            display: if ready { Display::None } else { Display::Flex },
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        });
        commands.entity(thumbnail.image).insert(Node {
            display: if ready { Display::Flex } else { Display::None },
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        });
        let message = match &preview {
            Preview::Ready(handle) => {
                commands
                    .entity(thumbnail.image)
                    .insert(ImageNode::new(handle.clone()));
                locale.text(match thumbnail.kind {
                    Kind::Material => "browser-material-thumbnail-ready",
                    Kind::Mesh => "browser-mesh-thumbnail-ready",
                    Kind::Effect => "browser-effect-thumbnail-ready",
                    _ => "browser-thumbnail-ready",
                })
            }
            Preview::Loading => locale.text("browser-thumbnail-loading"),
            Preview::Failed(error) => {
                format!("{}\n{error}", locale.text("browser-thumbnail-error"))
            }
        };
        if !ready {
            commands.entity(thumbnail.image).remove::<ImageNode>();
        }
        commands.entity(thumbnail.badge).insert((
            Text::new(if ready {
                ""
            } else if matches!(preview, Preview::Failed(_)) {
                "!"
            } else {
                "…"
            }),
            if ready {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            },
        ));
        commands
            .entity(entity)
            .insert(EditorTooltip::description(message));
        thumbnail.rendered = Some(preview);
    }
}

const HOVER_FADE_RATE: f32 = 6.0; // Full static<->live crossfade in ~0.17s.

fn overlay_node(display: Display) -> Node {
    Node {
        display,
        position_type: PositionType::Absolute,
        top: Val::Px(0.0),
        left: Val::Px(0.0),
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        ..default()
    }
}

fn reset_overlay(
    commands: &mut Commands,
    thumbnails: &Query<(Entity, &Thumbnail, &RelativeCursorPosition)>,
    entity: Entity,
) {
    if let Ok((_, thumb, _)) = thumbnails.get(entity) {
        commands
            .entity(thumb.live_image)
            .insert(overlay_node(Display::None))
            .remove::<ImageNode>();
    }
}

fn teardown_stage(
    stage: HoverStage,
    commands: &mut Commands,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
) {
    match stage {
        HoverStage::Preparing { cancelled, .. } => cancelled.store(true, Ordering::Relaxed),
        HoverStage::Live(live) => live.cleanup(commands, images, meshes),
    }
}

/// Plays the hovered effect thumbnail live and crossfades it over the static
/// image, reverting on mouse-out. One preview at a time; isolated from the
/// static capture pipeline.
#[allow(clippy::too_many_arguments)]
fn hover_preview(
    mut commands: Commands,
    mut cache: ResMut<ThumbnailCache>,
    catalog: Res<ProjectEffectCatalog>,
    state: Res<AssetBrowserState>,
    enabled: Option<Res<effect::Enabled>>,
    images: Option<ResMut<Assets<Image>>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    time: Res<Time>,
    thumbnails: Query<(Entity, &Thumbnail, &RelativeCursorPosition)>,
    mut players: Query<(
        &mut PresentedEffect,
        Option<&EffectRuntimeStatus>,
        Option<&GpuParticleStatistics>,
    )>,
) {
    let (Some(mut images), Some(mut meshes)) = (images, meshes) else {
        return;
    };
    let dt = time.delta_secs();
    let current_epoch = cache.epoch.clone();

    // The effect thumbnail under the pointer whose static preview is ready.
    let hovered: Option<(Entity, ProjectSourceId)> =
        if enabled.is_some() && state.scope == SourceScope::Project {
            thumbnails.iter().find_map(|(entity, thumb, cursor)| {
                (thumb.kind == Kind::Effect
                    && cursor.cursor_over()
                    && matches!(
                        cache.entries.get(&thumb.source).map(|entry| &entry.preview),
                        Some(Preview::Ready(_))
                    ))
                .then_some((entity, thumb.source))
            })
        } else {
            None
        };

    let mut hover = cache.hover.take();

    // Snap away when the epoch changed or the pointer moved to a different
    // thumbnail (a live→static crossfade only applies when leaving to empty space).
    if let Some(h) = &hover {
        let switch = Some(&h.epoch) != current_epoch.as_ref()
            || matches!(hovered, Some((_, source)) if source != h.source);
        if switch {
            let h = hover.take().unwrap();
            reset_overlay(&mut commands, &thumbnails, h.entity);
            teardown_stage(h.stage, &mut commands, &mut images, &mut meshes);
        }
    }

    // Start a preview when pointing at a ready effect and none is active.
    if hover.is_none()
        && let Some((entity, source)) = hovered
        && let Some(epoch) = current_epoch.clone()
    {
        let saved = effect::saved(catalog.content(), source);
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let root = catalog.root().to_owned();
        let task = IoTaskPool::get()
            .spawn(async move { effect::prepare(saved?, &root, &flag) });
        hover = Some(Hover {
            source,
            entity,
            epoch,
            fade: 0.0,
            fading_out: false,
            stage: HoverStage::Preparing { cancelled, task },
        });
    }

    if let Some(mut h) = hover.take() {
        h.fading_out = hovered.map(|(_, source)| source) != Some(h.source);
        // Advance the stage: finish preparing, or drive the live simulation.
        let mut abandon = false;
        match &mut h.stage {
            HoverStage::Preparing { task, .. } => {
                if let Some(result) = future::block_on(future::poll_once(task)) {
                    match result {
                        Ok(prepared) => {
                            let framing = cache.effect_framing.get(&h.source).cloned();
                            h.stage = HoverStage::Live(effect::LivePreview::start(
                                prepared,
                                &h.epoch,
                                framing,
                                &mut commands,
                                &mut images,
                                &mut meshes,
                            ));
                        }
                        // Unpreviewable effect: give up quietly, keep the static image.
                        Err(_) => abandon = true,
                    }
                }
            }
            HoverStage::Live(live) => live.advance(&mut players, dt),
        }
        if abandon {
            reset_overlay(&mut commands, &thumbnails, h.entity);
            cache.hover = None;
            return;
        }

        // Crossfade toward live once the render has settled, or back to static.
        let live_ready = matches!(&h.stage, HoverStage::Live(live) if live.ready);
        let target = if h.fading_out || !live_ready { 0.0 } else { 1.0 };
        h.fade = if h.fade < target {
            (h.fade + dt * HOVER_FADE_RATE).min(target)
        } else {
            (h.fade - dt * HOVER_FADE_RATE).max(target)
        };

        if let Ok((_, thumb, _)) = thumbnails.get(h.entity) {
            match &h.stage {
                HoverStage::Live(live) if h.fade > 0.001 => {
                    commands.entity(thumb.live_image).insert((
                        ImageNode {
                            color: Color::srgba(1.0, 1.0, 1.0, h.fade),
                            ..ImageNode::new(live.target.clone())
                        },
                        overlay_node(Display::Flex),
                    ));
                }
                _ => {
                    commands
                        .entity(thumb.live_image)
                        .insert(overlay_node(Display::None))
                        .remove::<ImageNode>();
                }
            }
        }

        // Fully faded out after leaving: tear the preview down.
        if h.fading_out && h.fade <= 0.001 {
            reset_overlay(&mut commands, &thumbnails, h.entity);
            teardown_stage(h.stage, &mut commands, &mut images, &mut meshes);
        } else {
            cache.hover = Some(h);
        }
    }
}

fn saved_material(
    content: &aestra_project::ProjectContent,
    source: ProjectSourceId,
) -> Result<MaterialProgram, String> {
    if content
        .source(source)
        .and_then(|entry| entry.metadata.as_ref())
        .is_some_and(|metadata| metadata.bytes > FILE_LIMIT)
    {
        return Err("Preview limit: 16 MiB per source".into());
    }
    let Some(aestra_project::ProjectAssetId::MaterialProgram(id)) =
        content.asset_for_source(source)
    else {
        return Err("Material source could not be parsed".into());
    };
    content
        .cached_material_program(aestra_core::material::MaterialProgramRef::Project(id))
        .map_err(|error| error.to_string())
}

fn linked(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.is_symlink()
    }
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        Err("Cancelled".into())
    } else {
        Ok(())
    }
}

// Shared bounded, root-confined file access for texture and mesh preview workers.
fn read_source(root: &Path, relative: &Path, cancelled: &AtomicBool) -> Result<Vec<u8>, String> {
    check_cancelled(cancelled)?;
    // Validate each component on the worker, including links replaced since discovery.
    // These checks are not an OS-level guarantee against concurrent filesystem substitution.
    aestra_project::ProjectSourceTree::validate_root(root)?;
    let mut path = root.to_owned();
    for part in relative.components() {
        let PathComponent::Normal(part) = part else {
            return Err("Invalid preview source path".into());
        };
        path.push(part);
        if linked(&fs::symlink_metadata(&path).map_err(|e| e.to_string())?) {
            return Err("Linked sources are not previewed".into());
        }
    }
    if !path
        .canonicalize()
        .map_err(|e| e.to_string())?
        .starts_with(root.canonicalize().map_err(|e| e.to_string())?)
    {
        return Err("Preview source is outside the project".into());
    }
    let file = fs::File::open(&path).map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > FILE_LIMIT {
        return Err("Preview limit: 16 MiB per source".into());
    }
    let mut bytes = Vec::new();
    file.take(FILE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > FILE_LIMIT {
        return Err("Preview limit: 16 MiB per source".into());
    }
    check_cancelled(cancelled)?;
    Ok(bytes)
}

fn decode(root: &Path, relative: &Path, cancelled: &AtomicBool) -> Result<Vec<u8>, String> {
    let bytes = read_source(root, relative, cancelled)?;
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    // TGA has no reliable magic; use its extension only for this supported raster format.
    if reader.format().is_none()
        && relative
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("tga"))
    {
        reader.set_format(image::ImageFormat::Tga);
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(DECODE_LIMIT);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|e| e.to_string())?;
    check_cancelled(cancelled)?;
    let small = decoded.thumbnail(EDGE, EDGE).to_rgba8();
    let mut rgba = vec![0; (EDGE * EDGE * 4) as usize];
    let left = (EDGE - small.width()) / 2;
    let top = (EDGE - small.height()) / 2;
    for y in 0..EDGE {
        for x in 0..EDGE {
            let background = if (x / 8 + y / 8) % 2 == 0 {
                40u16
            } else {
                56u16
            };
            let offset = ((y * EDGE + x) * 4) as usize;
            let pixel =
                if x >= left && x < left + small.width() && y >= top && y < top + small.height() {
                    small.get_pixel(x - left, y - top).0
                } else {
                    [0; 4]
                };
            for channel in 0..3 {
                rgba[offset + channel] = ((pixel[channel] as u16 * pixel[3] as u16
                    + background * (255 - pixel[3] as u16))
                    / 255) as u8;
            }
            rgba[offset + 3] = 255;
        }
    }
    check_cancelled(cancelled)?;
    Ok(rgba)
}

#[cfg(test)]
mod tests;
