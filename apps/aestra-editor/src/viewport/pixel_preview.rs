//! Render the XY scene to virtual pixels while keeping authoring overlays at window resolution.
use super::{
    PreviewDisplayState, navigation::DEFAULT_XY_HEIGHT, scene::PreviewXyCamera,
    xy_grid::XY_GRID_LAYER,
};
use crate::theme;
use aestra_bevy_render::pixel_view::{PixelGrid, PixelTrailGrid, pixel_render_target};
#[cfg(test)]
use bevy::render::render_resource::TextureFormat;
use bevy::{
    camera::{RenderTarget, ScalingMode, Viewport, visibility::RenderLayers},
    prelude::*,
};

pub(super) const VIRTUAL_SIZE: UVec2 = UVec2::new(192, 128);
const COMPOSITE_LAYER: usize = 17;

#[derive(Component, Clone, Copy)]
pub(super) enum PixelPreviewPass {
    Scene,
    Composite,
}

#[derive(Component)]
pub(super) struct PixelSceneCamera;

pub(super) fn spawn_pixel_preview(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let image = images.add(pixel_render_target(VIRTUAL_SIZE).expect("valid editor pixel size"));
    commands.spawn((
        Camera2d,
        Camera {
            order: -3,
            is_active: false,
            clear_color: ClearColorConfig::Custom(theme::VIEWPORT),
            ..default()
        },
        RenderTarget::Image(image.clone().into()),
        Msaa::Off,
        RenderLayers::from_layers(&[0, XY_GRID_LAYER]),
        PixelGrid,
        PixelTrailGrid,
        PixelPreviewPass::Scene,
        PixelSceneCamera,
        aestra_bevy_render::AestraLights2d::default(),
    ));
    commands.spawn((
        Camera2d,
        Camera {
            order: -2,
            is_active: false,
            clear_color: ClearColorConfig::Custom(theme::APP_BG),
            viewport: Some(Viewport {
                physical_size: VIRTUAL_SIZE,
                ..default()
            }),
            ..default()
        },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::Fixed {
                width: VIRTUAL_SIZE.x as f32,
                height: VIRTUAL_SIZE.y as f32,
            },
            ..OrthographicProjection::default_2d()
        }),
        // The window pass shares attachments with the UI and gizmos. The
        // virtual scene uses its own single-sample image and nearest sampler.
        RenderLayers::layer(COMPOSITE_LAYER),
        PixelPreviewPass::Composite,
    ));
    commands.spawn((
        Sprite {
            image,
            custom_size: Some(VIRTUAL_SIZE.as_vec2()),
            ..default()
        },
        RenderLayers::layer(COMPOSITE_LAYER),
        Pickable::IGNORE,
    ));
}

// The virtual preview defaults to one world unit per virtual pixel at zoom 1.
// Keep the XY camera's pose and zoom when switching presentation.
pub(super) fn configure_xy_projection(
    display: Res<PreviewDisplayState>,
    camera: Single<&mut Projection, With<PreviewXyCamera>>,
) {
    let height = if display.pixel_grid {
        VIRTUAL_SIZE.y as f32
    } else {
        DEFAULT_XY_HEIGHT
    };
    let mut camera = camera.into_inner();
    if let Projection::Orthographic(projection) = &*camera
        && !matches!(projection.scaling_mode, ScalingMode::FixedVertical { viewport_height } if viewport_height == height)
        && let Projection::Orthographic(projection) = &mut *camera
    {
        projection.scaling_mode = ScalingMode::FixedVertical {
            viewport_height: height,
        };
    }
}

pub(super) fn follow_xy_camera(
    source: Single<(&Transform, Ref<Projection>), With<PreviewXyCamera>>,
    target: Single<
        (&mut Transform, &mut Projection),
        (With<PixelSceneCamera>, Without<PreviewXyCamera>),
    >,
) {
    let (pose, projection) = source.into_inner();
    let (mut transform, mut target_projection) = target.into_inner();
    transform.set_if_neq(*pose);
    if projection.is_changed() {
        *target_projection = (*projection).clone();
    }
}

#[cfg(test)]
#[path = "tests/pixel_preview.rs"]
mod tests;
