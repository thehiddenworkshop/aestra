//! Shared SVG icon loading for editor controls.

use bevy::prelude::*;

mod svg;
pub(crate) use svg::{SvgColor, SvgFile, SvgPlugin, UiSvg};

use crate::theme;

/// Rasterize SVG controls above their display size so diagonal and curved edges remain smooth.
pub(crate) fn load_svg_icon(asset_server: &AssetServer, path: &'static str) -> Handle<SvgFile> {
    asset_server.load(path)
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
