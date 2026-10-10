//! Editor-only SVG boundary. `resvg` knows nothing about Bevy; panels know nothing
//! about its parser or pixmaps. One labeled image per asset is shared by all icons.

use bevy::{
    asset::{AssetLoader, LoadContext, RenderAssetUsages, io::Reader},
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

// Existing icons are displayed at 8–28 UI pixels. Preserve the previous 64px
// oversampling (including at 2x DPI) without rasterizing per entity or per frame.
const RASTER_SIZE: u32 = 64;

#[derive(Asset, TypePath)]
pub(crate) struct SvgFile {
    #[dependency]
    image: Handle<Image>,
}

#[derive(Component, Clone, Default)]
#[require(ImageNode = hidden_image())]
pub(crate) struct UiSvg(pub(crate) Handle<SvgFile>);

#[derive(Component, Clone, Copy)]
pub(crate) struct SvgColor(pub(crate) Color);

fn hidden_image() -> ImageNode {
    ImageNode {
        // Browser drag previews treat a non-default handle as a ready thumbnail.
        // Reserve the node without advertising Bevy's transparent placeholder.
        image: Handle::default(),
        color: Color::NONE,
        ..default()
    }
}

pub(crate) struct SvgPlugin;

impl Plugin for SvgPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<SvgFile>()
            .init_asset_loader::<SvgLoader>()
            // Panel Update systems can swap icons/tints. Apply them before UI
            // layout/render extraction, independently of their Update ordering.
            .add_systems(PostUpdate, sync_icons.before(bevy::ui::UiSystems::Layout));
    }
}

#[derive(Default, TypePath)]
struct SvgLoader;

#[derive(Debug, thiserror::Error)]
enum SvgError {
    #[error("Could not read SVG: {0}")]
    Read(#[from] std::io::Error),
    #[error("Invalid SVG: {0}")]
    Parse(#[from] resvg::usvg::Error),
    #[error("Could not allocate SVG raster")]
    Raster,
}

impl AssetLoader for SvgLoader {
    type Asset = SvgFile;
    type Settings = ();
    type Error = SvgError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        load_context: &mut LoadContext<'_>,
    ) -> Result<SvgFile, SvgError> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let image = rasterize(&bytes)?;
        Ok(SvgFile {
            image: load_context.add_labeled_asset("raster", image),
        })
    }

    fn extensions(&self) -> &[&str] {
        &["svg", "svgz"]
    }
}

fn rasterize(bytes: &[u8]) -> Result<Image, SvgError> {
    // Editor icons are self-contained vectors: no host fonts, file-system image
    // resolver, or network access. usvg also accepts gzip-compressed SVG data.
    let options = resvg::usvg::Options {
        image_href_resolver: resvg::usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..default()
    };
    let tree = resvg::usvg::Tree::from_data(bytes, &options)?;
    let mut pixmap = tiny_skia::Pixmap::new(RASTER_SIZE, RASTER_SIZE).ok_or(SvgError::Raster)?;
    let scale = RASTER_SIZE as f32 / tree.size().width().max(tree.size().height());
    let transform = tiny_skia::Transform::from_scale(scale, scale).post_translate(
        (RASTER_SIZE as f32 - tree.size().width() * scale) * 0.5,
        (RASTER_SIZE as f32 - tree.size().height() * scale) * 0.5,
    );
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    // tiny-skia stores premultiplied RGBA; ImageNode's normal alpha blending
    // expects straight alpha. Demultiply to avoid dark antialiased fringes.
    let mut rgba = Vec::with_capacity((RASTER_SIZE * RASTER_SIZE * 4) as usize);
    for pixel in pixmap.pixels() {
        let pixel = pixel.demultiply();
        rgba.extend_from_slice(&[pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()]);
    }
    let mut image = Image::new(
        Extent3d {
            width: RASTER_SIZE,
            height: RASTER_SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::linear();
    Ok(image)
}

fn sync_icons(
    assets: Res<Assets<SvgFile>>,
    mut icons: Query<(&UiSvg, Option<&SvgColor>, &mut ImageNode)>,
    mut removed: RemovedComponents<UiSvg>,
    mut commands: Commands,
) {
    for (svg, tint, mut node) in &mut icons {
        // Pending/failed/deleted assets stay invisible, including when swapping
        // away from a loaded icon. Newly loaded and hot-reloaded assets retry
        // naturally. No extra strong-handle cache can retain abandoned textures.
        let image = assets.get(&svg.0).map(|asset| &asset.image);
        let color = image.map_or(Color::NONE, |_| tint.map_or(Color::WHITE, |tint| tint.0));
        if node.color != color {
            node.color = color;
        }
        if let Some(image) = image
            && node.image != *image
        {
            node.image = image.clone();
        } else if image.is_none() && node.image != Handle::default() {
            node.image = Handle::default();
        }
    }
    for entity in removed.read() {
        // Replacing UiSvg in the same frame must not remove its new visual.
        if !icons.contains(entity) {
            commands.entity(entity).try_remove::<ImageNode>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), SvgPlugin))
            .init_asset::<Image>();
        app
    }

    fn asset(app: &mut App, bytes: &[u8]) -> Handle<SvgFile> {
        let image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(rasterize(bytes).unwrap());
        app.world_mut()
            .resource_mut::<Assets<SvgFile>>()
            .add(SvgFile { image })
    }

    const WHITE: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="white" fill-opacity="0.5"/></svg>"#;

    #[test]
    fn raster_preserves_straight_alpha_and_srgb() {
        let image = rasterize(WHITE).unwrap();
        assert_eq!(
            image.texture_descriptor.format,
            TextureFormat::Rgba8UnormSrgb
        );
        assert_eq!(&image.data.as_ref().unwrap()[0..4], &[255, 255, 255, 128]);
        assert_eq!(image.width(), 64);
        assert_eq!(image.height(), 64);
        assert!(rasterize(b"not svg").is_err());
    }

    #[test]
    fn rectangular_vectors_keep_aspect_ratio_and_transparent_padding() {
        let image = rasterize(br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="white"/></svg>"#).unwrap();
        let data = image.data.unwrap();
        assert_eq!(data[3], 0);
        assert_eq!(data[(32 * 64 + 32) * 4 + 3], 255);
    }

    #[test]
    fn shared_images_tints_swaps_reload_and_removal() {
        let mut app = app();
        let first = asset(&mut app, WHITE);
        let second = asset(
            &mut app,
            include_bytes!("../../../../../assets/icons/stop.svg"),
        );
        let a = app
            .world_mut()
            .spawn((UiSvg(first.clone()), SvgColor(Color::BLACK)))
            .id();
        let b = app.world_mut().spawn(UiSvg(first.clone())).id();
        app.update();
        assert_eq!(
            app.world().get::<ImageNode>(a).unwrap().image,
            app.world().get::<ImageNode>(b).unwrap().image
        );
        assert_eq!(app.world().get::<ImageNode>(a).unwrap().color, Color::BLACK);
        app.world_mut().entity_mut(a).remove::<SvgColor>();
        app.world_mut().get_mut::<UiSvg>(b).unwrap().0 = second.clone();
        app.update();
        assert_eq!(app.world().get::<ImageNode>(a).unwrap().color, Color::WHITE);
        assert_ne!(
            app.world().get::<ImageNode>(a).unwrap().image,
            app.world().get::<ImageNode>(b).unwrap().image
        );
        let replacement = app
            .world()
            .resource::<Assets<SvgFile>>()
            .get(&second)
            .unwrap()
            .image
            .clone();
        app.world_mut()
            .resource_mut::<Assets<SvgFile>>()
            .get_mut(&first)
            .unwrap()
            .image = replacement.clone();
        app.update();
        assert_eq!(app.world().get::<ImageNode>(a).unwrap().image, replacement);
        app.world_mut()
            .resource_mut::<Assets<SvgFile>>()
            .remove(first.id());
        app.update();
        assert_eq!(app.world().get::<ImageNode>(a).unwrap().color, Color::NONE);
        assert_eq!(
            app.world().get::<ImageNode>(a).unwrap().image,
            Handle::default()
        );
        app.world_mut().entity_mut(b).remove::<UiSvg>();
        app.update();
        assert!(app.world().get::<ImageNode>(b).is_none());
    }

    #[test]
    fn pending_asset_becomes_visible_without_respawning_and_idle_does_not_change_image() {
        let mut app = app();
        #[derive(Resource, Default)]
        struct ChangedCount(usize);
        app.init_resource::<ChangedCount>().add_systems(
            Last,
            |changed: Query<Entity, Changed<ImageNode>>, mut count: ResMut<ChangedCount>| {
                count.0 = changed.iter().count();
            },
        );
        let pending = app
            .world_mut()
            .resource_mut::<Assets<SvgFile>>()
            .reserve_handle();
        let entity = app.world_mut().spawn(UiSvg(pending.clone())).id();
        app.update();
        assert_eq!(
            app.world().get::<ImageNode>(entity).unwrap().color,
            Color::NONE
        );
        let image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(rasterize(WHITE).unwrap());
        app.world_mut()
            .resource_mut::<Assets<SvgFile>>()
            .insert(
                pending.id(),
                SvgFile {
                    image: image.clone(),
                },
            )
            .unwrap();
        app.update();
        assert_eq!(app.world().get::<ImageNode>(entity).unwrap().image, image);
        assert_eq!(
            app.world().get::<ImageNode>(entity).unwrap().color,
            Color::WHITE
        );
        app.update();
        assert_eq!(app.world().resource::<ChangedCount>().0, 0);
        assert_eq!(app.world().resource::<Assets<Image>>().len(), 1);
    }

    #[test]
    fn all_shipped_icons_rasterize_and_have_visible_pixels() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons");
        let mut count = 0;
        for entry in std::fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|ext| ext == "svg") {
                let image = rasterize(&std::fs::read(&path).unwrap())
                    .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
                assert!(
                    image
                        .data
                        .unwrap()
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .any(|pixel| pixel[3] != 0),
                    "empty icon: {}",
                    path.display()
                );
                count += 1;
            }
        }
        assert!(count > 30, "icon corpus was not exercised");
    }

    #[test]
    fn asset_loader_shares_labeled_raster_and_releases_it_after_last_icon() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin {
                file_path: root.to_string_lossy().into_owned(),
                ..default()
            },
            SvgPlugin,
        ))
        .init_asset::<Image>();
        let server = app.world().resource::<AssetServer>();
        let first: Handle<SvgFile> = server.load("icons/stop.svg");
        let second: Handle<SvgFile> = server.load("icons/stop.svg");
        assert_eq!(first, second);
        let a = app.world_mut().spawn(UiSvg(first.clone())).id();
        let b = app.world_mut().spawn(UiSvg(second.clone())).id();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !app
            .world()
            .resource::<AssetServer>()
            .is_loaded_with_dependencies(first.id())
        {
            assert!(
                std::time::Instant::now() < deadline,
                "SVG loader did not finish"
            );
            app.update();
            std::thread::yield_now();
        }
        app.update();
        let a_image = &app.world().get::<ImageNode>(a).unwrap().image;
        assert_eq!(*a_image, app.world().get::<ImageNode>(b).unwrap().image);
        assert_ne!(*a_image, Handle::default());
        assert_eq!(app.world().resource::<Assets<Image>>().len(), 1);
        let image_id = a_image.id();
        let mut events = bevy::ecs::message::MessageCursor::<AssetEvent<Image>>::default();
        events.clear(app.world().resource::<Messages<AssetEvent<Image>>>());
        app.world()
            .resource::<AssetServer>()
            .reload("icons/stop.svg");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            app.update();
            if events
                .read(app.world().resource::<Messages<AssetEvent<Image>>>())
                .any(|event| matches!(event, AssetEvent::Modified { id } if *id == image_id))
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "SVG reload did not replace its labeled raster"
            );
            std::thread::yield_now();
        }
        assert_eq!(
            app.world().get::<ImageNode>(a).unwrap().image.id(),
            image_id
        );
        assert_eq!(app.world().resource::<Assets<Image>>().len(), 1);
        drop(first);
        drop(second);
        app.world_mut().despawn(a);
        app.world_mut().despawn(b);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !app.world().resource::<Assets<SvgFile>>().is_empty()
            || !app.world().resource::<Assets<Image>>().is_empty()
        {
            assert!(
                std::time::Instant::now() < deadline,
                "abandoned SVG raster retained"
            );
            app.update();
            std::thread::yield_now();
        }
    }
}
