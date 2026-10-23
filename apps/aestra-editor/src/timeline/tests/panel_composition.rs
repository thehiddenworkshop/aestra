//! Full production panel tree and attached consumers, using explicit headless geometry.
use super::super::{
    action_controls,
    automation::emitter_automation_lanes,
    document_actions::{TimelineAction, TimelineSnapMode},
    drop_execution, execution, host_composition, motion_execution, motion_gestures,
    navigation_cursor::OverrideCursor,
    presentation,
    state::TimelineView,
    timeline_pointer::tests as pointer,
};
use super::*;
use crate::{
    ModulePaletteState,
    asset_drop::AssetPayload,
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    material_function_editor::FunctionEditor,
    menus::MenuState,
    persistence::DocumentProtectionState,
    transport::{EditorTransportPlugin, TransportAction, TransportSet},
};
use aestra_core::{
    ChoreographyEvent, ChoreographyEventPayload, ChoreographyTrackId, EffectAsset, EffectClip,
    EffectMarker, Emitter, HostTransformKey, HostTransformTrack, MarkerId,
};
use bevy::{
    asset::AssetPlugin,
    input_focus::InputFocus,
    picking::pointer::{PointerButton, PointerId},
    scene::ScenePlugin,
    text::TextPlugin,
    ui_widgets::{Activate, ControlOrientation, Scrollbar},
    window::{CursorIcon, PrimaryWindow},
};

#[derive(Resource)]
struct Built(Entity);
#[allow(
    clippy::too_many_arguments,
    reason = "Actual panel constructor's explicit resource contract"
)]
fn build(
    mut commands: Commands,
    session: Res<EditorSession>,
    state: Res<TimelineState>,
    catalog: Res<ProjectEffectCatalog>,
    registry: Res<EditorModuleRegistry>,
    curves: Res<CurvesState>,
    localizer: Res<Localizer>,
    assets: Res<AssetServer>,
) {
    let root = commands
        .spawn(Node::default())
        .with_children(|parent| {
            spawn_timeline(
                parent, &session, &state, &catalog, &registry, &curves, &localizer, &assets,
            );
        })
        .id();
    commands.insert_resource(Built(root));
}
struct Fixture {
    _root: tempfile::TempDir,
    app: App,
    window: Entity,
    source: Entity,
    self_source: Entity,
    before: EffectAsset,
    files: Vec<(std::path::PathBuf, Vec<u8>)>,
}
impl Fixture {
    fn new() -> Self {
        Self::configured("en-US", false)
    }
    fn configured(locale: &str, empty: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut child = EffectAsset::new("Child", 1.5);
        child
            .emitters
            .push(Emitter::basic_sprite("Child emitter", 1.5));
        let mut effect = EffectAsset::new("Full Timeline", 8.0);
        if !empty {
            effect.emitters.extend([
                Emitter::basic_sprite("First", 4.0),
                Emitter::basic_sprite("Second", 4.0),
            ]);
            effect
                .effect_clips
                .push(EffectClip::new(child.id, 1.0, 1.0));
            effect.choreography_order = vec![
                ChoreographyTrackId::Emitter(effect.emitters[0].id),
                ChoreographyTrackId::EffectClip(effect.effect_clips[0].id),
                ChoreographyTrackId::Emitter(effect.emitters[1].id),
            ];
            effect.markers.push(EffectMarker {
                id: MarkerId::new(),
                name: "Launch".into(),
                time: 1.0,
            });
            effect.choreography_events.push(ChoreographyEvent::new(
                "Sound",
                1.0,
                ChoreographyEventPayload::PlaySound {
                    cue: "burst".into(),
                },
            ));
            effect.host_transform_track = Some(HostTransformTrack::from_pose_keys(
                [0.0, 1.0, 2.0]
                    .into_iter()
                    .map(|time| HostTransformKey {
                        time,
                        transform: default(),
                    })
                    .collect(),
                false,
            ));
        }
        let mut files = Vec::new();
        for (name, asset) in [("child.aestra.ron", &child), ("parent.aestra.ron", &effect)] {
            let path = root.path().join(name);
            asset.save_ron(&path).unwrap();
            files.push((path.clone(), std::fs::read(path).unwrap()));
        }
        let catalog = ProjectEffectCatalog::scan(root.path());
        let payload = |path| {
            AssetPayload::capture(
                &catalog,
                catalog
                    .content()
                    .source_tree()
                    .at_relative_path(path)
                    .unwrap()
                    .id,
            )
        };
        let source = payload("child.aestra.ron");
        let self_source = payload("parent.aestra.ron");
        let localizer = Localizer::new(locale).unwrap();
        let registry = EditorModuleRegistry::default();
        let mut state = TimelineState::framed(8.0);
        state.view = TimelineView {
            start: 0.0,
            end: 8.0,
        };
        state.snap = TimelineSnapMode::None;
        if !empty {
            let emitter = &effect.emitters[0];
            state.expanded_automation_emitters.insert(emitter.id);
            let lanes = emitter_automation_lanes(&effect, emitter, &registry, &localizer);
            state
                .visible_automation_lanes
                .extend(lanes.iter().map(|l| l.id.clone()));
            state
                .automation_lane_heights
                .insert(lanes[0].id.clone(), 72.0);
            state
                .expanded_effect_clips
                .insert(EffectClipPath::root_path(effect.effect_clips[0].id));
        }
        let before = effect.clone();
        let mut session = EditorSession::from_test_effect(effect);
        session.playing = false;
        session.seek_time(0.5);
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<crate::feathers::icon::SvgFile>()
        .insert_resource(session)
        .insert_resource(state)
        .insert_resource(catalog)
        .insert_resource(localizer)
        .insert_resource(registry)
        .init_resource::<CurvesState>()
        .init_resource::<MenuState>()
        .init_resource::<ModulePaletteState>()
        .init_resource::<InputFocus>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<OverrideCursor>()
        .init_resource::<DocumentProtectionState>()
        .init_resource::<EditorHistoryLedger>()
        .init_resource::<MaterialProgramEditHistory>()
        .init_resource::<FunctionEditor>()
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
        .add_observer(action_controls::queue_choreography_action_activation)
        .add_observer(action_controls::activate_timeline_track_entry)
        .add_observer(execution::execute_timeline_action)
        .add_observer(execution::execute_choreography_action)
        .add_observer(host_composition::activate)
        .add_observer(motion_execution::execute)
        .add_observer(drop_execution::reject_project_effect_drop)
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
                action_controls::handle_choreography_action_buttons,
                host_composition::buttons,
            )
                .chain()
                .in_set(TransportSet::Actions),
        )
        .add_systems(
            Update,
            (
                presentation::update_timeline_visuals,
                presentation::update_timeline_marker_visuals,
                presentation::update_choreography_event_visuals,
                presentation::update_effect_clip_visuals,
                motion_gestures::update_keys,
                presentation::update_effect_drop_preview,
                presentation::sync_effect_drop_track_gap,
                presentation::sync_timeline_horizontal_scroll,
                presentation::sync_timeline_vertical_scroll,
            )
                .chain()
                .after(TransportSet::Playback),
        );
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow, CursorIcon::default()))
            .id();
        let source = app.world_mut().spawn(source).id();
        let self_source = app.world_mut().spawn(self_source).id();
        app.update();
        let mut f = Self {
            _root: root,
            app,
            window,
            source,
            self_source,
            before,
            files,
        };
        let canvas = f.one::<TimelineCanvas>();
        f.app.world_mut().entity_mut(canvas).insert(ComputedNode {
            size: Vec2::new(400.0, 300.0),
            ..default()
        });
        f.app.update();
        f
    }
    fn one<C: Component>(&mut self) -> Entity {
        let w = self.app.world_mut();
        w.query_filtered::<Entity, With<C>>().single(w).unwrap()
    }
    fn count<C: Component>(&mut self) -> usize {
        let w = self.app.world_mut();
        w.query::<&C>().iter(w).count()
    }
    fn pane(&mut self, kind: TimelineVerticalPane) -> Entity {
        let w = self.app.world_mut();
        w.query::<(Entity, &TimelineVerticalPane)>()
            .iter(w)
            .find_map(|(e, k)| (*k == kind).then_some(e))
            .unwrap()
    }
    fn node(&self, e: Entity) -> &Node {
        self.app.world().get(e).unwrap()
    }
    fn session(&self) -> &EditorSession {
        self.app.world().resource()
    }
    fn event<E: Event>(&mut self, event: E)
    where
        for<'a> E::Trigger<'a>: Default,
    {
        self.app.world_mut().trigger(event);
        self.app.world_mut().flush();
    }
    fn cursor(&mut self, x: f32) {
        let c = self.one::<TimelineCanvas>();
        self.app
            .world_mut()
            .entity_mut(c)
            .insert(RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::new(x, 0.0)),
            });
    }
    fn clean(&self) {
        assert_eq!(self.session().effect, self.before);
        assert!(!self.session().can_undo());
        self.disk();
    }
    fn disk(&self) {
        for (p, b) in &self.files {
            assert_eq!(&std::fs::read(p).unwrap(), b);
        }
    }
    fn roundtrip(&mut self) {
        let after = self.session().effect.clone();
        assert_ne!(after, self.before);
        self.event(HistoryAction::Undo);
        self.clean();
        self.event(HistoryAction::Redo);
        assert_eq!(self.session().effect, after);
        self.disk();
    }
    fn action(&mut self, a: TimelineAction) -> Entity {
        let w = self.app.world_mut();
        w.query::<(Entity, &TimelineAction)>()
            .iter(w)
            .find_map(|(e, c)| (*c == a).then_some(e))
            .unwrap()
    }
    fn activate(&mut self, e: Entity) {
        self.event(Activate { entity: e });
        self.app.update();
    }
    fn drop_on(&mut self, target: Entity, source: Entity) {
        self.event(pointer::drop_on(
            target,
            source,
            self.window,
            PointerButton::Primary,
        ));
    }
}

#[test]
fn full_panel_keeps_body_header_and_canvas_layout_contract() {
    let mut f = Fixture::new();
    let body = f.one::<TimelineBody>();
    let column = f.one::<TimelineHeaderColumn>();
    let canvas = f.one::<TimelineCanvas>();
    assert_eq!(f.node(body).flex_direction, FlexDirection::Row);
    assert_eq!(f.node(column).width, Val::Px(244.0));
    assert_eq!(f.node(canvas).padding.top, Val::Px(81.0));
    assert_eq!(f.node(canvas).min_width, Val::Px(0.0));
    assert!(f.app.world().get::<EditorNativeControl>(canvas).is_some());
    assert!(f.app.world().get::<Button>(canvas).is_some());
    f.clean();
}

#[test]
fn full_panel_columns_share_identical_expanded_grid_rows_and_content_height() {
    let mut f = Fixture::new();
    let w = f.app.world_mut();
    let contents = w
        .query_filtered::<(Entity, &Node), With<TimelineVerticalContent>>()
        .iter(w)
        .map(|(e, n)| (e, n.min_height))
        .collect::<Vec<_>>();
    assert_eq!(contents.len(), 2);
    assert_eq!(contents[0].1, contents[1].1);
    let rows = contents
        .iter()
        .map(|(e, _)| {
            let mut r = w
                .get::<Children>(*e)
                .unwrap()
                .iter()
                .filter_map(|e| {
                    Some((
                        w.get::<TimelineChoreographyGridRow>(e)?.0,
                        w.get::<Node>(e)?.height,
                    ))
                })
                .collect::<Vec<_>>();
            r.sort_by_key(|r| r.0);
            r
        })
        .collect::<Vec<_>>();
    assert_eq!(rows[0], rows[1]);
    assert_eq!(
        rows[0].iter().map(|r| r.0).collect::<Vec<_>>(),
        (1..=7).collect::<Vec<_>>()
    );
    f.clean();
}

#[test]
fn complete_panel_contains_toolbar_cues_motion_and_editable_and_referenced_tracks() {
    let mut f = Fixture::new();
    assert_eq!(f.count::<TimelineRulerTick>(), 32);
    assert_eq!(f.count::<TimelineMarker>(), 1);
    assert_eq!(f.count::<TimelineChoreographyEvent>(), 1);
    assert_eq!(f.count::<motion_gestures::KeyControl>(), 3);
    assert_eq!(f.count::<TimelineClip>(), 2);
    assert_eq!(f.count::<TimelineEffectClip>(), 1);
    assert_eq!(f.count::<TimelineReferencedEmitter>(), 1);
    assert_eq!(f.count::<TimelineEffectDropSpacer>(), 2);
    assert_eq!(f.count::<TimeLabel>(), 1);
    f.clean();
}

#[test]
fn empty_effect_retains_usable_toolbar_canvas_and_both_panes_without_dummy_tracks() {
    let mut f = Fixture::configured("en-US", true);
    assert_eq!(f.count::<TimelineCanvas>(), 1);
    assert_eq!(f.count::<TimelineVerticalContent>(), 2);
    assert_eq!(f.count::<TimelineClip>(), 0);
    assert_eq!(f.count::<TimelineEffectClip>(), 0);
    assert_eq!(f.count::<TimelineEffectDropSpacer>(), 2);
    assert_eq!(f.count::<TimelineRulerTick>(), 32);
    f.clean();
}

#[test]
fn full_panel_labels_use_actual_locale_and_catalog_identity() {
    for locale in ["en-US", "fr-FR"] {
        let mut f = Fixture::configured(locale, false);
        let canvas = f.one::<TimelineCanvas>();
        let h = f.one::<TimelineAssetDropTarget>();
        let l = f.app.world().resource::<Localizer>();
        assert_eq!(
            f.app.world().get::<AccessibleLabel>(canvas).unwrap().0,
            l.text("timeline-canvas-accessible")
        );
        assert_eq!(
            f.app.world().get::<AccessibleLabel>(h).unwrap().0,
            l.text("timeline-emitters")
        );
        let heading = f.one::<TimelineEffectHeading>();
        assert!(
            f.app
                .world()
                .get::<Text>(heading)
                .unwrap()
                .0
                .contains("Full Timeline")
        );
        f.clean();
    }
}

#[test]
fn full_panel_bars_target_the_constructed_horizontal_and_track_viewports() {
    let mut f = Fixture::new();
    let h = f.one::<TimelineHorizontalScrollViewport>();
    let t = f.pane(TimelineVerticalPane::Tracks);
    let w = f.app.world_mut();
    let bars = w.query::<&Scrollbar>().iter(w).collect::<Vec<_>>();
    // The native snapping select owns a third, bounded dropdown scrollbar.
    assert_eq!(bars.len(), 3);
    assert_eq!(
        bars.iter()
            .filter(|b| b.target != h
                && b.target != t
                && b.orientation == ControlOrientation::Vertical)
            .count(),
        1
    );
    assert!(
        bars.iter()
            .any(|b| b.orientation == ControlOrientation::Horizontal && b.target == h)
    );
    assert!(
        bars.iter()
            .any(|b| b.orientation == ControlOrientation::Vertical && b.target == t)
    );
    let header = f.one::<TimelineAssetDropTarget>();
    assert!(f.app.world().get::<ListBox>(header).is_some());
    assert!(f.app.world().get::<KeyboardNavigableList>(header).is_some());
    f.clean();
}

#[test]
fn overlay_layer_order_and_non_pickability_preserve_canvas_input() {
    let mut f = Fixture::new();
    for (e, layer) in [
        (f.one::<Playhead>(), 2),
        (f.one::<TimelineSnapGuide>(), 3),
        (f.one::<TimelineEffectDropPreview>(), 4),
        (f.one::<TimelineInvalidDropFeedback>(), 4),
    ] {
        assert_eq!(f.app.world().get::<ZIndex>(e).unwrap().0, layer);
        assert_eq!(*f.app.world().get::<Pickable>(e).unwrap(), Pickable::IGNORE);
    }
    f.clean();
}

#[test]
fn canvas_primary_press_seeks_and_closes_transient_menus_without_semantic_edit() {
    let mut f = Fixture::new();
    f.cursor(-0.25);
    let c = f.one::<TimelineCanvas>();
    let id = f.before.emitters[0].id;
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .context_emitter = Some(id);
    f.event(pointer::press(
        c,
        f.window,
        Vec2::ZERO,
        PointerId::Mouse,
        PointerButton::Primary,
    ));
    assert_eq!(f.session().time(), 2.0);
    assert!(
        f.app
            .world()
            .resource::<TimelineState>()
            .context_emitter
            .is_none()
    );
    f.clean();
}

#[test]
fn canvas_secondary_press_does_not_seek() {
    let mut f = Fixture::new();
    let time = f.session().time();
    f.cursor(0.4);
    let c = f.one::<TimelineCanvas>();
    f.event(pointer::press(
        c,
        f.window,
        Vec2::ZERO,
        PointerId::Mouse,
        PointerButton::Secondary,
    ));
    assert_eq!(f.session().time(), time);
    f.clean();
}

#[test]
fn constructed_cue_and_track_control_presses_do_not_seek_parent_canvas() {
    let mut f = Fixture::new();
    let time = f.session().time();
    f.cursor(0.4);
    let cue = f.one::<TimelineMarker>();
    let w = f.app.world_mut();
    let control = w
        .query_filtered::<Entity, With<TimelineClipInteraction>>()
        .iter(w)
        .next()
        .unwrap();
    for target in [cue, control] {
        f.event(pointer::press(
            target,
            f.window,
            Vec2::ZERO,
            PointerId::Mouse,
            PointerButton::Primary,
        ));
    }
    assert_eq!(f.session().time(), time);
    f.clean();
}

#[test]
fn complete_toolbar_marker_and_event_actions_commit_one_global_history_step() {
    for action in [
        TimelineAction::AddMarker,
        TimelineAction::AddChoreographyEvent,
    ] {
        let mut f = Fixture::new();
        let e = f.action(action);
        f.activate(e);
        if action == TimelineAction::AddMarker {
            assert_eq!(f.session().effect.markers.len(), 2);
        } else {
            assert_eq!(f.session().effect.choreography_events.len(), 2);
        }
        f.roundtrip();
    }
}

#[test]
fn header_add_button_dispatches_real_choreography_edit_once() {
    let mut f = Fixture::new();
    let w = f.app.world_mut();
    let e = w
        .query::<(Entity, &ChoreographyAction)>()
        .iter(w)
        .find_map(|(e, a)| (*a == ChoreographyAction::AddEmitter).then_some(e))
        .unwrap();
    f.activate(e);
    assert_eq!(f.session().effect.emitters.len(), 3);
    f.roundtrip();
}

#[test]
fn assembled_transport_frame_step_uses_real_transport_consumer_without_history() {
    let mut f = Fixture::new();
    let time = f.session().time();
    let w = f.app.world_mut();
    let e = w
        .query::<(Entity, &TransportAction)>()
        .iter(w)
        .find_map(|(e, a)| (*a == TransportAction::StepFrame(1)).then_some(e))
        .unwrap();
    f.activate(e);
    assert!(f.session().time() > time);
    f.clean();
}

#[test]
fn actual_panel_canvas_and_header_drops_use_real_payload_and_global_history() {
    for canvas in [false, true] {
        let mut f = Fixture::new();
        f.cursor(-0.25);
        let target = if canvas {
            f.one::<TimelineCanvas>()
        } else {
            f.one::<TimelineAssetDropTarget>()
        };
        f.drop_on(target, f.source);
        let clip = f.session().effect.effect_clips.last().unwrap();
        assert_eq!(clip.start_time, if canvas { 2.0 } else { 0.5 });
        assert_eq!(clip.duration, 1.5);
        f.roundtrip();
    }
}

#[test]
fn self_reference_drop_reaches_constructed_feedback_without_partial_edit() {
    let mut f = Fixture::new();
    f.cursor(-0.25);
    let c = f.one::<TimelineCanvas>();
    f.drop_on(c, f.self_source);
    let e = f.one::<TimelineInvalidDropFeedback>();
    assert!(
        f.app
            .world()
            .get::<TimelineInvalidDropFeedback>(e)
            .unwrap()
            .rejected
    );
    assert_eq!(f.node(e).display, Display::Flex);
    assert!(f.session().status.starts_with("Effect clip was not added:"));
    f.clean();
}

#[test]
fn cancelled_release_rejects_drop_in_the_constructed_panel() {
    let mut f = Fixture::new();
    f.cursor(-0.25);
    let c = f.one::<TimelineCanvas>();
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    f.drop_on(c, f.source);
    f.clean();
}

#[test]
fn real_drag_hover_updates_constructed_preview_label_and_both_drop_spacers() {
    let mut f = Fixture::new();
    f.cursor(-0.25);
    let c = f.one::<TimelineCanvas>();
    f.event(pointer::enter(
        c,
        f.source,
        f.window,
        PointerButton::Primary,
    ));
    f.app.update();
    let p = f.one::<TimelineEffectDropPreview>();
    let label = f.one::<TimelineEffectDropPreviewLabel>();
    assert_eq!(f.node(p).display, Display::Flex);
    assert_eq!(f.node(p).left, Val::Percent(25.0));
    assert_eq!(f.node(p).width, Val::Percent(18.75));
    assert_eq!(f.app.world().get::<Text>(label).unwrap().0, "Child");
    let w = f.app.world_mut();
    assert!(
        w.query_filtered::<&Node, With<TimelineEffectDropSpacer>>()
            .iter(w)
            .all(|n| n.display == Display::Flex)
    );
    f.clean();
}

#[test]
fn leaving_constructed_canvas_clears_preview_without_editing() {
    let mut f = Fixture::new();
    let c = f.one::<TimelineCanvas>();
    f.event(pointer::enter(
        c,
        f.source,
        f.window,
        PointerButton::Primary,
    ));
    f.event(pointer::leave(
        c,
        f.source,
        f.window,
        PointerButton::Primary,
    ));
    f.app.update();
    let p = f.one::<TimelineEffectDropPreview>();
    assert_eq!(f.node(p).display, Display::None);
    assert!(
        f.app
            .world()
            .resource::<TimelineState>()
            .effect_drop_preview
            .is_none()
    );
    f.clean();
}

#[test]
fn nested_constructed_effect_control_resolves_canvas_and_commits_once() {
    let mut f = Fixture::new();
    let w = f.app.world_mut();
    let e = w
        .query::<(Entity, &TimelineEffectClipInteraction)>()
        .iter(w)
        .find_map(|(e, c)| (c.kind == TimelineDragKind::Move).then_some(e))
        .unwrap();
    f.event(pointer::start(
        e,
        f.window,
        Vec2::ZERO,
        PointerId::Mouse,
        PointerButton::Primary,
    ));
    f.event(pointer::motion_delta(e, f.window, Vec2::new(25.0, 0.0)));
    f.clean();
    f.event(pointer::end(
        e,
        f.window,
        Vec2::ZERO,
        PointerId::Mouse,
        PointerButton::Primary,
    ));
    assert_eq!(f.session().effect.effect_clips[0].start_time, 1.5);
    f.roundtrip();
}

#[test]
fn constructed_vertical_panes_sync_scroll_with_no_semantic_history() {
    let mut f = Fixture::new();
    let h = f.pane(TimelineVerticalPane::Headers);
    let t = f.pane(TimelineVerticalPane::Tracks);
    for e in [h, t] {
        f.app.world_mut().entity_mut(e).insert(ComputedNode {
            size: Vec2::new(400.0, 100.0),
            content_size: Vec2::new(400.0, 500.0),
            ..default()
        });
    }
    f.app.update();
    f.app.world_mut().get_mut::<ScrollPosition>(t).unwrap().y = 90.0;
    f.app.update();
    assert_eq!(f.app.world().get::<ScrollPosition>(h).unwrap().y, 90.0);
    let gutter = f.one::<TimelineVerticalScrollbarGutter>();
    assert_eq!(f.node(gutter).display, Display::Flex);
    f.clean();
}

#[test]
fn idle_full_panel_preserves_control_entities_without_duplicate_roots() {
    let mut f = Fixture::new();
    let canvas = f.one::<TimelineCanvas>();
    let preview = f.one::<TimelineEffectDropPreview>();
    for _ in 0..3 {
        f.app.update();
    }
    assert_eq!(f.one::<TimelineCanvas>(), canvas);
    assert_eq!(f.one::<TimelineEffectDropPreview>(), preview);
    assert_eq!(f.count::<TimelineBody>(), 1);
    f.clean();
}

#[test]
fn full_panel_teardown_retires_nested_controls_and_keeps_document_unchanged() {
    let mut f = Fixture::new();
    let root = f.app.world().resource::<Built>().0;
    f.app.world_mut().despawn(root);
    f.app.update();
    assert_eq!(f.count::<TimelineBody>(), 0);
    assert_eq!(f.count::<TimelineCanvas>(), 0);
    assert_eq!(f.count::<TimelineEffectClipInteraction>(), 0);
    assert_eq!(f.count::<Scrollbar>(), 0);
    f.clean();
}
