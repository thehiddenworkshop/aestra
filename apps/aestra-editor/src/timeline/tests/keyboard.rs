//! Actual timeline consumer with real input dispatch, state and session on both engines.
use super::*;
use crate::{
    asset_actions::AssetOperationState, feathers::code_editor::CodeEditor,
    input::EditorKeyboardPlugin, material_function_editor::graph::FunctionGraphMenuState,
    material_graph::MaterialGraphPaletteState, persistence::DocumentProtectionState,
};
use aestra_core::{
    ChoreographyEvent, ChoreographyEventPayload, Curve, CurveKey, EffectMarker, ModuleParameters,
};
use bevy::{
    input::{
        ButtonState, InputPlugin,
        keyboard::{Key, KeyboardInput},
    },
    input_focus::{InputDispatchPlugin, InputFocusPlugin},
    time::TimeUpdateStrategy,
    window::PrimaryWindow,
};
use std::time::Duration;

#[derive(Debug, PartialEq)]
enum Routed {
    Timeline(TimelineAction),
    Choreography(ChoreographyAction),
    Motion(&'static str),
}
#[derive(Resource, Default)]
struct Actions(Vec<Routed>);
struct Fixture {
    app: App,
    window: Entity,
    canvas: Entity,
}
impl Fixture {
    fn new() -> Self {
        let session = crate::test_support::session_with_timing_slack();
        let duration = session.playback_duration();
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            InputPlugin,
            InputFocusPlugin,
            InputDispatchPlugin,
            EditorKeyboardPlugin,
        ))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO))
        .insert_resource(session)
        .insert_resource(TimelineState::framed(duration))
        .init_resource::<ModulePaletteState>()
        .init_resource::<Actions>()
        .add_systems(
            Update,
            choreography_keyboard_input.in_set(TimelineSet::Input),
        )
        .add_observer(
            |action: On<ChoreographyAction>, mut actions: ResMut<Actions>| {
                actions.0.push(Routed::Choreography(action.clone()));
            },
        )
        .add_observer(
            |action: On<TimelineAction>,
             mut actions: ResMut<Actions>,
             mut session: ResMut<EditorSession>| {
                actions.0.push(Routed::Timeline(*action));
                super::super::document_actions::apply(*action, &mut session, None);
            },
        )
        .add_observer(
            |action: On<super::super::motion_state::Action>, mut actions: ResMut<Actions>| {
                let kind = match *action {
                    super::super::motion_state::Action::Add => "add",
                    super::super::motion_state::Action::Delete => "delete",
                    _ => panic!("Keyboard must only dispatch pose add/delete"),
                };
                actions.0.push(Routed::Motion(kind));
            },
        );
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        let canvas = app.world_mut().spawn(TimelineCanvas).id();
        app.update();
        Self {
            app,
            window,
            canvas,
        }
    }
    fn key(&mut self, code: KeyCode, logical: Key, state: ButtonState, repeat: bool) {
        self.app.world_mut().write_message(KeyboardInput {
            key_code: code,
            logical_key: logical,
            state,
            repeat,
            text: None,
            window: self.window,
        });
    }
    fn tap(&mut self, code: KeyCode, logical: Key) {
        self.key(code, logical.clone(), ButtonState::Pressed, false);
        self.key(code, logical, ButtonState::Released, false);
    }
    fn control(&mut self, code: KeyCode, logical: Key) {
        self.key(
            KeyCode::ControlLeft,
            Key::Control,
            ButtonState::Pressed,
            false,
        );
        self.tap(code, logical);
        self.key(
            KeyCode::ControlLeft,
            Key::Control,
            ButtonState::Released,
            false,
        );
    }
    fn assert_actions(&self, expected: &[Routed]) {
        assert_eq!(self.app.world().resource::<Actions>().0, expected);
    }
    fn assert_blocked(&mut self) {
        self.control(KeyCode::Enter, Key::Enter);
        self.control(KeyCode::KeyD, Key::Character("d".into()));
        self.tap(KeyCode::Delete, Key::Delete);
        self.app.update();
        self.assert_actions(&[]);
    }
    fn curve_lane(&self, parameter: &str) -> AutomationLaneId {
        let effect = &self.app.world().resource::<EditorSession>().effect;
        let emitter = &effect.emitters[0];
        let module = emitter
            .modules
            .iter()
            .find(|module| matches!(module.parameters, ModuleParameters::Appearance { .. }))
            .unwrap();
        AutomationLaneId {
            emitter: emitter.id,
            module: module.id,
            input: 0,
            parameter: parameter.into(),
            channel: None,
        }
    }
    fn graph(&mut self, lane: AutomationLaneId, position: Option<Vec2>, hovered: bool) -> Entity {
        self.app
            .world_mut()
            .spawn((
                TimelineAutomationLaneGraph(lane),
                RelativeCursorPosition {
                    cursor_over: hovered,
                    normalized: position,
                },
            ))
            .id()
    }
}

#[test]
fn same_frame_chords_preserve_order_and_are_not_replayed() {
    let mut f = Fixture::new();
    for _ in 0..2 {
        f.control(KeyCode::Enter, Key::Enter);
        f.control(KeyCode::KeyD, Key::Character("d".into()));
    }
    f.app.update();
    let expected = [
        Routed::Choreography(ChoreographyAction::AddEmitter),
        Routed::Choreography(ChoreographyAction::DuplicateEmitter(None)),
        Routed::Choreography(ChoreographyAction::AddEmitter),
        Routed::Choreography(ChoreographyAction::DuplicateEmitter(None)),
    ];
    f.assert_actions(&expected);
    f.app.update();
    f.assert_actions(&expected);
    assert!(!f.app.world().resource::<EditorSession>().can_undo());
}

#[test]
fn logical_duplicate_and_event_time_modifiers_work_for_both_controls() {
    let mut f = Fixture::new();
    for control in [KeyCode::ControlLeft, KeyCode::ControlRight] {
        f.key(control, Key::Control, ButtonState::Pressed, false);
        f.tap(KeyCode::KeyD, Key::Character("x".into()));
        f.tap(KeyCode::KeyX, Key::Character("D".into()));
        f.key(control, Key::Control, ButtonState::Released, false);
        f.tap(KeyCode::KeyD, Key::Character("d".into()));
    }
    f.app.update();
    f.assert_actions(&[
        Routed::Choreography(ChoreographyAction::DuplicateEmitter(None)),
        Routed::Choreography(ChoreographyAction::DuplicateEmitter(None)),
    ]);
}

#[test]
fn selected_regions_own_duplicate_and_delete_before_semantic_selection() {
    let mut f = Fixture::new();
    let emitter = f.app.world().resource::<EditorSession>().effect.emitters[0].id;
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .select_only_emitter_region(emitter, EmitterRegionId::new());
    let marker = EffectMarker::new("Marker", 0.1);
    let id = marker.id;
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .effect
        .markers
        .push(marker);
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .select_marker(id);
    f.control(KeyCode::KeyD, Key::Character("d".into()));
    f.tap(KeyCode::Delete, Key::Delete);
    f.app.update();
    f.assert_actions(&[
        Routed::Choreography(ChoreographyAction::DuplicateSelectedEmitterRegions),
        Routed::Choreography(ChoreographyAction::DeleteSelectedEmitterRegions),
    ]);
}

#[test]
fn selected_automation_key_has_delete_priority_over_regions_and_markers() {
    let mut f = Fixture::new();
    let marker = EffectMarker::new("Marker", 0.1);
    let id = marker.id;
    let mut session = f.app.world_mut().resource_mut::<EditorSession>();
    session.effect.markers.push(marker);
    session.select_marker(id);
    let selection = TimelineAutomationKeySelection {
        lane: f.curve_lane("size"),
        key: 1,
    };
    let mut state = f.app.world_mut().resource_mut::<TimelineState>();
    state.select_only_emitter_region(selection.lane.emitter, EmitterRegionId::new());
    state.selected_automation_key = Some(selection.clone());
    f.tap(KeyCode::Delete, Key::Delete);
    f.app.update();
    f.assert_actions(&[Routed::Choreography(
        ChoreographyAction::DeleteAutomationKey(selection),
    )]);
}

#[test]
fn marker_and_event_delete_execute_the_real_document_branch_and_undo() {
    for is_marker in [true, false] {
        let mut f = Fixture::new();
        let mut session = f.app.world_mut().resource_mut::<EditorSession>();
        let action = if is_marker {
            let marker = EffectMarker::new("Delete", 0.5);
            let id = marker.id;
            session.effect.markers.push(marker);
            session.select_marker(id);
            TimelineAction::DeleteMarker(id)
        } else {
            let event = ChoreographyEvent::new(
                "Delete",
                0.5,
                ChoreographyEventPayload::GameplayNotify {
                    topic: "notify".into(),
                },
            );
            let id = event.id;
            session.effect.choreography_events.push(event);
            session.select_choreography_event(id);
            TimelineAction::DeleteChoreographyEvent(id)
        };
        let before = session.effect.clone();
        f.tap(KeyCode::Delete, Key::Delete);
        f.app.update();
        f.assert_actions(&[Routed::Timeline(action)]);
        let mut session = f.app.world_mut().resource_mut::<EditorSession>();
        assert!(session.effect.markers.is_empty() && session.effect.choreography_events.is_empty());
        assert!(session.can_undo());
        session.undo();
        assert_eq!(session.effect, before);
    }
}

#[test]
fn nested_inspection_protects_referenced_emitters_and_clips_but_not_markers() {
    for nested in [false, true] {
        for clip in [false, true] {
            let mut f = Fixture::new();
            let id = EffectClipId::new();
            if clip {
                f.app
                    .world_mut()
                    .resource_mut::<EditorSession>()
                    .selection
                    .primary = SemanticTarget::EffectClip(id);
            }
            if nested {
                f.app
                    .world_mut()
                    .resource_mut::<TimelineState>()
                    .inspected_child = Some(EffectClipChildSelection::EffectClip {
                    path: EffectClipPath::root_path(id),
                });
            }
            f.tap(KeyCode::Delete, Key::Delete);
            f.app.update();
            if nested {
                f.assert_actions(&[]);
            } else if clip {
                f.assert_actions(&[Routed::Choreography(ChoreographyAction::DeleteEffectClip(
                    id,
                ))]);
            } else {
                f.assert_actions(&[Routed::Choreography(ChoreographyAction::DeleteEmitter(
                    None,
                ))]);
            }
        }
    }
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .inspected_child = Some(EffectClipChildSelection::EffectClip {
        path: EffectClipPath::root_path(EffectClipId::new()),
    });
    let marker = EffectMarker::new("Local marker", 0.1);
    let id = marker.id;
    let mut session = f.app.world_mut().resource_mut::<EditorSession>();
    session.effect.markers.push(marker);
    session.select_marker(id);
    f.tap(KeyCode::Delete, Key::Delete);
    f.app.update();
    f.assert_actions(&[Routed::Timeline(TimelineAction::DeleteMarker(id))]);
}

#[test]
fn active_host_motion_owns_insert_delete_and_swallows_emitter_chords() {
    let mut f = Fixture::new();
    let effect = f.app.world().resource::<EditorSession>().effect.id;
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .selection
        .primary = SemanticTarget::Effect(effect);
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .host_motion
        .effect = Some(effect);
    f.control(KeyCode::Enter, Key::Enter);
    f.control(KeyCode::KeyD, Key::Character("d".into()));
    f.tap(KeyCode::Insert, Key::Insert);
    f.tap(KeyCode::Delete, Key::Delete);
    f.app.update();
    f.assert_actions(&[Routed::Motion("add"), Routed::Motion("delete")]);
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .host_motion
        .effect = Some(aestra_core::EffectId::new());
    f.control(KeyCode::Enter, Key::Enter);
    f.app.update();
    assert_eq!(
        f.app.world().resource::<Actions>().0.last(),
        Some(&Routed::Choreography(ChoreographyAction::AddEmitter))
    );
}

#[test]
fn insert_projects_actual_curve_and_gradient_at_the_hovered_lane() {
    for parameter in ["size", "color"] {
        let mut f = Fixture::new();
        let lane = f.curve_lane(parameter);
        let position = Vec2::ZERO;
        f.graph(lane.clone(), Some(position), true);
        let expected = super::super::automation_data::add_automation_key_at_pointer_action(
            &f.app.world().resource::<EditorSession>().effect,
            &lane,
            position,
        )
        .unwrap();
        f.tap(KeyCode::Insert, Key::Insert);
        f.app.update();
        f.assert_actions(&[Routed::Choreography(expected.clone())]);
        let ChoreographyAction::AddAutomationKeyAt {
            normalized_time_bits,
            value_bits,
            ..
        } = expected
        else {
            unreachable!()
        };
        assert_eq!(f32::from_bits(normalized_time_bits), 0.5);
        assert_eq!(value_bits.is_some(), parameter == "size");
    }
}

#[test]
fn insert_skips_missing_invalid_and_unhovered_lanes() {
    let mut f = Fixture::new();
    let lane = f.curve_lane("size");
    f.graph(lane.clone(), None, true);
    f.graph(lane.clone(), Some(Vec2::ZERO), false);
    let mut invalid = lane.clone();
    invalid.module = aestra_core::ModuleId::new();
    f.graph(invalid, Some(Vec2::ZERO), true);
    f.graph(lane.clone(), Some(Vec2::new(0.2, -0.2)), true);
    f.tap(KeyCode::Insert, Key::Insert);
    f.app.update();
    assert_eq!(f.app.world().resource::<Actions>().0.len(), 1);
    let Routed::Choreography(ChoreographyAction::AddAutomationKeyAt {
        lane: actual,
        normalized_time_bits,
        ..
    }) = &f.app.world().resource::<Actions>().0[0]
    else {
        panic!("Expected insertion")
    };
    assert_eq!(actual, &lane);
    assert!((f32::from_bits(*normalized_time_bits) - 0.7).abs() < 0.0001);
}

#[test]
fn held_and_repeat_keys_do_not_duplicate_actions() {
    let mut f = Fixture::new();
    f.key(KeyCode::Delete, Key::Delete, ButtonState::Pressed, false);
    f.app.update();
    f.app.update();
    f.key(KeyCode::Delete, Key::Delete, ButtonState::Pressed, true);
    f.app.update();
    f.assert_actions(&[Routed::Choreography(ChoreographyAction::DeleteEmitter(
        None,
    ))]);
}

#[test]
fn canvas_presence_not_hover_owns_shortcuts_without_duplicate_dispatch() {
    let mut f = Fixture::new();
    f.app.world_mut().spawn(TimelineCanvas);
    f.control(KeyCode::Enter, Key::Enter);
    f.app.update();
    f.assert_actions(&[Routed::Choreography(ChoreographyAction::AddEmitter)]);
    f.app.world_mut().despawn(f.canvas);
    let world = f.app.world_mut();
    let remaining: Vec<_> = world
        .query_filtered::<Entity, With<TimelineCanvas>>()
        .iter(world)
        .collect();
    for entity in remaining {
        world.despawn(entity);
    }
    f.app.world_mut().resource_mut::<Actions>().0.clear();
    f.assert_blocked();
}

#[test]
fn editable_code_menu_and_browser_ancestor_focus_block() {
    for kind in 0..4 {
        let mut f = Fixture::new();
        let parent = match kind {
            0 => f.app.world_mut().spawn(EditableText::new("Name")).id(),
            1 => f.app.world_mut().spawn(CodeEditor::new("code".into())).id(),
            2 => f.app.world_mut().spawn(bevy::ui_widgets::MenuItem).id(),
            _ => f
                .app
                .world_mut()
                .spawn(crate::asset_browser::BrowserSurface)
                .id(),
        };
        let child = f.app.world_mut().spawn(ChildOf(parent)).id();
        f.app.insert_resource(InputFocus::from_entity(child));
        f.assert_blocked();
    }
}

#[test]
fn modal_and_palette_ownership_blocks_and_fresh_chords_recover() {
    let mut f = Fixture::new();
    f.app.world_mut().resource_mut::<ModulePaletteState>().open = true;
    f.assert_blocked();
    f.app.world_mut().resource_mut::<ModulePaletteState>().open = false;
    for state in DocumentProtectionState::blocking_states_for_test() {
        f.app.insert_resource(state);
        f.assert_blocked();
    }
    f.app.insert_resource(DocumentProtectionState::default());
    f.app.insert_resource(AssetOperationState::open_for_test());
    f.assert_blocked();
    f.app.insert_resource(AssetOperationState::default());
    for state in MaterialGraphPaletteState::blocking_states_for_test() {
        f.app.insert_resource(state);
        f.assert_blocked();
    }
    f.app.insert_resource(MaterialGraphPaletteState::default());
    for state in FunctionGraphMenuState::blocking_states_for_test() {
        f.app.insert_resource(state);
        f.assert_blocked();
    }
    f.app.insert_resource(FunctionGraphMenuState::default());
    f.app.update();
    f.assert_actions(&[]);
    f.control(KeyCode::Enter, Key::Enter);
    f.app.update();
    f.assert_actions(&[Routed::Choreography(ChoreographyAction::AddEmitter)]);
}

#[test]
fn lost_window_focus_blocks_until_a_window_regains_focus() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .get_mut::<Window>(f.window)
        .unwrap()
        .focused = false;
    f.assert_blocked();
    f.app
        .world_mut()
        .get_mut::<Window>(f.window)
        .unwrap()
        .focused = true;
    f.control(KeyCode::Enter, Key::Enter);
    f.app.update();
    f.assert_actions(&[Routed::Choreography(ChoreographyAction::AddEmitter)]);
}

#[test]
fn actual_curve_projection_resolves_active_source_and_clamps_time() {
    let mut f = Fixture::new();
    let lane = f.curve_lane("size");
    let mut session = f.app.world_mut().resource_mut::<EditorSession>();
    let module = session.effect.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.id == lane.module)
        .unwrap();
    let source =
        aestra_core::PropertySource::Curve(aestra_core::PropertyEvaluationDomain::ParticleLife);
    module.property_sources.insert("size".into(), source);
    module.property_source_values.insert(
        "size".into(),
        vec![aestra_core::PropertySourceValue::new(
            source,
            aestra_core::Value::Curve(Curve::new(vec![
                CurveKey::new(0.0, 10.0),
                CurveKey::new(1.0, 20.0),
            ])),
        )],
    );
    let action = super::super::automation_data::add_automation_key_at_pointer_action(
        &session.effect,
        &lane,
        Vec2::new(2.0, 0.0),
    )
    .unwrap();
    let ChoreographyAction::AddAutomationKeyAt {
        normalized_time_bits,
        value_bits,
        ..
    } = action
    else {
        unreachable!()
    };
    assert_eq!(f32::from_bits(normalized_time_bits), 1.0);
    assert_eq!(f32::from_bits(value_bits.unwrap()), 15.0);
}

#[test]
fn hovering_a_real_material_viewport_blocks_the_timeline_and_leaving_restores_it() {
    let mut f = Fixture::new();
    let program = aestra_core::MaterialProgramId::new();
    let graph = f
        .app
        .world_mut()
        .spawn((
            MaterialGraphViewport {
                program,
                scope: None,
                editing_target: crate::material_target::MaterialEditingTarget::Program {
                    root: std::path::PathBuf::new(),
                    id: program,
                },
            },
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::ZERO),
            },
        ))
        .id();
    f.assert_blocked();
    f.app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(graph)
        .unwrap()
        .cursor_over = false;
    f.control(KeyCode::Enter, Key::Enter);
    f.app.update();
    f.assert_actions(&[Routed::Choreography(ChoreographyAction::AddEmitter)]);
}

#[test]
fn effect_parameter_bindings_take_priority_and_vector_channels_reject_stale_indices() {
    let mut f = Fixture::new();
    let mut lane = f.curve_lane("size");
    let mut session = f.app.world_mut().resource_mut::<EditorSession>();
    let id = aestra_core::ParameterId::new();
    session
        .effect
        .parameters
        .push(aestra_core::EffectParameter {
            id,
            name: "Bound".into(),
            exposed: true,
            default: aestra_core::Value::Vec3Curve(aestra_core::Vec3Curve {
                curves: std::array::from_fn(|axis| {
                    Curve::new(vec![
                        CurveKey::new(0.0, axis as f32 * 10.0),
                        CurveKey::new(1.0, axis as f32 * 10.0 + 2.0),
                    ])
                }),
            }),
        });
    session.effect.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.id == lane.module)
        .unwrap()
        .bindings
        .insert(lane.parameter.clone(), id);
    for channel in 0..3 {
        lane.channel = Some(channel);
        let action = super::super::automation_data::add_automation_key_at_pointer_action(
            &session.effect,
            &lane,
            Vec2::ZERO,
        )
        .unwrap();
        let ChoreographyAction::AddAutomationKeyAt { value_bits, .. } = action else {
            unreachable!()
        };
        assert_eq!(
            f32::from_bits(value_bits.unwrap()),
            channel as f32 * 10.0 + 1.0
        );
    }
    for channel in [None, Some(3), Some(u8::MAX)] {
        lane.channel = channel;
        assert!(
            super::super::automation_data::add_automation_key_at_pointer_action(
                &session.effect,
                &lane,
                Vec2::ZERO
            )
            .is_none()
        );
    }
}

#[test]
fn navigation_restore_keeps_view_context_but_clears_transient_selection_and_motion() {
    let mut f = Fixture::new();
    let effect = f.app.world().resource::<EditorSession>().effect.id;
    let emitter = f.app.world().resource::<EditorSession>().effect.emitters[0].id;
    let lane = f.curve_lane("size");
    let mut state = f.app.world_mut().resource_mut::<TimelineState>();
    state.frame_all(10.0);
    state.zoom_at(5.0, 0.5, 10.0, 60);
    state.set_snap(TimelineSnapMode::Frames);
    state.expanded_automation_emitters.insert(emitter);
    state.visible_automation_lanes.insert(lane.clone());
    state.automation_lane_heights.insert(lane.clone(), 150.0);
    state.vertical_scroll = 80.0;
    state.select_only_emitter_region(emitter, EmitterRegionId::new());
    state.host_motion.effect = Some(effect);
    state.panning = true;
    state.snap_guide = Some(5.0);
    let snapshot = state.navigation_snapshot();
    state.restore_navigation(snapshot, 2.0);
    assert_eq!(state.view.start, 0.0);
    assert_eq!(state.view.end, 2.0);
    assert_eq!(state.snap, TimelineSnapMode::Frames);
    assert_eq!(state.vertical_scroll, 80.0);
    assert_eq!(state.automation_lane_heights.get(&lane), Some(&150.0));
    assert!(state.expanded_automation_emitters.contains(&emitter));
    assert!(state.visible_automation_lanes.contains(&lane));
    assert!(state.selected_emitters.is_empty() && state.selected_emitter_regions.is_empty());
    assert!(state.host_motion.effect.is_none() && state.snap_guide.is_none() && !state.panning);
}

#[test]
fn normalized_track_order_deduplicates_and_skips_stale_tracks_for_selection() {
    let mut f = Fixture::new();
    let mut session = f.app.world_mut().resource_mut::<EditorSession>();
    let first = session.effect.emitters[0].id;
    let second = session.effect.emitters[1].id;
    let clip = EffectClip::new(aestra_core::EffectId::new(), 0.0, 1.0);
    let clip_id = clip.id;
    session.effect.effect_clips.push(clip);
    session.effect.choreography_order = vec![
        ChoreographyTrackId::Emitter(second),
        ChoreographyTrackId::Emitter(EmitterId::new()),
        ChoreographyTrackId::EffectClip(clip_id),
        ChoreographyTrackId::Emitter(second),
        ChoreographyTrackId::Emitter(first),
    ];
    let order = super::super::state::normalized_choreography_order(&session.effect);
    assert_eq!(
        &order[..3],
        &[
            ChoreographyTrackId::Emitter(second),
            ChoreographyTrackId::EffectClip(clip_id),
            ChoreographyTrackId::Emitter(first)
        ]
    );
    assert_eq!(order.len(), session.effect.emitters.len() + 1);
    let effect = session.effect.clone();
    let mut state = f.app.world_mut().resource_mut::<TimelineState>();
    state.select_emitter(&effect, None, second, false, false);
    state.select_emitter(&effect, Some(second), first, false, true);
    assert_eq!(state.selected_local_emitters(&effect), [second, first]);
}
