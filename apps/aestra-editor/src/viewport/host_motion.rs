//! Document host poses share selection with the timeline and the standard TRS gizmo.
use super::*;
use crate::timeline::host_motion as timeline_motion;
use aestra_core::HostTransformTrack;

const MAX_PATH_KEYS: usize = 512;
const PICK_RADIUS: f32 = 9.0;

#[derive(Resource, Default)]
pub(super) struct HostMotionViewport {
    pub(super) hovered: Option<usize>,
}

// Bound drawing and picking costs without changing the authored track. Always retain
// the endpoints and selected key, even for imported tracks with many samples.
fn displayed_keys(count: usize, selected: Option<usize>) -> Vec<usize> {
    if count == 0 {
        return Vec::new();
    }
    let stride = (count - 1).div_ceil(MAX_PATH_KEYS - 1).max(1);
    let mut indices: Vec<_> = (0..count).step_by(stride).collect();
    indices.push(count - 1);
    if let Some(index) = selected.filter(|index| *index < count) {
        indices.push(index);
    }
    indices.sort_unstable();
    indices.dedup();
    indices
}

fn displayed_track(session: &EditorSession) -> Option<&HostTransformTrack> {
    session
        .preview
        .as_ref()
        .and_then(|preview| preview.host_transform_track())
        .map(|track| track.source())
        .or(session.effect.host_transform_track.as_ref())
}

fn hit_pose(
    points: impl Iterator<Item = (usize, Vec2)>,
    cursor: Vec2,
    selected: Option<usize>,
) -> Option<usize> {
    points
        .map(|(index, point)| (index, point.distance_squared(cursor)))
        .filter(|(_, distance)| *distance <= PICK_RADIUS * PICK_RADIUS)
        .min_by(|(a, da), (b, db)| {
            da.total_cmp(db)
                .then_with(|| (Some(*a) == selected).cmp(&(Some(*b) == selected)))
        })
        .map(|(index, _)| index)
        // Leave the selected pose's center available to the transform gizmo. At a
        // closed seam an overlapping, unselected endpoint can still be selected.
        .filter(|index| Some(*index) != selected)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn pick_pose(
    mut session: ResMut<EditorSession>,
    mut timeline: ResMut<TimelineState>,
    mut state: ResMut<HostMotionViewport>,
    gizmo: Res<TransformGizmoState>,
    buttons: Res<ButtonInput<MouseButton>>,
    menu: Res<MenuState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    canvases: Query<&RelativeCursorPosition, With<PreviewCanvas>>,
    cameras: Query<(&Camera, &GlobalTransform), With<PreviewRenderCamera>>,
    controls: Query<&RelativeCursorPosition, With<FeathersActionButton>>,
) {
    state.hovered = None;
    if !timeline_motion::active(&session, &timeline)
        || timeline_motion::busy(&timeline)
        || gizmo.active
        || session.pending_change.is_some()
        || menu.open.is_some()
        || menu.tab_context.is_some()
        || buttons.pressed(MouseButton::Middle)
        || buttons.pressed(MouseButton::Right)
        || controls.iter().any(RelativeCursorPosition::cursor_over)
        || !canvases.iter().any(RelativeCursorPosition::cursor_over)
    {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok((camera, transform)) = cameras.single() else {
        return;
    };
    let Some(rect) = camera.logical_viewport_rect() else {
        return;
    };
    if !rect.contains(cursor) {
        return;
    }
    let Some(track) = displayed_track(&session) else {
        return;
    };
    let selected = timeline_motion::selected_pose(&session, &timeline).map(|(index, _)| index);
    state.hovered = hit_pose(
        displayed_keys(track.keys.len(), selected)
            .into_iter()
            .filter_map(|index| {
                camera
                    .world_to_viewport(
                        transform,
                        Vec3::from_array(track.keys[index].transform.translation),
                    )
                    .ok()
                    .filter(|point| rect.contains(*point))
                    .map(|point| (index, point))
            }),
        cursor,
        selected,
    );
    if buttons.just_pressed(MouseButton::Left)
        && let Some(index) = state.hovered
    {
        timeline_motion::select_pose(&mut session, &mut timeline, index);
    }
}

pub(super) fn draw_path(
    session: Res<EditorSession>,
    timeline: Res<TimelineState>,
    state: Res<HostMotionViewport>,
    cameras: Query<(&Camera, &GlobalTransform), With<PreviewRenderCamera>>,
    mut gizmos: Gizmos<PreviewSceneGizmos>,
) {
    if !timeline_motion::active(&session, &timeline) {
        return;
    }
    let Some(track) = displayed_track(&session) else {
        return;
    };
    let Ok((camera, camera_transform)) = cameras.single() else {
        return;
    };
    let Some(rect) = camera.logical_viewport_rect() else {
        return;
    };
    let selected = timeline_motion::selected_pose(&session, &timeline).map(|(index, _)| index);
    let indices = displayed_keys(track.keys.len(), selected);
    let position = |index: usize| Vec3::from_array(track.keys[index].transform.translation);
    for pair in indices.windows(2) {
        gizmos.line(
            position(pair[0]),
            position(pair[1]),
            Color::srgb(0.58, 0.42, 1.0),
        );
    }
    for index in indices {
        let center = position(index);
        if !camera
            .world_to_viewport(camera_transform, center)
            .is_ok_and(|point| rect.contains(point))
        {
            continue;
        }
        let emphasized = Some(index) == selected || Some(index) == state.hovered;
        let radius = screen_space_gizmo_radius(
            camera,
            camera_transform,
            center,
            if emphasized { 7.0 } else { 5.0 },
        );
        gizmos.circle(
            Isometry3d::new(center, camera_transform.rotation()),
            radius,
            if emphasized {
                Color::WHITE
            } else {
                Color::srgb(0.7, 0.58, 1.0)
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pose_app(mode: TransformGizmoMode) -> (App, Entity) {
        let mut effect = crate::test_support::effect_with_timing_slack();
        effect.host_transform_track = Some(HostTransformTrack {
            keys: [0.0, 1.0, 2.0]
                .into_iter()
                .map(|time| aestra_core::HostTransformKey {
                    time,
                    transform: EmitterTransform::default(),
                })
                .collect(),
            repeat: true,
        });
        let mut session = EditorSession::from_test_effect(effect);
        let mut timeline = TimelineState::default();
        timeline_motion::select_pose(&mut session, &mut timeline, 0);
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(timeline)
            .insert_resource(TransformGizmoState {
                active: true,
                ..default()
            })
            .insert_resource(TransformGizmoSettings {
                mode,
                ..preview_transform_gizmo_settings()
            })
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<EmitterTransformGizmoInteraction>()
            .add_systems(Update, update_emitter_transform_gizmo);
        let proxy = app
            .world_mut()
            .spawn((EmitterTransformGizmoProxy, Transform::IDENTITY))
            .id();
        (app, proxy)
    }

    #[test]
    fn pose_gizmo_previews_trs_and_commits_one_undo_step_with_closed_seam() {
        for (mode, transform) in [
            (
                TransformGizmoMode::Translate,
                Transform::from_xyz(4.0, 5.0, 6.0),
            ),
            (
                TransformGizmoMode::Rotate,
                Transform::from_rotation(Quat::from_rotation_y(0.5)),
            ),
            (
                TransformGizmoMode::Scale,
                Transform::from_scale(Vec3::new(2.0, 3.0, 4.0)),
            ),
        ] {
            let (mut app, proxy) = pose_app(mode);
            app.update();
            app.world_mut().entity_mut(proxy).insert(transform);
            app.update();
            app.update();
            let expected = emitter_transform_from_bevy(&transform);
            let session = app.world().resource::<EditorSession>();
            assert_eq!(
                session.effect.host_transform_track.as_ref().unwrap().keys[0].transform,
                EmitterTransform::default()
            );
            assert!(!session.can_undo());
            let preview = displayed_track(session).unwrap();
            assert_eq!(preview.keys[0].transform, expected);
            assert_eq!(preview.keys[2].transform, expected);

            app.world_mut().resource_mut::<TransformGizmoState>().active = false;
            app.update();
            let mut session = app.world_mut().resource_mut::<EditorSession>();
            assert_eq!(
                session.effect.host_transform_track.as_ref().unwrap().keys[0].transform,
                expected
            );
            assert_eq!(
                session.selected_layer().transform,
                EmitterTransform::default()
            );
            session.undo();
            assert!(!session.can_undo(), "a whole drag is one undo step");
            assert_eq!(
                session.effect.host_transform_track.as_ref().unwrap().keys[0].transform,
                EmitterTransform::default()
            );
            session.redo();
            assert_eq!(
                session.effect.host_transform_track.as_ref().unwrap().keys[2].transform,
                expected
            );
        }
    }

    #[test]
    fn escape_cancels_pose_preview_and_late_release_cannot_commit() {
        let (mut app, proxy) = pose_app(TransformGizmoMode::Translate);
        app.world_mut()
            .entity_mut(proxy)
            .insert(Transform::from_xyz(9.0, 0.0, 0.0));
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.world_mut()
            .entity_mut(proxy)
            .insert(Transform::from_xyz(12.0, 0.0, 0.0));
        app.update();
        app.world_mut().resource_mut::<TransformGizmoState>().active = false;
        app.update();
        let session = app.world().resource::<EditorSession>();
        assert!(!session.can_undo());
        assert_eq!(
            displayed_track(session).unwrap().keys[0].transform,
            EmitterTransform::default()
        );
        assert_eq!(
            *app.world().entity(proxy).get::<Transform>().unwrap(),
            Transform::IDENTITY
        );
    }

    #[test]
    fn changing_pose_selection_cancels_instead_of_editing_another_key() {
        let (mut app, proxy) = pose_app(TransformGizmoMode::Translate);
        app.world_mut()
            .entity_mut(proxy)
            .insert(Transform::from_xyz(9.0, 0.0, 0.0));
        app.update();
        app.world_mut()
            .resource_scope(|world, mut session: Mut<EditorSession>| {
                timeline_motion::select_pose(
                    &mut session,
                    &mut world.resource_mut::<TimelineState>(),
                    1,
                );
            });
        app.world_mut().resource_mut::<TransformGizmoState>().active = false;
        app.update();
        let session = app.world().resource::<EditorSession>();
        assert!(!session.can_undo());
        assert!(
            displayed_track(session)
                .unwrap()
                .keys
                .iter()
                .all(|key| key.transform == EmitterTransform::default())
        );
        assert_eq!(
            timeline_motion::selected_pose(session, app.world().resource::<TimelineState>())
                .unwrap()
                .0,
            1
        );
        assert_eq!(
            session.selection.primary,
            SemanticTarget::Effect(session.effect.id)
        );
    }

    #[test]
    fn invalid_pose_never_reaches_the_command_or_changes_other_keys() {
        let (app, _) = pose_app(TransformGizmoMode::Translate);
        let session = app.world().resource::<EditorSession>();
        let target = PreviewTransformTarget::HostPose(session.effect.id, 1);
        assert!(
            preview_transform_command(
                target,
                EmitterTransform {
                    scale: [0.0; 3],
                    ..default()
                },
                session
            )
            .is_none()
        );
        assert!(
            preview_transform_command(
                PreviewTransformTarget::HostPose(session.effect.id, 99),
                EmitterTransform::default(),
                session
            )
            .is_none()
        );
    }

    #[test]
    fn large_paths_keep_endpoints_and_selection_with_bounded_cost() {
        let indices = displayed_keys(100_000, Some(42_111));
        assert!(indices.len() <= MAX_PATH_KEYS + 1);
        assert_eq!(indices.first(), Some(&0));
        assert_eq!(indices.last(), Some(&99_999));
        assert!(indices.contains(&42_111));
        assert_eq!(displayed_keys(3, Some(1)), vec![0, 1, 2]);
        assert!(displayed_keys(0, None).is_empty());
    }

    #[test]
    fn marker_picking_uses_screen_pixels_and_allows_closed_seam_selection() {
        let points = [(0, Vec2::ZERO), (1, Vec2::new(30.0, 0.0)), (2, Vec2::ZERO)];
        assert_eq!(hit_pose(points.into_iter(), Vec2::ZERO, Some(0)), Some(2));
        assert_eq!(hit_pose(points.into_iter(), Vec2::ZERO, Some(2)), Some(0));
        assert_eq!(
            hit_pose(points.into_iter(), Vec2::new(30.0, 0.0), Some(1)),
            None
        );
        assert_eq!(
            hit_pose(points.into_iter(), Vec2::new(30.0, 8.0), None),
            Some(1)
        );
        assert_eq!(
            hit_pose(points.into_iter(), Vec2::new(30.0, 10.0), None),
            None
        );
    }
}
