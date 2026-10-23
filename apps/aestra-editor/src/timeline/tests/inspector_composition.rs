//! Construct the real inspector, numeric scenes, action payloads and scroll wrappers on both engines.
use super::super::{host_composition, motion_execution};
use super::*;
use crate::{
    CurvesState, Localizer, ProjectEffectCatalog,
    curves::CurvesAction,
    feathers::{
        action_activation::FeathersActionButton,
        field_row::{FIELD_CONTROL_MIN_WIDTH, FIELD_LABEL_WIDTH},
        number_input::NumberScrubPlugin,
        scroll::{PersistedScroll, spawn_scroll_area_xy},
        text_input::tests::{app_with, focus, tap, value},
    },
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    material_function_editor::FunctionEditor,
};
use aestra_core::{EffectAsset, EmitterTransform, HostTransformKey, HostTransformTrack};
use bevy::{
    ecs::system::RunSystemOnce,
    input::keyboard::Key,
    text::{EditableText, TextEdit},
    ui_widgets::{Activate, ControlOrientation, ScrollArea, Scrollbar, ValueChange},
};

#[derive(Resource)]
struct Built {
    root: Entity,
    accepted: bool,
}
#[derive(Resource, Default)]
struct Values(Vec<(Entity, f32, bool)>);
fn build(mut commands: Commands, session: Res<EditorSession>, state: Res<TimelineState>) {
    let mut accepted = false;
    let root = commands
        .spawn(Node::default())
        .with_children(|parent| {
            accepted = spawn_inspector(parent, &session, &state);
        })
        .id();
    commands.insert_resource(Built { root, accepted });
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
struct Fixture {
    _root: tempfile::TempDir,
    app: App,
    before: EffectAsset,
    path: std::path::PathBuf,
    bytes: Vec<u8>,
}
impl Fixture {
    fn new() -> Self {
        Self::configured(|_, _| {})
    }
    fn configured(prepare: impl FnOnce(&mut EditorSession, &mut TimelineState)) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut effect = EffectAsset::new("Inspector", 4.0);
        effect.host_transform_track = Some(track());
        let mut session = EditorSession::from_test_effect(effect);
        let mut state = TimelineState::framed(4.0);
        motion_execution::select(&mut session, &mut state, Some(1));
        prepare(&mut session, &mut state);
        let before = session.effect.clone();
        let path = root.path().join("motion.aestra.ron");
        before.save_ron(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mut app = app_with(|app| {
            app.init_asset::<crate::feathers::icon::SvgFile>()
                .insert_resource(session)
                .insert_resource(state)
                .insert_resource(Localizer::new("en-US").unwrap())
                .insert_resource(ProjectEffectCatalog::scan(root.path()))
                .init_resource::<CurvesState>()
                .init_resource::<EditorHistoryLedger>()
                .init_resource::<MaterialProgramEditHistory>()
                .init_resource::<FunctionEditor>()
                .init_resource::<Values>()
                .add_plugins(NumberScrubPlugin)
                .add_observer(host_composition::activate)
                .add_observer(motion_execution::execute)
                .add_observer(motion_execution::number_changed)
                .add_observer(|event: On<ValueChange<f32>>, mut values: ResMut<Values>| {
                    values.0.push((event.source, event.value, event.is_final))
                })
                .add_observer(
                    |event: On<HistoryAction>, routing: Routing, mut commands: Commands| {
                        routing.execute(*event, &mut commands, |_, _, _, _| {
                            panic!("No filesystem history expected")
                        });
                    },
                )
                .add_systems(Startup, build)
                .add_systems(
                    Update,
                    (
                        initialize_numbers,
                        host_composition::buttons,
                        motion_execution::cancel_drag,
                    )
                        .chain(),
                );
        });
        for _ in 0..3 {
            app.update();
        }
        Self {
            _root: root,
            app,
            before,
            path,
            bytes,
        }
    }
    fn session(&self) -> &EditorSession {
        self.app.world().resource()
    }
    fn state(&self) -> &TimelineState {
        self.app.world().resource()
    }
    fn event<E: Event>(&mut self, event: E)
    where
        for<'a> E::Trigger<'a>: Default,
    {
        self.app.world_mut().trigger(event);
        self.app.world_mut().flush();
    }
    fn inputs(&mut self) -> Vec<(Entity, NumberControl)> {
        let w = self.app.world_mut();
        w.query::<(Entity, &NumberControl)>()
            .iter(w)
            .map(|(e, c)| (e, *c))
            .collect()
    }
    fn field(&mut self, target: Field) -> Entity {
        self.inputs()
            .into_iter()
            .find_map(|(e, c)| {
                let same = match (c.field, target) {
                    (Field::Time, Field::Time) => true,
                    (Field::Position(a), Field::Position(b))
                    | (Field::Rotation(a), Field::Rotation(b))
                    | (Field::Scale(a), Field::Scale(b)) => a == b,
                    _ => false,
                };
                same.then_some(e)
            })
            .unwrap()
    }
    fn text(&self, owner: Entity) -> Entity {
        let native = self
            .app
            .world()
            .get::<Children>(owner)
            .unwrap()
            .iter()
            .find(|&e| {
                self.app
                    .world()
                    .get::<bevy::feathers::controls::FeathersNumberInput>(e)
                    .is_some()
            })
            .unwrap();
        self.app
            .world()
            .get::<Children>(native)
            .unwrap()
            .iter()
            .find(|e| self.app.world().get::<EditableText>(*e).is_some())
            .unwrap()
    }
    fn labels(&mut self) -> Vec<String> {
        let w = self.app.world_mut();
        w.query::<&Text>().iter(w).map(|t| t.0.clone()).collect()
    }
    fn change(&mut self, field: Field, value: f32, is_final: bool) {
        let source = self.field(field);
        self.event(ValueChange {
            source,
            value,
            is_final,
        });
    }
    fn click_action(&mut self, test: impl Fn(&Action) -> bool) {
        let w = self.app.world_mut();
        let e = w
            .query::<(Entity, &Action)>()
            .iter(w)
            .find_map(|(e, a)| test(a).then_some(e))
            .unwrap();
        self.event(Activate { entity: e });
        self.app.update();
    }
    fn clean(&self) {
        assert_eq!(self.session().effect, self.before);
        assert!(!self.session().can_undo());
        self.files_unchanged();
    }
    fn files_unchanged(&self) {
        assert_eq!(std::fs::read(&self.path).unwrap(), self.bytes);
    }
    fn roundtrip(&mut self) {
        let after = self.session().effect.clone();
        assert_ne!(after, self.before);
        self.event(HistoryAction::Undo);
        assert_eq!(self.session().effect, self.before);
        assert!(
            !self.session().can_undo(),
            "Exactly one semantic transaction"
        );
        self.event(HistoryAction::Redo);
        assert_eq!(self.session().effect, after);
        self.files_unchanged();
    }
}

#[test]
fn inactive_inspector_declines_without_spawning_content() {
    let f = Fixture::configured(|_, s| s.host_motion.effect = None);
    let b = f.app.world().resource::<Built>();
    assert!(!b.accepted);
    assert!(f.app.world().get::<Children>(b.root).is_none());
    f.clean();
}

#[test]
fn selected_key_builds_ten_real_fields_with_silent_initialized_text_and_values() {
    let mut f = Fixture::new();
    assert!(f.app.world().resource::<Built>().accepted);
    let fields = f.inputs();
    assert_eq!(fields.len(), 10);
    for (e, c) in fields {
        let scrub = f.app.world().get::<ScrubbableNumber>(e).unwrap();
        assert_eq!(scrub.value, c.initial);
        assert_eq!(value(&f.app, f.text(e)).parse::<f32>().unwrap(), c.initial);
        assert_eq!(c.effect, f.session().effect.id);
        assert_eq!(c.index, 1);
    }
    assert!(f.app.world().resource::<Values>().0.is_empty());
    f.clean();
}

#[test]
fn anchor_has_nine_pose_fields_and_no_editable_time() {
    let mut f = Fixture::configured(|_, s| s.host_motion.selected = Some(0));
    let fields = f.inputs();
    assert_eq!(fields.len(), 9);
    assert!(fields.iter().all(|(_, c)| !matches!(c.field, Field::Time)));
    assert!(f.labels().iter().any(|s| s == "Time: 0 s (anchor)"));
    f.clean();
}

#[test]
fn missing_track_has_actionable_empty_state_without_numeric_controls() {
    let mut f = Fixture::configured(|session, _| session.effect.host_transform_track = None);
    assert!(f.inputs().is_empty());
    assert!(f.labels().iter().any(|s| s.starts_with("No motion track.")));
    let w = f.app.world_mut();
    assert_eq!(w.query::<&Action>().iter(w).count(), 3);
    f.clean();
}

#[test]
fn stale_selected_key_is_a_prompt_instead_of_stale_numeric_targets() {
    for selected in [None, Some(99)] {
        let mut f = Fixture::configured(|_, s| s.host_motion.selected = selected);
        assert!(f.inputs().is_empty());
        assert!(
            f.labels()
                .iter()
                .any(|s| s == "Select a pose key in the Host Motion lane.")
        );
        f.clean();
    }
}

#[test]
fn current_motion_error_is_retained_in_the_constructed_inspector() {
    let mut f =
        Fixture::configured(|_, s| s.host_motion.error = Some("Existing motion error".into()));
    assert!(f.labels().iter().any(|s| s == "Existing motion error"));
    f.clean();
}

#[test]
fn inspector_scroll_area_keeps_actual_properties_memory_and_adjacent_vertical_bar() {
    let mut f = Fixture::new();
    let w = f.app.world_mut();
    let (target, key, parent) = w
        .query::<(Entity, &PersistedScroll, &ChildOf)>()
        .single(w)
        .unwrap();
    assert_eq!(key.0, crate::ScrollMemoryKey::Properties);
    assert!(w.get::<ScrollArea>(target).is_some());
    let parent = parent.parent();
    let (bar, bar_parent) = w.query::<(&Scrollbar, &ChildOf)>().single(w).unwrap();
    assert_eq!(bar.target, target);
    assert_eq!(bar.orientation, ControlOrientation::Vertical);
    assert_eq!(bar_parent.parent(), parent);
    assert_eq!(
        w.get::<Node>(parent).unwrap().flex_direction,
        FlexDirection::Row
    );
    f.clean();
}

#[test]
fn curves_link_uses_actual_public_action_identity_and_feathers_composition() {
    let mut f = Fixture::new();
    let w = f.app.world_mut();
    let (e, action) = w.query::<(Entity, &CurvesAction)>().single(w).unwrap();
    assert_eq!(*action, CurvesAction::OpenHost(0));
    assert!(w.get::<FeathersActionButton>(e).is_some());
    assert_eq!(
        w.get::<AccessibleLabel>(e).unwrap().0,
        "Edit transform curves"
    );
    f.clean();
}

#[test]
fn closed_repeat_track_preserves_selected_mode_and_loop_explanation() {
    let mut f = Fixture::configured(|session, _| {
        let t = session.effect.host_transform_track.as_mut().unwrap();
        t.curves.set_pose(2.0, EmitterTransform::default(), false);
        t.repeat = true;
    });
    let labels = f.labels();
    assert!(labels.iter().any(|s| s == "● Repeat"));
    assert!(
        labels
            .iter()
            .any(|s| s.starts_with("First and last pose edits"))
    );
    f.clean();
}

#[test]
fn built_position_field_previews_then_commits_exactly_one_globally_reversible_edit() {
    let mut f = Fixture::new();
    f.change(Field::Position(0), 18.0, false);
    f.clean();
    assert_eq!(
        f.session()
            .preview()
            .unwrap()
            .host_transform_at(1.0)
            .translation[0],
        18.0
    );
    f.change(Field::Position(0), 18.0, true);
    assert_eq!(
        f.session()
            .effect
            .host_transform_track
            .as_ref()
            .unwrap()
            .keys()[1]
            .transform
            .translation[0],
        18.0
    );
    f.roundtrip();
}

#[test]
fn invalid_final_scale_restores_authored_pose_and_keeps_history_clean() {
    let mut f = Fixture::new();
    f.change(Field::Scale(0), 2.0, false);
    f.change(Field::Scale(0), 0.0, true);
    assert_eq!(
        f.session().preview().unwrap().host_transform_at(1.0).scale[0],
        1.0
    );
    assert!(f.state().host_motion.error.is_some());
    f.clean();
}

#[test]
fn built_time_field_reorders_keys_and_preserves_the_moved_pose() {
    let mut f = Fixture::new();
    f.change(Field::Time, 3.0, true);
    let keys = f
        .session()
        .effect
        .host_transform_track
        .as_ref()
        .unwrap()
        .keys();
    assert_eq!(keys[2].time, 3.0);
    assert_eq!(keys[2].transform.translation[0], 10.0);
    assert_eq!(f.state().host_motion.selected, Some(2));
    f.roundtrip();
}

#[test]
fn degree_field_changes_valid_quaternion_through_real_numeric_consumer() {
    let mut f = Fixture::new();
    f.change(Field::Rotation(2), 90.0, true);
    let pose = f
        .session()
        .effect
        .host_transform_track
        .as_ref()
        .unwrap()
        .keys()[1]
        .transform;
    let (_, _, z) = Quat::from_array(pose.rotation).to_euler(EulerRot::XYZ);
    assert!((z.to_degrees() - 90.0).abs() < 0.001);
    assert!(pose.is_valid());
    f.roundtrip();
}

#[test]
fn constructed_numeric_ranges_steps_and_all_axes_match_domain_contract() {
    let mut f = Fixture::new();
    for (e, c) in f.inputs() {
        let s = f.app.world().get::<ScrubbableNumber>(e).unwrap();
        let expected = match c.field {
            Field::Time => (0.0, f32::MAX, 0.01),
            Field::Position(_) => (-f32::MAX, f32::MAX, 0.1),
            Field::Rotation(_) => (-360.0, 360.0, 0.5),
            Field::Scale(_) => (0.001, f32::MAX, 0.01),
        };
        assert_eq!((s.min, s.max, s.step), expected);
    }
    f.clean();
}

#[test]
fn escape_cancels_built_numeric_preview_and_late_final_cannot_commit() {
    let mut f = Fixture::new();
    f.change(Field::Position(0), 30.0, false);
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    f.app
        .world_mut()
        .run_system_once(motion_execution::cancel_drag)
        .unwrap();
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    f.change(Field::Position(0), 30.0, true);
    assert_eq!(
        f.session()
            .preview()
            .unwrap()
            .host_transform_at(1.0)
            .translation[0],
        10.0
    );
    f.clean();
}

#[test]
fn real_programmatic_display_update_is_silent_and_not_an_authored_edit() {
    let mut f = Fixture::new();
    let e = f.field(Field::Position(0));
    f.event(UpdateNumberInput {
        entity: e,
        value: NumberInputValue::F32(42.0),
    });
    for _ in 0..3 {
        f.app.update();
    }
    assert_eq!(value(&f.app, f.text(e)), "42");
    assert!(f.app.world().resource::<Values>().0.is_empty());
    f.clean();
}

#[test]
fn real_text_edit_previews_and_enter_commits_once_at_the_constructed_field() {
    let mut f = Fixture::new();
    let owner = f.field(Field::Position(0));
    let input = f.text(owner);
    focus(&mut f.app, input);
    {
        let mut text = f.app.world_mut().get_mut::<EditableText>(input).unwrap();
        text.queue_edit(TextEdit::SelectAll);
        text.queue_edit(TextEdit::Insert("25".into()));
    }
    for _ in 0..3 {
        f.app.update();
    }
    f.clean();
    assert_eq!(
        f.session()
            .preview()
            .unwrap()
            .host_transform_at(1.0)
            .translation[0],
        25.0
    );
    tap(&mut f.app, KeyCode::Enter, Key::Enter, None);
    assert_eq!(
        f.session()
            .effect
            .host_transform_track
            .as_ref()
            .unwrap()
            .keys()[1]
            .transform
            .translation[0],
        25.0
    );
    f.roundtrip();
}

#[test]
fn real_inspector_add_delete_and_clear_buttons_dispatch_single_history_steps() {
    for mode in 0..3 {
        let mut f = Fixture::new();
        f.app
            .world_mut()
            .resource_mut::<EditorSession>()
            .seek_time(0.5);
        f.click_action(|a| match mode {
            0 => matches!(a, Action::Add),
            1 => matches!(a, Action::Delete),
            _ => matches!(a, Action::Clear),
        });
        match mode {
            0 => assert_eq!(
                f.session()
                    .effect
                    .host_transform_track
                    .as_ref()
                    .unwrap()
                    .keys()
                    .len(),
                4
            ),
            1 => assert_eq!(
                f.session()
                    .effect
                    .host_transform_track
                    .as_ref()
                    .unwrap()
                    .keys()
                    .len(),
                2
            ),
            _ => assert!(f.session().effect.host_transform_track.is_none()),
        }
        f.roundtrip();
    }
}

#[test]
fn repeat_requires_closed_seam_and_close_loop_remains_globally_reversible() {
    let mut f = Fixture::new();
    f.click_action(|a| matches!(a, Action::Repeat(true)));
    assert!(f.state().host_motion.error.is_some());
    f.clean();
    f.click_action(|a| matches!(a, Action::CloseLoop));
    let t = f.session().effect.host_transform_track.as_ref().unwrap();
    assert!(t.repeat);
    t.validate().unwrap();
    f.roundtrip();
}

#[test]
fn shared_generic_action_button_api_keeps_primary_and_secondary_payloads() {
    let mut f = Fixture::new();
    let mut commands = f.app.world_mut().commands();
    commands.spawn(Node::default()).with_children(|parent| {
        crate::spawn_feathers_action_button(parent, "Primary", CurvesAction::HostAddKey, true);
        crate::spawn_feathers_action_button(
            parent,
            "Secondary",
            CurvesAction::HostDeleteKey,
            false,
        );
    });
    f.app.world_mut().flush();
    let w = f.app.world_mut();
    let actions = w
        .query::<(&CurvesAction, &AccessibleLabel, Has<FeathersActionButton>)>()
        .iter(w)
        .map(|(a, label, marked)| (*a, label.0.clone(), marked))
        .collect::<Vec<_>>();
    assert!(actions.contains(&(CurvesAction::HostAddKey, "Primary".into(), true)));
    assert!(actions.contains(&(CurvesAction::HostDeleteKey, "Secondary".into(), true)));
    f.clean();
}

#[test]
fn shared_field_row_configurations_preserve_stable_indentation_and_minimum_width() {
    let mut f = Fixture::new();
    let mut row = Entity::PLACEHOLDER;
    let mut commands = f.app.world_mut().commands();
    commands.spawn(Node::default()).with_children(|parent| {
        spawn_field_row(
            parent,
            FieldRowProps::new("Nested")
                .indented(2)
                .with_control_min_width(1.0),
            (),
            |controls| {
                row = controls.target_entity();
            },
        );
    });
    f.app.world_mut().flush();
    let w = f.app.world();
    let controls = w.get::<Node>(row).unwrap();
    assert_eq!(controls.min_width, Val::Px(FIELD_CONTROL_MIN_WIDTH));
    let parent = w.get::<ChildOf>(row).unwrap().parent();
    let n = w.get::<Node>(parent).unwrap();
    assert_eq!(n.padding.left, Val::Px(24.0));
    let label = w.get::<Children>(parent).unwrap()[0];
    assert_eq!(
        w.get::<Node>(label).unwrap().width,
        Val::Px(FIELD_LABEL_WIDTH - 24.0)
    );
    f.clean();
}

#[test]
fn actual_xy_scroll_wrapper_preserves_memory_and_both_bar_target_identities() {
    assert!(crate::feathers::scroll::scrollbar_needed(100.0, 101.0));
    let mut f = Fixture::new();
    let mut target = Entity::PLACEHOLDER;
    let mut commands = f.app.world_mut().commands();
    commands.spawn(Node::default()).with_children(|parent| {
        target = spawn_scroll_area_xy(
            parent,
            crate::ScrollMemoryKey::Curves,
            Node::default(),
            |_| {},
        );
    });
    f.app.world_mut().flush();
    let w = f.app.world_mut();
    assert_eq!(
        w.get::<PersistedScroll>(target).unwrap().0,
        crate::ScrollMemoryKey::Curves
    );
    let bars = w
        .query::<&Scrollbar>()
        .iter(w)
        .filter(|b| b.target == target)
        .collect::<Vec<_>>();
    assert_eq!(bars.len(), 2);
    assert!(
        bars.iter()
            .any(|b| b.orientation == ControlOrientation::Horizontal)
    );
    assert!(
        bars.iter()
            .any(|b| b.orientation == ControlOrientation::Vertical)
    );
    f.clean();
}
