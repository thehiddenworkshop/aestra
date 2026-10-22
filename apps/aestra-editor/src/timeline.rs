#[cfg(test)]
use crate::EditorTooltip;
#[cfg(test)]
use crate::KeyboardNavigableListRow;
#[cfg(test)]
use crate::TransportAction;
#[cfg(test)]
use crate::WorkspaceLayout;
#[cfg(test)]
use crate::feathers::automation_curve;
#[cfg(test)]
use crate::feathers::icon::{SvgFile, UiSvg};
use crate::material_graph::MaterialGraphViewport;
#[cfg(test)]
use crate::project_content::EditorProjectContent as ProjectEffectCatalog;
#[cfg(test)]
use crate::{
    CurvesState, EditorModuleRegistry, EditorNativeControl, KeyboardNavigableList, Localizer, theme,
};
#[cfg(test)]
use crate::{FeathersActionButton, PendingFeathersActivation};
use crate::{ModulePaletteState, session::EditorSession};
use aestra_authoring::SemanticTarget;
#[cfg(test)]
use aestra_authoring::{EffectCommand, EffectTransaction};
#[cfg(test)]
use aestra_core::EffectAssetRef;
#[cfg(test)]
use aestra_core::EffectClip;
#[cfg(test)]
use aestra_core::Value;
#[cfg(test)]
use aestra_core::{
    ChoreographyEvent, ChoreographyEventId, ChoreographyEventPayload, CurveKey, EffectMarker,
    EffectParameter, MarkerId, ModuleId,
};
use aestra_core::{
    ChoreographyTrackId, EffectAsset, EffectClipId, Emitter, EmitterId, EmitterRegionId,
};
use bevy::input_focus::InputFocus;
#[cfg(test)]
use bevy::input_focus::InputFocusVisible;
#[cfg(test)]
use bevy::picking::pointer::PointerButton;
#[cfg(test)]
use bevy::ui_widgets::ActiveDescendant;
#[cfg(test)]
use bevy::ui_widgets::{Activate, Button};
#[cfg(test)]
use bevy::ui_widgets::{ControlOrientation, Scrollbar, ValueChange};
#[cfg(test)]
use bevy::{
    feathers::controls::ButtonVariant,
    ui::Selected,
    ui_widgets::{
        ListItem,
        popover::{Popover, PopoverAlign, PopoverSide},
    },
};
#[cfg(test)]
use bevy::{
    input_focus::tab_navigation::TabIndex,
    ui_widgets::{ListBox, ScrollArea},
};
use bevy::{prelude::*, text::EditableText, ui::RelativeCursorPosition};
use std::collections::{BTreeMap, BTreeSet};

mod presentation;
use presentation::*;
mod action_controls;
#[cfg(test)]
mod action_execution_tests;
mod actions;
mod cue_composition;
mod drop_execution;
mod effect_clip_composition;
mod emitter_composition;
mod host_composition;
mod inspector_composition;
mod panel_composition;
pub(crate) mod panel_input;
mod toolbar_composition;
pub(crate) use panel_composition::spawn_timeline;
mod plugin;
pub(crate) use plugin::TimelinePlugin;
mod projections;
mod scroll_composition;
mod track_composition;
mod track_style;
use drop_execution::*;
#[cfg(test)]
use emitter_composition::color_picker_popover_positions;
#[cfg(test)]
use track_style::layer_color;
mod reorder;
use action_controls::*;
#[cfg(test)]
use reorder::*;
mod asset_drop;
use asset_drop::clear_cancelled_preview;
mod automation;
mod automation_data;
#[path = "timeline/control_input_020.rs"]
mod control_input;
#[cfg(test)]
#[path = "timeline/document_actions/native_tests.rs"]
mod document_action_tests;
use automation_data::add_automation_key_at_pointer_action;
mod automation_execution;
mod automation_gestures;
mod clip_gestures;
mod lane_composition;
#[cfg(test)]
use clip_gestures::*;
mod clicks;
use clicks::*;
mod execution;
mod gesture_math;
mod gestures;
mod region_gestures;
#[path = "timeline/pointer_020.rs"]
mod timeline_pointer;
#[cfg(test)]
use gesture_math::*;
#[cfg(test)]
mod execution_tests;
mod referenced_execution;
mod region_execution;
#[cfg(test)]
use execution::select_choreography_target;
use execution::{execute_choreography_action, execute_timeline_action};
pub(crate) mod host_motion;
mod host_motion_install;
mod interaction_state;
mod keyboard;
mod motion_execution;
mod motion_gestures;
mod motion_state;
mod navigation;
#[path = "timeline/cursor_020.rs"]
mod navigation_cursor;
#[cfg(test)]
use navigation::timeline_cursor_fraction;
#[cfg(test)]
use navigation::{TimelineWheelIntent, should_dismiss_timeline_popover, timeline_wheel_intent};
use navigation::{
    dismiss_timeline_popovers, navigate_timeline, restore_timeline_context_menu_focus,
};
mod referenced_effect;
mod regions;
mod state;
use interaction_state::*;
pub(crate) use interaction_state::{
    AutomationLaneId, EffectClipChildSelection, EffectClipPath, TimelineAutomationKeySelection,
    TimelineSet,
};
#[cfg(test)]
use keyboard::choreography_keyboard_input;
#[cfg(test)]
use state::normalized_choreography_order;

pub(crate) use actions::{ChoreographyAction, TimelineAction};

#[cfg(test)]
use automation::AutomationLaneKeys;
#[cfg(test)]
use automation::automation_lane_count;
#[cfg(test)]
use automation::{automation_lane_is_visible, emitter_automation_lanes};
#[cfg(test)]
use automation_data::automation_lane_keys;
use automation_gestures::{
    update_automation_curve_drag_preview, update_automation_key_visuals,
    update_automation_lane_graph_visuals,
};
#[cfg(test)]
use projections::{automation_lanes_height, visible_automation_lane_count};
#[cfg(test)]
use referenced_effect::{ReferencedTrackKind, referenced_track_projections};
pub(crate) use referenced_execution::resolve_effect_clip_path;
#[cfg(test)]
use region_gestures::timeline_region_preview_timing;
use regions::dismiss_emitter_region_selection;
#[cfg(test)]
use regions::{commit_timeline_drag, update_timeline_drag};
#[cfg(test)]
use state::TimelineView;
pub(crate) use state::{TimelineNavigationSnapshot, TimelineState};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_picker_prefers_upward_alignment_near_the_window_bottom() {
        let positions = color_picker_popover_positions();

        assert_eq!(positions[0].side, PopoverSide::Right);
        assert_eq!(positions[0].align, PopoverAlign::End);
        assert!(positions.iter().any(|position| {
            position.side == PopoverSide::Top && position.align == PopoverAlign::Start
        }));
        assert!(positions.iter().any(|position| {
            position.side == PopoverSide::Bottom && position.align == PopoverAlign::Start
        }));
    }

    #[test]
    fn timeline_popovers_close_only_for_primary_clicks_outside_their_surface() {
        assert!(should_dismiss_timeline_popover(true, false, true));
        assert!(!should_dismiss_timeline_popover(true, true, true));
        assert!(!should_dismiss_timeline_popover(true, false, false));
        assert!(!should_dismiss_timeline_popover(false, false, true));
    }

    use crate::test_support;
    use bevy::{asset::AssetPlugin, scene::ScenePlugin, text::TextPlugin};

    fn spawn_test_timeline(
        mut commands: Commands,
        asset_server: Res<AssetServer>,
        session: Res<EditorSession>,
        state: Res<TimelineState>,
        catalog: Res<ProjectEffectCatalog>,
        registry: Res<EditorModuleRegistry>,
        curves: Res<CurvesState>,
        localizer: Res<Localizer>,
    ) {
        commands.spawn(Node::default()).with_children(|parent| {
            spawn_timeline(
                parent,
                &session,
                &state,
                &catalog,
                &registry,
                &curves,
                &localizer,
                &asset_server,
            );
        });
    }

    fn choreography_app(session: EditorSession) -> App {
        let mut app = App::new();
        let duration = session.playback_duration();
        app.insert_resource(session)
            .insert_resource(TimelineState::framed(duration))
            .insert_resource(ProjectEffectCatalog::from_entries(Vec::new()))
            .init_resource::<CurvesState>()
            .init_resource::<WorkspaceLayout>()
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_observer(execute_choreography_action);
        app
    }

    #[test]
    fn timeline_cursor_coordinates_cover_the_full_canvas() {
        assert_eq!(timeline_cursor_fraction(-0.5), 0.0);
        assert_eq!(timeline_cursor_fraction(0.0), 0.5);
        assert_eq!(timeline_cursor_fraction(0.5), 1.0);
    }

    fn session_with_first_emitter_timing_slack() -> EditorSession {
        test_support::session_with_timing_slack()
    }

    fn session_with_three_regions() -> (EditorSession, EmitterId, [EmitterRegionId; 3]) {
        let mut session = session_with_first_emitter_timing_slack();
        let emitter = session.effect.emitters[0].clone();
        let first = emitter.implicit_region_id();
        let second = EmitterRegionId::from_u128(0x7a);
        let mut regions = emitter
            .split_timeline_region(first, emitter.start_time + emitter.duration / 3.0, second)
            .unwrap();
        let split = regions[1].start_time + regions[1].duration / 2.0;
        let mut split_emitter = emitter.clone();
        split_emitter.regions = regions;
        let third = EmitterRegionId::from_u128(0x7b);
        regions = split_emitter
            .split_timeline_region(second, split, third)
            .unwrap();
        session.effect.emitters[0].regions = regions;
        session.select_emitter(emitter.id);
        (session, emitter.id, [first, second, third])
    }

    #[test]
    fn duplicate_selected_regions_is_one_undoable_region_edit() {
        let (session, emitter, [first, second, third]) = session_with_three_regions();
        let emitter_count = session.effect.emitters.len();
        let mut app = choreography_app(session);
        app.init_resource::<ModulePaletteState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, choreography_keyboard_input);
        app.world_mut().spawn(TimelineCanvas);
        {
            let mut state = app.world_mut().resource_mut::<TimelineState>();
            state.select_only_emitter_regions(emitter, &[first, third]);
        }
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::ControlLeft);
            keys.press(KeyCode::KeyD);
        }

        app.update();

        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect.emitters.len(), emitter_count);
        let regions = session.effect.emitters[0].timeline_regions();
        assert_eq!(regions.len(), 5);
        assert_eq!(
            app.world()
                .resource::<TimelineState>()
                .selected_emitter_regions
                .len(),
            2
        );
        assert!(
            app.world()
                .resource::<TimelineState>()
                .selected_emitter_regions
                .iter()
                .all(|(_, region)| ![first, second, third].contains(region))
        );

        app.world_mut().resource_mut::<EditorSession>().undo();
        assert_eq!(
            app.world().resource::<EditorSession>().effect.emitters[0]
                .timeline_regions()
                .len(),
            3
        );
    }

    #[test]
    fn delete_selected_regions_keeps_the_emitter_and_is_one_undoable_edit() {
        let (session, emitter, [first, second, third]) = session_with_three_regions();
        let emitter_count = session.effect.emitters.len();
        let mut app = choreography_app(session);
        app.world_mut()
            .resource_mut::<TimelineState>()
            .select_only_emitter_regions(emitter, &[first, second]);

        app.world_mut()
            .trigger(ChoreographyAction::DeleteSelectedEmitterRegions);
        app.update();

        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect.emitters.len(), emitter_count);
        assert_eq!(session.effect.emitters[0].timeline_regions().len(), 1);
        assert_eq!(session.effect.emitters[0].timeline_regions()[0].id, third);
        assert!(session.pending_change.is_none());

        app.world_mut().resource_mut::<EditorSession>().undo();
        assert_eq!(
            app.world().resource::<EditorSession>().effect.emitters[0]
                .timeline_regions()
                .len(),
            3
        );
    }

    #[test]
    fn deleting_every_selected_region_uses_reviewed_emitter_deletion() {
        let (session, emitter, regions) = session_with_three_regions();
        let emitter_count = session.effect.emitters.len();
        let mut app = choreography_app(session);
        app.world_mut()
            .resource_mut::<TimelineState>()
            .select_only_emitter_regions(emitter, &regions);

        app.world_mut()
            .trigger(ChoreographyAction::DeleteSelectedEmitterRegions);
        app.update();

        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect.emitters.len(), emitter_count);
        assert!(session.pending_change.is_some());
        assert!(
            session
                .effect
                .emitters
                .iter()
                .any(|item| item.id == emitter)
        );
    }

    #[test]
    fn split_toolbar_action_cuts_the_selected_emitter_region_at_the_playhead() {
        let mut session = test_support::session_with_timing_slack();
        let emitter = session.effect.emitters[0].clone();
        let region = emitter.timeline_regions()[0];
        let split_time = region.start_time + region.duration * 0.5;
        session.select_emitter(emitter.id);
        session.seek_time(split_time);
        let mut app = choreography_app(session);
        app.add_observer(execute_timeline_action);
        app.world_mut()
            .resource_mut::<TimelineState>()
            .select_only_emitter_region(emitter.id, region.id);

        app.world_mut().trigger(TimelineAction::SplitEmitterRegion);
        app.update();

        let session = app.world().resource::<EditorSession>();
        let regions = session.effect.emitters[0].timeline_regions();
        assert_eq!(regions.len(), 2);
        assert_eq!(regions[0].id, region.id);
        assert!((regions[0].end_time() - split_time).abs() <= 0.000_1);
        assert!((regions[1].start_time - split_time).abs() <= 0.000_1);
    }

    #[test]
    fn vector_gravity_curve_projects_and_edits_as_three_automation_lanes() {
        let mut session = test_support::session_with_timing_slack();
        let emitter = &mut session.effect.emitters[0];
        let motion = emitter
            .modules
            .iter_mut()
            .find(|module| module.module_type.0 == aestra_core::MODULE_MOTION)
            .unwrap();
        let module = motion.id;
        let source =
            aestra_core::PropertySource::Curve(aestra_core::PropertyEvaluationDomain::ParticleLife);
        motion.property_sources.insert("gravity".into(), source);
        let curves = aestra_core::Vec3Curve {
            curves: [
                aestra_core::Curve::new(vec![CurveKey::new(0.0, 0.1), CurveKey::new(1.0, 0.2)]),
                aestra_core::Curve::new(vec![CurveKey::new(0.0, 0.3), CurveKey::new(1.0, 0.4)]),
                aestra_core::Curve::new(vec![CurveKey::new(0.0, 0.5), CurveKey::new(1.0, 0.6)]),
            ],
        };
        motion.property_source_values.insert(
            "gravity".into(),
            vec![aestra_core::PropertySourceValue::new(
                source,
                Value::Vec3Curve(curves.clone()),
            )],
        );
        let registry = EditorModuleRegistry::default();
        let localizer = Localizer::new("en-US").unwrap();
        let gravity_lanes: Vec<_> = emitter_automation_lanes(
            &session.effect,
            &session.effect.emitters[0],
            &registry,
            &localizer,
        )
        .into_iter()
        .filter(|lane| lane.id.module == module && lane.id.parameter == "gravity")
        .collect();

        assert_eq!(gravity_lanes.len(), 3);
        assert_eq!(
            gravity_lanes
                .iter()
                .map(|lane| lane.id.channel)
                .collect::<Vec<_>>(),
            vec![Some(0), Some(1), Some(2)]
        );
        assert!(gravity_lanes[0].label.ends_with(" X"));
        assert!(gravity_lanes[1].label.ends_with(" Y"));
        assert!(gravity_lanes[2].label.ends_with(" Z"));
        assert_eq!(automation_lane_count(&session.effect.emitters[0]), 6);

        let y_lane = gravity_lanes[1].id.clone();
        let mut app = choreography_app(session);
        app.world_mut()
            .trigger(ChoreographyAction::AddAutomationKeyAt {
                lane: y_lane.clone(),
                normalized_time_bits: 0.5_f32.to_bits(),
                value_bits: Some(0.35_f32.to_bits()),
            });
        app.update();

        let session = app.world().resource::<EditorSession>();
        let motion = session.effect.emitters[0]
            .modules
            .iter()
            .find(|candidate| candidate.id == module)
            .unwrap();
        let Value::Vec3Curve(updated) =
            motion.property_value_for_source("gravity", source).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(updated.curves[0].keys, curves.curves[0].keys);
        assert_eq!(updated.curves[1].keys.len(), 3);
        assert_eq!(updated.curves[1].keys[1], CurveKey::new(0.5, 0.35));
        assert_eq!(updated.curves[2].keys, curves.curves[2].keys);
        assert_eq!(
            app.world()
                .resource::<TimelineState>()
                .selected_automation_key
                .as_ref()
                .map(|selection| (&selection.lane, selection.key)),
            Some((&y_lane, 1))
        );
        assert_eq!(
            app.world()
                .resource::<CurvesState>()
                .selected_vector_channel(),
            1
        );
    }

    #[test]
    fn timeline_curve_edits_update_the_bound_public_value() {
        let mut session = test_support::session_with_timing_slack();
        let emitter = &mut session.effect.emitters[0];
        let emission = emitter
            .modules
            .iter_mut()
            .find(|module| module.module_type.0 == aestra_core::MODULE_EMISSION)
            .unwrap();
        let module = emission.id;
        let source =
            aestra_core::PropertySource::Curve(aestra_core::PropertyEvaluationDomain::EmitterTime);
        emission
            .property_sources
            .insert("spawn_rate".into(), source);
        let bank_curve =
            aestra_core::Curve::new(vec![CurveKey::new(0.0, 4.0), CurveKey::new(1.0, 24.0)]);
        emission.property_source_values.insert(
            "spawn_rate".into(),
            vec![aestra_core::PropertySourceValue::new(
                source,
                Value::Curve(bank_curve.clone()),
            )],
        );
        let parameter_id = aestra_core::ParameterId::new();
        emission.bindings.insert("spawn_rate".into(), parameter_id);
        let public_curve =
            aestra_core::Curve::new(vec![CurveKey::new(0.0, 8.0), CurveKey::new(1.0, 16.0)]);
        session.effect.parameters.push(EffectParameter {
            id: parameter_id,
            name: "Spawn Rate".into(),
            default: Value::Curve(public_curve),
            exposed: true,
        });
        let registry = EditorModuleRegistry::default();
        let localizer = Localizer::new("en-US").unwrap();
        let lane = emitter_automation_lanes(
            &session.effect,
            &session.effect.emitters[0],
            &registry,
            &localizer,
        )
        .into_iter()
        .find(|lane| lane.id.module == module && lane.id.parameter == "spawn_rate")
        .unwrap();
        let mut app = choreography_app(session);

        app.world_mut()
            .trigger(ChoreographyAction::AddAutomationKeyAt {
                lane: lane.id,
                normalized_time_bits: 0.5_f32.to_bits(),
                value_bits: Some(12.0_f32.to_bits()),
            });
        app.update();

        let session = app.world().resource::<EditorSession>();
        let Value::Curve(public_curve) = &session
            .effect
            .parameters
            .iter()
            .find(|parameter| parameter.id == parameter_id)
            .unwrap()
            .default
        else {
            unreachable!()
        };
        assert_eq!(public_curve.keys.len(), 3);
        let emission = session.effect.emitters[0]
            .modules
            .iter()
            .find(|candidate| candidate.id == module)
            .unwrap();
        let Value::Curve(stored_curve) = emission
            .property_value_for_source("spawn_rate", source)
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(stored_curve.keys, bank_curve.keys);
        assert!(session.effect.validation_report().is_valid());
    }

    #[test]
    fn expanded_automation_lanes_shift_following_tracks_and_content_height() {
        let session = test_support::session_with_timing_slack();
        let catalog = ProjectEffectCatalog::from_entries(Vec::new());
        let first = &session.effect.emitters[0];
        let second = &session.effect.emitters[1];
        let collapsed = TimelineState::framed(session.playback_duration());
        let collapsed_row = choreography_grid_row(
            &session.effect,
            &collapsed,
            &catalog,
            ChoreographyTrackId::Emitter(second.id),
        );
        let collapsed_height =
            timeline_vertical_content_height(&session.effect, &collapsed, &catalog);
        let mut expanded = collapsed;
        expanded.expanded_automation_emitters.insert(first.id);
        let registry = EditorModuleRegistry::default();
        let localizer = Localizer::new("en-US").unwrap();
        let lanes = emitter_automation_lanes(&session.effect, first, &registry, &localizer);
        expanded
            .visible_automation_lanes
            .extend(lanes.iter().map(|lane| lane.id.clone()));
        expanded
            .automation_lane_heights
            .insert(lanes[0].id.clone(), 118.0);

        assert_eq!(
            choreography_grid_row(
                &session.effect,
                &expanded,
                &catalog,
                ChoreographyTrackId::Emitter(second.id),
            ),
            collapsed_row + automation_lane_count(first) as i16
        );
        assert_eq!(
            timeline_vertical_content_height(&session.effect, &expanded, &catalog),
            collapsed_height + automation_lanes_height(&expanded, first)
        );
        assert_eq!(expanded.automation_lane_height(&lanes[0].id), 118.0);
        assert_eq!(
            expanded.automation_lane_height(&lanes[1].id),
            automation_curve::DEFAULT_HEIGHT
        );
    }

    #[test]
    fn automation_visibility_defaults_to_hidden_and_updates_layout_per_lane() {
        let session = test_support::session_with_timing_slack();
        let catalog = ProjectEffectCatalog::from_entries(Vec::new());
        let first = &session.effect.emitters[0];
        let second = &session.effect.emitters[1];
        let registry = EditorModuleRegistry::default();
        let localizer = Localizer::new("en-US").unwrap();
        let lanes = emitter_automation_lanes(&session.effect, first, &registry, &localizer);
        let mut state = TimelineState::framed(session.playback_duration());
        state.expanded_automation_emitters.insert(first.id);

        assert_eq!(visible_automation_lane_count(&state, first), 0);
        let all_hidden_row = choreography_grid_row(
            &session.effect,
            &state,
            &catalog,
            ChoreographyTrackId::Emitter(second.id),
        );

        state.visible_automation_lanes.insert(lanes[0].id.clone());

        assert_eq!(visible_automation_lane_count(&state, first), 1);
        assert!(automation_lane_is_visible(&state, &lanes[0].id));
        assert!(!automation_lane_is_visible(&state, &lanes[1].id));
        assert_eq!(
            choreography_grid_row(
                &session.effect,
                &state,
                &catalog,
                ChoreographyTrackId::Emitter(second.id),
            ),
            all_hidden_row + 1
        );
    }

    #[test]
    fn automation_lane_actions_keep_the_chooser_open_and_bulk_actions_close_it() {
        let session = test_support::session_with_timing_slack();
        let registry = EditorModuleRegistry::default();
        let localizer = Localizer::new("en-US").unwrap();
        let emitter = session.effect.emitters[0].clone();
        let lanes = emitter_automation_lanes(&session.effect, &emitter, &registry, &localizer);
        let lane_ids = lanes.iter().map(|lane| lane.id.clone()).collect::<Vec<_>>();
        let lane = lanes[0].id.clone();
        let mut app = choreography_app(session);

        app.world_mut()
            .trigger(ChoreographyAction::ToggleEmitterAutomation(emitter.id));
        app.update();
        app.world_mut()
            .trigger(ChoreographyAction::SetAutomationLaneVisibility {
                lane: lane.clone(),
                visible: true,
            });
        app.update();

        let state = app.world().resource::<TimelineState>();
        assert_eq!(state.automation_menu_emitter, Some(emitter.id));
        assert!(state.expanded_automation_emitters.contains(&emitter.id));
        assert!(state.visible_automation_lanes.contains(&lane));

        app.world_mut()
            .trigger(ChoreographyAction::SetEmitterAutomationVisibility {
                emitter: emitter.id,
                lanes: lane_ids.clone(),
                visible: true,
            });
        app.update();
        let state = app.world().resource::<TimelineState>();
        assert_eq!(state.automation_menu_emitter, None);
        assert!(
            lane_ids
                .iter()
                .all(|lane| state.visible_automation_lanes.contains(lane))
        );

        app.world_mut()
            .trigger(ChoreographyAction::ToggleEmitterAutomation(emitter.id));
        app.update();
        app.world_mut()
            .trigger(ChoreographyAction::SetEmitterAutomationVisibility {
                emitter: emitter.id,
                lanes: lane_ids.clone(),
                visible: false,
            });
        app.update();
        let state = app.world().resource::<TimelineState>();
        assert_eq!(state.automation_menu_emitter, None);
        assert!(
            lane_ids
                .iter()
                .all(|lane| !state.visible_automation_lanes.contains(lane))
        );
    }

    #[test]
    fn automation_key_add_and_delete_are_transactional_and_share_curves_selection() {
        let mut session = test_support::session_with_timing_slack();
        let registry = EditorModuleRegistry::default();
        let localizer = Localizer::new("en-US").unwrap();
        let emitter = session.effect.emitters[0].clone();
        let lane = emitter_automation_lanes(&session.effect, &emitter, &registry, &localizer)
            .into_iter()
            .find(|lane| matches!(lane.keys, AutomationLaneKeys::Curve(..)))
            .unwrap();
        let original_len = lane.keys.len();
        session.seek_time(emitter.start_time + emitter.duration * 0.5);
        let mut app = choreography_app(session);

        app.world_mut()
            .trigger(ChoreographyAction::AddAutomationKey(lane.id.clone()));
        app.update();

        let selected = app
            .world()
            .resource::<TimelineState>()
            .selected_automation_key
            .clone()
            .unwrap();
        assert_eq!(
            automation_lane_keys(&app.world().resource::<EditorSession>().effect, &lane.id,)
                .unwrap()
                .len(),
            original_len + 1
        );
        let curve_selection = app
            .world()
            .resource::<CurvesState>()
            .selected_key()
            .unwrap();
        assert_eq!(curve_selection.module, lane.id.module);
        assert_eq!(curve_selection.input, lane.id.input);
        assert_eq!(curve_selection.key, selected.key);

        app.world_mut()
            .trigger(ChoreographyAction::DeleteAutomationKey(selected));
        app.update();
        assert_eq!(
            automation_lane_keys(&app.world().resource::<EditorSession>().effect, &lane.id,)
                .unwrap()
                .len(),
            original_len
        );
        app.world_mut().resource_mut::<EditorSession>().undo();
        assert_eq!(
            automation_lane_keys(&app.world().resource::<EditorSession>().effect, &lane.id,)
                .unwrap()
                .len(),
            original_len + 1
        );
    }

    #[test]
    fn graph_key_add_uses_the_pointer_time_and_value() {
        let session = test_support::session_with_timing_slack();
        let registry = EditorModuleRegistry::default();
        let localizer = Localizer::new("en-US").unwrap();
        let emitter = session.effect.emitters[0].clone();
        let lane = emitter_automation_lanes(&session.effect, &emitter, &registry, &localizer)
            .into_iter()
            .find(|lane| matches!(lane.keys, AutomationLaneKeys::Curve(..)))
            .unwrap();
        let mut app = choreography_app(session);

        app.world_mut()
            .trigger(ChoreographyAction::AddAutomationKeyAt {
                lane: lane.id.clone(),
                normalized_time_bits: 0.375_f32.to_bits(),
                value_bits: Some(42.5_f32.to_bits()),
            });
        app.update();

        let keys = automation_lane_keys(&app.world().resource::<EditorSession>().effect, &lane.id)
            .unwrap();
        let AutomationLaneKeys::Curve(keys, _) = keys else {
            panic!("expected curve lane");
        };
        let inserted = keys
            .iter()
            .find(|key| (key.time - 0.375).abs() < 0.0001)
            .expect("pointer-time key");
        assert!((inserted.value - 42.5).abs() < 0.0001);
        assert!(app.world().resource::<EditorSession>().can_undo());
    }

    #[test]
    fn effect_clip_placement_uses_pointer_time_and_fits_the_owner() {
        assert_eq!(effect_clip_placement(0.75, 2.8, 1.0), Some((0.75, 1.0)));
        assert_eq!(effect_clip_placement(2.8, 2.8, 1.0), Some((1.8, 1.0)));
        assert_eq!(effect_clip_placement(1.0, 2.8, 4.0), Some((0.0, 2.8)));
        assert_eq!(effect_clip_placement(0.0, 0.0, 1.0), None);
        assert_eq!(effect_clip_placement(0.0, 2.8, 0.0), None);
    }

    #[test]
    fn asset_drop_insertion_uses_the_hovered_track_boundary() {
        let first = ChoreographyTrackId::Emitter(EmitterId::new());
        let second = ChoreographyTrackId::Emitter(EmitterId::new());
        let third = ChoreographyTrackId::Emitter(EmitterId::new());
        let order = [first, second, third];

        assert_eq!(choreography_insertion_index(&order, second, true), 1);
        assert_eq!(choreography_insertion_index(&order, second, false), 2);
        assert_eq!(
            timeline_insertion_for_cursor(
                second,
                &RelativeCursorPosition {
                    cursor_over: true,
                    normalized: Some(Vec2::new(0.0, -0.25)),
                },
            ),
            Some((second, true))
        );
        assert_eq!(
            timeline_insertion_for_cursor(
                second,
                &RelativeCursorPosition {
                    cursor_over: true,
                    normalized: Some(Vec2::new(0.0, 0.25)),
                },
            ),
            Some((second, false))
        );
        assert_eq!(
            timeline_insertion_for_cursor(second, &RelativeCursorPosition::default()),
            None
        );
        assert_eq!(
            choreography_insertion_index(
                &order,
                ChoreographyTrackId::EffectClip(EffectClipId::new()),
                true,
            ),
            order.len()
        );
    }

    #[test]
    fn asset_drop_ghost_uses_the_selected_insertion_row() {
        let session = test_support::session_with_timing_slack();
        let state = TimelineState::framed(session.playback_duration());
        let catalog = ProjectEffectCatalog::from_entries(Vec::new());
        let order = normalized_choreography_order(&session.effect);
        let first = order[0];
        let second = order[1];

        assert_eq!(
            choreography_insertion_grid_row(&session.effect, &state, &catalog, Some((first, true)),),
            1
        );
        assert_eq!(
            choreography_insertion_grid_row(
                &session.effect,
                &state,
                &catalog,
                Some((second, true)),
            ),
            2
        );
        assert_eq!(
            choreography_insertion_grid_row(
                &session.effect,
                &state,
                &catalog,
                Some((second, false)),
            ),
            3
        );
        assert_eq!(
            choreography_insertion_grid_row(&session.effect, &state, &catalog, None),
            order.len() as i16 + 1
        );
    }

    #[test]
    fn asset_drop_ghost_reserves_a_synchronized_track_gap() {
        let session = test_support::session_with_timing_slack();
        let order = normalized_choreography_order(&session.effect);
        let mut state = TimelineState::framed(session.playback_duration());
        state.effect_drop_preview = Some(EffectDropPreview {
            source_duration: 1.0,
            display_name: "Reusable effect".into(),
        });
        state.effect_drop_insertion = Some((order[1], true));
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(state)
            .insert_resource(ProjectEffectCatalog::from_entries(Vec::new()))
            .add_systems(Update, sync_effect_drop_track_gap);
        let first = app
            .world_mut()
            .spawn((
                TimelineChoreographyGridRow(1),
                Node {
                    grid_row: GridPlacement::start(1),
                    ..default()
                },
            ))
            .id();
        let second = app
            .world_mut()
            .spawn((
                TimelineChoreographyGridRow(2),
                Node {
                    grid_row: GridPlacement::start(2),
                    ..default()
                },
            ))
            .id();
        let spacer = app
            .world_mut()
            .spawn((TimelineEffectDropSpacer, Node::default()))
            .id();

        app.update();

        assert_eq!(
            app.world().get::<Node>(first).unwrap().grid_row,
            GridPlacement::start(1)
        );
        assert_eq!(
            app.world().get::<Node>(second).unwrap().grid_row,
            GridPlacement::start(3)
        );
        let spacer_node = app.world().get::<Node>(spacer).unwrap();
        assert_eq!(spacer_node.display, Display::Flex);
        assert_eq!(spacer_node.grid_row, GridPlacement::start(2));

        app.world_mut()
            .resource_mut::<TimelineState>()
            .effect_drop_preview = None;
        app.update();

        assert_eq!(
            app.world().get::<Node>(second).unwrap().grid_row,
            GridPlacement::start(2)
        );
        assert_eq!(
            app.world().get::<Node>(spacer).unwrap().display,
            Display::None
        );
    }

    #[test]
    fn asset_drop_uses_the_reserved_gap_without_track_edge_highlights() {
        let emitter = EmitterId::new();
        let track = ChoreographyTrackId::Emitter(emitter);
        let state = TimelineState {
            effect_drop_preview: Some(EffectDropPreview {
                source_duration: 1.0,
                display_name: "Reusable effect".into(),
            }),
            ..default()
        };
        let mut app = App::new();
        app.insert_resource(state).add_systems(
            Update,
            (
                update_effect_drop_insertion,
                sync_emitter_reorder_hints,
                sync_timeline_track_drop_hints,
            )
                .chain(),
        );
        let left = app
            .world_mut()
            .spawn((
                EmitterTrackHeader { emitter },
                TimelineTrackHeader { track },
                RelativeCursorPosition {
                    cursor_over: true,
                    normalized: Some(Vec2::new(0.0, -0.25)),
                },
                Node::default(),
                BorderColor::all(theme::BORDER),
            ))
            .id();
        let right = app
            .world_mut()
            .spawn((
                TimelineTrackDropRow { track },
                RelativeCursorPosition::default(),
                Node::default(),
                BorderColor::all(theme::BORDER),
            ))
            .id();

        app.update();

        assert_eq!(
            app.world()
                .resource::<TimelineState>()
                .effect_drop_insertion,
            Some((track, true))
        );
        assert_eq!(
            app.world().get::<Node>(left).unwrap().border.top,
            Val::Px(0.0)
        );
        assert_eq!(
            app.world().get::<Node>(right).unwrap().border.top,
            Val::Px(0.0)
        );
        assert_ne!(
            app.world().get::<BorderColor>(left).unwrap().top,
            theme::DOCK_TARGET
        );
        assert_ne!(
            app.world().get::<BorderColor>(right).unwrap().top,
            theme::DOCK_TARGET
        );
    }

    #[test]
    fn asset_drop_insertion_stays_stable_while_crossing_the_open_gap() {
        let track = ChoreographyTrackId::Emitter(EmitterId::new());
        let state = TimelineState {
            effect_drop_preview: Some(EffectDropPreview {
                source_duration: 1.0,
                display_name: "Reusable effect".into(),
            }),
            effect_drop_insertion: Some((track, true)),
            ..default()
        };
        let mut app = App::new();
        app.insert_resource(state)
            .add_systems(Update, update_effect_drop_insertion);
        let spacer = app
            .world_mut()
            .spawn((
                TimelineEffectDropSpacer,
                RelativeCursorPosition {
                    cursor_over: true,
                    normalized: Some(Vec2::ZERO),
                },
            ))
            .id();

        app.update();

        assert_eq!(
            app.world()
                .resource::<TimelineState>()
                .effect_drop_insertion,
            Some((track, true))
        );

        app.world_mut()
            .get_mut::<RelativeCursorPosition>(spacer)
            .unwrap()
            .cursor_over = false;
        app.update();

        assert_eq!(
            app.world()
                .resource::<TimelineState>()
                .effect_drop_insertion,
            None
        );
    }

    #[test]
    fn effect_clip_creation_selection_and_undo_preserve_semantic_identity() {
        let mut session = test_support::session_with_timing_slack();
        let original_clips = session.effect.effect_clips.clone();
        let fallback_emitter = session.effect.emitters[0].id;
        let source = EffectAssetRef::new(aestra_core::EffectId::from_u128(0xfeed));
        let clip = EffectClip::new(source, 0.4, 0.8);
        let id = clip.id;

        assert!(session.execute(
            "Added effect clip",
            EffectCommand::AddEffectClip { clip, index: 0 },
            true,
        ));
        assert!(session.select_effect_clip(id));
        assert_eq!(session.selection.primary, SemanticTarget::EffectClip(id));
        assert_eq!(session.selected_layer().unwrap().id, fallback_emitter);
        assert!(!select_choreography_target(&mut session, None));
        assert_eq!(session.effect.emitters[0].id, fallback_emitter);

        session.undo();
        assert_eq!(session.effect.effect_clips, original_clips);
        assert_eq!(
            session.selection.primary,
            SemanticTarget::Emitter(fallback_emitter)
        );
        session.redo();
        let restored = session
            .effect
            .effect_clips
            .iter()
            .find(|clip| clip.id == id)
            .unwrap();
        assert_eq!(restored.source, source);
    }

    #[test]
    fn double_click_opens_effect_clip_source_without_hijacking_trim_handles() {
        let clip = EffectClipId::new();
        let selection = ChoreographyAction::SelectEffectClip(clip);
        let move_control = TimelineEffectClipInteraction {
            clip,
            kind: TimelineDragKind::Move,
        };
        let trim_control = TimelineEffectClipInteraction {
            clip,
            kind: TimelineDragKind::TrimStart,
        };

        assert_eq!(
            effect_clip_click_action(1, &move_control, &selection),
            selection
        );
        assert_eq!(
            effect_clip_click_action(2, &move_control, &selection),
            ChoreographyAction::EditEffectClipSource(clip)
        );
        assert_eq!(
            effect_clip_click_action(2, &trim_control, &selection),
            selection
        );
    }

    #[test]
    fn effect_clip_track_state_toggles_and_delete_are_coherent() {
        let mut session = test_support::session_with_timing_slack();
        let source = EffectAssetRef::new(aestra_core::EffectId::from_u128(0xfeed));
        let clip = EffectClip::new(source, 0.2, 0.7);
        let id = clip.id;
        assert!(session.execute(
            "Added effect clip",
            EffectCommand::AddEffectClip { clip, index: 0 },
            true,
        ));
        let mut app = choreography_app(session);

        app.world_mut()
            .commands()
            .trigger(ChoreographyAction::ToggleEffectClipExpanded(
                EffectClipPath::root_path(id),
            ));
        app.update();
        assert!(
            app.world()
                .resource::<TimelineState>()
                .expanded_effect_clips
                .contains(&EffectClipPath::root_path(id))
        );

        app.world_mut()
            .commands()
            .trigger(ChoreographyAction::ToggleEffectClipMuted(id));
        app.world_mut()
            .commands()
            .trigger(ChoreographyAction::ToggleEffectClipSolo(id));
        app.update();
        let state = app.world().resource::<TimelineState>();
        assert!(state.muted_effect_clips.contains(&id));
        assert_eq!(state.solo_effect_clip, Some(id));

        app.world_mut()
            .commands()
            .trigger(ChoreographyAction::DeleteEffectClip(id));
        app.update();
        let state = app.world().resource::<TimelineState>();
        assert!(
            !state
                .expanded_effect_clips
                .contains(&EffectClipPath::root_path(id))
        );
        assert!(!state.muted_effect_clips.contains(&id));
        assert_eq!(state.solo_effect_clip, None);
        assert!(
            !app.world()
                .resource::<EditorSession>()
                .effect
                .effect_clips
                .iter()
                .any(|clip| clip.id == id)
        );
    }

    #[test]
    fn effect_clip_drag_maps_parent_and_source_windows_with_bounds() {
        let session = test_support::session_with_timing_slack();
        let clip = EffectClipId::new();
        let view = TimelineView {
            start: 0.0,
            end: session.playback_duration(),
        };
        let mut guide = None;
        let base = EffectClipTimelineDrag {
            clip,
            kind: TimelineDragKind::Move,
            pointer_start: 0.0,
            original_start: 0.4,
            original_source_offset: 0.2,
            original_duration: 0.8,
            current_start: 0.4,
            current_source_offset: 0.2,
            current_duration: 0.8,
            source_duration: 1.0,
            source_looping: false,
        };

        let mut moved = base;
        update_effect_clip_timeline_drag(
            &mut moved,
            0.3,
            &session,
            TimelineSnapMode::None,
            view,
            1_000.0,
            &mut guide,
        );
        assert!((moved.current_start - 0.7).abs() < 0.000_1);
        assert!((moved.current_source_offset - 0.2).abs() < 0.000_1);
        assert!((moved.current_duration - 0.8).abs() < 0.000_1);

        let mut trimmed_start = EffectClipTimelineDrag {
            kind: TimelineDragKind::TrimStart,
            ..base
        };
        update_effect_clip_timeline_drag(
            &mut trimmed_start,
            0.25,
            &session,
            TimelineSnapMode::None,
            view,
            1_000.0,
            &mut guide,
        );
        assert!((trimmed_start.current_start - 0.65).abs() < 0.000_1);
        assert!((trimmed_start.current_source_offset - 0.45).abs() < 0.000_1);
        assert!((trimmed_start.current_duration - 0.55).abs() < 0.000_1);

        let mut expanded_start = EffectClipTimelineDrag {
            kind: TimelineDragKind::TrimStart,
            ..base
        };
        update_effect_clip_timeline_drag(
            &mut expanded_start,
            -1.0,
            &session,
            TimelineSnapMode::None,
            view,
            1_000.0,
            &mut guide,
        );
        assert!((expanded_start.current_start - 0.2).abs() < 0.000_1);
        assert!(expanded_start.current_source_offset.abs() < 0.000_1);
        assert!((expanded_start.current_duration - 1.0).abs() < 0.000_1);

        let mut expanded_looping_start = EffectClipTimelineDrag {
            kind: TimelineDragKind::TrimStart,
            original_start: 0.4,
            original_source_offset: 0.0,
            original_duration: 0.8,
            current_start: 0.4,
            current_source_offset: 0.0,
            current_duration: 0.8,
            source_duration: 1.0,
            source_looping: true,
            ..base
        };
        update_effect_clip_timeline_drag(
            &mut expanded_looping_start,
            -0.3,
            &session,
            TimelineSnapMode::None,
            view,
            1_000.0,
            &mut guide,
        );
        assert!((expanded_looping_start.current_start - 0.1).abs() < 0.000_1);
        assert!((expanded_looping_start.current_source_offset - 0.7).abs() < 0.000_1);
        assert!((expanded_looping_start.current_duration - 1.1).abs() < 0.000_1);

        let mut trimmed_end = EffectClipTimelineDrag {
            kind: TimelineDragKind::TrimEnd,
            original_duration: 0.5,
            current_duration: 0.5,
            ..base
        };
        update_effect_clip_timeline_drag(
            &mut trimmed_end,
            2.0,
            &session,
            TimelineSnapMode::None,
            view,
            1_000.0,
            &mut guide,
        );
        assert!((trimmed_end.current_duration - 0.8).abs() < 0.000_1);
    }

    #[test]
    fn effect_clip_drag_commit_is_one_undoable_timing_command() {
        let mut session = test_support::session_with_timing_slack();
        let source = EffectAssetRef::new(aestra_core::EffectId::from_u128(0xbeef));
        let mut clip = EffectClip::new(source, 0.4, 0.8);
        clip.source_offset = 0.2;
        let id = clip.id;
        assert!(session.execute(
            "Added effect clip",
            EffectCommand::AddEffectClip { clip, index: 0 },
            true,
        ));
        let drag = EffectClipTimelineDrag {
            clip: id,
            kind: TimelineDragKind::TrimStart,
            pointer_start: 0.0,
            original_start: 0.4,
            original_source_offset: 0.2,
            original_duration: 0.8,
            current_start: 0.6,
            current_source_offset: 0.4,
            current_duration: 0.6,
            source_duration: 1.2,
            source_looping: false,
        };
        let localizer = Localizer::new("en-US").unwrap();
        let ui_revision = session.ui_revision;

        commit_effect_clip_timeline_drag(&mut session, drag, &localizer);
        let clip = &session.effect.effect_clips[0];
        assert!((clip.start_time - 0.6).abs() < 0.000_1);
        assert!((clip.source_offset - 0.4).abs() < 0.000_1);
        assert!((clip.duration - 0.6).abs() < 0.000_1);
        assert_eq!(session.ui_revision, ui_revision);

        session.undo();
        let clip = &session.effect.effect_clips[0];
        assert!((clip.start_time - 0.4).abs() < 0.000_1);
        assert!((clip.source_offset - 0.2).abs() < 0.000_1);
        assert!((clip.duration - 0.8).abs() < 0.000_1);
    }

    #[test]
    fn trim_handles_only_appear_for_real_boundaries_inside_the_view() {
        let view = TimelineView {
            start: 2.0,
            end: 2.1,
        };
        assert!(!timeline_boundary_is_visible(0.0, view));
        assert!(timeline_boundary_is_visible(2.0, view));
        assert!(timeline_boundary_is_visible(2.05, view));
        assert!(timeline_boundary_is_visible(2.1, view));
        assert!(!timeline_boundary_is_visible(2.8, view));
    }

    #[test]
    fn timeline_drag_converts_screen_pixels_to_logical_ui_units() {
        assert_eq!(screen_distance_to_logical(24.0, 1.0), 24.0);
        assert_eq!(screen_distance_to_logical(24.0, 1.5), 16.0);
        assert_eq!(screen_distance_to_logical(-30.0, 2.0), -15.0);
    }

    #[test]
    fn emitter_reorder_uses_the_target_half_as_an_insertion_edge() {
        assert_eq!(track_reorder_index(0, 2, true, 4), Some(1));
        assert_eq!(track_reorder_index(0, 2, false, 4), Some(2));
        assert_eq!(track_reorder_index(3, 1, true, 4), Some(1));
        assert_eq!(track_reorder_index(3, 1, false, 4), Some(2));
        assert_eq!(track_reorder_index(0, 1, true, 4), None);
        assert_eq!(track_reorder_index(1, 0, false, 4), None);
        assert_eq!(track_reorder_index(2, 2, true, 4), None);
    }

    #[test]
    fn emitter_reorder_is_one_undoable_stable_id_edit() {
        let mut session = test_support::session_with_timing_slack();
        let original = session
            .effect
            .emitters
            .iter()
            .map(|emitter| emitter.id)
            .collect::<Vec<_>>();
        let moved = original[0];
        let automatic_colors = original
            .iter()
            .map(|id| (*id, layer_color(*id, None)))
            .collect::<Vec<_>>();
        session.select_emitter(moved);

        assert!(session.execute(
            "Reordered emitter tracks",
            EffectCommand::MoveEmitter {
                id: moved,
                index: 2,
            },
            true,
        ));
        assert_eq!(session.effect.emitters[2].id, moved);
        assert_eq!(session.selected_layer().unwrap().id, moved);
        for emitter in &session.effect.emitters {
            let original_color = automatic_colors
                .iter()
                .find_map(|(id, color)| (*id == emitter.id).then_some(*color))
                .unwrap();
            assert_eq!(layer_color(emitter.id, None), original_color);
        }

        session.undo();
        assert_eq!(
            session
                .effect
                .emitters
                .iter()
                .map(|emitter| emitter.id)
                .collect::<Vec<_>>(),
            original
        );
        session.redo();
        assert_eq!(session.effect.emitters[2].id, moved);
    }

    #[test]
    fn effect_clip_reorder_is_one_undoable_stable_id_edit() {
        let mut session = test_support::session_with_timing_slack();
        let first = EffectClip::new(aestra_core::EffectId::from_u128(0xC11D), 0.0, 0.8);
        let first_id = first.id;
        let second = EffectClip::new(aestra_core::EffectId::from_u128(0xC11E), 0.8, 0.8);
        let second_id = second.id;
        session.effect.effect_clips = vec![first, second];
        session.selection.select_effect_clip(first_id);

        assert!(session.execute(
            "Reordered effect clips",
            EffectCommand::MoveEffectClip {
                id: first_id,
                index: 1,
            },
            true,
        ));
        assert_eq!(session.effect.effect_clips[0].id, second_id);
        assert_eq!(session.effect.effect_clips[1].id, first_id);
        assert_eq!(
            session.selection.primary,
            SemanticTarget::EffectClip(first_id)
        );

        session.undo();
        assert_eq!(session.effect.effect_clips[0].id, first_id);
        assert_eq!(session.effect.effect_clips[1].id, second_id);
    }

    #[test]
    fn effect_clip_can_reorder_across_local_emitter_tracks() {
        let mut session = test_support::session_with_timing_slack();
        session.effect.effect_clips.clear();
        session.effect.choreography_order.clear();
        let clip = EffectClip::new(aestra_core::EffectId::from_u128(0xC11D), 0.0, 0.8);
        let clip_id = clip.id;
        let emitter_id = session.effect.emitters[0].id;
        session.effect.effect_clips.push(clip);
        assert_eq!(
            normalized_choreography_order(&session.effect)[0],
            ChoreographyTrackId::EffectClip(clip_id)
        );
        let mut order = normalized_choreography_order(&session.effect);
        let clip_index = order
            .iter()
            .position(|track| *track == ChoreographyTrackId::EffectClip(clip_id))
            .unwrap();
        let clip_track = order.remove(clip_index);
        let emitter_index = order
            .iter()
            .position(|track| *track == ChoreographyTrackId::Emitter(emitter_id))
            .unwrap();
        order.insert(emitter_index + 1, clip_track);

        assert!(session.execute(
            "Reordered effect clips",
            EffectCommand::SetChoreographyOrder { order },
            true,
        ));
        let reordered = normalized_choreography_order(&session.effect);
        assert_eq!(reordered[0], ChoreographyTrackId::Emitter(emitter_id));
        assert_eq!(reordered[1], ChoreographyTrackId::EffectClip(clip_id));

        session.undo();
        assert_eq!(
            normalized_choreography_order(&session.effect)[0],
            ChoreographyTrackId::EffectClip(clip_id)
        );
    }

    #[test]
    fn timeline_wheel_routes_scroll_pan_and_zoom_like_a_track_editor() {
        assert_eq!(
            timeline_wheel_intent(Vec2::new(0.0, -1.0), -21.0, false, false),
            TimelineWheelIntent::ScrollTracks(-21.0)
        );
        assert_eq!(
            timeline_wheel_intent(Vec2::new(0.0, -1.0), -21.0, false, true),
            TimelineWheelIntent::PanTime(-1.0)
        );
        assert_eq!(
            timeline_wheel_intent(Vec2::new(0.0, -1.0), -21.0, true, false),
            TimelineWheelIntent::ZoomTime(-1.0)
        );
        assert_eq!(
            timeline_wheel_intent(Vec2::new(2.0, 0.25), 5.25, false, false),
            TimelineWheelIntent::PanTime(2.0)
        );
    }

    #[test]
    fn synchronized_vertical_scroll_prefers_tracks_and_clamps_to_overflow() {
        assert_eq!(resolved_vertical_scroll(12.0, None, None, 90.0), 12.0);
        assert_eq!(resolved_vertical_scroll(12.0, Some(28.0), None, 90.0), 28.0);
        assert_eq!(
            resolved_vertical_scroll(12.0, Some(28.0), Some(44.0), 90.0),
            44.0
        );
        assert_eq!(
            resolved_vertical_scroll(12.0, None, Some(144.0), 90.0),
            90.0
        );
    }

    #[test]
    fn timeline_ruler_uses_human_readable_intervals() {
        assert_eq!(nice_timeline_step(10.0, 1_000.0), 1.0);
        assert_eq!(nice_timeline_step(2.8, 1_000.0), 0.5);
        assert_eq!(nice_timeline_step(0.2, 1_000.0), 0.02);
    }

    #[test]
    fn timeline_timing_commit_is_one_undoable_command() {
        let mut session = session_with_first_emitter_timing_slack();
        let emitter = session.effect.emitters[0].clone();
        let ui_revision = session.ui_revision;

        let transaction = session
            .emitter_region_timing_transaction(
                emitter.id,
                emitter.implicit_region_id(),
                emitter.start_time + 0.1,
                0.0,
                emitter.duration,
                "Moved emitter on timeline",
            )
            .unwrap();
        assert!(session.execute_transaction(transaction, false));
        assert!(session.can_undo());
        assert_eq!(session.ui_revision, ui_revision);
        session.undo();

        let restored = session
            .effect
            .emitters
            .iter()
            .find(|candidate| candidate.id == emitter.id)
            .unwrap();
        assert_eq!(restored.start_time, emitter.start_time);
        assert_eq!(restored.duration, emitter.duration);
    }

    #[test]
    fn moving_one_selected_region_previews_and_commits_the_whole_group() {
        let (mut session, emitter_id, [first, second, third]) = session_with_three_regions();
        let emitter = session
            .effect
            .emitters
            .iter()
            .find(|emitter| emitter.id == emitter_id)
            .unwrap()
            .clone();
        let original = emitter.timeline_regions();
        let first_region = emitter.timeline_region(first).unwrap();
        let third_region = emitter.timeline_region(third).unwrap();
        let drag_regions = vec![
            TimelineRegionMove {
                region: first,
                original_start: first_region.start_time,
                duration: first_region.duration,
            },
            TimelineRegionMove {
                region: third,
                original_start: third_region.start_time,
                duration: third_region.duration,
            },
        ];
        let mut drag = TimelineDrag {
            emitter: emitter_id,
            region: first,
            kind: TimelineDragKind::Move,
            pointer_start: 0.0,
            original_start: first_region.start_time,
            original_duration: first_region.duration,
            original_source_offset: first_region.source_offset,
            current_start: first_region.start_time,
            current_duration: first_region.duration,
            current_source_offset: first_region.source_offset,
            source_duration: emitter.duration,
        };
        let requested_delta = 0.2_f32.min(
            (session.effect.duration - third_region.end_time())
                .max(0.0)
                .max(0.05),
        );
        let view = TimelineView {
            start: 0.0,
            end: session.effect.duration,
        };
        update_timeline_drag(
            &mut drag,
            &drag_regions,
            requested_delta,
            &session,
            TimelineSnapMode::None,
            view,
            1_000.0,
            &mut None,
        );
        let delta = drag.current_start - drag.original_start;
        assert!(delta > 0.0);
        let state = TimelineState {
            drag: Some(drag),
            drag_regions: drag_regions.clone(),
            ..TimelineState::framed(session.effect.duration)
        };
        let preview = timeline_region_preview_timing(&state, emitter_id, third_region);
        assert!((preview.0 - (third_region.start_time + delta)).abs() <= 0.000_1);

        commit_timeline_drag(&mut session, drag, &drag_regions);
        let moved = session
            .effect
            .emitters
            .iter()
            .find(|emitter| emitter.id == emitter_id)
            .unwrap();
        assert!(
            (moved.timeline_region(first).unwrap().start_time - (first_region.start_time + delta))
                .abs()
                <= 0.000_1
        );
        assert!(
            (moved.timeline_region(third).unwrap().start_time - (third_region.start_time + delta))
                .abs()
                <= 0.000_1
        );
        assert_eq!(
            moved.timeline_region(second).unwrap().start_time,
            emitter.timeline_region(second).unwrap().start_time
        );
        session.undo();
        assert_eq!(session.effect.emitters[0].timeline_regions(), original);
    }

    #[test]
    fn trimming_an_emitter_region_can_grow_its_source_to_the_effect_end() {
        let mut session = session_with_first_emitter_timing_slack();
        let emitter = session.effect.emitters[0].clone();
        let region = emitter.timeline_regions()[0];
        let available = session.effect.duration - region.start_time;
        assert!(available > region.duration);
        let mut drag = TimelineDrag {
            emitter: emitter.id,
            region: region.id,
            kind: TimelineDragKind::TrimEnd,
            pointer_start: 0.0,
            original_start: region.start_time,
            original_duration: region.duration,
            original_source_offset: region.source_offset,
            current_start: region.start_time,
            current_duration: region.duration,
            current_source_offset: region.source_offset,
            source_duration: emitter.duration,
        };
        let requested_growth = (available - region.duration).min(0.4);
        update_timeline_drag(
            &mut drag,
            &[],
            requested_growth,
            &session,
            TimelineSnapMode::None,
            TimelineView {
                start: 0.0,
                end: session.effect.duration,
            },
            1_000.0,
            &mut None,
        );
        assert!(drag.current_duration > emitter.duration);

        commit_timeline_drag(&mut session, drag, &[]);
        let grown = session
            .effect
            .emitters
            .iter()
            .find(|candidate| candidate.id == emitter.id)
            .unwrap();
        assert!((grown.duration - drag.current_duration).abs() <= 0.000_1);
        assert!((grown.timeline_regions()[0].duration - drag.current_duration).abs() <= 0.000_1);
        session.undo();
        assert_eq!(session.effect.emitters[0], emitter);
    }

    #[test]
    fn trimming_an_emitter_region_left_can_grow_before_source_zero() {
        let mut session = session_with_first_emitter_timing_slack();
        let emitter = session.effect.emitters[0].clone();
        let region = emitter.timeline_regions()[0];
        assert!(region.start_time > 0.0);
        let original_end = region.end_time();
        let growth = region.start_time.min(0.2);
        let mut drag = TimelineDrag {
            emitter: emitter.id,
            region: region.id,
            kind: TimelineDragKind::TrimStart,
            pointer_start: 0.0,
            original_start: region.start_time,
            original_duration: region.duration,
            original_source_offset: region.source_offset,
            current_start: region.start_time,
            current_duration: region.duration,
            current_source_offset: region.source_offset,
            source_duration: emitter.duration,
        };
        update_timeline_drag(
            &mut drag,
            &[],
            -growth,
            &session,
            TimelineSnapMode::None,
            TimelineView {
                start: 0.0,
                end: session.effect.duration,
            },
            1_000.0,
            &mut None,
        );
        assert!(drag.current_start < region.start_time);
        assert_eq!(drag.current_source_offset, 0.0);
        assert!((drag.current_start + drag.current_duration - original_end).abs() <= 0.000_1);

        commit_timeline_drag(&mut session, drag, &[]);
        let grown = session
            .effect
            .emitters
            .iter()
            .find(|candidate| candidate.id == emitter.id)
            .unwrap();
        let grown_region = grown.timeline_regions()[0];
        assert!((grown_region.start_time - drag.current_start).abs() <= 0.000_1);
        assert!((grown_region.duration - drag.current_duration).abs() <= 0.000_1);
        assert!((grown_region.end_time() - original_end).abs() <= 0.000_1);
        session.undo();
        assert_eq!(session.effect.emitters[0], emitter);
    }

    #[test]
    fn timeline_trim_creates_a_lossless_emitter_region_and_undo_restores_legacy_timing() {
        let mut session = test_support::session_with_timing_slack();
        let emitter = session.effect.emitters[0].clone();
        let trim = 0.2_f32.min(emitter.duration * 0.25);
        let drag = TimelineDrag {
            emitter: emitter.id,
            region: emitter.implicit_region_id(),
            kind: TimelineDragKind::TrimStart,
            pointer_start: 0.0,
            original_start: emitter.start_time,
            original_duration: emitter.duration,
            original_source_offset: 0.0,
            current_start: emitter.start_time + trim,
            current_duration: emitter.duration - trim,
            current_source_offset: trim,
            source_duration: emitter.duration,
        };

        commit_timeline_drag(&mut session, drag, &[]);

        let trimmed = session
            .effect
            .emitters
            .iter()
            .find(|candidate| candidate.id == emitter.id)
            .unwrap();
        assert_eq!(trimmed.duration, emitter.duration);
        assert_eq!(trimmed.regions.len(), 1);
        assert!((trimmed.regions[0].source_offset - trim).abs() < 0.000_1);
        assert!((trimmed.regions[0].duration - (emitter.duration - trim)).abs() < 0.000_1);

        session.undo();
        assert!(session.selected_layer().unwrap().regions.is_empty());
    }

    #[test]
    fn timeline_visual_queries_initialize_without_aliasing() {
        let session = test_support::session_with_timing_slack();
        let timeline = TimelineState::framed(session.playback_duration());
        let mut app = App::new();
        app.insert_resource(session);
        app.insert_resource(timeline);
        app.add_systems(
            Update,
            (update_timeline_visuals, sync_timeline_horizontal_scroll).chain(),
        );

        app.update();
    }

    #[test]
    fn emitter_reveal_waits_for_layout_then_focuses_the_timeline_row() {
        let emitter = EmitterId::new();
        let mut state = TimelineState::framed(1.0);
        state.reveal_emitter(emitter);
        let mut app = App::new();
        app.insert_resource(state)
            .init_resource::<InputFocus>()
            .init_resource::<InputFocusVisible>()
            .add_systems(Update, reveal_timeline_emitter);
        let list = app
            .world_mut()
            .spawn((TimelineVerticalPane::Headers, ListBox))
            .id();
        let row = app
            .world_mut()
            .spawn((EmitterTrackHeader { emitter }, ChildOf(list)))
            .id();

        app.update();
        assert_eq!(
            app.world().resource::<TimelineState>().reveal_emitter,
            Some(emitter)
        );

        app.update();

        assert_eq!(app.world().resource::<TimelineState>().reveal_emitter, None);
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(list));
        assert!(!app.world().resource::<InputFocusVisible>().0);
        assert_eq!(
            app.world().get::<ActiveDescendant>(list).unwrap().0,
            Some(row)
        );
    }

    #[test]
    fn vertical_scroll_panes_measure_an_explicit_grid_content_extent() {
        let session = test_support::session_with_timing_slack();
        let duration = session.playback_duration();
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<SvgFile>()
        .insert_resource(session)
        .insert_resource(TimelineState::framed(duration))
        .init_resource::<ProjectEffectCatalog>()
        .init_resource::<EditorModuleRegistry>()
        .init_resource::<CurvesState>()
        .insert_resource(Localizer::new("en-US").unwrap())
        .add_systems(Startup, spawn_test_timeline);

        app.update();

        let contents = {
            let world = app.world_mut();
            let mut query = world.query::<(&TimelineVerticalContent, &Node)>();
            query
                .iter(world)
                .map(|(_, node)| (node.min_height, node.flex_shrink))
                .collect::<Vec<_>>()
        };
        assert_eq!(contents.len(), 2);
        assert!(contents.iter().all(|(height, shrink)| {
            matches!(height, Val::Px(value) if *value > 0.0) && *shrink == 0.0
        }));
    }

    #[test]
    fn recursive_clip_expansion_drives_rows_selection_and_vertical_overflow() {
        let temporary = tempfile::tempdir().unwrap();

        let mut leaf = EffectAsset::new("Leaf", 1.0);
        let leaf_emitter = Emitter::basic_sprite("Leaf emitter", 1.0);
        let leaf_emitter_id = leaf_emitter.id;
        leaf.emitters.push(leaf_emitter);
        leaf.save_ron(temporary.path().join("leaf.aestra.ron"))
            .unwrap();

        let mut child = EffectAsset::new("Child", 1.5);
        child
            .emitters
            .push(Emitter::basic_sprite("Child emitter", 1.5));
        let nested_clip = EffectClip::new(leaf.id, 0.2, 1.0);
        let nested_clip_id = nested_clip.id;
        child.effect_clips.push(nested_clip);
        child.choreography_order = vec![
            ChoreographyTrackId::EffectClip(nested_clip_id),
            ChoreographyTrackId::Emitter(child.emitters[0].id),
        ];
        child
            .save_ron(temporary.path().join("child.aestra.ron"))
            .unwrap();

        let catalog = ProjectEffectCatalog::scan(temporary.path());
        let mut session = test_support::session_with_timing_slack();
        session.effect.effect_clips.clear();
        session.effect.choreography_order.clear();
        let root_clip = EffectClip::new(child.id, 0.0, 1.5);
        let root_clip_id = root_clip.id;
        session.effect.effect_clips.push(root_clip.clone());
        session.effect.choreography_order = vec![ChoreographyTrackId::EffectClip(root_clip_id)];
        session.effect.choreography_order.extend(
            session
                .effect
                .emitters
                .iter()
                .map(|emitter| ChoreographyTrackId::Emitter(emitter.id)),
        );

        let root_path = EffectClipPath::root_path(root_clip_id);
        let nested_path = root_path.child(nested_clip_id);
        let mut state = TimelineState::framed(session.playback_duration());
        state.expanded_effect_clips.insert(root_path.clone());
        state.expanded_effect_clips.insert(nested_path.clone());
        let projections = referenced_track_projections(&catalog, &state, &root_clip);
        assert_eq!(projections.len(), 3);
        assert_eq!(projections[0].path, nested_path);
        assert_eq!(projections[0].depth, 1);
        assert_eq!(projections[1].path, nested_path);
        assert_eq!(projections[1].depth, 2);
        assert!(matches!(
            &projections[1].kind,
            ReferencedTrackKind::Emitter { emitter, .. } if emitter.id == leaf_emitter_id
        ));

        let expected_height = timeline_vertical_content_height(&session.effect, &state, &catalog);
        assert!(expected_height > 180.0);
        assert!(crate::feathers::scroll::scrollbar_needed(
            180.0,
            expected_height
        ));

        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<SvgFile>()
        .insert_resource(session)
        .insert_resource(state)
        .insert_resource(catalog)
        .init_resource::<EditorModuleRegistry>()
        .init_resource::<CurvesState>()
        .insert_resource(Localizer::new("en-US").unwrap())
        .add_systems(Startup, spawn_test_timeline);
        app.update();

        let actions = {
            let world = app.world_mut();
            let mut query =
                world.query_filtered::<&ChoreographyAction, With<ReferencedEmitterTrackHeader>>();
            query.iter(world).cloned().collect::<Vec<_>>()
        };
        assert!(
            actions.contains(&ChoreographyAction::SelectReferencedEffectClip(
                nested_path.clone()
            ))
        );
        assert!(
            actions.contains(&ChoreographyAction::SelectEffectClipEmitter {
                path: nested_path,
                emitter: leaf_emitter_id,
            })
        );
        let content_heights = {
            let world = app.world_mut();
            let mut query = world.query_filtered::<&Node, With<TimelineVerticalContent>>();
            query
                .iter(world)
                .map(|node| node.min_height)
                .collect::<Vec<_>>()
        };
        assert_eq!(content_heights.len(), 2);
        assert!(content_heights.iter().all(
            |height| matches!(height, Val::Px(value) if (*value - expected_height).abs() < 0.001)
        ));
    }

    #[test]
    fn rejected_project_effect_drop_is_feedback_only() {
        let session = test_support::session_with_timing_slack();
        let original = session.effect.clone();
        let original_revision = session.ui_revision;
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_observer(reject_project_effect_drop);
        let feedback = app
            .world_mut()
            .spawn((TimelineInvalidDropFeedback::default(), Node::default()))
            .id();

        app.world_mut().trigger(RejectProjectEffectDrop {
            reason: "reference cycle".into(),
        });
        app.update();

        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect, original);
        assert_eq!(session.ui_revision, original_revision);
        assert!(!session.dirty);
        assert!(!session.can_undo());
        assert!(session.status.starts_with("Effect clip was not added:"));
        assert!(session.status.contains("reference cycle"));
        let feedback_state = app
            .world()
            .get::<TimelineInvalidDropFeedback>(feedback)
            .unwrap();
        assert!(feedback_state.rejected);
        assert!(!feedback_state.timer.is_paused());
        assert_eq!(
            app.world().get::<Node>(feedback).unwrap().display,
            Display::Flex
        );
    }

    #[test]
    fn timeline_actions_own_duration_snap_and_framing() {
        let session = test_support::session_with_timing_slack();
        let initial_duration = session.effect.duration;
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(TimelineState {
                view: TimelineView {
                    start: 0.5,
                    end: 1.0,
                },
                ..default()
            })
            .add_observer(execute_timeline_action);

        app.world_mut()
            .trigger(TimelineAction::AdjustEffectDuration(0.25));
        app.update();
        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect.duration, initial_duration + 0.25);
        assert!(session.can_undo());

        app.world_mut()
            .trigger(TimelineAction::SetSnap(TimelineSnapMode::None));
        app.update();
        assert_eq!(
            app.world().resource::<TimelineState>().snap,
            TimelineSnapMode::None
        );

        app.world_mut().trigger(TimelineAction::FrameAll);
        app.update();
        let state = app.world().resource::<TimelineState>();
        assert_eq!(state.view.start, 0.0);
        assert_eq!(state.view.end, initial_duration + 0.25);
    }

    #[test]
    fn add_marker_action_creates_and_selects_one_undoable_marker() {
        let mut session = test_support::session_with_timing_slack();
        let initial_count = session.effect.markers.len();
        session.seek_time(0.75);
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(TimelineState::framed(2.8))
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_observer(execute_timeline_action);

        app.world_mut().trigger(TimelineAction::AddMarker);
        app.update();

        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect.markers.len(), initial_count + 1);
        let marker = session.effect.markers.last().unwrap();
        assert_eq!(marker.name, format!("Marker {}", initial_count + 1));
        assert!((marker.time - 0.75).abs() < 0.000_1);
        assert_eq!(session.selection.primary, SemanticTarget::Marker(marker.id));
        assert!(session.can_undo());
    }

    #[test]
    fn add_choreography_event_action_creates_and_selects_one_undoable_event() {
        let mut session = test_support::session_with_timing_slack();
        let initial_count = session.effect.choreography_events.len();
        session.seek_time(0.75);
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(TimelineState::framed(2.8))
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_observer(execute_timeline_action);

        app.world_mut()
            .trigger(TimelineAction::AddChoreographyEvent);
        app.update();

        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect.choreography_events.len(), initial_count + 1);
        let event = session.effect.choreography_events.last().unwrap();
        assert_eq!(event.name, format!("Event {}", initial_count + 1));
        assert!((event.time - 0.75).abs() < 0.000_1);
        assert!(matches!(
            event.payload,
            ChoreographyEventPayload::GameplayNotify { ref topic } if topic.is_empty()
        ));
        assert_eq!(
            session.selection.primary,
            SemanticTarget::ChoreographyEvent(event.id)
        );
        assert!(session.can_undo());
    }

    #[test]
    fn timeline_toolbar_uses_icons_for_framing_marker_and_event_tools() {
        let mut session = test_support::session_with_timing_slack();
        session.new_effect();
        let duration = session.playback_duration();
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<SvgFile>()
        .insert_resource(session)
        .insert_resource(TimelineState::framed(duration))
        .init_resource::<ProjectEffectCatalog>()
        .init_resource::<EditorModuleRegistry>()
        .init_resource::<CurvesState>()
        .insert_resource(Localizer::new("en-US").unwrap())
        .add_systems(Startup, spawn_test_timeline);
        app.update();

        let world = app.world_mut();
        let mut buttons = world.query::<(&TimelineAction, &Children)>();
        for expected in [
            TimelineAction::FrameAll,
            TimelineAction::AddMarker,
            TimelineAction::AddChoreographyEvent,
        ] {
            let children = buttons
                .iter(world)
                .find_map(|(action, children)| (*action == expected).then_some(children))
                .expect("timeline tool must exist");
            assert!(
                children
                    .iter()
                    .any(|child| world.get::<UiSvg>(child).is_some())
            );
            assert!(
                children
                    .iter()
                    .all(|child| world.get::<Text>(child).is_none())
            );
        }
    }

    #[test]
    fn delete_key_removes_the_selected_marker_or_choreography_event() {
        for delete_marker in [true, false] {
            let mut session = test_support::session_with_timing_slack();
            session.new_effect();
            let marker = EffectMarker::new("Delete me", 0.5);
            let marker_id = marker.id;
            let event = ChoreographyEvent::new(
                "Delete me",
                0.75,
                ChoreographyEventPayload::GameplayNotify {
                    topic: "delete".into(),
                },
            );
            let event_id = event.id;
            session.effect.markers.push(marker);
            session.effect.choreography_events.push(event);
            if delete_marker {
                session.select_marker(marker_id);
            } else {
                session.select_choreography_event(event_id);
            }

            let mut app = choreography_app(session);
            app.init_resource::<ModulePaletteState>()
                .init_resource::<ButtonInput<KeyCode>>()
                .add_observer(execute_timeline_action)
                .add_systems(Update, choreography_keyboard_input);
            app.world_mut().spawn(TimelineCanvas);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Delete);
            app.update();

            let session = app.world().resource::<EditorSession>();
            assert_eq!(session.effect.markers.len(), usize::from(!delete_marker));
            assert_eq!(
                session.effect.choreography_events.len(),
                usize::from(delete_marker)
            );
            assert!(session.can_undo());
        }
    }

    #[test]
    fn authored_names_and_display_color_are_projected_into_the_timeline() {
        let mut session = test_support::session_with_timing_slack();
        assert!(session.set_effect_name("Renamed Effect"));
        assert!(session.set_selected_emitter_name("Renamed Emitter"));
        session
            .effect
            .markers
            .push(EffectMarker::new("Impact", 0.75));
        let emitter = session.selected_layer().unwrap().id;
        let authored_color = [0.28, 0.78, 0.45, 1.0];
        assert!(session.set_emitter_display_color(emitter, Some(authored_color)));
        let duration = session.playback_duration();
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<SvgFile>()
        .insert_resource(session)
        .insert_resource(TimelineState::framed(duration))
        .init_resource::<ProjectEffectCatalog>()
        .init_resource::<EditorModuleRegistry>()
        .init_resource::<CurvesState>()
        .insert_resource(Localizer::new("en-US").unwrap())
        .add_systems(Startup, spawn_test_timeline)
        .add_systems(Update, audit_timeline_controls);

        app.update();

        let heading = {
            let world = app.world_mut();
            let mut query = world.query_filtered::<&Text, With<TimelineEffectHeading>>();
            query.single(world).unwrap().0.clone()
        };
        assert!(heading.contains("Renamed Effect"));
        assert!(heading.ends_with(" · CHOREOGRAPHY"));
        let renamed_track = {
            let world = app.world_mut();
            let mut labels = world.query_filtered::<&Text, With<TimelineTrackNameLabel>>();
            labels
                .iter(world)
                .find(|name| name.0 == "Renamed Emitter")
                .map(|name| name.0.clone())
        };
        assert_eq!(renamed_track.as_deref(), Some("Renamed Emitter"));
        let chip_color = {
            let world = app.world_mut();
            let mut query =
                world.query_filtered::<&BackgroundColor, With<EmitterTrackColorSwatch>>();
            query
                .iter(world)
                .find(|color| color.0 == Color::srgba(0.28, 0.78, 0.45, 1.0))
                .map(|color| color.0)
                .unwrap()
        };
        assert_eq!(chip_color, Color::srgba(0.28, 0.78, 0.45, 1.0));
        let handle_count = {
            let world = app.world_mut();
            let mut query = world.query::<&EmitterTrackReorderHandle>();
            query.iter(world).count()
        };
        assert_eq!(
            handle_count,
            app.world()
                .resource::<EditorSession>()
                .effect
                .emitters
                .len()
        );
    }

    #[test]
    fn track_context_menus_anchor_to_rows_without_consuming_header_space() {
        let session = test_support::session_with_timing_slack();
        let emitter = session.effect.emitters[0].id;
        let mut state = TimelineState::framed(session.playback_duration());
        state.context_emitter = Some(emitter);
        state.context_menu_position = Vec2::new(17.0, 23.0);
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<SvgFile>()
        .insert_resource(session)
        .insert_resource(state)
        .init_resource::<ProjectEffectCatalog>()
        .init_resource::<EditorModuleRegistry>()
        .init_resource::<CurvesState>()
        .insert_resource(Localizer::new("en-US").unwrap())
        .add_systems(Startup, spawn_test_timeline);

        app.update();

        let header = {
            let world = app.world_mut();
            let mut query = world.query::<(Entity, &EmitterTrackHeader)>();
            query
                .iter(world)
                .find_map(|(entity, header)| (header.emitter == emitter).then_some(entity))
                .unwrap()
        };
        let menu_layer = {
            let world = app.world_mut();
            let mut query = world.query_filtered::<(
                &ChildOf,
                &GlobalZIndex,
                Option<&OverrideClip>,
                &Popover,
                &BackgroundColor,
                &Node,
            ), With<EmitterTrackContextMenu>>();
            let (parent, z_index, override_clip, popover, background, node) =
                query.single(world).unwrap();
            let anchor = parent.parent();
            let anchor_parent = world.get::<ChildOf>(anchor).unwrap().parent();
            let anchor_node = world.get::<Node>(anchor).unwrap();
            (
                anchor_parent == header,
                anchor_node.left,
                anchor_node.top,
                z_index.0,
                override_clip.is_some(),
                popover.positions[0].side,
                popover.positions[0].align,
                background.0,
                node.row_gap,
            )
        };
        assert_eq!(
            menu_layer,
            (
                true,
                Val::Px(17.0),
                Val::Px(23.0),
                250,
                true,
                PopoverSide::Right,
                PopoverAlign::Start,
                theme::MENU,
                Val::Px(0.0),
            )
        );
    }

    #[test]
    fn timeline_color_picker_targets_the_track_and_commits_one_semantic_edit() {
        let session = test_support::session_with_timing_slack();
        let emitter = session.effect.emitters[2].id;
        let mut app = choreography_app(session);
        app.add_observer(handle_timeline_track_color_change);

        app.world_mut()
            .trigger(ChoreographyAction::ToggleEmitterColorPicker(emitter));
        app.update();
        assert_eq!(
            app.world().resource::<TimelineState>().color_picker_emitter,
            Some(emitter)
        );
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .selected_layer()
                .unwrap()
                .id,
            emitter
        );

        let picker = app
            .world_mut()
            .spawn(EmitterTrackColorPicker { emitter })
            .id();
        let swatch = app
            .world_mut()
            .spawn((
                EmitterTrackColorSwatch { emitter },
                BackgroundColor(Color::BLACK),
            ))
            .id();
        let color = [0.13, 0.62, 0.91, 1.0];
        app.world_mut().trigger(ValueChange {
            source: picker,
            value: Some(color),
            is_final: false,
        });
        app.update();
        assert_eq!(
            app.world().get::<BackgroundColor>(swatch).unwrap().0,
            Color::srgba(color[0], color[1], color[2], color[3])
        );
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .selected_layer()
                .unwrap()
                .display_color,
            None
        );
        assert!(!app.world().resource::<EditorSession>().can_undo());

        app.world_mut().trigger(ValueChange {
            source: picker,
            value: Some(color),
            is_final: true,
        });
        app.update();

        let mut session = app.world_mut().resource_mut::<EditorSession>();
        assert_eq!(session.selected_layer().unwrap().display_color, Some(color));
        assert!(session.can_undo());
        session.undo();
        assert_eq!(session.selected_layer().unwrap().display_color, None);
    }

    #[test]
    fn feathers_activation_queues_one_timeline_action() {
        let mut app = App::new();
        app.add_observer(queue_timeline_action_activation);
        let action = app
            .world_mut()
            .spawn((TimelineAction::FrameAll, FeathersActionButton))
            .id();

        app.world_mut().trigger(Activate { entity: action });
        app.update();

        let action = app.world().entity(action);
        assert!(action.contains::<PendingFeathersActivation>());
        assert!(action.contains::<bevy::ui::Pressed>());
    }

    #[test]
    fn automation_graph_preserves_source_time_during_region_trim_preview() {
        let session = test_support::session_with_timing_slack();
        let emitter = session.effect.emitters[0].id;
        let region = session.effect.emitters[0].implicit_region_id();
        let mut timeline = TimelineState::framed(session.playback_duration());
        let view = timeline.view;
        timeline.drag = Some(TimelineDrag {
            emitter,
            region,
            kind: TimelineDragKind::TrimEnd,
            pointer_start: 0.0,
            original_start: 0.0,
            original_duration: 1.0,
            original_source_offset: 0.0,
            current_start: 0.35,
            current_duration: 0.8,
            current_source_offset: 0.0,
            source_duration: 1.0,
        });
        let lane = AutomationLaneId {
            emitter,
            module: ModuleId::new(),
            input: 0,
            parameter: "test".into(),
            channel: None,
        };
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(timeline)
            .add_systems(Update, update_automation_lane_graph_visuals);
        let graph = app
            .world_mut()
            .spawn((TimelineAutomationLaneGraph(lane), Node::default()))
            .id();

        app.update();

        let node = app.world().get::<Node>(graph).unwrap();
        assert_eq!(node.left, Val::Percent(view.normalized_time(0.35) * 100.0));
        assert_eq!(node.width, Val::Percent(1.0 / view.span() * 100.0));
    }

    #[test]
    fn emitter_automation_disclosure_is_an_active_a_button() {
        let session = test_support::session_with_timing_slack();
        let target = session
            .effect
            .emitters
            .iter()
            .find(|emitter| automation_lane_count(emitter) > 0)
            .unwrap()
            .id;
        let mut timeline = TimelineState::framed(session.playback_duration());
        timeline.expanded_automation_emitters.insert(target);
        timeline.automation_menu_emitter = Some(target);
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<SvgFile>()
        .insert_resource(session)
        .insert_resource(timeline)
        .init_resource::<ProjectEffectCatalog>()
        .init_resource::<EditorModuleRegistry>()
        .init_resource::<CurvesState>()
        .insert_resource(Localizer::new("en-US").unwrap())
        .add_systems(Startup, spawn_test_timeline);

        app.update();

        let menu_count = {
            let world = app.world_mut();
            let mut query = world.query_filtered::<Entity, With<EmitterAutomationVisibilityMenu>>();
            query.iter(world).count()
        };
        assert_eq!(menu_count, 1);
        let visibility_actions = {
            let world = app.world_mut();
            let mut query = world.query::<&ChoreographyAction>();
            query
                .iter(world)
                .filter(|action| {
                    matches!(
                        action,
                        ChoreographyAction::SetEmitterAutomationVisibility { .. }
                            | ChoreographyAction::SetAutomationLaneVisibility { .. }
                    )
                })
                .count()
        };
        assert!(visibility_actions >= 3);

        let toggles = {
            let world = app.world_mut();
            let mut query =
                world.query::<(Entity, &ChoreographyAction, Has<Selected>, &ButtonVariant)>();
            query
                .iter(world)
                .filter_map(|(entity, action, selected, variant)| match action {
                    ChoreographyAction::ToggleEmitterAutomation(emitter) => {
                        Some((entity, *emitter, selected, variant.clone()))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert!(!toggles.is_empty());
        let target_toggle = toggles
            .iter()
            .find(|(_, emitter, _, _)| *emitter == target)
            .map(|(entity, _, _, _)| *entity)
            .unwrap();
        for (entity, emitter, selected, variant) in toggles {
            let label = app
                .world()
                .get::<Children>(entity)
                .and_then(|children| {
                    children
                        .iter()
                        .find_map(|child| app.world().get::<Text>(child))
                })
                .map(|text| text.0.as_str());
            assert_eq!(label, Some("A"));
            assert_eq!(selected, emitter == target);
            assert_eq!(variant == ButtonVariant::Primary, emitter == target);
        }
        let world = app.world();
        let parent = world.get::<ChildOf>(target_toggle).unwrap().parent();
        let children = world.get::<Children>(parent).unwrap();
        let solo_position = children
            .iter()
            .position(|child| {
                world.get::<ChoreographyAction>(child)
                    == Some(&ChoreographyAction::ToggleEmitterSolo(target))
            })
            .unwrap();
        let automation_position = children
            .iter()
            .position(|child| child == target_toggle)
            .unwrap();
        assert_eq!(automation_position, solo_position + 1);
    }

    #[test]
    fn track_headers_and_clips_expose_the_same_stable_selection_actions() {
        let mut session = test_support::session_with_timing_slack();
        session.effect.effect_clips.clear();
        session
            .effect
            .choreography_order
            .retain(|track| matches!(track, ChoreographyTrackId::Emitter(_)));
        let effect_clip = EffectClip::new(
            EffectAssetRef::new(aestra_core::EffectId::from_u128(0xcafe)),
            0.2,
            0.6,
        );
        let effect_clip_id = effect_clip.id;
        session.effect.effect_clips.push(effect_clip);
        session.effect.emitters[1].enabled = false;
        let solo = session.effect.emitters[0].id;
        assert!(session.toggle_preview_solo(solo));
        session.diagnostics.push(aestra_core::Diagnostic::error(
            aestra_core::DiagnosticCode::InvalidTiming,
            "effect.emitters[1].duration",
            "test diagnostic",
        ));
        let emitter_count = session.effect.emitters.len();
        let duration = session.playback_duration();
        let mut app = App::new();
        let mut timeline = TimelineState::framed(duration);
        timeline.vertical_scroll = 72.0;
        timeline.context_effect_clip = Some(effect_clip_id);
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<SvgFile>()
        .insert_resource(session)
        .insert_resource(timeline)
        .init_resource::<ProjectEffectCatalog>()
        .init_resource::<EditorModuleRegistry>()
        .init_resource::<CurvesState>()
        .insert_resource(Localizer::new("en-US").unwrap())
        .add_systems(Startup, spawn_test_timeline);

        app.update();

        let effect_clip_headers = {
            let world = app.world_mut();
            let mut query =
                world.query_filtered::<&ChoreographyAction, With<EffectClipTrackHeader>>();
            query
                .iter(world)
                .filter(|action| **action == ChoreographyAction::SelectEffectClip(effect_clip_id))
                .count()
        };
        assert_eq!(effect_clip_headers, 1);
        let edit_source_actions = {
            let world = app.world_mut();
            let mut query = world.query::<&ChoreographyAction>();
            query
                .iter(world)
                .filter(|action| {
                    **action == ChoreographyAction::EditEffectClipSource(effect_clip_id)
                })
                .count()
        };
        assert_eq!(edit_source_actions, 1);
        let effect_clip_reorder_handles = {
            let world = app.world_mut();
            let mut query = world.query::<&EffectClipTrackReorderHandle>();
            query.iter(world).count()
        };
        assert_eq!(effect_clip_reorder_handles, 1);
        let timeline_svg_sizes = {
            let world = app.world_mut();
            let mut query = world.query_filtered::<&Node, With<UiSvg>>();
            query
                .iter(world)
                .map(|node| (node.width, node.height))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            timeline_svg_sizes
                .iter()
                .filter(|size| **size == (Val::Px(18.0), Val::Px(20.0)))
                .count(),
            emitter_count + 1,
            "every reorder handle should use the legible SVG grip"
        );
        assert!(
            timeline_svg_sizes.contains(&(Val::Px(18.0), Val::Px(18.0))),
            "the effect-clip disclosure should use the enlarged SVG chevron"
        );
        let effect_clip_bars = {
            let world = app.world_mut();
            let mut query = world.query::<(&TimelineEffectClip, &Node)>();
            query
                .iter(world)
                .filter(|(marker, _)| marker.clip == effect_clip_id)
                .map(|(_, node)| (node.left, node.width))
                .collect::<Vec<_>>()
        };
        assert_eq!(effect_clip_bars.len(), 1);
        let (Val::Percent(left), Val::Percent(width)) = effect_clip_bars[0] else {
            panic!("effect clips should spawn with percentage geometry");
        };
        assert!((left - 0.2 / duration * 100.0).abs() < 0.000_1);
        assert!((width - 0.6 / duration * 100.0).abs() < 0.000_1);
        let drop_rows = {
            let world = app.world_mut();
            let mut query = world.query::<&TimelineTrackDropRow>();
            query.iter(world).count()
        };
        assert_eq!(drop_rows, emitter_count + 1);
        let drop_spacers = {
            let world = app.world_mut();
            let mut query = world.query_filtered::<
                (&RelativeCursorPosition, &Pickable),
                With<TimelineEffectDropSpacer>,
            >();
            query
                .iter(world)
                .map(|(_, pickable)| *pickable)
                .collect::<Vec<_>>()
        };
        assert_eq!(drop_spacers, vec![Pickable::IGNORE; 2]);
        let effect_clip_interactions = {
            let world = app.world_mut();
            let mut query = world.query::<&TimelineEffectClipInteraction>();
            query
                .iter(world)
                .filter(|interaction| interaction.clip == effect_clip_id)
                .map(|interaction| interaction.kind)
                .collect::<Vec<_>>()
        };
        assert_eq!(effect_clip_interactions.len(), 3);
        assert!(effect_clip_interactions.contains(&TimelineDragKind::Move));
        assert!(effect_clip_interactions.contains(&TimelineDragKind::TrimStart));
        assert!(effect_clip_interactions.contains(&TimelineDragKind::TrimEnd));

        let headers = {
            let world = app.world_mut();
            let mut query = world.query::<(
                &EmitterTrackHeader,
                &ChoreographyAction,
                &AccessibleLabel,
                Has<Button>,
                Has<ListItem>,
                Has<KeyboardNavigableListRow>,
                Has<Selected>,
            )>();
            query
                .iter(world)
                .map(
                    |(header, action, label, button, list_item, keyboard_row, selected)| {
                        (
                            header.emitter,
                            action.clone(),
                            label.0.clone(),
                            button,
                            list_item,
                            keyboard_row,
                            selected,
                        )
                    },
                )
                .collect::<Vec<_>>()
        };
        let clips = {
            let world = app.world_mut();
            let mut query = world.query::<(
                &TimelineClipInteraction,
                &ChoreographyAction,
                &AccessibleLabel,
                Has<EditorTooltip>,
            )>();
            query
                .iter(world)
                .filter(|(clip, _, _, _)| clip.kind == TimelineDragKind::Move)
                .map(|(clip, action, label, tooltip)| {
                    (
                        clip.emitter,
                        clip.region,
                        action.clone(),
                        label.0.clone(),
                        tooltip,
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(headers.len(), emitter_count);
        assert_eq!(clips.len(), emitter_count);
        assert_eq!(headers.iter().filter(|header| header.6).count(), 1);
        for (emitter, action, label, button, list_item, keyboard_row, _) in headers {
            assert!(button);
            assert!(list_item);
            assert!(keyboard_row);
            assert_eq!(action, ChoreographyAction::SelectEmitter(emitter));
            let clip = clips.iter().find(|clip| clip.0 == emitter).unwrap();
            assert_eq!(
                clip.2,
                ChoreographyAction::SelectEmitterRegion {
                    emitter,
                    region: clip.1,
                }
            );
            assert!(!clip.3.is_empty());
            assert!(clip.4);
            assert!(!label.is_empty());
        }
        let track_controls = {
            let world = app.world_mut();
            let mut query = world.query::<(
                &ChoreographyAction,
                &AccessibleLabel,
                Has<EditorTooltip>,
                &ButtonVariant,
                Has<Selected>,
            )>();
            query
                .iter(world)
                .filter(|(action, _, _, _, _)| {
                    matches!(
                        action,
                        ChoreographyAction::SetEmitterEnabled { .. }
                            | ChoreographyAction::ToggleEmitterSolo(_)
                    )
                })
                .map(|(action, label, tooltip, variant, selected)| {
                    (
                        action.clone(),
                        label.0.clone(),
                        tooltip,
                        variant.clone(),
                        selected,
                    )
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(track_controls.len(), emitter_count * 2);
        assert!(
            track_controls
                .iter()
                .all(|(_, label, tooltip, _, _)| *tooltip && label != "M" && label != "S")
        );
        assert_eq!(
            track_controls
                .iter()
                .filter(|(_, _, _, variant, selected)| {
                    *selected && *variant == ButtonVariant::Primary
                })
                .count(),
            2
        );
        let context_safe_controls = {
            let world = app.world_mut();
            let mut query = world.query::<(
                &ChoreographyAction,
                Has<TimelineTrackActionControl>,
                Has<Button>,
            )>();
            query
                .iter(world)
                .filter(|(action, context_safe, _)| {
                    *context_safe
                        && matches!(
                            action,
                            ChoreographyAction::ToggleEmitterColorPicker(_)
                                | ChoreographyAction::SetEmitterEnabled { .. }
                                | ChoreographyAction::ToggleEmitterSolo(_)
                                | ChoreographyAction::ToggleEffectClipExpanded(_)
                                | ChoreographyAction::ToggleEffectClipMuted(_)
                                | ChoreographyAction::ToggleEffectClipSolo(_)
                        )
                })
                .map(|(_, context_safe, button)| (context_safe, button))
                .collect::<Vec<_>>()
        };
        assert_eq!(context_safe_controls.len(), emitter_count * 3 + 3);
        assert!(
            context_safe_controls
                .iter()
                .all(|(context_safe, button)| *context_safe && !*button),
            "track action controls must leave secondary clicks available to their row"
        );
        let timeline_icon_controls = {
            let world = app.world_mut();
            let mut transport =
                world.query::<(&TransportAction, &AccessibleLabel, Has<EditorTooltip>)>();
            transport
                .iter(world)
                .map(|(_, label, tooltip)| (label.0.clone(), tooltip))
                .collect::<Vec<_>>()
        };
        assert_eq!(timeline_icon_controls.len(), 2);
        assert!(timeline_icon_controls.iter().all(|(label, tooltip)| {
            *tooltip && !matches!(label.as_str(), "<" | ">" | "+" | "-")
        }));
        let clip_controls = {
            let world = app.world_mut();
            let mut query = world.query::<(
                &TimelineClipInteraction,
                &AccessibleLabel,
                Has<EditorTooltip>,
            )>();
            query
                .iter(world)
                .map(|(_, label, tooltip)| (label.0.clone(), tooltip))
                .collect::<Vec<_>>()
        };
        assert_eq!(clip_controls.len(), emitter_count * 3);
        assert!(
            clip_controls
                .iter()
                .all(|(label, tooltip)| *tooltip && !label.is_empty())
        );
        let disabled = {
            let world = app.world_mut();
            let mut query = world.query_filtered::<Entity, With<EmitterTrackDisabled>>();
            query.iter(world).count()
        };
        let diagnostics = {
            let world = app.world_mut();
            let mut query = world.query_filtered::<Entity, With<EmitterTrackDiagnostic>>();
            query.iter(world).count()
        };
        assert_eq!(disabled, 1);
        assert_eq!(diagnostics, 1);

        let (panes, track_target) = {
            let world = app.world_mut();
            let mut query = world.query::<(Entity, &TimelineVerticalPane, &ScrollPosition)>();
            let panes = query
                .iter(world)
                .map(|(entity, kind, position)| (entity, *kind, position.y))
                .collect::<Vec<_>>();
            let track_target = panes
                .iter()
                .find(|(_, kind, _)| *kind == TimelineVerticalPane::Tracks)
                .map(|(entity, _, _)| *entity)
                .unwrap();
            (panes, track_target)
        };
        assert_eq!(panes.len(), 2);
        assert!(panes.iter().all(|(_, _, offset)| *offset == 72.0));
        let pane_layout = {
            let world = app.world_mut();
            let mut query =
                world.query::<(&TimelineVerticalPane, &Node, Has<TimelineAssetDropTarget>)>();
            query
                .iter(world)
                .map(|(pane, node, drop_target)| (*pane, node.align_content, drop_target))
                .collect::<Vec<_>>()
        };
        assert_eq!(pane_layout.len(), 2);
        assert!(
            pane_layout
                .iter()
                .all(|(_, alignment, _)| *alignment == AlignContent::Start),
            "mixed clip/emitter rows must stay packed at the top"
        );
        assert!(pane_layout.iter().any(|(pane, _, drop_target)| {
            *pane == TimelineVerticalPane::Headers && *drop_target
        }));
        let vertical_contents = {
            let world = app.world_mut();
            let mut query = world.query::<(&TimelineVerticalContent, &Node)>();
            query
                .iter(world)
                .map(|(_, node)| (node.min_height, node.display, node.flex_shrink))
                .collect::<Vec<_>>()
        };
        assert_eq!(vertical_contents.len(), 2);
        assert!(
            vertical_contents
                .iter()
                .all(|(min_height, display, shrink)| {
                    matches!(min_height, Val::Px(height) if *height > 0.0)
                        && *display == Display::Grid
                        && *shrink == 0.0
                })
        );
        let header_navigation = {
            let world = app.world_mut();
            let mut query = world.query_filtered::<(
                Has<ListBox>,
                Has<ScrollArea>,
                Has<KeyboardNavigableList>,
                Has<TabIndex>,
            ), With<TimelineVerticalPane>>();
            query.iter(world).find(|(list, _, _, _)| *list).unwrap()
        };
        assert_eq!(header_navigation, (true, true, true, true));
        let horizontal_target = {
            let world = app.world_mut();
            let mut query =
                world.query_filtered::<Entity, With<TimelineHorizontalScrollViewport>>();
            query.single(world).unwrap()
        };
        let scrollbar_targets = {
            let world = app.world_mut();
            let mut query = world.query::<&Scrollbar>();
            query
                .iter(world)
                .map(|scrollbar| (scrollbar.orientation, scrollbar.target))
                .collect::<Vec<_>>()
        };
        assert!(scrollbar_targets.contains(&(ControlOrientation::Vertical, track_target)));
        assert!(scrollbar_targets.contains(&(ControlOrientation::Horizontal, horizontal_target)));
        assert_eq!(scrollbar_targets.len(), 2);

        let constrained_ancestors = {
            let world = app.world_mut();
            let mut bodies = world.query_filtered::<&Node, With<TimelineBody>>();
            let body = bodies.single(world).unwrap();
            let body_constraints = (body.min_width, body.min_height, body.overflow);
            let mut headers = world.query_filtered::<&Node, With<TimelineHeaderColumn>>();
            let header = headers.single(world).unwrap();
            let header_constraints = (header.min_width, header.min_height);
            let mut canvases = world.query_filtered::<&Node, With<TimelineCanvas>>();
            let canvas = canvases.single(world).unwrap();
            let canvas_constraints = (canvas.min_width, canvas.min_height);
            (body_constraints, header_constraints, canvas_constraints)
        };
        assert_eq!(
            constrained_ancestors,
            (
                (Val::Px(0.0), Val::Px(0.0), Overflow::clip()),
                (Val::Px(0.0), Val::Px(0.0)),
                (Val::Px(0.0), Val::Px(0.0)),
            )
        );

        let (
            gutter_height,
            scrollbar_position,
            scrollbar_height,
            scrollbar_padding,
            horizontal_scrollbar_gutter,
            vertical_gutter_width,
            vertical_gutter_border,
        ) = {
            let world = app.world_mut();
            let mut gutters = world.query_filtered::<&Node, With<TimelineHorizontalGutter>>();
            let gutter_height = gutters.single(world).unwrap().height;
            let mut scrollbars = world.query::<(&Scrollbar, &Node)>();
            let (_, scrollbar) = scrollbars
                .iter(world)
                .find(|(scrollbar, _)| scrollbar.orientation == ControlOrientation::Horizontal)
                .unwrap();
            let scrollbar_layout = (scrollbar.position_type, scrollbar.height, scrollbar.padding);
            let mut scrollbar_gutters =
                world.query_filtered::<&Node, With<TimelineHorizontalScrollbarGutter>>();
            let horizontal_scrollbar_gutter = scrollbar_gutters.single(world).unwrap();
            let horizontal_scrollbar_gutter = (
                horizontal_scrollbar_gutter.height,
                horizontal_scrollbar_gutter.border.top,
            );
            let mut vertical_gutters =
                world.query_filtered::<&Node, With<TimelineVerticalScrollbarGutter>>();
            let vertical_gutter = vertical_gutters.single(world).unwrap();
            (
                gutter_height,
                scrollbar_layout.0,
                scrollbar_layout.1,
                scrollbar_layout.2,
                horizontal_scrollbar_gutter,
                vertical_gutter.width,
                vertical_gutter.border.left,
            )
        };
        assert_eq!(gutter_height, Val::Px(15.0));
        assert_eq!(scrollbar_position, PositionType::Relative);
        assert_eq!(scrollbar_height, Val::Px(10.0));
        assert_eq!(scrollbar_padding.top, Val::Px(3.0));
        assert_eq!(scrollbar_padding.bottom, Val::Px(3.0));
        assert_eq!(horizontal_scrollbar_gutter, (Val::Px(15.0), Val::Px(1.0)));
        assert_eq!(vertical_gutter_width, Val::Px(15.0));
        assert_eq!(vertical_gutter_border, Val::Px(1.0));
    }

    #[test]
    fn choreography_selection_is_stable_and_clears_incompatible_curve_state() {
        let session = test_support::session_with_timing_slack();
        let target = session.effect.emitters[2].id;
        let mut app = choreography_app(session);
        app.insert_resource({
            let mut curves = CurvesState::default();
            curves.select_for_test(aestra_core::ModuleId::new(), 0, 0);
            curves
        });

        app.world_mut()
            .trigger(ChoreographyAction::SelectEmitter(target));
        app.update();

        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .selection
                .emitter(&app.world().resource::<EditorSession>().effect),
            Some(target)
        );
        assert!(!app.world().resource::<CurvesState>().has_selection());
    }

    #[test]
    fn unmodified_click_on_the_primary_track_collapses_multi_selection_after_a_context_menu() {
        let emitter = EmitterId::new();

        assert!(should_collapse_emitter_multi_selection(
            PointerButton::Primary,
            false,
            false,
            false,
            3,
            Some(emitter),
            emitter,
        ));
        assert!(!should_collapse_emitter_multi_selection(
            PointerButton::Secondary,
            false,
            false,
            false,
            3,
            Some(emitter),
            emitter,
        ));
        assert!(!should_collapse_emitter_multi_selection(
            PointerButton::Primary,
            true,
            false,
            false,
            3,
            Some(emitter),
            emitter,
        ));
        assert!(!should_collapse_emitter_multi_selection(
            PointerButton::Primary,
            false,
            true,
            false,
            3,
            Some(emitter),
            emitter,
        ));
    }

    #[test]
    fn track_list_value_change_selects_the_emitter_through_its_semantic_action() {
        let session = test_support::session_with_timing_slack();
        let target = session.effect.emitters[2].id;
        let mut app = choreography_app(session);
        app.add_observer(activate_timeline_track_entry);
        let list = app.world_mut().spawn(KeyboardNavigableList).id();
        let row = app
            .world_mut()
            .spawn((
                EmitterTrackHeader { emitter: target },
                ChoreographyAction::SelectEmitter(target),
            ))
            .id();

        app.world_mut().trigger(ValueChange::<Entity> {
            source: list,
            value: row,
            is_final: true,
        });
        app.update();

        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.selection.emitter(&session.effect), Some(target));
    }

    #[test]
    fn choreography_add_duplicate_and_enabled_actions_remain_undoable() {
        let session = test_support::session_with_timing_slack();
        let original_count = session.effect.emitters.len();
        let original = session.effect.emitters[0].clone();
        let mut app = choreography_app(session);

        app.world_mut().trigger(ChoreographyAction::AddEmitter);
        app.update();
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .effect
                .emitters
                .len(),
            original_count + 1
        );
        app.world_mut().resource_mut::<EditorSession>().undo();
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .effect
                .emitters
                .len(),
            original_count
        );

        app.world_mut()
            .trigger(ChoreographyAction::DuplicateEmitter(Some(original.id)));
        app.update();
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .effect
                .emitters
                .len(),
            original_count + 1
        );
        app.world_mut().resource_mut::<EditorSession>().undo();
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .effect
                .emitters
                .len(),
            original_count
        );

        app.world_mut()
            .trigger(ChoreographyAction::SetEmitterEnabled {
                emitter: original.id,
                enabled: !original.enabled,
            });
        app.update();
        let edited = app
            .world()
            .resource::<EditorSession>()
            .effect
            .emitters
            .iter()
            .find(|emitter| emitter.id == original.id)
            .unwrap();
        assert_eq!(edited.enabled, !original.enabled);
        app.world_mut().resource_mut::<EditorSession>().undo();
        let restored = app
            .world()
            .resource::<EditorSession>()
            .effect
            .emitters
            .iter()
            .find(|emitter| emitter.id == original.id)
            .unwrap();
        assert_eq!(restored.enabled, original.enabled);
    }

    #[test]
    fn emitter_solo_is_preview_only_and_isolates_runtime_output() {
        let mut session = test_support::session_with_timing_slack();
        session.effect.effect_clips.clear();
        session.effect.effect_clips.push(EffectClip::new(
            EffectAssetRef::new(aestra_core::EffectId::from_u128(0x5010)),
            0.0,
            1.0,
        ));
        let target = session.effect.emitters[1].id;
        let original_effect = session.effect.clone();
        let mut app = choreography_app(session);

        app.world_mut()
            .trigger(ChoreographyAction::ToggleEmitterSolo(target));
        app.update();

        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.solo_emitter, Some(target));
        assert_eq!(session.effect, original_effect);
        assert!(!session.dirty);
        let preview = session.preview().unwrap().effect();
        assert!(
            preview
                .emitters
                .iter()
                .find(|emitter| emitter.source == target)
                .unwrap()
                .enabled
        );
        assert!(
            preview
                .emitters
                .iter()
                .filter(|emitter| emitter.source != target)
                .all(|emitter| !emitter.enabled)
        );
        assert!(
            preview.effect_clips.is_empty(),
            "soloing a local emitter must suppress referenced effect clips"
        );

        app.world_mut()
            .trigger(ChoreographyAction::ToggleEmitterSolo(target));
        app.update();
        assert_eq!(app.world().resource::<EditorSession>().solo_emitter, None);
    }

    #[test]
    fn emitter_and_effect_clip_solo_are_mutually_exclusive() {
        let mut session = test_support::session_with_timing_slack();
        session.effect.effect_clips.clear();
        let clip = EffectClip::new(
            EffectAssetRef::new(aestra_core::EffectId::from_u128(0x5010)),
            0.0,
            1.0,
        );
        let clip_id = clip.id;
        session.effect.effect_clips.push(clip);
        let emitter = session.effect.emitters[0].id;
        let mut app = choreography_app(session);

        app.world_mut()
            .trigger(ChoreographyAction::ToggleEffectClipSolo(clip_id));
        app.update();
        assert_eq!(
            app.world().resource::<TimelineState>().solo_effect_clip,
            Some(clip_id)
        );

        app.world_mut()
            .trigger(ChoreographyAction::ToggleEmitterSolo(emitter));
        app.update();
        assert_eq!(
            app.world().resource::<EditorSession>().solo_emitter,
            Some(emitter)
        );
        assert_eq!(
            app.world().resource::<TimelineState>().solo_effect_clip,
            None
        );

        app.world_mut()
            .trigger(ChoreographyAction::ToggleEffectClipSolo(clip_id));
        app.update();
        assert_eq!(app.world().resource::<EditorSession>().solo_emitter, None);
        assert_eq!(
            app.world().resource::<TimelineState>().solo_effect_clip,
            Some(clip_id)
        );
    }

    #[test]
    fn choreography_delete_retains_review_and_minimum_emitter_guard() {
        let session = test_support::session_with_timing_slack();
        let target = session.effect.emitters[1].id;
        let mut app = choreography_app(session);
        app.world_mut()
            .trigger(ChoreographyAction::DeleteEmitter(Some(target)));
        app.update();
        assert!(
            app.world()
                .resource::<EditorSession>()
                .pending_change
                .is_some()
        );
        assert!(
            app.world()
                .resource::<EditorSession>()
                .effect
                .emitters
                .iter()
                .any(|item| item.id == target)
        );

        let mut single = test_support::session_with_timing_slack();
        single.effect.emitters.truncate(1);
        single
            .selection
            .select_emitter(single.effect.emitters[0].id);
        let mut guarded = choreography_app(single);
        guarded
            .world_mut()
            .trigger(ChoreographyAction::DeleteEmitter(None));
        guarded.update();
        let session = guarded.world().resource::<EditorSession>();
        assert_eq!(session.effect.emitters.len(), 1);
        assert!(session.pending_change.is_none());
        assert_eq!(session.status, "An effect must keep at least one emitter");
    }

    #[test]
    fn choreography_shortcuts_require_timeline_context_and_ignore_text_editing() {
        for (timeline_visible, text_focused, expected_delta) in
            [(false, false, 0), (true, true, 0), (true, false, 1)]
        {
            let session = test_support::session_with_timing_slack();
            let initial = session.effect.emitters.len();
            let mut app = choreography_app(session);
            app.init_resource::<ModulePaletteState>()
                .init_resource::<ButtonInput<KeyCode>>()
                .add_systems(Update, choreography_keyboard_input);
            if timeline_visible {
                app.world_mut().spawn(TimelineCanvas);
            }
            if text_focused {
                let input = app.world_mut().spawn(EditableText::new("editing")).id();
                app.insert_resource(InputFocus::from_entity(input));
            }
            {
                let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                keys.press(KeyCode::ControlLeft);
                keys.press(KeyCode::Enter);
            }

            app.update();

            assert_eq!(
                app.world()
                    .resource::<EditorSession>()
                    .effect
                    .emitters
                    .len(),
                initial + expected_delta
            );
        }
    }

    #[test]
    fn native_delete_in_assets_does_not_also_create_a_timeline_deletion_proposal() {
        use bevy::input::{
            InputPlugin,
            keyboard::{Key, KeyboardInput},
        };
        use bevy::input_focus::{FocusedInput, InputDispatchPlugin, InputFocusPlugin};
        #[derive(Resource, Default)]
        struct AssetDeletes(usize);
        for nested in [false, true] {
            let mut app = choreography_app(test_support::session_with_timing_slack());
            app.add_plugins((
                InputPlugin,
                InputFocusPlugin,
                InputDispatchPlugin,
                crate::input::EditorKeyboardPlugin,
            ))
            .init_resource::<ModulePaletteState>()
            .init_resource::<AssetDeletes>()
            .add_systems(Update, choreography_keyboard_input)
            .add_observer(
                |mut event: On<FocusedInput<KeyboardInput>>, mut deletes: ResMut<AssetDeletes>| {
                    if event.input.key_code == KeyCode::Delete && event.input.state.is_pressed() {
                        deletes.0 += 1;
                        event.propagate(false);
                    }
                },
            );
            let window = app
                .world_mut()
                .spawn((Window::default(), bevy::window::PrimaryWindow))
                .id();
            let timeline = app.world_mut().spawn(TimelineCanvas).id();
            let surface = app
                .world_mut()
                .spawn(crate::asset_browser::BrowserSurface)
                .id();
            let target = if nested {
                app.world_mut().spawn(ChildOf(surface)).id()
            } else {
                surface
            };
            app.insert_resource(InputFocus::from_entity(target));
            crate::input::tests::tap(&mut app, window, KeyCode::Delete, Key::Delete);
            app.update();
            assert_eq!(app.world().resource::<AssetDeletes>().0, 1);
            assert!(
                app.world()
                    .resource::<EditorSession>()
                    .pending_change
                    .is_none()
            );
            // The same shortcut still works after returning focus to the timeline.
            app.insert_resource(InputFocus::from_entity(timeline));
            crate::input::tests::tap(&mut app, window, KeyCode::Delete, Key::Delete);
            app.update();
            assert!(
                app.world()
                    .resource::<EditorSession>()
                    .pending_change
                    .is_some()
            );
        }
    }

    #[test]
    fn insert_and_delete_shortcuts_edit_the_selected_automation_lane() {
        let mut session = test_support::session_with_timing_slack();
        let registry = EditorModuleRegistry::default();
        let localizer = Localizer::new("en-US").unwrap();
        let emitter = session.effect.emitters[0].clone();
        let lane = emitter_automation_lanes(&session.effect, &emitter, &registry, &localizer)
            .into_iter()
            .find(|lane| matches!(lane.keys, AutomationLaneKeys::Curve(..)))
            .unwrap();
        let original_len = lane.keys.len();
        session.seek_time(emitter.start_time + emitter.duration * 0.43);
        let mut app = choreography_app(session);
        app.init_resource::<ModulePaletteState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, choreography_keyboard_input);
        app.world_mut().spawn(TimelineCanvas);
        app.world_mut().spawn((
            TimelineAutomationLaneGraph(lane.id.clone()),
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::new(0.17, -0.1)),
            },
        ));
        app.world_mut()
            .resource_mut::<TimelineState>()
            .selected_automation_key = Some(TimelineAutomationKeySelection {
            lane: lane.id.clone(),
            key: 0,
        });
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Insert);

        app.update();

        assert_eq!(
            automation_lane_keys(&app.world().resource::<EditorSession>().effect, &lane.id)
                .unwrap()
                .len(),
            original_len + 1
        );
        let inserted =
            automation_lane_keys(&app.world().resource::<EditorSession>().effect, &lane.id)
                .unwrap();
        assert!(inserted.times().any(|time| (time - 0.67).abs() < 0.0001));
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.clear();
            keys.release(KeyCode::Insert);
            keys.press(KeyCode::Delete);
        }
        app.update();
        assert_eq!(
            automation_lane_keys(&app.world().resource::<EditorSession>().effect, &lane.id)
                .unwrap()
                .len(),
            original_len
        );
    }
}

mod document_actions;
pub(crate) use document_actions::TimelineSnapMode;
