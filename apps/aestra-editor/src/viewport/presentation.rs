//! Production world grid/material, preview display routing and viewport control feedback.
use super::{
    PreviewDisplayMode,
    actions::PreviewDisplayState,
    navigation::{PreviewCameraController, PreviewRenderCamera},
    shortcuts::PreviewCanvas,
};
use crate::{feathers::icon::SvgColor, menus::MenuState, theme};
use aestra_bevy_render::{EffectRenderMode, PresentedEffect};
use bevy::{
    gizmos::transform_gizmo::{TransformGizmoMode, TransformGizmoSettings, TransformGizmoSpace},
    mesh::MeshVertexBufferLayoutRef,
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    reflect::TypePath,
    render::render_resource::{
        AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
    },
    shader::ShaderRef,
    ui::RelativeCursorPosition,
};

pub(super) const PREVIEW_GRID_SHADER_PATH: &str = "shaders/preview_grid.wesl";
pub(super) const PREVIEW_GRID_Y: f32 = -0.05;

#[derive(Component)]
pub(super) struct PreviewGridPlane;

#[derive(Clone, Copy, Debug, PartialEq, ShaderType)]
pub(super) struct PreviewGridUniform {
    pub(super) minor_color: Vec4,
    pub(super) major_color: Vec4,
    pub(super) x_axis_color: Vec4,
    pub(super) z_axis_color: Vec4,
    pub(super) parameters: Vec4,
    pub(super) focus: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub(super) struct PreviewGridMaterial {
    #[uniform(0)]
    pub(super) grid: PreviewGridUniform,
}

impl Material for PreviewGridMaterial {
    fn fragment_shader() -> ShaderRef {
        PREVIEW_GRID_SHADER_PATH.into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn depth_bias(&self) -> f32 {
        100.0
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

#[derive(Component)]
pub(super) struct PreviewDisplayModeIcon(pub(super) PreviewDisplayMode);

#[derive(Component)]
pub(super) struct TransformGizmoModeIcon(pub(super) TransformGizmoMode);

#[derive(Component)]
pub(super) struct PreviewPresentedEffect;

#[derive(Component)]
pub(super) struct GizmoModeLabel;

pub(super) fn sync_preview_grid(
    menu: Res<MenuState>,
    controller: Res<PreviewCameraController>,
    camera: Single<&GlobalTransform, With<PreviewRenderCamera>>,
    grid: Single<
        (
            &mut Transform,
            &mut Visibility,
            &MeshMaterial3d<PreviewGridMaterial>,
        ),
        With<PreviewGridPlane>,
    >,
    mut materials: ResMut<Assets<PreviewGridMaterial>>,
) {
    let (mut transform, mut visibility, material_handle) = grid.into_inner();
    *visibility = if menu.show_grid {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };

    let plane_radius = (controller.distance * 24.0).clamp(200.0, 100_000.0);
    // Navigation moves the camera, not the ground reference beneath the effect.
    transform.translation = Vec3::new(0.0, PREVIEW_GRID_Y, 0.0);
    transform.scale = Vec3::new(plane_radius, 1.0, plane_radius);

    let view_angle = camera.forward().dot(Vec3::NEG_Y).abs();
    let angle_fade = ((view_angle - 0.015) / 0.16).clamp(0.0, 1.0);
    if let Some(mut material) = materials.get_mut(&material_handle.0) {
        let uniform = preview_grid_uniform(controller.distance, angle_fade);
        if material.grid != uniform {
            material.grid = uniform;
        }
    }
}

pub(super) fn preview_grid_uniform(distance: f32, angle_fade: f32) -> PreviewGridUniform {
    PreviewGridUniform {
        minor_color: Vec4::new(0.20, 0.22, 0.29, 0.22),
        major_color: Vec4::new(0.27, 0.30, 0.39, 0.38),
        x_axis_color: Vec4::new(0.58, 0.20, 0.27, 0.62),
        z_axis_color: Vec4::new(0.20, 0.40, 0.68, 0.62),
        parameters: Vec4::new(
            adaptive_grid_spacing(distance / 18.0),
            (distance * 8.0).max(90.0),
            angle_fade,
            10.0,
        ),
        // Keep the fade centered on the world origin as well as the grid axes.
        focus: Vec4::ZERO,
    }
}

pub(super) fn sync_preview_display_mode(
    display: Res<PreviewDisplayState>,
    mut players: Query<(&mut PresentedEffect, &mut Visibility), With<PreviewPresentedEffect>>,
) {
    let render_mode = match display.mode {
        PreviewDisplayMode::Wireframe => EffectRenderMode::Wireframe,
        PreviewDisplayMode::Rendered => EffectRenderMode::Rendered,
    };
    for (mut player, mut visibility) in &mut players {
        player.set_render_mode(render_mode);
        *visibility = Visibility::Inherited;
    }
}

pub(super) fn update_preview_display_controls(
    display: Res<PreviewDisplayState>,
    mut icons: Query<(&PreviewDisplayModeIcon, &mut SvgColor)>,
) {
    for (icon, mut color) in &mut icons {
        let active = icon.0 == display.mode;
        color.0 = if active { theme::ACCENT } else { theme::TEXT };
    }
}

pub(super) fn update_transform_gizmo_controls(
    keys: Res<ButtonInput<KeyCode>>,
    canvas: Single<&RelativeCursorPosition, With<PreviewCanvas>>,
    mut settings: ResMut<TransformGizmoSettings>,
    mut labels: Query<&mut Text, With<GizmoModeLabel>>,
    mut icons: Query<(&TransformGizmoModeIcon, &mut SvgColor)>,
) {
    if canvas.cursor_over() && keys.just_pressed(KeyCode::KeyX) {
        settings.space = match settings.space {
            TransformGizmoSpace::World => TransformGizmoSpace::Local,
            TransformGizmoSpace::Local => TransformGizmoSpace::World,
        };
    }
    let mode = match settings.mode {
        TransformGizmoMode::Translate => "MOVE",
        TransformGizmoMode::Rotate => "ROTATE",
        TransformGizmoMode::Scale => "SCALE",
    };
    let space = match settings.space {
        TransformGizmoSpace::World => "WORLD",
        TransformGizmoSpace::Local => "LOCAL",
    };
    for mut label in &mut labels {
        **label = format!("{mode} · {space}");
    }
    for (icon, mut color) in &mut icons {
        color.0 = if icon.0 == settings.mode {
            theme::ACCENT
        } else {
            theme::TEXT
        };
    }
}

pub(super) fn adaptive_grid_spacing(desired: f32) -> f32 {
    if !desired.is_finite() || desired <= f32::EPSILON {
        return 1.0;
    }
    let magnitude = 10.0_f32.powf(desired.log10().floor());
    let normalized = desired / magnitude;
    let multiplier = if normalized <= 1.0 {
        1.0
    } else if normalized <= 2.0 {
        2.0
    } else if normalized <= 5.0 {
        5.0
    } else {
        10.0
    };
    magnitude * multiplier
}

#[cfg(test)]
#[path = "presentation/tests.rs"]
mod tests;
