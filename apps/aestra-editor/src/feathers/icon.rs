//! Shared SVG icon loading for editor controls.

use bevy::prelude::*;
use bevy_resvg::prelude::{SvgColor, SvgFile, SvgFileLoaderSettings, TargetRenderSize, UiSvg};

use crate::theme;

/// Rasterize SVG controls above their display size so diagonal and curved edges remain smooth.
///
/// `bevy_resvg` converts an SVG to a texture once when the asset loads. Without an explicit
/// target it uses the source view box, which is only 8-28 pixels for several of our icons.
const EDITOR_ICON_RASTER_SIZE: u32 = 64;

pub(crate) fn load_svg_icon(asset_server: &AssetServer, path: &'static str) -> Handle<SvgFile> {
    asset_server
        .load_builder()
        .with_settings(|settings: &mut SvgFileLoaderSettings| {
            settings.target_render_size = Some(TargetRenderSize {
                width: EDITOR_ICON_RASTER_SIZE,
                height: EDITOR_ICON_RASTER_SIZE,
            });
        })
        .load(path)
}

pub(crate) fn spawn_breadcrumb_chevron<B: Bundle>(
    parent: &mut ChildSpawnerCommands,
    asset_server: &AssetServer,
    bundle: B,
) {
    parent
        .spawn((
            bundle,
            Node {
                width: Val::Px(18.0),
                height: Val::Px(28.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_child((
            Node {
                width: Val::Px(14.0),
                height: Val::Px(14.0),
                ..default()
            },
            UiSvg(load_svg_icon(asset_server, "icons/chevron-right.svg")),
            SvgColor(theme::TEXT_MUTED),
            Pickable::IGNORE,
        ));
}
