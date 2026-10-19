//! Actual preview camera/grid/player setup and editor stand-in gizmo drawing.
use super::{
    gizmo_state::PreviewSceneGizmos,
    navigation::{
        DEFAULT_PREVIEW_PITCH, EmitterTransformGizmoProxy, PreviewRenderCamera,
        preview_camera_transform,
    },
    presentation::{PREVIEW_GRID_Y, PreviewGridMaterial, PreviewGridPlane, preview_grid_uniform},
    runtime_preview::spawn_preview_effect_player,
    transform_gizmo::{bevy_transform_from_emitter, screen_space_gizmo_radius},
};
use crate::{session::EditorSession, theme};
use bevy::{
    camera::{Viewport, visibility::RenderLayers},
    gizmos::transform_gizmo::{TransformGizmoCamera, TransformGizmoFocus},
    prelude::*,
};

pub(super) fn setup_preview_scene(
    mut commands: Commands,
    session: Res<EditorSession>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut grid_materials: ResMut<Assets<PreviewGridMaterial>>,
) {
    spawn_preview_camera(&mut commands);
    commands.spawn((
        PreviewGridPlane,
        Mesh3d(meshes.add(Plane3d::default().mesh().size(2.0, 2.0))),
        MeshMaterial3d(grid_materials.add(PreviewGridMaterial {
            grid: preview_grid_uniform(140.0, 1.0),
        })),
        Transform::from_xyz(0.0, PREVIEW_GRID_Y, 0.0).with_scale(Vec3::new(2_800.0, 1.0, 2_800.0)),
        RenderLayers::layer(0),
    ));
    spawn_preview_effect_player(&mut commands, &session, Transform::IDENTITY);
    commands.spawn((
        EmitterTransformGizmoProxy,
        TransformGizmoFocus,
        session
            .selected_layer()
            .map_or(Transform::IDENTITY, |emitter| {
                bevy_transform_from_emitter(emitter.transform)
            }),
    ));
}

pub(super) fn spawn_preview_camera(commands: &mut Commands) -> Entity {
    commands
        .spawn((
            PreviewRenderCamera,
            TransformGizmoCamera,
            Camera3d::default(),
            Camera {
                order: -2,
                clear_color: ClearColorConfig::Custom(theme::VIEWPORT),
                viewport: Some(Viewport {
                    physical_size: UVec2::splat(128),
                    ..default()
                }),
                ..default()
            },
            preview_camera_transform(Vec3::ZERO, 140.0, 0.0, DEFAULT_PREVIEW_PITCH),
            RenderLayers::layer(0),
        ))
        .id()
}

pub(super) fn draw_preview_mocks(
    session: Res<EditorSession>,
    camera: Single<(&Camera, &GlobalTransform), With<PreviewRenderCamera>>,
    mut gizmos: Gizmos<PreviewSceneGizmos>,
) {
    let time = session.simulation_time();
    for binding in &session.effect.bindings {
        let Some(mock) = session.preview_mocks.get(&binding.id) else {
            continue;
        };
        let (position, velocity) = mock.pose(time);
        let position = Vec3::from_array(position);
        let radius = screen_space_gizmo_radius(camera.0, camera.1, position, 7.0);
        gizmos.sphere(position, radius, theme::PLAYHEAD);
        let velocity = Vec3::from_array(velocity);
        if velocity.length_squared() > 1e-6 {
            gizmos.arrow(
                position,
                position + velocity.normalize() * radius * 4.0,
                theme::PLAYHEAD,
            );
        }
    }
}

#[cfg(test)]
#[path = "scene/tests.rs"]
mod tests;
