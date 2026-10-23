//! Actual pose picking and draw consumers with real Camera projections, not substitute geometry.
use super::*;
use bevy::{
    camera::{ComputedCameraValues, RenderTargetInfo, Viewport},
    window::WindowResolution,
};
struct Fixture {
    app: App,
    window: Entity,
    camera: Entity,
    canvas: Entity,
}
impl Fixture {
    fn new() -> Self {
        let mut effect = crate::test_support::effect_with_timing_slack();
        effect.host_transform_track = Some(HostTransformTrack::from_pose_keys(
            [0.0, 1.0, 2.0]
                .into_iter()
                .map(|time| aestra_core::HostTransformKey {
                    time,
                    transform: EmitterTransform {
                        translation: [(time - 1.0) * 20.0, 0.0, 0.0],
                        ..default()
                    },
                })
                .collect(),
            false,
        ));
        let mut session = EditorSession::from_test_effect(effect);
        let mut timeline = TimelineState::default();
        timeline_motion::select_pose(&mut session, &mut timeline, 0);
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(timeline)
            .init_resource::<HostMotionViewport>()
            .init_resource::<TransformGizmoState>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<MenuState>()
            .add_systems(Update, pick_pose);
        let mut window = Window {
            resolution: WindowResolution::new(800, 600),
            ..default()
        };
        window.set_cursor_position(Some(Vec2::new(400.0, 250.0)));
        let window = app.world_mut().spawn((window, PrimaryWindow)).id();
        let canvas = app
            .world_mut()
            .spawn((
                PreviewCanvas,
                RelativeCursorPosition {
                    cursor_over: true,
                    ..default()
                },
            ))
            .id();
        let camera = app
            .world_mut()
            .spawn((
                PreviewRenderCamera,
                Camera {
                    viewport: Some(Viewport {
                        physical_position: UVec2::new(100, 50),
                        physical_size: UVec2::new(600, 400),
                        ..default()
                    }),
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
            ))
            .id();
        Self {
            app,
            window,
            camera,
            canvas,
        }
    }
    fn tick(&mut self) {
        self.app.update();
    }
    fn hovered(&self) -> Option<usize> {
        self.app.world().resource::<HostMotionViewport>().hovered
    }
}
#[test]
fn projected_hover_and_click_select_pose_without_document_or_history_changes() {
    let mut f = Fixture::new();
    let s = f.app.world().resource::<EditorSession>();
    let before = s.effect.clone();
    let revision = s.document_revision();
    let ui = s.ui_revision;
    let time = s.simulation_time();
    f.tick();
    assert_eq!(f.hovered(), Some(1));
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    f.tick();
    let s = f.app.world().resource::<EditorSession>();
    assert_eq!(
        timeline_motion::selected_pose(s, f.app.world().resource::<TimelineState>())
            .unwrap()
            .0,
        1
    );
    assert_eq!(s.effect, before);
    assert_eq!(s.document_revision(), revision);
    assert!(!s.can_undo());
    assert!(!s.dirty);
    assert!(s.ui_revision > ui);
    assert_eq!(s.simulation_time(), time);
}
#[test]
fn selected_center_is_left_for_the_transform_gizmo() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .resource_scope(|world, mut s: Mut<EditorSession>| {
            timeline_motion::select_pose(&mut s, &mut world.resource_mut::<TimelineState>(), 1);
        });
    f.tick();
    assert_eq!(f.hovered(), None);
}
#[test]
fn navigation_buttons_and_active_trs_clear_old_pose_hover() {
    for mode in 0..3 {
        let mut f = Fixture::new();
        f.tick();
        assert_eq!(f.hovered(), Some(1));
        match mode {
            0 => f
                .app
                .world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Right),
            1 => f
                .app
                .world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Middle),
            _ => {
                f.app
                    .world_mut()
                    .resource_mut::<TransformGizmoState>()
                    .active = true
            }
        }
        f.tick();
        assert_eq!(f.hovered(), None);
    }
}
#[test]
fn canvas_exit_and_hovered_toolbar_block_pose_picking() {
    let mut f = Fixture::new();
    f.tick();
    assert_eq!(f.hovered(), Some(1));
    f.app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(f.canvas)
        .unwrap()
        .cursor_over = false;
    f.tick();
    assert_eq!(f.hovered(), None);
    f.app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(f.canvas)
        .unwrap()
        .cursor_over = true;
    let toolbar = f
        .app
        .world_mut()
        .spawn((
            FeathersActionButton,
            RelativeCursorPosition {
                cursor_over: true,
                ..default()
            },
        ))
        .id();
    f.tick();
    assert_eq!(f.hovered(), None);
    f.app.world_mut().entity_mut(toolbar).despawn();
    f.tick();
    assert_eq!(f.hovered(), Some(1));
}
#[test]
fn missing_camera_window_cursor_and_viewport_fail_closed() {
    for mode in 0..4 {
        let mut f = Fixture::new();
        f.tick();
        assert_eq!(f.hovered(), Some(1));
        match mode {
            0 => {
                f.app.world_mut().entity_mut(f.camera).despawn();
            }
            1 => {
                f.app.world_mut().entity_mut(f.window).despawn();
            }
            2 => {
                f.app
                    .world_mut()
                    .get_mut::<Window>(f.window)
                    .unwrap()
                    .set_cursor_position(None);
            }
            _ => {
                f.app
                    .world_mut()
                    .get_mut::<Camera>(f.camera)
                    .unwrap()
                    .computed
                    .target_info = None;
            }
        }
        f.tick();
        assert_eq!(f.hovered(), None);
    }
}
#[test]
fn cursor_outside_camera_viewport_cannot_select_a_pose() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .get_mut::<Window>(f.window)
        .unwrap()
        .set_cursor_position(Some(Vec2::new(50.0, 30.0)));
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    f.tick();
    assert_eq!(f.hovered(), None);
    assert_eq!(
        timeline_motion::selected_pose(
            f.app.world().resource::<EditorSession>(),
            f.app.world().resource::<TimelineState>()
        )
        .unwrap()
        .0,
        0
    );
}
#[test]
fn invalid_pending_review_preserves_pose_selection_and_clears_hover() {
    let mut f = Fixture::new();
    let mut s = f.app.world_mut().resource_mut::<EditorSession>();
    let emitter = s.effect.emitters[0].id;
    let module = s.effect.emitters[0]
        .module_by_type(aestra_core::MODULE_EMISSION)
        .unwrap()
        .id;
    assert!(
        s.preview_transaction(aestra_authoring::EffectTransaction::single(
            "Invalid",
            aestra_authoring::EffectCommand::RemoveModule { emitter, module }
        ))
    );
    f.tick();
    assert_eq!(f.hovered(), None);
    assert!(!f.app.world().resource::<EditorSession>().dirty);
}
#[test]
fn screen_space_marker_radius_uses_actual_projection_and_safe_fallback() {
    let f = Fixture::new();
    let camera = f.app.world().get::<Camera>(f.camera).unwrap();
    let transform = f.app.world().get::<GlobalTransform>(f.camera).unwrap();
    let radius = screen_space_gizmo_radius(camera, transform, Vec3::ZERO, 9.0);
    assert!((radius - 1.5).abs() < 1e-3);
    assert_eq!(
        screen_space_gizmo_radius(&Camera::default(), transform, Vec3::ZERO, 9.0),
        0.01
    );
}

#[test]
fn draw_path_emits_actual_gizmo_assets_and_inactive_motion_clears_the_handle() {
    use bevy::{
        asset::AssetPlugin,
        gizmos::{GizmoAsset, GizmoHandles},
    };
    let mut f = Fixture::new();
    f.app
        .add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<GizmoAsset>()
        .init_gizmo_group::<PreviewSceneGizmos>()
        .add_systems(PostUpdate, draw_path);
    f.tick();
    let assets = f.app.world().resource::<Assets<GizmoAsset>>();
    let (_, asset) = assets
        .iter()
        .find(|(_, asset)| asset.config_typeid() == std::any::TypeId::of::<PreviewSceneGizmos>())
        .unwrap();
    let buffer = asset.buffer().buffer();
    assert_eq!(buffer.list_positions.len(), 16);
    assert!(!buffer.strip_positions.is_empty());
    assert!(buffer.list_positions.iter().all(|p| p.is_finite()));
    let id = f.app.world().resource::<EditorSession>().effect.emitters[0].id;
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .selection
        .primary = SemanticTarget::Emitter(id);
    f.tick();
    assert!(
        f.app
            .world()
            .resource::<GizmoHandles>()
            .handles()
            .get(&std::any::TypeId::of::<PreviewSceneGizmos>())
            .unwrap()
            .is_none()
    );
}
