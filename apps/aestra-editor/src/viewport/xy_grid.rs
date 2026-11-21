//! World-space reference grid for the orthographic editor view.
use super::{navigation::PreviewRenderCamera, scene::PreviewXyCamera};
use crate::menus::MenuState;
use bevy::{
    asset::embedded_asset,
    camera::visibility::{NoFrustumCulling, RenderLayers},
    prelude::*,
    render::render_resource::AsBindGroup,
    shader::ShaderRef,
    sprite_render::{AlphaMode2d, Material2d, Material2dPlugin, MeshMaterial2d},
};

/// Only the XY preview camera renders this layer.
pub(super) const XY_GRID_LAYER: usize = 16;
const SHADER: &str = "embedded://aestra_editor/viewport/xy_grid.wesl";

#[derive(Component)]
pub(super) struct PreviewXyGrid;

#[derive(Asset, TypePath, AsBindGroup, Clone)]
struct PreviewXyGridMaterial {}

impl Material2d for PreviewXyGridMaterial {
    fn vertex_shader() -> ShaderRef {
        SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn depth_bias(&self) -> f32 {
        // The grid is background decoration, even for negative effect Z layers.
        f32::NEG_INFINITY
    }
}

pub(super) fn register(app: &mut App) {
    embedded_asset!(app, "xy_grid.wesl");
    app.add_plugins(Material2dPlugin::<PreviewXyGridMaterial>::default())
        .add_systems(Startup, spawn_xy_grid.in_set(super::ViewportSet::Setup));
}

fn spawn_xy_grid(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PreviewXyGridMaterial>>,
) {
    commands.spawn((
        PreviewXyGrid,
        Mesh2d(meshes.add(Rectangle::new(2.0, 2.0))),
        MeshMaterial2d(materials.add(PreviewXyGridMaterial {})),
        RenderLayers::layer(XY_GRID_LAYER),
        NoFrustumCulling,
        Pickable::IGNORE,
        Visibility::Hidden,
    ));
}

pub(super) fn sync_xy_grid_visibility(
    menu: Res<MenuState>,
    camera: Query<(), (With<PreviewXyCamera>, With<PreviewRenderCamera>)>,
    mut grid: Single<&mut Visibility, With<PreviewXyGrid>>,
) {
    grid.set_if_neq(if menu.show_grid && !camera.is_empty() {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
}

#[cfg(test)]
#[path = "tests/xy_grid.rs"]
mod tests;
