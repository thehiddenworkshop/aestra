//! Shared SVG icon loading for editor controls.

use bevy::prelude::*;
use bevy_resvg::prelude::{SvgFile, SvgFileLoaderSettings, TargetRenderSize};

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
