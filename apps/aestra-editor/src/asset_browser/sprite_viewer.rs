//! Docked, nearest-filtered inspection of project textures and sprite atlases.
use super::{BrowserSurface, state::Kind};
use crate::{
    DockingSet, EditorSession, Localizer, ProjectEffectCatalog,
    docking::{ToolPanel, WorkspaceLayout},
    feathers::button::spawn_action_button,
    shell::reveal_dock_panel,
    theme,
};
use aestra_project::{ProjectContentVersion, ProjectSourceId};
use bevy::{
    asset::{AssetEvent, AssetPath, LoadState},
    image::ImageSampler,
    prelude::*,
    ui_widgets::Activate,
};

#[derive(Event)]
pub(super) struct Open(pub ProjectSourceId, pub ProjectContentVersion);

#[derive(Resource, Default)]
struct Viewer {
    source: Option<(ProjectSourceId, u64)>,
    zoom: Zoom,
}

#[derive(Component, Default, Clone, Copy, PartialEq)]
enum Zoom {
    #[default]
    Fit,
    Pixels(f32),
}

#[derive(Component)]
struct ViewerStatus;

#[derive(Component)]
struct Canvas;

#[derive(Component)]
struct PreviewImage;

pub(super) fn register(app: &mut App) {
    app.init_resource::<Viewer>()
        .add_observer(open)
        .add_observer(set_zoom)
        .add_systems(Update, sync.after(DockingSet::Sync));
}

fn open(
    event: On<Open>,
    catalog: Res<ProjectEffectCatalog>,
    mut viewer: ResMut<Viewer>,
    mut layout: ResMut<WorkspaceLayout>,
    mut session: ResMut<EditorSession>,
) {
    if catalog.content_revision() != event.1
        || !catalog
            .content()
            .source(event.0)
            .is_some_and(|entry| Kind::of(entry) == Kind::Texture)
    {
        return;
    }
    viewer.source = Some((event.0, event.1.generation));
    viewer.zoom = Zoom::Fit;
    reveal_dock_panel(&mut layout, &mut session, ToolPanel::SpriteViewer);
}

fn set_zoom(event: On<Activate>, buttons: Query<&Zoom>, mut viewer: ResMut<Viewer>) {
    if let Ok(&zoom) = buttons.get(event.entity) {
        viewer.zoom = zoom;
    }
}

pub(crate) fn spawn_sprite_viewer(parent: &mut ChildSpawnerCommands, localizer: &Localizer) {
    parent
        .spawn((
            BrowserSurface,
            crate::feathers::node_graph::FeathersGraphNavigationBlocker,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                min_width: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(8.0)),
                row_gap: Val::Px(8.0),
                ..default()
            },
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    flex_wrap: FlexWrap::Wrap,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(4.0),
                    ..default()
                })
                .with_children(|toolbar| {
                    spawn_action_button(
                        toolbar,
                        &localizer.text("sprite-viewer-fit"),
                        Zoom::Fit,
                        false,
                    );
                    for (label, zoom) in [("1×", 1.0), ("2×", 2.0), ("4×", 4.0)] {
                        spawn_action_button(toolbar, label, Zoom::Pixels(zoom), false);
                    }
                    toolbar.spawn((
                        ViewerStatus,
                        Text::default(),
                        TextColor(theme::TEXT_MUTED),
                        TextFont::from_font_size(13.0),
                        Node {
                            margin: UiRect::left(Val::Px(8.0)),
                            ..default()
                        },
                    ));
                });
            panel
                .spawn((
                    Canvas,
                    Node {
                        flex_grow: 1.0,
                        min_height: Val::Px(0.0),
                        overflow: Overflow::clip(),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(theme::APP_BG),
                ))
                .with_children(|canvas| {
                    canvas.spawn((
                        PreviewImage,
                        ImageNode::default(),
                        Pickable::IGNORE,
                        Node {
                            flex_shrink: 0.0,
                            display: Display::None,
                            ..default()
                        },
                    ));
                });
        });
}

#[derive(Default)]
struct Preview {
    path: Option<std::path::PathBuf>,
    source: Handle<Image>,
    nearest: Option<(Handle<Image>, Vec2)>,
}

#[allow(clippy::too_many_arguments)]
fn sync(
    viewer: Res<Viewer>,
    catalog: Res<ProjectEffectCatalog>,
    localizer: Res<Localizer>,
    assets: Res<AssetServer>,
    mut images: ResMut<Assets<Image>>,
    mut events: MessageReader<AssetEvent<Image>>,
    canvases: Query<&ComputedNode, With<Canvas>>,
    mut previews: Query<(&ChildOf, &mut ImageNode, &mut Node), With<PreviewImage>>,
    mut labels: Query<&mut Text, With<ViewerStatus>>,
    mut current: Local<Preview>,
) {
    let mut modified = false;
    for event in events.read() {
        modified |= event.is_added(current.source.id())
            || event.is_modified(current.source.id())
            || event.is_removed(current.source.id());
    }
    if canvases.is_empty() {
        *current = Preview::default();
        return;
    }
    let entry = viewer
        .source
        .filter(|(_, generation)| *generation == catalog.content_revision().generation)
        .and_then(|(source, _)| catalog.content().source(source))
        .filter(|entry| Kind::of(entry) == Kind::Texture);
    let path = entry.map(|entry| &entry.path);
    if current.path.as_ref() != path {
        current.path = path.cloned();
        current.source = path.map_or_else(Handle::default, |path| {
            assets
                .load_builder()
                .override_unapproved()
                .load(AssetPath::from_path_buf(path.clone()))
        });
        current.nearest = None;
    }
    if modified {
        current.nearest = None;
    }
    if current.nearest.is_none()
        && current.path.is_some()
        && let Some(mut image) = images.get(&current.source).cloned()
    {
        image.sampler = ImageSampler::nearest();
        let size = image.size().as_vec2();
        current.nearest = Some((images.add(image), size));
    }
    for (parent, mut image, mut node) in &mut previews {
        let Some((handle, size)) = &current.nearest else {
            if node.display != Display::None {
                node.display = Display::None;
                image.image = Handle::default();
            }
            continue;
        };
        let Ok(canvas) = canvases.get(parent.parent()) else {
            continue;
        };
        let size = preview_size(
            *size,
            canvas.size(),
            canvas.inverse_scale_factor(),
            viewer.zoom,
        );
        if node.display != Display::Flex
            || node.width != Val::Px(size.x)
            || node.height != Val::Px(size.y)
        {
            node.display = Display::Flex;
            node.width = Val::Px(size.x);
            node.height = Val::Px(size.y);
        }
        if image.image != *handle {
            image.image = handle.clone();
        }
    }
    for mut label in &mut labels {
        label.set_if_neq(Text::new(match (entry, &current.nearest) {
            (Some(entry), Some((_, size))) => format!(
                "{}  ·  {} × {}",
                entry.name.to_string_lossy(),
                size.x,
                size.y
            ),
            (Some(_), None)
                if matches!(assets.load_state(current.source.id()), LoadState::Failed(_)) =>
            {
                localizer.text("sprite-viewer-unavailable")
            }
            (Some(_), None) => localizer.text("sprite-viewer-loading"),
            _ => localizer.text("sprite-viewer-empty"),
        }));
    }
}

fn preview_size(image: Vec2, viewport: Vec2, inverse_scale: f32, zoom: Zoom) -> Vec2 {
    let scale = match zoom {
        Zoom::Fit => {
            let fit = (viewport / image).min_element();
            if fit >= 1.0 { fit.floor() } else { fit }
        }
        Zoom::Pixels(scale) => scale,
    };
    image * scale * inverse_scale
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_uses_integer_magnification_and_keeps_larger_sprites_visible() {
        let image = Vec2::new(32.0, 16.0);
        assert_eq!(
            preview_size(image, Vec2::new(110.0, 65.0), 1.0, Zoom::Fit),
            Vec2::new(96.0, 48.0)
        );
        assert_eq!(
            preview_size(image, Vec2::splat(16.0), 1.0, Zoom::Fit),
            Vec2::new(16.0, 8.0)
        );
        assert_eq!(
            preview_size(image, Vec2::splat(100.0), 0.5, Zoom::Pixels(2.0)),
            image
        );
    }

    #[test]
    fn opening_a_texture_from_assets_loads_the_original_atlas_with_nearest_sampling() {
        use crate::asset_browser::{
            AssetBrowserState, actions::BrowserAction, tests::browser_app_with_assets,
        };
        use bevy::{
            asset::{AssetPlugin, UnapprovedPathMode},
            ecs::system::RunSystemOnce,
            image::{CompressedImageFormats, ImageLoader},
        };
        use std::time::{Duration, Instant};

        let root = tempfile::tempdir().unwrap();
        let atlas = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../sample-project/textures/pixel_pulse.png");
        std::fs::copy(atlas, root.path().join("sprite.png")).unwrap();
        let mut app = browser_app_with_assets(
            root.path(),
            AssetPlugin {
                unapproved_path_mode: UnapprovedPathMode::Deny,
                ..default()
            },
        );
        app.register_asset_loader(ImageLoader::new(CompressedImageFormats::NONE));
        // The test builds and activates the dock slot without saving user preferences.
        let mut layout = WorkspaceLayout::default();
        assert!(layout.show(ToolPanel::SpriteViewer));
        app.insert_resource(layout);
        app.world_mut()
            .run_system_once(|mut commands: Commands, locale: Res<Localizer>| {
                commands
                    .spawn_empty()
                    .with_children(|parent| spawn_sprite_viewer(parent, &locale));
            })
            .unwrap();
        let source = app
            .world()
            .resource::<ProjectEffectCatalog>()
            .content()
            .source_tree()
            .at_relative_path("sprite.png")
            .unwrap()
            .id;
        app.world_mut().resource_mut::<AssetBrowserState>().selected = Some(source);
        app.world_mut().trigger(BrowserAction::OpenSelected);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            app.update();
            let world = app.world_mut();
            let node = world
                .query_filtered::<&ImageNode, With<PreviewImage>>()
                .single(world)
                .unwrap();
            if let Some(image) = world.resource::<Assets<Image>>().get(&node.image) {
                assert_eq!(image.size(), UVec2::new(16, 8));
                assert_eq!(image.sampler, ImageSampler::nearest());
                break;
            }
            assert!(
                Instant::now() < deadline,
                "sprite viewer did not load the project texture"
            );
            std::thread::yield_now();
        }
    }
}
