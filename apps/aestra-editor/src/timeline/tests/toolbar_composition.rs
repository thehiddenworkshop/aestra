//! Actual toolbar, combo and transport/timeline consumers on both engine graphs.
use super::super::{action_controls, execution, presentation, state::TimelineView};
use super::*;
use crate::feathers::{action_activation::PendingFeathersActivation, icon::SvgFile};
use crate::{
    CurvesState, ModulePaletteState, ProjectEffectCatalog,
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    material_function_editor::FunctionEditor,
    menus::MenuState,
    transport::{EditorTransportPlugin, TransportSet},
};
use aestra_core::{EffectAsset, Emitter, EmitterRegionId};
use bevy::{
    asset::AssetPlugin,
    ecs::system::RunSystemOnce,
    feathers::controls::{FeathersListRow, SelectPlugin},
    input_focus::InputFocus,
    scene::ScenePlugin,
    text::TextPlugin,
    ui_widgets::{Activate, Button, MenuButton},
};

#[derive(Resource)]
struct Built(Entity);
#[derive(Resource, Default)]
struct Actions(Vec<TimelineAction>);
fn build(
    mut commands: Commands,
    session: Res<EditorSession>,
    state: Res<TimelineState>,
    localizer: Res<Localizer>,
    assets: Res<AssetServer>,
) {
    let root = commands
        .spawn(Node::default())
        .with_children(|parent| {
            spawn_timeline_toolbar(parent, &session, &state, &localizer, &assets)
        })
        .id();
    commands.insert_resource(Built(root));
}
struct Fixture {
    root: tempfile::TempDir,
    app: App,
    before: EffectAsset,
    bytes: Vec<u8>,
}
impl Fixture {
    fn new() -> Self {
        Self::configured("en-US", |_| {})
    }
    fn configured(locale: &str, prepare: impl FnOnce(&mut App)) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut effect = EffectAsset::new("Toolbar", 8.0);
        effect.emitters.push(Emitter::basic_sprite("Emitter", 4.0));
        let mut session = EditorSession::from_test_effect(effect);
        session.playing = false;
        session.seek_time(1.5);
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
            SelectPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<SvgFile>()
        .insert_resource(session)
        .insert_resource(TimelineState::framed(8.0))
        .insert_resource(Localizer::new(locale).unwrap())
        .insert_resource(ProjectEffectCatalog::scan(root.path()))
        .init_resource::<CurvesState>()
        .init_resource::<MenuState>()
        .init_resource::<ModulePaletteState>()
        .init_resource::<InputFocus>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<EditorHistoryLedger>()
        .init_resource::<MaterialProgramEditHistory>()
        .init_resource::<FunctionEditor>()
        .init_resource::<Actions>()
        .add_message::<bevy::input::keyboard::KeyboardInput>()
        .configure_sets(
            Update,
            (
                TransportSet::Input,
                TransportSet::Actions,
                TransportSet::Playback,
                TransportSet::Sync,
            )
                .chain(),
        )
        .add_plugins(EditorTransportPlugin)
        .add_observer(action_controls::queue_timeline_action_activation)
        .add_observer(execution::execute_timeline_action)
        .add_observer(|event: On<TimelineAction>, mut actions: ResMut<Actions>| {
            actions.0.push(*event)
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
                action_controls::handle_timeline_action_buttons,
                action_controls::audit_timeline_controls,
            )
                .chain()
                .in_set(TransportSet::Actions),
        );
        prepare(&mut app);
        let before = app.world().resource::<EditorSession>().effect.clone();
        before
            .save_ron(root.path().join("toolbar.aestra.ron"))
            .unwrap();
        let bytes = std::fs::read(root.path().join("toolbar.aestra.ron")).unwrap();
        app.update();
        Self {
            root,
            app,
            before,
            bytes,
        }
    }
    fn control(&mut self, action: TimelineAction) -> Entity {
        let w = self.app.world_mut();
        w.query::<(Entity, &TimelineAction)>()
            .iter(w)
            .find_map(|(e, a)| (*a == action).then_some(e))
            .unwrap()
    }
    fn transport(&mut self, action: TransportAction) -> Entity {
        let w = self.app.world_mut();
        w.query::<(Entity, &TransportAction)>()
            .iter(w)
            .find_map(|(e, a)| (*a == action).then_some(e))
            .unwrap()
    }
    fn event<E: Event>(&mut self, event: E)
    where
        for<'a> E::Trigger<'a>: Default,
    {
        self.app.world_mut().trigger(event);
        self.app.world_mut().flush();
    }
    fn activate(&mut self, entity: Entity) {
        self.event(Activate { entity });
        self.app.update();
    }
    fn clean(&self) {
        let s = self.app.world().resource::<EditorSession>();
        assert_eq!(s.effect, self.before);
        assert!(!s.dirty);
        assert!(!s.can_undo());
        self.disk();
    }
    fn disk(&self) {
        assert_eq!(
            std::fs::read(self.root.path().join("toolbar.aestra.ron")).unwrap(),
            self.bytes
        );
    }
    fn roundtrip(&mut self) {
        let after = self.app.world().resource::<EditorSession>().effect.clone();
        assert_ne!(after, self.before);
        self.event(HistoryAction::Undo);
        assert_eq!(
            self.app.world().resource::<EditorSession>().effect,
            self.before
        );
        assert!(!self.app.world().resource::<EditorSession>().can_undo());
        self.event(HistoryAction::Redo);
        assert_eq!(self.app.world().resource::<EditorSession>().effect, after);
        self.disk();
    }
}
fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 0.001, "{a} != {b}");
}

#[test]
fn production_toolbar_keeps_outer_geometry_and_retained_identity() {
    let mut f = Fixture::new();
    let root = f.app.world().resource::<Built>().0;
    let header = f.app.world().get::<Children>(root).unwrap()[0];
    let node = f.app.world().get::<Node>(header).unwrap();
    assert_eq!(node.height, Val::Px(38.0));
    assert_eq!(node.column_gap, Val::Px(14.0));
    assert_eq!(node.padding.left, Val::Px(14.0));
    assert_eq!(
        f.app.world().get::<BackgroundColor>(header).unwrap().0,
        theme::PANEL_LIGHT
    );
    f.app.update();
    assert_eq!(f.app.world().get::<Children>(root).unwrap()[0], header);
    f.clean();
}

#[test]
fn real_icon_actions_keep_button_classification_and_svg_geometry() {
    let mut f = Fixture::new();
    for action in [
        TimelineAction::FrameAll,
        TimelineAction::AddMarker,
        TimelineAction::AddChoreographyEvent,
        TimelineAction::SplitEmitterRegion,
        TimelineAction::JoinEmitterRegion,
    ] {
        let e = f.control(action);
        let w = f.app.world();
        assert!(w.get::<Button>(e).is_some());
        assert!(w.get::<FeathersActionButton>(e).is_some());
        assert!(w.get::<EditorTooltip>(e).is_some());
        assert_eq!(w.get::<Node>(e).unwrap().width, Val::Px(28.0));
        assert_eq!(w.get::<Node>(e).unwrap().height, Val::Px(26.0));
        let icon = w.get::<Children>(e).unwrap()[0];
        assert!(w.get::<UiSvg>(icon).is_some());
        assert_eq!(w.get::<Node>(icon).unwrap().width, Val::Px(16.0));
    }
    f.clean();
}

#[test]
fn transport_and_duration_buttons_have_actual_payloads_and_localized_labels() {
    for locale in ["en-US", "fr-FR"] {
        let mut f = Fixture::configured(locale, |_| {});
        for (action, key) in [
            (TransportAction::StepFrame(-1), "timeline-previous-frame"),
            (TransportAction::StepFrame(1), "timeline-next-frame"),
        ] {
            let e = f.transport(action);
            let w = f.app.world();
            assert_eq!(
                w.get::<AccessibleLabel>(e).unwrap().0,
                w.resource::<Localizer>().text(key)
            );
            assert!(w.get::<EditorTooltip>(e).is_some());
        }
        for (action, key) in [
            (
                TimelineAction::AdjustEffectDuration(-0.25),
                "timeline-decrease-duration",
            ),
            (
                TimelineAction::AdjustEffectDuration(0.25),
                "timeline-increase-duration",
            ),
        ] {
            let e = f.control(action);
            assert_eq!(
                f.app.world().get::<AccessibleLabel>(e).unwrap().0,
                f.app.world().resource::<Localizer>().text(key)
            );
        }
        f.clean();
    }
}

#[test]
fn snap_combo_contains_all_real_modes_in_native_listbox_with_one_selected_row() {
    for mode in TimelineSnapMode::ALL {
        let mut f = Fixture::configured("en-US", |app| {
            app.world_mut().resource_mut::<TimelineState>().snap = mode
        });
        for candidate in TimelineSnapMode::ALL {
            let e = f.control(TimelineAction::SetSnap(candidate));
            let w = f.app.world();
            assert!(w.get::<FeathersListRow>(e).is_some());
            assert!(w.get::<FeathersActionButton>(e).is_some());
            assert_eq!(w.get::<bevy::ui::Selected>(e).is_some(), candidate == mode);
            let listbox =
                std::iter::successors(Some(e), |e| w.get::<ChildOf>(*e).map(ChildOf::parent))
                    .find(|e| w.get::<bevy::ui_widgets::ListBox>(*e).is_some())
                    .unwrap();
            assert!(w.get::<bevy::ui_widgets::ListBox>(listbox).is_some());
            assert_eq!(
                w.get::<bevy::input_focus::tab_navigation::TabIndex>(listbox)
                    .unwrap()
                    .0,
                0
            );
        }
        f.clean();
    }
}

#[test]
fn snap_trigger_uses_real_menu_button_and_localized_description() {
    let mut f = Fixture::configured("fr-FR", |_| {});
    let w = f.app.world_mut();
    let e = w
        .query_filtered::<Entity, With<MenuButton>>()
        .single(w)
        .unwrap();
    assert_eq!(
        w.get::<AccessibleLabel>(e).unwrap().0,
        w.resource::<Localizer>()
            .text("timeline-snapping-description")
    );
    let label = w.get::<Children>(e).unwrap()[0];
    assert_eq!(
        w.get::<Text>(label).unwrap().0,
        w.resource::<Localizer>()
            .text(TimelineSnapMode::Smart.message_id())
    );
    f.clean();
}

#[test]
fn snap_activation_changes_only_view_state_and_dispatches_once() {
    let mut f = Fixture::new();
    let e = f.control(TimelineAction::SetSnap(TimelineSnapMode::Frames));
    f.activate(e);
    assert_eq!(
        f.app.world().resource::<TimelineState>().snap,
        TimelineSnapMode::Frames
    );
    assert_eq!(
        f.app.world().resource::<Actions>().0,
        vec![TimelineAction::SetSnap(TimelineSnapMode::Frames)]
    );
    assert!(f.app.world().get::<PendingFeathersActivation>(e).is_none());
    f.app.update();
    assert_eq!(f.app.world().resource::<Actions>().0.len(), 1);
    f.clean();
}

#[test]
fn frame_all_action_uses_production_view_execution_without_editing() {
    let mut f = Fixture::new();
    f.app.world_mut().resource_mut::<TimelineState>().view = TimelineView {
        start: 2.0,
        end: 3.0,
    };
    let e = f.control(TimelineAction::FrameAll);
    f.activate(e);
    let view = f.app.world().resource::<TimelineState>().view;
    near(view.start, 0.0);
    near(view.end, 8.0);
    f.clean();
}

#[test]
fn add_marker_toolbar_action_is_one_globally_reversible_edit_at_playhead() {
    let mut f = Fixture::new();
    let time = f.app.world().resource::<EditorSession>().time();
    let e = f.control(TimelineAction::AddMarker);
    f.activate(e);
    let s = f.app.world().resource::<EditorSession>();
    assert_eq!(s.effect.markers.len(), 1);
    near(s.effect.markers[0].time, time);
    f.roundtrip();
}

#[test]
fn add_event_toolbar_action_preserves_actual_document_default_and_history() {
    let mut f = Fixture::new();
    let e = f.control(TimelineAction::AddChoreographyEvent);
    f.activate(e);
    let s = f.app.world().resource::<EditorSession>();
    assert_eq!(s.effect.choreography_events.len(), 1);
    near(s.effect.choreography_events[0].time, 1.5);
    assert!(matches!(
        s.effect.choreography_events[0].payload,
        aestra_core::ChoreographyEventPayload::GameplayNotify { .. }
    ));
    f.roundtrip();
}

#[test]
fn accepted_activate_never_replays_without_another_activation() {
    let mut f = Fixture::new();
    let e = f.control(TimelineAction::AddMarker);
    f.activate(e);
    f.app.update();
    f.app.update();
    assert_eq!(
        f.app
            .world()
            .resource::<EditorSession>()
            .effect
            .markers
            .len(),
        1
    );
    assert_eq!(f.app.world().resource::<Actions>().0.len(), 1);
    f.roundtrip();
}

#[test]
fn stepping_buttons_use_actual_transport_plugin_without_semantic_edits() {
    let mut f = Fixture::new();
    let before = f.app.world().resource::<EditorSession>().frame();
    let next = f.transport(TransportAction::StepFrame(1));
    f.activate(next);
    assert_eq!(
        f.app.world().resource::<EditorSession>().frame(),
        before + 1
    );
    let previous = f.transport(TransportAction::StepFrame(-1));
    f.activate(previous);
    assert_eq!(f.app.world().resource::<EditorSession>().frame(), before);
    assert!(!f.app.world().resource::<EditorSession>().playing);
    f.clean();
}

#[test]
fn stepping_at_zero_uses_existing_lower_bound() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .seek_time(0.0);
    let previous = f.transport(TransportAction::StepFrame(-1));
    f.activate(previous);
    assert_eq!(f.app.world().resource::<EditorSession>().frame(), 0);
    f.clean();
}

#[test]
fn duration_buttons_are_semantic_edits_and_roundtrip_global_history() {
    for delta in [-0.25, 0.25] {
        let mut f = Fixture::new();
        let e = f.control(TimelineAction::AdjustEffectDuration(delta));
        f.activate(e);
        near(
            f.app.world().resource::<EditorSession>().effect.duration,
            8.0 + delta,
        );
        f.roundtrip();
    }
}

#[test]
fn region_buttons_keep_selection_preservation_marker_and_cursor_tracking() {
    let mut f = Fixture::new();
    for action in [
        TimelineAction::SplitEmitterRegion,
        TimelineAction::JoinEmitterRegion,
    ] {
        let e = f.control(action);
        assert!(f.app.world().get::<TimelineRegionToolButton>(e).is_some());
        assert!(f.app.world().get::<RelativeCursorPosition>(e).is_some());
    }
    f.clean();
}

#[test]
fn actual_split_button_invokes_region_consumer_and_roundtrips_history() {
    let mut f = Fixture::new();
    let e = f.control(TimelineAction::SplitEmitterRegion);
    f.activate(e);
    assert_eq!(
        f.app.world().resource::<EditorSession>().effect.emitters[0]
            .timeline_regions()
            .len(),
        2
    );
    f.roundtrip();
}

#[test]
fn actual_merge_button_invokes_selected_region_consumer_and_history() {
    let mut f = Fixture::configured("en-US", |app| {
        let (emitter, regions) = {
            let s = app.world().resource::<EditorSession>();
            let e = &s.effect.emitters[0];
            (
                e.id,
                e.split_timeline_region(e.implicit_region_id(), 2.0, EmitterRegionId::new())
                    .unwrap(),
            )
        };
        app.world_mut()
            .resource_mut::<EditorSession>()
            .effect
            .emitters[0]
            .regions = regions.clone();
        let mut state = app.world_mut().resource_mut::<TimelineState>();
        state
            .selected_emitter_regions
            .extend(regions.into_iter().map(|r| (emitter, r.id)));
    });
    let e = f.control(TimelineAction::JoinEmitterRegion);
    f.activate(e);
    assert_eq!(
        f.app.world().resource::<EditorSession>().effect.emitters[0]
            .timeline_regions()
            .len(),
        1
    );
    f.roundtrip();
}

#[test]
fn invalid_region_tool_requests_keep_document_and_history_unchanged() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .seek_time(0.0);
    let split = f.control(TimelineAction::SplitEmitterRegion);
    f.activate(split);
    f.clean();
    let merge = f.control(TimelineAction::JoinEmitterRegion);
    f.activate(merge);
    f.clean();
}

#[test]
fn real_time_label_presentation_updates_the_constructed_control_in_place() {
    let mut f = Fixture::new();
    let w = f.app.world_mut();
    let e = w
        .query_filtered::<Entity, With<TimeLabel>>()
        .single(w)
        .unwrap();
    assert_eq!(w.get::<Text>(e).unwrap().0, "00:00.000  /  00:02.800");
    w.run_system_once(presentation::update_timeline_time_label)
        .unwrap();
    let text = f.app.world().get::<Text>(e).unwrap().0.clone();
    assert!(text.contains("01.500"));
    assert!(text.contains("08.000"));
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .seek_time(2.0);
    f.app
        .world_mut()
        .run_system_once(presentation::update_timeline_time_label)
        .unwrap();
    assert!(f.app.world().get::<Text>(e).unwrap().0.contains("02.000"));
    f.clean();
}

#[test]
fn heading_helper_and_hertz_duration_text_preserve_localized_production_content() {
    for locale in ["en-US", "fr-FR"] {
        let mut f = Fixture::configured(locale, |_| {});
        let w = f.app.world_mut();
        let label = timeline_effect_heading(w.resource::<Localizer>(), "Named effect");
        assert!(label.contains("Named effect"));
        let texts = w
            .query::<&Text>()
            .iter(w)
            .map(|t| t.0.as_str())
            .collect::<Vec<_>>();
        let duration = format!(
            "{} 8.00s",
            w.resource::<Localizer>().text("timeline-duration")
        );
        assert!(texts.contains(&duration.as_str()));
        f.clean();
    }
}

#[test]
fn idle_updates_preserve_loaded_icon_and_control_entities() {
    let mut f = Fixture::new();
    let e = f.control(TimelineAction::AddMarker);
    let icon = f.app.world().get::<Children>(e).unwrap()[0];
    let handle = f.app.world().get::<UiSvg>(icon).unwrap().0.clone();
    f.app.update();
    f.app.update();
    assert_eq!(f.control(TimelineAction::AddMarker), e);
    assert_eq!(f.app.world().get::<UiSvg>(icon).unwrap().0, handle);
    f.clean();
}
