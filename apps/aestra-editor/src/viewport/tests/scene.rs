//! Actual preview setup and stand-in drawing with real assets and PresentedEffect payloads.
use super::super::{
    presentation::PreviewPresentedEffect, runtime_preview::PreviewEffectInstancePath,
};
use super::*;
use crate::{preview_mocks::PreviewMock, test_support};
use aestra_bevy_render::PresentedEffect;
use aestra_core::{BindingUpdateMode, EffectAsset, EffectBinding, EmitterTransform};
use bevy::{
    asset::AssetPlugin,
    camera::{ComputedCameraValues, RenderTargetInfo},
    gizmos::{GizmoAsset, GizmoHandles},
};

fn setup(session: EditorSession) -> App {
    let mut app = App::new();
    app.insert_resource(session)
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<PreviewGridMaterial>>()
        .add_systems(Startup, setup_preview_scene);
    app.update();
    app
}
fn count<T: Component>(app: &mut App) -> usize {
    app.world_mut()
        .query_filtered::<Entity, With<T>>()
        .iter(app.world())
        .count()
}

#[test]
fn startup_creates_one_actual_camera_grid_player_and_transform_proxy() {
    let mut app = setup(test_support::session_with_timing_slack());
    assert_eq!(count::<PreviewRenderCamera>(&mut app), 1);
    assert_eq!(count::<PreviewGridPlane>(&mut app), 1);
    assert_eq!(count::<PreviewPresentedEffect>(&mut app), 1);
    assert_eq!(count::<EmitterTransformGizmoProxy>(&mut app), 1);
    let w = app.world_mut();
    let (mesh, material, transform, layers) = w
        .query_filtered::<(
            &Mesh3d,
            &MeshMaterial3d<PreviewGridMaterial>,
            &Transform,
            &RenderLayers,
        ), With<PreviewGridPlane>>()
        .single(w)
        .unwrap();
    assert_eq!(transform.translation, Vec3::new(0.0, PREVIEW_GRID_Y, 0.0));
    assert_eq!(transform.scale, Vec3::new(2800.0, 1.0, 2800.0));
    assert_eq!(layers, &RenderLayers::layer(0));
    assert_eq!(
        w.resource::<Assets<Mesh>>()
            .get(&mesh.0)
            .unwrap()
            .count_vertices(),
        4
    );
    assert_eq!(
        w.resource::<Assets<PreviewGridMaterial>>()
            .get(&material.0)
            .unwrap()
            .grid,
        preview_grid_uniform(140.0, 1.0)
    );
}
#[test]
fn actual_camera_keeps_initial_viewport_clear_order_layer_and_gizmo_marker() {
    let mut app = setup(test_support::session_with_timing_slack());
    let w = app.world_mut();
    let (e, camera, transform, layers) = w
        .query_filtered::<(Entity, &Camera, &Transform, &RenderLayers), With<PreviewRenderCamera>>()
        .single(w)
        .unwrap();
    assert_eq!(camera.order, -2);
    assert!(matches!(camera.clear_color,ClearColorConfig::Custom(c) if c==theme::VIEWPORT));
    assert_eq!(
        camera.viewport.as_ref().unwrap().physical_size,
        UVec2::splat(128)
    );
    assert_eq!(
        transform,
        &preview_camera_transform(Vec3::ZERO, 140.0, 0.0, DEFAULT_PREVIEW_PITCH)
    );
    assert_eq!(layers, &RenderLayers::layer(0));
    assert!(w.get::<TransformGizmoCamera>(e).is_some());
    assert!(w.get::<Camera3d>(e).is_some());
}
#[test]
fn setup_uses_session_time_seed_compiled_identity_and_authored_proxy_pose_without_editing() {
    let mut effect = test_support::effect_with_timing_slack();
    effect.emitters[0].transform = EmitterTransform {
        translation: [3.0, 4.0, 5.0],
        ..default()
    };
    let mut session = EditorSession::from_test_effect(effect);
    session.preview_seed = 42;
    session.seek_time(1.25);
    let before = session.effect.clone();
    let revision = session.document_revision();
    let artifact = session.preview().unwrap().effect().clone();
    let mut app = setup(session);
    let w = app.world_mut();
    let (player,path,transform)=w.query_filtered::<(&PresentedEffect,&PreviewEffectInstancePath,&Transform),With<PreviewPresentedEffect>>().single(w).unwrap();
    assert!(std::sync::Arc::ptr_eq(player.effect(), &artifact));
    assert_eq!(player.simulation_time(), 1.25);
    assert_eq!(player.instance.seed(), 42);
    assert!(path.0.is_empty());
    assert_eq!(*transform, Transform::IDENTITY);
    let (pose, _) = w
        .query_filtered::<(&Transform, &TransformGizmoFocus), With<EmitterTransformGizmoProxy>>()
        .single(w)
        .unwrap();
    assert_eq!(pose.translation, Vec3::new(3.0, 4.0, 5.0));
    let s = w.resource::<EditorSession>();
    assert_eq!(s.effect, before);
    assert_eq!(s.document_revision(), revision);
    assert!(!s.dirty && !s.can_undo());
    assert_eq!(s.simulation_time(), 1.25);
    assert_eq!(s.preview_seed, 42);
}
#[test]
fn empty_effect_keeps_identity_proxy_and_a_valid_empty_presented_player() {
    let mut app = setup(EditorSession::from_test_effect(EffectAsset::new(
        "Empty", 4.0,
    )));
    assert_eq!(count::<PreviewPresentedEffect>(&mut app), 1);
    let w = app.world_mut();
    assert_eq!(
        *w.query_filtered::<&Transform, With<EmitterTransformGizmoProxy>>()
            .single(w)
            .unwrap(),
        Transform::IDENTITY
    );
    assert!(
        w.query_filtered::<&PresentedEffect, With<PreviewPresentedEffect>>()
            .single(w)
            .unwrap()
            .effect()
            .emitters
            .is_empty()
    );
}
#[test]
fn repeated_updates_do_not_duplicate_initial_scene_assets_or_entities() {
    let mut app = setup(test_support::session_with_timing_slack());
    let entities = app.world().entities().len();
    for _ in 0..4 {
        app.update();
    }
    assert_eq!(app.world().entities().len(), entities);
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 1);
    assert_eq!(
        app.world().resource::<Assets<PreviewGridMaterial>>().len(),
        1
    );
}
fn mock_app(mock: Option<PreviewMock>) -> (App, aestra_core::BindingId) {
    let mut effect = test_support::effect_with_timing_slack();
    let binding = EffectBinding::spatial("Target", BindingUpdateMode::Live);
    let id = binding.id;
    effect.bindings.push(binding);
    let mut session = EditorSession::from_test_effect(effect);
    if let Some(mock) = mock {
        session.preview_mocks.insert(id, mock);
    }
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<GizmoAsset>()
        .init_gizmo_group::<PreviewSceneGizmos>()
        .insert_resource(session)
        .add_systems(PostUpdate, draw_preview_mocks);
    app.world_mut().spawn((
        PreviewRenderCamera,
        Camera {
            computed: ComputedCameraValues {
                clip_from_view: Mat4::from_scale(Vec3::new(0.02, 0.02, -0.001)),
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::new(800, 600),
                    scale_factor: 1.0,
                }),
                ..default()
            },
            ..default()
        },
        GlobalTransform::from(Transform::from_xyz(0.0, 0.0, 10.0)),
    ));
    (app, id)
}
fn mock_geometry(app: &App) -> (Vec<Vec3>, Vec<Vec3>) {
    let assets = app.world().resource::<Assets<GizmoAsset>>();
    let (_, asset) = assets
        .iter()
        .find(|(_, a)| a.config_typeid() == std::any::TypeId::of::<PreviewSceneGizmos>())
        .unwrap();
    let b = asset.buffer().buffer();
    (b.list_positions.clone(), b.strip_positions.clone())
}
#[test]
fn still_stand_in_draws_only_the_actual_sphere_at_its_pose() {
    let (mut app, _) = mock_app(Some(PreviewMock::Still));
    app.update();
    let (lines, strips) = mock_geometry(&app);
    assert!(lines.is_empty());
    let points = strips
        .iter()
        .filter(|p| p.is_finite())
        .copied()
        .collect::<Vec<_>>();
    assert!(!points.is_empty());
    let min = points
        .iter()
        .fold(Vec3::splat(f32::INFINITY), |v, p| v.min(*p));
    let max = points
        .iter()
        .fold(Vec3::splat(f32::NEG_INFINITY), |v, p| v.max(*p));
    assert!(((min + max) * 0.5).abs_diff_eq(Vec3::new(0.0, 2.0, 5.0), 1e-4));
}
#[test]
fn moving_stand_ins_draw_velocity_arrows_and_follow_the_actual_playhead() {
    for mock in [PreviewMock::Orbit, PreviewMock::FlyBy] {
        let (mut app, _) = mock_app(Some(mock));
        app.update();
        let (first, _) = mock_geometry(&app);
        assert!(!first.is_empty());
        app.world_mut()
            .resource_mut::<EditorSession>()
            .seek_time(1.5);
        app.update();
        let (later, strips) = mock_geometry(&app);
        assert_ne!(later, first);
        assert!(later.iter().all(|p| p.is_finite()));
        assert!(strips.iter().any(|p| p.is_finite()));
        let position = Vec3::from_array(mock.pose(1.5).0);
        assert!(later.iter().any(|p| p.abs_diff_eq(position, 1e-4)));
    }
}
#[test]
fn missing_stand_in_or_orphan_assignment_cannot_draw_a_binding() {
    for orphan in [false, true] {
        let (mut app, _) = mock_app(None);
        if orphan {
            app.world_mut()
                .resource_mut::<EditorSession>()
                .preview_mocks
                .insert(aestra_core::BindingId::new(), PreviewMock::Still);
        }
        app.update();
        assert!(
            app.world()
                .resource::<GizmoHandles>()
                .handles()
                .get(&std::any::TypeId::of::<PreviewSceneGizmos>())
                .unwrap()
                .is_none()
        );
    }
}
#[test]
fn removing_a_stand_in_retires_the_current_geometry_handle() {
    let (mut app, id) = mock_app(Some(PreviewMock::Orbit));
    app.update();
    app.world_mut()
        .resource_mut::<EditorSession>()
        .preview_mocks
        .remove(&id);
    app.update();
    assert!(
        app.world()
            .resource::<GizmoHandles>()
            .handles()
            .get(&std::any::TypeId::of::<PreviewSceneGizmos>())
            .unwrap()
            .is_none()
    );
}
#[test]
fn stand_in_drawing_preserves_authored_effect_history_seed_and_clock() {
    let (mut app, _) = mock_app(Some(PreviewMock::Orbit));
    let s = app.world().resource::<EditorSession>();
    let before = s.effect.clone();
    let rev = s.document_revision();
    let seed = s.preview_seed;
    let time = s.simulation_time();
    app.update();
    let s = app.world().resource::<EditorSession>();
    assert_eq!(s.effect, before);
    assert_eq!(s.document_revision(), rev);
    assert_eq!(s.preview_seed, seed);
    assert_eq!(s.simulation_time(), time);
    assert!(!s.dirty && !s.can_undo());
}
