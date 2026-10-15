//! Real motion/automation execution, preview transactions and ordered history on both engines.
use super::automation_data::{automation_lane_keys, automation_lane_value};
use super::motion_execution::{Field, NumberControl};
use super::motion_state::Action as Motion;
use super::*;
use crate::{
    CurvesState, ProjectEffectCatalog,
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    localization::Localizer,
    material_function_editor::FunctionEditor,
};
use aestra_core::{
    Curve, CurveKey, EffectParameter, EmitterTransform, HostTransformKey, HostTransformTrack,
    ModuleParameters, ParameterId, PropertyEvaluationDomain, PropertySource, PropertySourceValue,
    Value,
};
use bevy::{
    input::{
        ButtonState, InputPlugin,
        keyboard::{Key, KeyboardInput},
    },
    input_focus::{InputDispatchPlugin, InputFocusPlugin},
    time::TimeUpdateStrategy,
    ui_widgets::ValueChange,
    window::PrimaryWindow,
};
use std::time::Duration;

#[derive(Resource, Default)]
struct Unhandled(usize);
struct Fixture {
    _root: tempfile::TempDir,
    app: App,
}
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
impl Fixture {
    fn new(motion: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut effect = crate::test_support::effect_with_timing_slack();
        if motion {
            effect.host_transform_track = Some(track());
        }
        let session = EditorSession::from_test_effect(effect);
        let duration = session.playback_duration();
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(TimelineState::framed(duration))
            .insert_resource(ProjectEffectCatalog::scan(root.path()))
            .insert_resource(Localizer::new("en-US").unwrap())
            .init_resource::<CurvesState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Unhandled>()
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<FunctionEditor>()
            .add_observer(motion_execution::execute)
            .add_observer(motion_execution::number_changed)
            .add_observer(
                |action: On<ChoreographyAction>,
                 mut commands: Commands,
                 mut session: ResMut<EditorSession>,
                 mut curves: ResMut<CurvesState>,
                 mut state: ResMut<TimelineState>,
                 localizer: Res<Localizer>,
                 mut unhandled: ResMut<Unhandled>| {
                    if !automation_execution::handle_automation_action(
                        &action,
                        &mut commands,
                        &mut session,
                        &mut curves,
                        &mut state,
                        &localizer,
                    ) {
                        unhandled.0 += 1;
                    }
                },
            )
            .add_observer(
                |action: On<HistoryAction>, routing: Routing, mut commands: Commands| {
                    routing.execute(*action, &mut commands, |_, _, _, _| {
                        panic!("No filesystem history expected")
                    });
                },
            );
        Self { _root: root, app }
    }
    fn action<E: Event>(&mut self, action: E)
    where
        for<'a> E::Trigger<'a>: Default,
    {
        self.app.world_mut().trigger(action);
        self.app.world_mut().flush();
    }
    fn session(&self) -> &EditorSession {
        self.app.world().resource::<EditorSession>()
    }
    fn seek(&mut self, time: f32) {
        self.app
            .world_mut()
            .resource_mut::<EditorSession>()
            .seek_time(time);
    }
    fn lane(&self, parameter: &str) -> AutomationLaneId {
        let emitter = &self.session().effect.emitters[0];
        let module = emitter
            .modules
            .iter()
            .find(|m| matches!(m.parameters, ModuleParameters::Appearance { .. }))
            .unwrap();
        AutomationLaneId {
            emitter: emitter.id,
            module: module.id,
            input: 0,
            parameter: parameter.into(),
            channel: None,
        }
    }
    fn add(&mut self, lane: AutomationLaneId, time: f32, value: Option<f32>) {
        self.action(ChoreographyAction::AddAutomationKeyAt {
            lane,
            normalized_time_bits: time.to_bits(),
            value_bits: value.map(f32::to_bits),
        });
    }
    fn input(&mut self, field: Field, initial: f32) -> Entity {
        let effect = self.session().effect.id;
        self.app
            .world_mut()
            .spawn(NumberControl {
                effect,
                index: 1,
                field,
                initial,
            })
            .id()
    }
    fn change(&mut self, source: Entity, value: f32, is_final: bool) {
        self.app.world_mut().trigger(ValueChange {
            source,
            value,
            is_final,
        });
        self.app.world_mut().flush();
    }
    fn keyboard(&mut self) -> Entity {
        self.app
            .add_plugins((
                MinimalPlugins,
                InputPlugin,
                InputFocusPlugin,
                InputDispatchPlugin,
                crate::input::EditorKeyboardPlugin,
            ))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO))
            .init_resource::<ModulePaletteState>()
            .add_systems(
                Update,
                keyboard::choreography_keyboard_input.in_set(TimelineSet::Input),
            );
        self.app.world_mut().spawn(TimelineCanvas);
        let window = self
            .app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        self.app.update();
        window
    }
    fn tap(&mut self, window: Entity, code: KeyCode, key: Key) {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            self.app.world_mut().write_message(KeyboardInput {
                key_code: code,
                logical_key: key.clone(),
                state,
                repeat: false,
                text: None,
                window,
            });
        }
        self.app.update();
    }
    fn assert_no_edit(&self, before: &EffectAsset) {
        assert_eq!(&self.session().effect, before);
        assert!(!self.session().can_undo());
        assert!(!self.session().dirty);
    }
}

#[test]
fn motion_keyboard_insert_and_delete_execute_and_undo_without_duplicate_edits() {
    let mut f = Fixture::new(true);
    f.seek(0.5);
    f.action(Motion::Inspect);
    let before = f.session().effect.clone();
    let window = f.keyboard();
    f.tap(window, KeyCode::Insert, Key::Insert);
    assert_eq!(
        f.session()
            .effect
            .host_transform_track
            .as_ref()
            .unwrap()
            .keys()
            .len(),
        4
    );
    assert_eq!(
        f.app
            .world()
            .resource::<TimelineState>()
            .host_motion
            .selected,
        Some(1)
    );
    f.tap(window, KeyCode::Delete, Key::Delete);
    assert_eq!(f.session().effect, before);
    f.action(HistoryAction::Undo);
    assert_eq!(
        f.session()
            .effect
            .host_transform_track
            .as_ref()
            .unwrap()
            .keys()
            .len(),
        4
    );
    f.action(HistoryAction::Undo);
    f.assert_no_edit(&before);
    f.action(HistoryAction::Redo);
    f.action(HistoryAction::Redo);
    assert_eq!(f.session().effect, before);
}

#[test]
fn motion_add_creates_anchor_and_existing_time_only_selects_without_history() {
    let mut f = Fixture::new(false);
    let before = f.session().effect.clone();
    f.seek(0.75);
    f.action(Motion::Add);
    let created = f.session().effect.host_transform_track.clone().unwrap();
    assert_eq!(
        created
            .keys()
            .iter()
            .map(|key| key.time)
            .collect::<Vec<_>>(),
        [0.0, 0.75]
    );
    f.action(Motion::Add);
    assert_eq!(f.session().effect.host_transform_track, Some(created));
    f.action(HistoryAction::Undo);
    f.assert_no_edit(&before);
}

#[test]
fn inspect_select_and_stale_selection_do_not_mutate_authored_motion() {
    let mut f = Fixture::new(true);
    let before = f.session().effect.clone();
    f.seek(1.8);
    f.action(Motion::Inspect);
    assert_eq!(
        f.app
            .world()
            .resource::<TimelineState>()
            .host_motion
            .selected,
        Some(2)
    );
    f.action(Motion::Select(usize::MAX));
    assert_eq!(
        f.app
            .world()
            .resource::<TimelineState>()
            .host_motion
            .selected,
        Some(2)
    );
    f.action(Motion::Select(1));
    assert_eq!(
        f.app
            .world()
            .resource::<TimelineState>()
            .host_motion
            .selected,
        Some(1)
    );
    f.assert_no_edit(&before);
}

#[test]
fn first_anchor_delete_rejects_and_single_key_delete_removes_the_track() {
    let mut f = Fixture::new(true);
    let before = f.session().effect.clone();
    f.action(Motion::Select(0));
    f.action(Motion::Delete);
    f.assert_no_edit(&before);
    assert!(
        f.app
            .world()
            .resource::<TimelineState>()
            .host_motion
            .error
            .is_some()
    );
    let mut f = Fixture::new(false);
    f.action(Motion::Add);
    f.action(Motion::Delete);
    assert!(f.session().effect.host_transform_track.is_none());
    f.action(HistoryAction::Undo);
    assert_eq!(
        f.session()
            .effect
            .host_transform_track
            .as_ref()
            .unwrap()
            .keys()
            .len(),
        1
    );
}

#[test]
fn repeat_rejects_open_seams_close_loop_and_clear_are_reversible() {
    let mut f = Fixture::new(true);
    let before = f.session().effect.clone();
    f.action(Motion::Select(1));
    f.action(Motion::Repeat(true));
    f.assert_no_edit(&before);
    f.action(Motion::CloseLoop);
    let closed = f.session().effect.clone();
    let motion = closed.host_transform_track.as_ref().unwrap();
    assert!(motion.repeat);
    assert_eq!(
        motion.keys()[0].transform,
        motion.keys().last().unwrap().transform
    );
    f.action(Motion::Repeat(false));
    assert!(
        !f.session()
            .effect
            .host_transform_track
            .as_ref()
            .unwrap()
            .repeat
    );
    f.action(Motion::Clear);
    assert!(f.session().effect.host_transform_track.is_none());
    f.action(HistoryAction::Undo);
    f.action(HistoryAction::Undo);
    assert_eq!(f.session().effect, closed);
    f.action(HistoryAction::Undo);
    f.assert_no_edit(&before);
}

#[test]
fn inactive_and_stale_motion_owners_cannot_delete_clear_or_repeat() {
    for stale in [false, true] {
        let mut f = Fixture::new(true);
        let before = f.session().effect.clone();
        if stale {
            f.app
                .world_mut()
                .resource_mut::<TimelineState>()
                .host_motion
                .effect = Some(aestra_core::EffectId::new());
        }
        for action in [
            Motion::Delete,
            Motion::Clear,
            Motion::Repeat(true),
            Motion::CloseLoop,
        ] {
            f.action(action);
        }
        f.assert_no_edit(&before);
    }
}

#[test]
fn numeric_live_preview_then_final_is_exactly_one_reversible_commit() {
    let mut f = Fixture::new(true);
    f.action(Motion::Select(1));
    let before = f.session().effect.clone();
    let source = f.input(Field::Position(0), 10.0);
    let revision = f.session().ui_revision;
    f.change(source, 15.0, false);
    f.change(source, 18.0, false);
    assert_eq!(f.session().effect, before);
    assert!(!f.session().can_undo());
    assert_eq!(f.session().ui_revision, revision);
    assert_eq!(
        f.session()
            .preview()
            .unwrap()
            .host_transform_at(1.0)
            .translation[0],
        18.0
    );
    f.change(source, 18.0, true);
    let edited = f.session().effect.clone();
    assert_ne!(edited, before);
    f.action(HistoryAction::Undo);
    f.assert_no_edit(&before);
    f.action(HistoryAction::Redo);
    assert_eq!(f.session().effect, edited);
}

#[test]
fn numeric_unchanged_and_invalid_final_values_restore_committed_preview() {
    for value in [10.0, f32::INFINITY, f32::NAN] {
        let mut f = Fixture::new(true);
        f.action(Motion::Select(1));
        let before = f.session().effect.clone();
        let source = f.input(Field::Position(0), 10.0);
        f.change(source, 18.0, false);
        f.change(source, value, true);
        f.assert_no_edit(&before);
        assert_eq!(
            f.session()
                .preview()
                .unwrap()
                .host_transform_at(1.0)
                .translation[0],
            10.0
        );
        assert!(!motion_execution::busy(
            f.app.world().resource::<TimelineState>()
        ));
    }
}

#[test]
fn numeric_escape_cancels_and_late_release_cannot_commit() {
    let mut f = Fixture::new(true);
    f.action(Motion::Select(1));
    let before = f.session().effect.clone();
    let source = f.input(Field::Position(0), 10.0);
    f.change(source, 15.0, false);
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    f.change(source, 20.0, true);
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    f.change(source, 20.0, true);
    f.assert_no_edit(&before);
    assert_eq!(
        f.session()
            .preview()
            .unwrap()
            .host_transform_at(1.0)
            .translation[0],
        10.0
    );
}

#[test]
fn numeric_retime_rejects_collisions_and_reorders_selection_after_valid_commit() {
    let mut f = Fixture::new(true);
    f.action(Motion::Select(1));
    let before = f.session().effect.clone();
    let source = f.input(Field::Time, 1.0);
    f.change(source, 2.0, true);
    f.assert_no_edit(&before);
    f.change(source, 1.5, true);
    assert_eq!(
        f.session()
            .effect
            .host_transform_track
            .as_ref()
            .unwrap()
            .keys()[1]
            .time,
        1.5
    );
    f.action(HistoryAction::Undo);
    f.assert_no_edit(&before);
}

#[test]
fn numeric_stale_effect_selection_or_unregistered_source_cannot_mutate() {
    for stale in 0..3 {
        let mut f = Fixture::new(true);
        f.action(Motion::Select(1));
        let before = f.session().effect.clone();
        let source = f.input(Field::Position(0), 10.0);
        match stale {
            0 => {
                f.app
                    .world_mut()
                    .get_mut::<NumberControl>(source)
                    .unwrap()
                    .effect = aestra_core::EffectId::new()
            }
            1 => f.action(Motion::Select(2)),
            _ => {
                f.app
                    .world_mut()
                    .entity_mut(source)
                    .remove::<NumberControl>();
            }
        }
        f.change(source, 20.0, true);
        f.assert_no_edit(&before);
    }
}

#[test]
fn keyboard_automation_insert_delete_executes_and_undo_restores_exact_value() {
    let mut f = Fixture::new(false);
    let lane = f.lane("size");
    let before = f.session().effect.clone();
    let window = f.keyboard();
    f.app.world_mut().spawn((
        TimelineAutomationLaneGraph(lane.clone()),
        RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::ZERO),
        },
    ));
    f.tap(window, KeyCode::Insert, Key::Insert);
    let inserted = f.session().effect.clone();
    let selection = f
        .app
        .world()
        .resource::<TimelineState>()
        .selected_automation_key
        .clone()
        .unwrap();
    assert_eq!(
        automation_lane_keys(&inserted, &lane).unwrap().len(),
        automation_lane_keys(&before, &lane).unwrap().len() + 1
    );
    assert_eq!(
        f.app
            .world()
            .resource::<CurvesState>()
            .selected_key()
            .unwrap()
            .key,
        selection.key
    );
    f.tap(window, KeyCode::Delete, Key::Delete);
    assert_eq!(f.session().effect, before);
    f.action(HistoryAction::Undo);
    assert_eq!(f.session().effect, inserted);
    f.action(HistoryAction::Undo);
    f.assert_no_edit(&before);
}

#[test]
fn duplicate_automation_time_selects_existing_key_without_an_edit() {
    let mut f = Fixture::new(false);
    let lane = f.lane("size");
    let before = f.session().effect.clone();
    let first = automation_lane_keys(&before, &lane)
        .unwrap()
        .times()
        .next()
        .unwrap();
    f.add(lane.clone(), first + 0.0001, Some(99.0));
    f.assert_no_edit(&before);
    assert_eq!(
        f.app
            .world()
            .resource::<TimelineState>()
            .selected_automation_key
            .as_ref()
            .unwrap()
            .key,
        0
    );
    assert_eq!(
        f.app
            .world()
            .resource::<CurvesState>()
            .selected_key()
            .unwrap()
            .module,
        lane.module
    );
}

#[test]
fn gradient_insert_samples_color_and_delete_undo_preserve_identity_and_keys() {
    let mut f = Fixture::new(false);
    let lane = f.lane("color");
    let before = f.session().effect.clone();
    let Some(Value::Gradient(original)) = automation_lane_value(&before, &lane) else {
        panic!("Gradient fixture")
    };
    f.add(lane.clone(), 0.25, None);
    let Some(Value::Gradient(edited)) = automation_lane_value(&f.session().effect, &lane) else {
        unreachable!()
    };
    assert!(
        edited
            .keys
            .iter()
            .any(|key| key.time == 0.25 && key.color == original.sample(0.25))
    );
    f.action(HistoryAction::Undo);
    f.assert_no_edit(&before);
    f.action(HistoryAction::Redo);
    assert_eq!(
        automation_lane_value(&f.session().effect, &lane),
        Some(Value::Gradient(edited))
    );
}

#[test]
fn automation_minimum_keys_and_stale_lane_or_key_reject_without_history() {
    for stale in 0..3 {
        let mut f = Fixture::new(false);
        let mut lane = f.lane("size");
        let mut effect = f.session().effect.clone();
        let module = effect.emitters[0]
            .modules
            .iter_mut()
            .find(|m| m.id == lane.module)
            .unwrap();
        let ModuleParameters::Appearance { size, .. } = &mut module.parameters else {
            unreachable!()
        };
        size.keys.remove(1);
        f.app
            .insert_resource(EditorSession::from_test_effect(effect.clone()));
        let before = effect;
        if stale == 1 {
            lane.module = aestra_core::ModuleId::new();
        }
        f.action(ChoreographyAction::DeleteAutomationKey(
            TimelineAutomationKeySelection {
                lane,
                key: if stale == 2 { usize::MAX } else { 0 },
            },
        ));
        f.assert_no_edit(&before);
    }
    let mut f = Fixture::new(false);
    let mut lane = f.lane("size");
    lane.emitter = EmitterId::new();
    let before = f.session().effect.clone();
    f.add(lane, 0.5, None);
    f.assert_no_edit(&before);
}

#[test]
fn automation_selection_preserves_selected_region_and_visibility_is_nonsemantic() {
    let mut f = Fixture::new(false);
    let lane = f.lane("size");
    let before = f.session().effect.clone();
    let region = before.emitters[0].implicit_region_id();
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .select_only_emitter_region(lane.emitter, region);
    f.action(ChoreographyAction::SelectAutomationKey(
        TimelineAutomationKeySelection {
            lane: lane.clone(),
            key: 0,
        },
    ));
    assert_eq!(
        f.app
            .world()
            .resource::<TimelineState>()
            .selected_emitter_region,
        Some((lane.emitter, region))
    );
    f.action(ChoreographyAction::ToggleEmitterAutomation(lane.emitter));
    f.action(ChoreographyAction::SetEmitterAutomationVisibility {
        emitter: lane.emitter,
        lanes: vec![lane.clone()],
        visible: true,
    });
    assert!(
        f.app
            .world()
            .resource::<TimelineState>()
            .visible_automation_lanes
            .contains(&lane)
    );
    f.action(ChoreographyAction::SetAutomationLaneVisibility {
        lane: lane.clone(),
        visible: false,
    });
    assert!(
        f.app
            .world()
            .resource::<TimelineState>()
            .selected_automation_key
            .is_none()
    );
    f.action(ChoreographyAction::SetEmitterAutomationVisibility {
        emitter: lane.emitter,
        lanes: vec![lane],
        visible: false,
    });
    f.assert_no_edit(&before);
}

#[test]
fn scalar_effect_parameter_binding_edits_parameter_not_module_bank() {
    let mut f = Fixture::new(false);
    let lane = f.lane("size");
    let id = ParameterId::new();
    let mut effect = f.session().effect.clone();
    effect.parameters.push(EffectParameter {
        id,
        name: "Size".into(),
        exposed: true,
        default: Value::Curve(Curve::new(vec![
            CurveKey::new(0.0, 1.0),
            CurveKey::new(1.0, 2.0),
        ])),
    });
    effect.emitters[0]
        .modules
        .iter_mut()
        .find(|m| m.id == lane.module)
        .unwrap()
        .bindings
        .insert("size".into(), id);
    f.app
        .insert_resource(EditorSession::from_test_effect(effect.clone()));
    f.add(lane.clone(), 0.5, Some(1.25));
    assert_eq!(f.session().effect.emitters, effect.emitters);
    let Value::Curve(curve) = &f.session().effect.parameters.last().unwrap().default else {
        unreachable!()
    };
    assert!(curve.keys.contains(&CurveKey::new(0.5, 1.25)));
    f.action(HistoryAction::Undo);
    f.assert_no_edit(&effect);
}

#[test]
fn vector_source_bank_edit_changes_only_the_requested_channel() {
    let mut f = Fixture::new(false);
    let mut effect = f.session().effect.clone();
    let emitter = effect.emitters[0].id;
    let module = effect.emitters[0]
        .modules
        .iter_mut()
        .find(|m| m.module_type.0 == aestra_core::MODULE_MOTION)
        .unwrap();
    let id = module.id;
    let source = PropertySource::Curve(PropertyEvaluationDomain::ParticleLife);
    let original = aestra_core::Vec3Curve {
        curves: std::array::from_fn(|axis| {
            Curve::new(vec![
                CurveKey::new(0.0, axis as f32 * 0.1),
                CurveKey::new(1.0, axis as f32 * 0.1 + 0.1),
            ])
        }),
    };
    module.property_sources.insert("gravity".into(), source);
    module.property_source_values.insert(
        "gravity".into(),
        vec![PropertySourceValue::new(
            source,
            Value::Vec3Curve(original.clone()),
        )],
    );
    f.app
        .insert_resource(EditorSession::from_test_effect(effect.clone()));
    let lane = AutomationLaneId {
        emitter,
        module: id,
        input: 0,
        parameter: "gravity".into(),
        channel: Some(1),
    };
    f.add(lane.clone(), 0.5, Some(0.15));
    let Some(Value::Vec3Curve(edited)) = automation_lane_value(&f.session().effect, &lane) else {
        unreachable!()
    };
    assert_eq!(edited.curves[0], original.curves[0]);
    assert_eq!(edited.curves[2], original.curves[2]);
    assert_eq!(edited.curves[1].keys.len(), 3);
    assert_eq!(
        f.app
            .world()
            .resource::<CurvesState>()
            .selected_vector_channel(),
        1
    );
    f.action(HistoryAction::Undo);
    f.assert_no_edit(&effect);
}

#[test]
fn mixed_motion_and_automation_actions_follow_real_global_history_order_and_fork_redo() {
    let mut f = Fixture::new(true);
    let lane = f.lane("size");
    let before = f.session().effect.clone();
    f.seek(0.5);
    f.action(Motion::Add);
    let motion_only = f.session().effect.clone();
    f.add(lane.clone(), 0.5, Some(1.0));
    let both = f.session().effect.clone();
    f.action(HistoryAction::Undo);
    assert_eq!(f.session().effect, motion_only);
    f.action(HistoryAction::Undo);
    f.assert_no_edit(&before);
    f.action(HistoryAction::Redo);
    f.action(HistoryAction::Redo);
    assert_eq!(f.session().effect, both);
    f.action(HistoryAction::Undo);
    f.add(lane, 0.6, Some(1.5));
    let fork = f.session().effect.clone();
    f.action(HistoryAction::Redo);
    assert_eq!(f.session().effect, fork);
}

#[test]
fn automation_add_at_playhead_uses_selected_region_source_offset() {
    let mut f = Fixture::new(false);
    let lane = f.lane("size");
    let mut effect = f.session().effect.clone();
    let region = aestra_core::EmitterRegion {
        id: EmitterRegionId::new(),
        start_time: 1.0,
        source_offset: 0.5,
        duration: 1.0,
    };
    effect.emitters[0].regions = vec![region];
    f.app
        .insert_resource(EditorSession::from_test_effect(effect.clone()));
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .select_only_emitter_region(lane.emitter, region.id);
    f.seek(1.5);
    f.action(ChoreographyAction::AddAutomationKey(lane.clone()));
    let times = automation_lane_keys(&f.session().effect, &lane)
        .unwrap()
        .times()
        .collect::<Vec<_>>();
    assert!(times.iter().any(|time| (*time - 0.4).abs() < 0.0001));
    f.action(HistoryAction::Undo);
    f.assert_no_edit(&effect);
}

#[test]
fn nonfinite_automation_key_bits_reject_without_semantic_or_history_mutation() {
    for (time, value) in [
        (f32::NAN, Some(1.0)),
        (0.5, Some(f32::NAN)),
        (0.5, Some(f32::INFINITY)),
    ] {
        let mut f = Fixture::new(false);
        let lane = f.lane("size");
        let before = f.session().effect.clone();
        f.add(lane, time, value);
        f.assert_no_edit(&before);
        assert!(
            f.app
                .world()
                .resource::<TimelineState>()
                .selected_automation_key
                .is_none()
        );
    }
}

#[test]
fn numeric_loop_endpoint_pose_edits_preserve_the_seam_and_undo() {
    for field in [Field::Position(0), Field::Rotation(2), Field::Scale(1)] {
        let mut f = Fixture::new(true);
        let mut effect = f.session().effect.clone();
        let track = effect.host_transform_track.as_mut().unwrap();
        track
            .curves
            .set_pose(track.end_time(), track.keys()[0].transform, false);
        track.repeat = true;
        track.validate().unwrap();
        f.app
            .insert_resource(EditorSession::from_test_effect(effect.clone()));
        f.action(Motion::Select(0));
        let source = f.input(
            field,
            if matches!(field, Field::Scale(_)) {
                1.0
            } else {
                0.0
            },
        );
        f.app
            .world_mut()
            .get_mut::<NumberControl>(source)
            .unwrap()
            .index = 0;
        f.change(source, 2.5, true);
        let track = f.session().effect.host_transform_track.as_ref().unwrap();
        assert_eq!(
            track.keys()[0].transform,
            track.keys().last().unwrap().transform
        );
        assert_ne!(f.session().effect, effect);
        track.validate().unwrap();
        f.action(HistoryAction::Undo);
        f.assert_no_edit(&effect);
    }
}

#[test]
fn automation_adapter_declines_unowned_actions_without_mutating_session() {
    let mut f = Fixture::new(false);
    let before = f.session().effect.clone();
    f.action(ChoreographyAction::AddEmitter);
    assert_eq!(f.app.world().resource::<Unhandled>().0, 1);
    f.assert_no_edit(&before);
}
