//! Actual preview camera/grid/player setup and editor stand-in gizmo drawing.
use super::{
    PreviewDisplayMode, PreviewDisplayState,
    actions::PreviewLighting2d,
    gizmo_state::PreviewSceneGizmos,
    navigation::{
        DEFAULT_PREVIEW_PITCH, DEFAULT_XY_HEIGHT, EmitterTransformGizmoProxy,
        PreviewCameraController, PreviewRenderCamera, preview_camera_transform,
    },
    pixel_preview::PixelSceneCamera,
    presentation::{PREVIEW_GRID_Y, PreviewGridMaterial, PreviewGridPlane, preview_grid_uniform},
    runtime_preview::spawn_preview_effect_player,
    transform_gizmo::{bevy_transform_from_emitter, screen_space_gizmo_radius},
    xy_grid::XY_GRID_LAYER,
};
use crate::{session::EditorSession, theme};
use aestra_bevy_render::{AestraLights2d, AestraPointLight2d, PointLightFalloff2d};
use bevy::{
    camera::{ScalingMode, Viewport, visibility::RenderLayers},
    gizmos::transform_gizmo::{TransformGizmoCamera, TransformGizmoFocus},
    prelude::*,
};

#[derive(Component)]
pub(super) struct PreviewPerspectiveCamera;

#[derive(Component)]
pub(super) struct PreviewXyCamera;

pub(super) fn setup_preview_scene(
    mut commands: Commands,
    session: Res<EditorSession>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut grid_materials: ResMut<Assets<PreviewGridMaterial>>,
) {
    spawn_preview_camera(&mut commands);
    commands.spawn((
        PreviewXyCamera,
        Camera2d,
        AestraLights2d::default(),
        preview_camera(false),
        Transform::from_xyz(0.0, 0.0, DEFAULT_XY_HEIGHT),
        Projection::Orthographic(OrthographicProjection {
            far: 10_000.0,
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: DEFAULT_XY_HEIGHT,
            },
            ..OrthographicProjection::default_2d()
        }),
        RenderLayers::from_layers(&[0, XY_GRID_LAYER]),
    ));
    commands.spawn((
        PreviewGridPlane,
        Mesh3d(meshes.add(Plane3d::default().mesh().size(2.0, 2.0))),
        MeshMaterial3d(grid_materials.add(PreviewGridMaterial {
            grid: preview_grid_uniform(140.0, 1.0),
        })),
        Transform::from_xyz(0.0, PREVIEW_GRID_Y, 0.0).with_scale(Vec3::new(2_800.0, 1.0, 2_800.0)),
        RenderLayers::layer(0),
        Visibility::Inherited,
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

pub(super) fn sync_preview_lights_2d(
    time: Res<Time>,
    display: Res<PreviewDisplayState>,
    mut cameras: Query<&mut AestraLights2d, Or<(With<PreviewXyCamera>, With<PixelSceneCamera>)>>,
) {
    let falloff = match (display.mode, display.lighting_2d) {
        (PreviewDisplayMode::Rendered, PreviewLighting2d::Continuous) => {
            Some(PointLightFalloff2d::Continuous)
        }
        (PreviewDisplayMode::Rendered, PreviewLighting2d::Stepped) => {
            Some(PointLightFalloff2d::Stepped(4.try_into().unwrap()))
        }
        _ => None,
    };
    let phase = time.elapsed_secs() * 1.5;
    let point = falloff.map(|falloff| AestraPointLight2d {
        position: Vec2::new(phase.cos() * 46.0, phase.sin() * 18.0),
        radius: 64.0,
        color: Vec3::new(1.0, 0.75, 0.28),
        intensity: 1.0,
        falloff,
    });
    for mut lights in &mut cameras {
        if lights.lights.as_slice() != point.as_slice() {
            lights.lights.clear();
            lights.lights.extend(point);
        }
    }
}

pub(super) fn spawn_preview_camera(commands: &mut Commands) -> Entity {
    commands
        .spawn((
            PreviewRenderCamera,
            PreviewPerspectiveCamera,
            TransformGizmoCamera,
            Camera3d::default(),
            preview_camera(true),
            preview_camera_transform(Vec3::ZERO, 140.0, 0.0, DEFAULT_PREVIEW_PITCH),
            RenderLayers::layer(0),
        ))
        .id()
}

fn preview_camera(is_active: bool) -> Camera {
    Camera {
        is_active,
        order: -2,
        clear_color: ClearColorConfig::Custom(theme::VIEWPORT),
        viewport: Some(Viewport {
            physical_size: UVec2::splat(128),
            ..default()
        }),
        ..default()
    }
}

pub(super) fn select_preview_camera(
    mut commands: Commands,
    controller: Res<PreviewCameraController>,
    cameras: Query<
        (Entity, Has<PreviewXyCamera>, Has<PreviewRenderCamera>),
        Or<(With<PreviewPerspectiveCamera>, With<PreviewXyCamera>)>,
    >,
) {
    for (entity, xy, selected) in &cameras {
        match (xy == controller.xy, selected) {
            (true, false) => {
                commands
                    .entity(entity)
                    .insert((PreviewRenderCamera, TransformGizmoCamera));
            }
            (false, true) => {
                commands
                    .entity(entity)
                    .remove::<(PreviewRenderCamera, TransformGizmoCamera)>();
            }
            _ => {}
        }
    }
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
#[path = "tests/scene.rs"]
mod tests;
