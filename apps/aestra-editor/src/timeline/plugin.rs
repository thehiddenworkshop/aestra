//! Production timeline observer and schedule registration, shared by both engine graphs.
use super::*;

pub(crate) struct TimelinePlugin;

impl Plugin for TimelinePlugin {
    fn build(&self, app: &mut App) {
        host_motion_install::install(app);
        let duration = app
            .world()
            .get_resource::<EditorSession>()
            .map_or(1.0, EditorSession::playback_duration);
        app.insert_resource(TimelineState::framed(duration))
            .add_observer(queue_timeline_action_activation)
            .add_observer(execute_timeline_action)
            .add_observer(queue_choreography_action_activation)
            .add_observer(activate_timeline_track_entry)
            .add_observer(handle_timeline_track_color_change)
            .add_observer(begin_project_effect_drag_preview)
            .add_observer(finish_project_effect_drag_preview)
            .add_observer(reject_project_effect_drop)
            .add_observer(execute_choreography_action)
            .add_systems(
                Update,
                (
                    keyboard::choreography_keyboard_input,
                    open_focused_timeline_context_menu,
                    navigate_timeline,
                    dismiss_timeline_popovers,
                    dismiss_emitter_region_selection,
                )
                    .chain()
                    .in_set(TimelineSet::Input),
            )
            .add_systems(
                Update,
                (
                    handle_timeline_action_buttons,
                    handle_choreography_action_buttons,
                    audit_timeline_controls,
                )
                    .chain()
                    .in_set(TimelineSet::Actions),
            )
            .add_systems(
                Update,
                (
                    update_timeline_time_label,
                    update_timeline_visuals,
                    update_timeline_marker_visuals,
                    update_choreography_event_visuals,
                    update_effect_clip_visuals,
                    update_automation_lane_graph_visuals,
                    update_automation_key_visuals,
                    update_automation_curve_drag_preview,
                    update_effect_drop_insertion,
                    clear_cancelled_preview,
                    sync_effect_drop_track_gap,
                    update_effect_drop_preview,
                    reveal_timeline_emitter,
                    restore_timeline_context_menu_focus,
                    sync_timeline_vertical_scroll,
                    sync_timeline_horizontal_scroll,
                    sync_emitter_reorder_hints,
                    sync_effect_clip_reorder_hints,
                    sync_timeline_track_drop_hints,
                    tick_invalid_timeline_drop_feedback,
                )
                    .chain()
                    .in_set(TimelineSet::Visuals),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_action_path_consumes_one_native_button_activation() {
        let mut app = App::new();
        app.insert_resource(EditorSession::from_test_effect(EffectAsset::new(
            "Timeline controls",
            4.0,
        )))
        .init_resource::<crate::menus::MenuState>()
        .add_plugins(TimelinePlugin)
        .configure_sets(
            Update,
            (
                TimelineSet::Input.run_if(|| false),
                TimelineSet::Visuals.run_if(|| false),
            ),
        );
        let button = app
            .world_mut()
            .spawn((
                TimelineAction::SetSnap(document_actions::TimelineSnapMode::Frames),
                crate::feathers::action_activation::FeathersActionButton,
            ))
            .id();
        control_input::set_pressed(app.world_mut(), button, false);
        app.world_mut()
            .trigger(bevy::ui_widgets::Activate { entity: button });
        app.world_mut().flush();
        app.world_mut().run_schedule(Update);

        assert_eq!(
            app.world().resource::<TimelineState>().snap,
            document_actions::TimelineSnapMode::Frames
        );
        assert!(!app.world().resource::<EditorSession>().dirty);
        assert!(
            app.world()
                .get::<crate::feathers::action_activation::PendingFeathersActivation>(button)
                .is_none()
        );
        assert!(!control_input::is_pressed(app.world(), button));
        let revision = app.world().resource::<EditorSession>().ui_revision;
        app.world_mut().run_schedule(Update);
        assert_eq!(
            app.world().resource::<EditorSession>().ui_revision,
            revision
        );
    }

    #[test]
    fn installed_timeline_schedule_preserves_input_action_and_host_visual_order() {
        let mut app = App::new();
        app.insert_resource(EditorSession::from_test_effect(EffectAsset::new(
            "Timeline schedule",
            7.0,
        )));
        app.add_plugins(TimelinePlugin);
        assert_eq!(app.world().resource::<TimelineState>().view.end, 7.0);
        app.world_mut().trigger(TimelineAction::SetSnap(
            document_actions::TimelineSnapMode::Frames,
        ));
        assert_eq!(
            app.world().resource::<TimelineState>().snap,
            document_actions::TimelineSnapMode::Frames
        );
        assert!(!app.world().resource::<EditorSession>().dirty);

        app.world_mut().schedule_scope(Update, |world, schedule| {
            schedule.initialize(world).unwrap();
            let order = schedule
                .systems()
                .unwrap()
                .map(|(key, _)| key)
                .collect::<Vec<_>>();
            macro_rules! index {
                ($system:expr) => {{
                    let keys = schedule
                        .graph()
                        .systems_in_set($system.into_system_set().intern())
                        .unwrap();
                    assert_eq!(keys.len(), 1, "expected one installed system");
                    order
                        .iter()
                        .position(|key| key == keys.first().unwrap())
                        .expect("installed system is absent from execution order")
                }};
            }

            assert!(
                index!(keyboard::choreography_keyboard_input)
                    < index!(open_focused_timeline_context_menu)
            );
            assert!(index!(open_focused_timeline_context_menu) < index!(navigate_timeline));
            assert!(
                index!(handle_timeline_action_buttons) < index!(handle_choreography_action_buttons)
            );
            assert!(index!(handle_choreography_action_buttons) < index!(audit_timeline_controls));
            assert!(index!(motion_execution::cancel_drag) < index!(update_timeline_visuals));
            assert!(index!(update_timeline_visuals) < index!(motion_gestures::update_keys));
            assert!(index!(update_timeline_time_label) < index!(update_timeline_visuals));
            index!(inspector_composition::initialize_numbers);
            index!(host_composition::buttons);
        });
    }
}
