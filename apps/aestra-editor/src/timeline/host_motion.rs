//! Effect-level pose keys. UI state never owns semantic motion or bypasses command history.
use super::inspector_composition::initialize_numbers;
pub(crate) use super::inspector_composition::spawn_inspector;
#[cfg(test)]
use super::motion_execution::timeline_duration;
#[cfg(test)]
use super::motion_execution::{Field, NumberControl};
#[cfg(test)]
use super::motion_execution::{add_key, edit_pose, move_key, select};
pub(crate) use super::motion_execution::{busy, replace_pose, select_pose, selected_pose};
use super::motion_execution::{cancel_drag, execute, number_changed};
#[cfg(test)]
use super::motion_gestures::drag_time;
use super::motion_gestures::update_keys;
#[cfg(test)]
use super::motion_gestures::{KeyControl, click_key, end_drag};
#[cfg(test)]
use super::motion_state::Action;
#[cfg(test)]
use super::motion_state::KeyDrag;
pub(crate) use super::motion_state::active;
pub(super) use super::motion_state::keyboard_input;
use super::*;
#[cfg(test)]
use crate::feathers::number_input::{NumberInputValue, ScrubbableNumber, UpdateNumberInput};
#[cfg(test)]
use aestra_core::{EmitterTransform, HostTransformKey, HostTransformTrack};
#[cfg(test)]
use bevy::camera::NormalizedRenderTarget;

use super::host_composition::{activate, buttons};
pub(super) use super::host_composition::{spawn_header, spawn_lane};
pub(super) use super::interaction_state::HostMotionControl;

pub(super) fn install(app: &mut App) {
    app.add_observer(activate)
        .add_observer(execute)
        .add_observer(number_changed)
        .add_systems(Update, buttons.in_set(TimelineSet::Actions))
        .add_systems(
            Update,
            (
                initialize_numbers,
                cancel_drag.before(update_timeline_visuals),
                update_keys.after(update_timeline_visuals),
            )
                .in_set(TimelineSet::Visuals),
        );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track() -> HostTransformTrack {
        HostTransformTrack::from_pose_keys(
            [0.0, 1.0, 2.0]
                .into_iter()
                .map(|time| HostTransformKey {
                    time,
                    transform: EmitterTransform {
                        translation: [time * 10.0, 0.0, 0.0],
                        ..default()
                    },
                })
                .collect(),
            false,
        )
    }

    fn app() -> App {
        let mut effect = crate::test_support::effect_with_timing_slack();
        effect.host_transform_track = Some(track());
        let mut session = EditorSession::from_test_effect(effect);
        let mut state = TimelineState::framed(session.playback_duration());
        select(&mut session, &mut state, Some(1));
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(state)
            .init_resource::<ButtonInput<KeyCode>>();
        install(&mut app);
        app
    }

    fn number_control(app: &mut App, field: Field) -> Entity {
        let effect = app.world().resource::<EditorSession>().effect.id;
        app.world_mut()
            .spawn(NumberControl {
                effect,
                index: 1,
                field,
                initial: 10.0,
            })
            .id()
    }

    fn change(app: &mut App, source: Entity, value: f32, is_final: bool) {
        app.world_mut().trigger(ValueChange {
            source,
            value,
            is_final,
        });
    }

    #[test]
    fn add_creates_anchor_and_samples_existing_motion_without_duplicates() {
        let (created, index) = add_key(None, 0.75);
        assert_eq!(index, 1);
        assert_eq!(created.keys()[0].time, 0.0);
        created.validate().unwrap();
        let (inserted, index) = add_key(Some(&track()), 0.5);
        assert_eq!(index, 1);
        assert_eq!(
            inserted.keys()[index].transform.translation,
            [5.0, 0.0, 0.0]
        );
        let (again, same) = add_key(Some(&inserted), 0.5);
        assert_eq!(same, index);
        assert_eq!(again, inserted);
    }

    #[test]
    fn retime_preserves_anchor_and_orders_keys_without_collisions() {
        let original = track();
        assert!(move_key(&original, 0, 0.1).is_err());
        assert!(move_key(&original, 1, 2.0).is_err());
        assert!(move_key(&original, 1, f32::NAN).is_err());
        let (moved, selected) = move_key(&original, 1, 3.0).unwrap();
        assert_eq!(selected, 2);
        assert_eq!(
            moved.keys()[selected].transform,
            original.keys()[1].transform
        );
        assert_eq!(
            moved.keys().iter().map(|k| k.time).collect::<Vec<_>>(),
            [0.0, 2.0, 3.0]
        );
    }

    #[test]
    fn repeat_endpoint_edits_keep_the_seam_closed() {
        let mut closed = track();
        closed
            .curves
            .set_pose(closed.end_time(), closed.keys()[0].transform, false);
        closed.repeat = true;
        for field in [Field::Position(0), Field::Rotation(2), Field::Scale(1)] {
            closed = edit_pose(&closed, 0, field, 2.5).unwrap();
            assert_eq!(closed.keys()[0].transform, closed.keys()[2].transform);
            closed.validate().unwrap();
        }
        assert!(move_key(&closed, 2, 0.5).is_err());
        assert!(move_key(&closed, 1, 2.5).is_err());
        let (inserted, index) = add_key(Some(&closed), 2.5);
        assert_eq!(inserted.keys()[index].time, 0.5);
        assert!(edit_pose(&closed, 1, Field::Scale(0), 0.0).is_err());
        assert!(edit_pose(&closed, 1, Field::Position(0), f32::INFINITY).is_err());
    }

    #[test]
    fn numeric_drag_previews_then_commits_one_undoable_edit() {
        let mut app = app();
        let input = number_control(&mut app, Field::Position(0));
        let revision = app.world().resource::<EditorSession>().ui_revision;
        change(&mut app, input, 15.0, false);
        change(&mut app, input, 18.0, false);
        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect.host_transform_track, Some(track()));
        assert!(!session.can_undo());
        assert_eq!(session.ui_revision, revision);
        assert_eq!(
            session
                .preview()
                .unwrap()
                .host_transform_at(1.0)
                .translation[0],
            18.0
        );
        change(&mut app, input, 18.0, true);
        let mut session = app.world_mut().resource_mut::<EditorSession>();
        assert!(session.dirty);
        session.seek_time(0.5);
        let edited = session.preview().unwrap().host_transform_at(0.5);
        assert_eq!(edited.translation[0], 9.0);
        session.undo();
        assert_eq!(session.effect.host_transform_track, Some(track()));
        assert!(!session.can_undo());
        assert_eq!(
            session
                .preview()
                .unwrap()
                .host_transform_at(0.5)
                .translation[0],
            5.0
        );
        session.redo();
        assert_eq!(session.preview().unwrap().host_transform_at(0.5), edited);
    }

    #[test]
    fn unchanged_final_value_does_not_rebuild_controls_or_add_history() {
        let mut app = app();
        let input = number_control(&mut app, Field::Position(0));
        let revision = app.world().resource::<EditorSession>().ui_revision;
        change(&mut app, input, 10.0, false);
        assert!(
            app.world()
                .resource::<TimelineState>()
                .host_motion
                .number_preview
                .is_none()
        );
        for _ in 0..3 {
            change(&mut app, input, 10.0, true);
        }
        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.ui_revision, revision);
        assert!(!session.can_undo());
    }

    #[test]
    fn invalid_final_value_restores_committed_preview() {
        let mut app = app();
        let input = number_control(&mut app, Field::Scale(0));
        change(&mut app, input, 2.0, false);
        change(&mut app, input, 0.0, true);
        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect.host_transform_track, Some(track()));
        assert!(!session.can_undo());
        assert_eq!(
            session.preview().unwrap().host_transform_at(1.0).scale[0],
            1.0
        );
        assert!(
            app.world()
                .resource::<TimelineState>()
                .host_motion
                .error
                .is_some()
        );
    }

    #[test]
    fn escape_cancels_preview_and_ignores_late_release() {
        let mut app = app();
        let input = number_control(&mut app, Field::Position(0));
        change(&mut app, input, 30.0, false);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        change(&mut app, input, 30.0, true);
        let session = app.world().resource::<EditorSession>();
        assert!(!session.can_undo());
        assert_eq!(
            session
                .preview()
                .unwrap()
                .host_transform_at(1.0)
                .translation[0],
            10.0
        );
    }

    #[test]
    fn changing_selection_cancels_uncommitted_pose_preview() {
        let mut app = app();
        let input = number_control(&mut app, Field::Position(0));
        change(&mut app, input, 30.0, false);
        app.world_mut()
            .resource_mut::<EditorSession>()
            .selection
            .primary =
            SemanticTarget::Emitter(app.world().resource::<EditorSession>().effect.emitters[0].id);
        app.update();
        change(&mut app, input, 30.0, true);
        let session = app.world().resource::<EditorSession>();
        assert!(!session.can_undo());
        assert_eq!(
            session
                .preview()
                .unwrap()
                .host_transform_at(1.0)
                .translation[0],
            10.0
        );
    }

    #[test]
    fn actions_close_loop_delete_and_clear_through_history() {
        let mut app = app();
        app.world_mut().trigger(Action::Repeat(true));
        assert!(!app.world().resource::<EditorSession>().can_undo());
        app.world_mut().trigger(Action::CloseLoop);
        let closed = app
            .world()
            .resource::<EditorSession>()
            .effect
            .host_transform_track
            .clone()
            .unwrap();
        assert!(closed.repeat);
        assert_eq!(closed.keys()[0].transform, closed.keys()[2].transform);
        app.world_mut().trigger(Action::Delete);
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .effect
                .host_transform_track
                .as_ref()
                .unwrap()
                .keys()
                .len(),
            2
        );
        app.world_mut().trigger(Action::Clear);
        let mut session = app.world_mut().resource_mut::<EditorSession>();
        assert!(session.effect.host_transform_track.is_none());
        session.undo();
        session.undo();
        assert_eq!(session.effect.host_transform_track, Some(closed));
        session.undo();
        assert_eq!(session.effect.host_transform_track, Some(track()));
    }

    #[test]
    fn feather_activation_adds_once_at_the_playhead() {
        let mut app = app();
        app.world_mut()
            .resource_mut::<EditorSession>()
            .seek_time(0.5);
        let button = app
            .world_mut()
            .spawn((Action::Add, FeathersActionButton))
            .id();
        app.world_mut().trigger(Activate { entity: button });
        app.update();
        app.update();
        let mut session = app.world_mut().resource_mut::<EditorSession>();
        assert_eq!(
            session
                .effect
                .host_transform_track
                .as_ref()
                .unwrap()
                .keys()
                .len(),
            4
        );
        session.undo();
        assert_eq!(session.effect.host_transform_track, Some(track()));
        assert!(!session.can_undo());
    }

    #[test]
    fn frame_bounds_include_motion_without_changing_effect_duration() {
        let mut session = crate::test_support::session_with_timing_slack();
        let duration = session.playback_duration();
        let (extended, _) = move_key(&track(), 2, duration + 10.0).unwrap();
        session.effect.host_transform_track = Some(extended);
        assert_eq!(timeline_duration(&session), duration + 10.0);
        assert_eq!(session.playback_duration(), duration);
    }

    #[test]
    fn retiming_uses_screen_displacement_at_the_current_zoom() {
        assert_eq!(drag_time(1.0, 200.0, 1600.0 / 2.0, 4.0), 2.0);
        assert_eq!(drag_time(1.0, 200.0, 800.0, 2.0), 1.5);
        assert_eq!(drag_time(1.0, -1000.0, 800.0, 4.0), 0.0);
    }

    #[test]
    fn switching_from_region_to_motion_does_not_despawn_the_press_target() {
        let mut app = app();
        let emitter = app.world().resource::<EditorSession>().effect.emitters[0].clone();
        let region = emitter.timeline_regions()[0].id;
        app.world_mut()
            .resource_mut::<EditorSession>()
            .select_emitter_region(emitter.id, region);
        app.world_mut()
            .resource_mut::<TimelineState>()
            .select_only_emitter_region(emitter.id, region);
        let revision = app.world().resource::<EditorSession>().ui_revision;
        app.init_resource::<ButtonInput<MouseButton>>()
            .add_systems(Update, dismiss_emitter_region_selection);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        let control = app
            .world_mut()
            .spawn((
                HostMotionControl,
                RelativeCursorPosition {
                    cursor_over: true,
                    normalized: Some(Vec2::ZERO),
                },
            ))
            .id();
        app.update();
        assert_eq!(
            app.world().resource::<EditorSession>().ui_revision,
            revision
        );
        assert!(app.world().get_entity(control).is_ok());
        app.world_mut().trigger(Action::Inspect);
        assert!(
            app.world()
                .resource::<TimelineState>()
                .selected_emitter_regions
                .is_empty()
        );
        assert!(
            app.world()
                .resource::<EditorSession>()
                .selected_emitter_region
                .is_none()
        );
    }

    #[test]
    fn drop_keeps_the_reordered_key_selected_despite_a_trailing_click() {
        use bevy::picking::{
            backend::HitData,
            pointer::{Location, PointerId},
        };
        let mut app = app();
        let effect = app.world().resource::<EditorSession>().effect.id;
        let key = app
            .world_mut()
            .spawn(KeyControl { effect, index: 1 })
            .observe(end_drag)
            .observe(click_key)
            .id();
        let (candidate, selected) = move_key(&track(), 1, 3.0).unwrap();
        app.world_mut()
            .resource_mut::<TimelineState>()
            .host_motion
            .drag = Some(KeyDrag {
            effect,
            index: 1,
            start: 1.0,
            original: track(),
            candidate,
            selected,
        });
        let location = Location {
            target: NormalizedRenderTarget::None {
                width: 800,
                height: 600,
            },
            position: Vec2::ZERO,
        };
        app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            location.clone(),
            DragEnd {
                button: PointerButton::Primary,
                distance: Vec2::ZERO,
            },
            key,
        ));
        app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            location,
            Click {
                button: PointerButton::Primary,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                duration: std::time::Duration::ZERO,
                count: 1,
            },
            key,
        ));
        assert_eq!(
            app.world().resource::<TimelineState>().host_motion.selected,
            Some(2)
        );
        let mut session = app.world_mut().resource_mut::<EditorSession>();
        assert_eq!(
            session.effect.host_transform_track.as_ref().unwrap().keys()[2].time,
            3.0
        );
        session.undo();
        assert_eq!(session.effect.host_transform_track, Some(track()));
        assert!(!session.can_undo());
    }

    #[test]
    fn inspector_spawns_shared_numeric_controls_with_initialized_values_and_adjacent_scrollbar() {
        use bevy::{
            asset::AssetPlugin,
            scene::ScenePlugin,
            text::TextPlugin,
            ui_widgets::{ScrollArea, Scrollbar},
        };
        #[derive(Resource, Default)]
        struct InitialValues(Vec<(Entity, f32)>);
        let mut app = app();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<SvgFile>()
        .init_resource::<InputFocus>()
        .init_resource::<InitialValues>()
        .add_observer(
            |event: On<UpdateNumberInput>, mut values: ResMut<InitialValues>| {
                if let NumberInputValue::F32(value) = event.value {
                    values.0.push((event.entity, value));
                }
            },
        )
        .add_systems(
            Startup,
            |mut commands: Commands, session: Res<EditorSession>, state: Res<TimelineState>| {
                commands.spawn(Node::default()).with_children(|parent| {
                    spawn_header(parent);
                    spawn_lane(parent, &session);
                    assert!(spawn_inspector(parent, &session, &state));
                });
            },
        );
        app.update();
        let world = app.world_mut();
        let mut actions = world.query_filtered::<(&Node, &AccessibleLabel), With<Action>>();
        for (node, label) in actions.iter(world) {
            if label.0 != "+" {
                assert_eq!(
                    node.width,
                    Val::Auto,
                    "text actions must not use icon widths"
                );
                assert_eq!(node.flex_shrink, 0.0);
            }
        }
        let mut inputs = world.query::<(Entity, &NumberControl, &ScrubbableNumber)>();
        assert_eq!(inputs.iter(world).count(), 10);
        for (entity, control, scrub) in inputs.iter(world) {
            assert_eq!(control.initial, scrub.value);
            assert!(
                world
                    .resource::<InitialValues>()
                    .0
                    .contains(&(entity, control.initial))
            );
        }
        let mut slots =
            world.query_filtered::<(&KeyControl, &ChildOf), With<EditorNativeControl>>();
        assert_eq!(slots.iter(world).count(), 3);
        for (_, parent) in slots.iter(world) {
            let lane = world.get::<Node>(parent.parent()).unwrap();
            assert_eq!(lane.top, Val::Px(53.0));
            assert_eq!(lane.height, Val::Px(28.0));
        }
        let mut scrollbars = world.query::<(&Scrollbar, &ChildOf)>();
        for (bar, parent) in scrollbars.iter(world) {
            assert!(world.get::<ScrollArea>(bar.target).is_some());
            assert_eq!(
                world.get::<ChildOf>(bar.target).unwrap().parent(),
                parent.parent()
            );
            assert_eq!(
                world.get::<Node>(parent.parent()).unwrap().flex_direction,
                FlexDirection::Row
            );
        }
    }
}
