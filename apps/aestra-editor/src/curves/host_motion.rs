//! Target adapter for effect transform curves; uses the shared curve model and raster widget.
#[cfg(test)]
use super::host_editing::*;
#[cfg(test)]
use super::state::{DragState, Selection};
#[cfg(test)]
use super::*;
#[cfg(test)]
use crate::timeline::TimelineState;
#[cfg(test)]
use aestra_authoring::{EffectCommand, EffectTransaction};
#[cfg(test)]
use aestra_core::CurveInterpolation;
#[cfg(test)]
use aestra_core::EffectId;
#[cfg(test)]
use aestra_core::HostTransformTrack;

#[cfg(test)]
pub(super) use super::host_composition::*;

pub(super) use super::host_install::install;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feathers::icon::SvgFile;
    use aestra_core::{EmitterTransform, HostTransformKey};

    fn track() -> HostTransformTrack {
        HostTransformTrack::from_pose_keys(
            vec![
                HostTransformKey {
                    time: 0.0,
                    transform: EmitterTransform::default(),
                },
                HostTransformKey {
                    time: 2.0,
                    transform: EmitterTransform {
                        translation: [8.0, 4.0, 0.0],
                        ..default()
                    },
                },
            ],
            false,
        )
    }

    #[test]
    fn transform_view_spawns_shared_curve_raster_and_feather_controls() {
        use bevy::{
            asset::AssetPlugin, scene::ScenePlugin, text::TextPlugin, ui_widgets::Scrollbar,
        };
        let mut effect = test_support::effect_with_timing_slack();
        effect.host_transform_track = Some(track());
        let selection = Selection {
            effect: effect.id,
            channel: 0,
            key: 1,
        };
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<SvgFile>()
        .init_resource::<InputFocus>()
        .insert_resource(EditorSession::from_test_effect(effect))
        .insert_resource(CurvesState {
            host: Some(selection),
            ..default()
        })
        .add_systems(
            Startup,
            |mut commands: Commands, session: Res<EditorSession>, state: Res<CurvesState>| {
                commands.spawn(Node::default()).with_children(|parent| {
                    assert!(spawn(parent, &session, &state));
                });
            },
        );
        app.update();
        let world = app.world_mut();
        assert_eq!(
            world
                .query::<&automation_curve::AutomationCurveRaster>()
                .iter(world)
                .count(),
            1
        );
        assert_eq!(world.query::<&Key>().iter(world).count(), 2);
        assert_eq!(world.query::<&Scrollbar>().iter(world).count(), 1);
        let controls: Vec<_> = world
            .query::<&CurvesAction>()
            .iter(world)
            .copied()
            .collect();
        assert_eq!(
            controls
                .iter()
                .filter(|a| matches!(a, CurvesAction::SetInterpolation(_)))
                .count(),
            3
        );
        assert_eq!(
            controls
                .iter()
                .filter(|a| matches!(a, CurvesAction::OpenHost(_)))
                .count(),
            10
        );
    }

    #[test]
    fn interpolation_and_independent_keys_are_undoable() {
        let mut session = test_support::session_with_timing_slack();
        let original = track();
        assert!(session.execute(
            "Set track",
            EffectCommand::SetHostTransformTrack {
                track: Some(original.clone())
            },
            true
        ));
        let mut state = CurvesState::default();
        let mut timeline = TimelineState::default();
        open(&mut session, &mut state, &mut timeline, 0);
        set_interpolation(
            &mut session,
            &mut state,
            &mut timeline,
            CurveInterpolation::Step,
        );
        let changed = session.effect.host_transform_track.as_ref().unwrap();
        assert_eq!(
            changed.curves.translation[0].interpolation,
            CurveInterpolation::Step
        );
        assert_eq!(
            changed.curves.translation[1],
            original.curves.translation[1]
        );
        session.undo();
        assert_eq!(
            session.effect.host_transform_track.as_ref(),
            Some(&original)
        );
        session.redo();
        assert_eq!(
            session
                .effect
                .host_transform_track
                .as_ref()
                .unwrap()
                .curves
                .translation[0]
                .interpolation,
            CurveInterpolation::Step
        );

        session.seek_time(1.0);
        add_key(&mut session, &mut state, &mut timeline);
        let changed = session.effect.host_transform_track.as_ref().unwrap();
        assert_eq!(changed.curves.translation[0].keys.len(), 3);
        assert_eq!(changed.curves.translation[1].keys.len(), 2);
        assert_eq!(changed.curves.rotation.keys.len(), 2);
        delete_key(&mut session, &mut state, &mut timeline);
        assert_eq!(
            session
                .effect
                .host_transform_track
                .as_ref()
                .unwrap()
                .curves
                .translation[0]
                .keys
                .len(),
            2
        );
        session.undo();
        assert_eq!(
            session
                .effect
                .host_transform_track
                .as_ref()
                .unwrap()
                .curves
                .translation[0]
                .keys
                .len(),
            3
        );
    }

    #[test]
    fn drag_keeps_axes_fixed_and_changes_only_the_target_channel() {
        let original = track();
        let selection = Selection {
            effect: EffectId::new(),
            channel: 0,
            key: 1,
        };
        let (candidate, time) = edit_key(
            &original,
            selection,
            Vec2::new(-25.0, -10.0),
            Vec2::splat(100.0),
        )
        .unwrap();
        assert_eq!(time, 1.5);
        assert_eq!(
            candidate.curves.translation[1..],
            original.curves.translation[1..]
        );
        assert_eq!(candidate.curves.scale, original.curves.scale);
        let base = projection(&original, 0);
        let preview = drag_projection(&original, &candidate, 0);
        for value in [0.0, 4.0, 8.0] {
            assert_eq!(
                base.top_percent_for_value(value),
                preview.top_percent_for_value(value)
            );
        }
        assert!(
            edit_key(
                &original,
                selection,
                Vec2::new(-100.0, 0.0),
                Vec2::splat(100.0)
            )
            .is_none(),
            "a key cannot collide with the time-zero anchor"
        );
    }

    #[test]
    fn quaternion_component_drag_retimes_without_breaking_rotation() {
        let original = track();
        let selection = Selection {
            effect: EffectId::new(),
            channel: 9,
            key: 1,
        };
        let (candidate, time) = edit_key(
            &original,
            selection,
            Vec2::new(-25.0, 90.0),
            Vec2::splat(100.0),
        )
        .unwrap();
        assert_eq!(time, 1.5);
        assert_eq!(
            candidate.curves.rotation.keys[1].value,
            original.curves.rotation.keys[1].value
        );
        assert_eq!(candidate.curves.translation, original.curves.translation);
        candidate.validate().unwrap();
    }

    #[test]
    fn escape_cancels_the_temporary_curve_without_touching_history() {
        let mut session = test_support::session_with_timing_slack();
        let original = track();
        session.execute(
            "Set track",
            EffectCommand::SetHostTransformTrack {
                track: Some(original.clone()),
            },
            true,
        );
        let selection = Selection {
            effect: session.effect.id,
            channel: 0,
            key: 1,
        };
        let (candidate, time) = edit_key(
            &original,
            selection,
            Vec2::new(-25.0, 0.0),
            Vec2::splat(100.0),
        )
        .unwrap();
        assert!(session.preview_interaction(EffectTransaction::single(
            "Preview",
            EffectCommand::SetHostTransformTrack {
                track: Some(candidate.clone())
            }
        )));
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(CurvesState {
                host: Some(selection),
                host_drag: Some(DragState {
                    selection,
                    original: original.clone(),
                    candidate,
                    time,
                }),
                ..default()
            })
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, cancel_drag);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert!(app.world().resource::<CurvesState>().host_drag.is_none());
        let mut session = app.world_mut().resource_mut::<EditorSession>();
        assert_eq!(
            session.effect.host_transform_track.as_ref(),
            Some(&original)
        );
        session.undo();
        assert!(
            session.effect.host_transform_track.is_none(),
            "cancel must not introduce a history entry"
        );
    }
}
