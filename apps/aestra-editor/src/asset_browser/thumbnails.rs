//! Read-only, bounded previews of the current project page. No AssetServer full-size loads.
use super::{
    panel,
    state::{AssetBrowserState, Kind, SourceScope},
};
use crate::*;
use aestra_project::{ProjectContentVersion, ProjectSourceId};
use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    tasks::{IoTaskPool, Task, futures_lite::future},
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
    app.init_resource::<ThumbnailCache>().add_systems(
        Update,
        update
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
    task: Task<Result<Vec<u8>, String>>,
}
#[derive(Resource, Default)]
struct ThumbnailCache {
    epoch: Option<Epoch>,
    entries: BTreeMap<ProjectSourceId, Entry>,
    jobs: Vec<Job>,
    tick: u64,
}
impl ThumbnailCache {
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

#[derive(Component)]
pub(super) struct ThumbnailBadge;

#[derive(Component)]
struct Thumbnail {
    source: ProjectSourceId,
    fallback: Entity,
    image: Entity,
    badge: Entity,
    rendered: Option<Preview>,
}

pub(super) fn spawn(
    parent: &mut ChildSpawnerCommands,
    assets: &AssetServer,
    source: ProjectSourceId,
) -> Entity {
    let mut host = parent.spawn((
        Node::default(),
        Pickable {
            should_block_lower: false,
            is_hoverable: true,
        },
    ));
    let entity = host.id();
    let mut thumbnail = Thumbnail {
        source,
        fallback: Entity::PLACEHOLDER,
        image: Entity::PLACEHOLDER,
        badge: Entity::PLACEHOLDER,
        rendered: None,
    };
    host.with_children(|root| {
        thumbnail.fallback = panel::icon(root, assets, Kind::Texture.icon(), 22.0);
        root.commands().entity(thumbnail.fallback).insert((
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            bevy_resvg::prelude::SvgColor(panel::kind_color(Kind::Texture)),
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
) {
    let Some(mut images) = images else {
        return;
    };
    let epoch = (catalog.root().to_owned(), catalog.content_revision());
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
    for job in &cache.jobs {
        if !wanted.contains(&job.source) {
            job.cancelled.store(true, Ordering::Relaxed);
        }
    }
    for index in (0..cache.jobs.len()).rev() {
        if let Some(result) = future::block_on(future::poll_once(&mut cache.jobs[index].task)) {
            let job = cache.jobs.swap_remove(index);
            if !job.cancelled.load(Ordering::Relaxed) && wanted.contains(&job.source) {
                cache.accept(job.source, &job.epoch, result, &mut images);
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
        if cache.jobs.len() >= WORKERS || !cache.room(&wanted, &mut images) {
            continue;
        }
        let Some(entry) = catalog
            .content()
            .source(*source)
            .filter(|entry| Kind::of(entry) == Kind::Texture)
        else {
            continue;
        };
        let root = catalog.root().to_owned();
        let relative = entry.relative_path.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let task = IoTaskPool::get().spawn(async move { decode(&root, &relative, &flag) });
        cache.jobs.push(Job {
            source: *source,
            epoch: epoch.clone(),
            cancelled,
            task,
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
                locale.text("browser-thumbnail-ready")
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

fn decode(root: &Path, relative: &Path, cancelled: &AtomicBool) -> Result<Vec<u8>, String> {
    let check = || {
        if cancelled.load(Ordering::Relaxed) {
            Err("Cancelled".to_owned())
        } else {
            Ok(())
        }
    };
    check()?;
    // Validate each component on the worker, including links replaced since discovery.
    // These checks are not an OS-level guarantee against concurrent filesystem substitution.
    aestra_project::ProjectSourceTree::validate_root(root)?;
    let mut path = root.to_owned();
    for part in relative.components() {
        let PathComponent::Normal(part) = part else {
            return Err("Invalid texture path".into());
        };
        path.push(part);
        if linked(&fs::symlink_metadata(&path).map_err(|e| e.to_string())?) {
            return Err("Linked textures are not previewed".into());
        }
    }
    if !path
        .canonicalize()
        .map_err(|e| e.to_string())?
        .starts_with(root.canonicalize().map_err(|e| e.to_string())?)
    {
        return Err("Texture is outside the project".into());
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
    check()?;
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
    check()?;
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
    check()?;
    Ok(rgba)
}

#[cfg(test)]
mod tests;
