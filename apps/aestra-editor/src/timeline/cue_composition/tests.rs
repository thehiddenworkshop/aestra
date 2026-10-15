//! Production cue composition with real presentation, pointers and history; explicit geometry only.
use super::super::{
    TimelineSnapMode, TimelineState, action_controls, execution, gestures,
    navigation_cursor::OverrideCursor, presentation, state::TimelineView,
    timeline_pointer::tests as pointer,
};
use super::*;
use crate::feathers::action_activation::{FeathersActionButton, PendingFeathersActivation};
use crate::{
    CurvesState, ProjectEffectCatalog,
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    material_function_editor::FunctionEditor,
    menus::MenuState,
};
use aestra_core::{
    ChoreographyEvent, ChoreographyEventId, ChoreographyEventPayload, EffectAsset, EffectMarker,
    Emitter, MarkerId,
};
use bevy::{
    asset::AssetPlugin,
    ecs::system::RunSystemOnce,
    picking::pointer::{PointerButton, PointerId},
    scene::ScenePlugin,
    text::TextPlugin,
    ui_widgets::Activate,
    window::{CursorIcon, PrimaryWindow},
};

#[derive(Resource)]
struct Built {
    canvas: Entity,
    ruler: Entity,
}
#[derive(Resource, Default)]
struct Bubbled {
    clicks: usize,
    presses: usize,
    drags: usize,
}
#[derive(Clone, Copy)]
enum Kind {
    Marker,
    Event,
}
fn build(mut commands: Commands, session: Res<EditorSession>, localizer: Res<Localizer>) {
    let mut ruler = Entity::PLACEHOLDER;
    let canvas = commands
        .spawn((
            TimelineCanvas,
            Node::default(),
            ComputedNode {
                size: Vec2::new(400.0, 140.0),
                ..default()
            },
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::ZERO),
            },
        ))
        .observe(gestures::seek_timeline_on_press)
        .observe(gestures::seek_timeline_on_drag)
        .observe(
            |_: On<super::super::timeline_pointer::PointerClick>, mut b: ResMut<Bubbled>| {
                b.clicks += 1
            },
        )
        .observe(
            |_: On<super::super::timeline_pointer::PointerPress>, mut b: ResMut<Bubbled>| {
                b.presses += 1
            },
        )
        .observe(
            |_: On<super::super::timeline_pointer::PointerDrag>, mut b: ResMut<Bubbled>| {
                b.drags += 1
            },
        )
        .with_children(|parent| {
            ruler = parent
                .spawn(Node {
                    height: Val::Px(25.0),
                    ..default()
                })
                .with_children(|row| spawn_ruler(row, &session, &localizer))
                .id();
            spawn_choreography_event_lane(parent, &session, &localizer);
            spawn_effect_drop_spacer(parent);
        })
        .id();
    commands.insert_resource(Built { canvas, ruler });
}
struct Fixture {
    root: tempfile::TempDir,
    app: App,
    window: Entity,
    before: EffectAsset,
    bytes: Vec<u8>,
    markers: [MarkerId; 2],
    events: [ChoreographyEventId; 2],
}
impl Fixture {
    fn new() -> Self {
        Self::configured("en-US", 1.0, |_, _, _| {})
    }
    fn configured(
        locale: &str,
        scale: f32,
        prepare: impl FnOnce(&mut App, MarkerId, ChoreographyEventId),
    ) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut effect = EffectAsset::new("Cues", 8.0);
        effect.emitters.push(Emitter::basic_sprite("Emitter", 4.0));
        effect.markers.extend([
            EffectMarker {
                id: MarkerId::new(),
                name: "Launch".into(),
                time: 1.0,
            },
            EffectMarker {
                id: MarkerId::new(),
                name: "Finale".into(),
                time: 3.5,
            },
        ]);
        effect.choreography_events.extend([
            ChoreographyEvent::new(
                "Burst sound",
                1.0,
                ChoreographyEventPayload::PlaySound {
                    cue: "burst".into(),
                },
            ),
            ChoreographyEvent::new(
                "Final sound",
                3.5,
                ChoreographyEventPayload::PlaySound {
                    cue: "finale".into(),
                },
            ),
        ]);
        let markers = [effect.markers[0].id, effect.markers[1].id];
        let events = [
            effect.choreography_events[0].id,
            effect.choreography_events[1].id,
        ];
        let mut state = TimelineState::framed(8.0);
        state.snap = TimelineSnapMode::None;
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
        ))
        .insert_resource(EditorSession::from_test_effect(effect))
        .insert_resource(state)
        .insert_resource(Localizer::new(locale).unwrap())
        .insert_resource(ProjectEffectCatalog::scan(root.path()))
        .init_resource::<CurvesState>()
        .init_resource::<MenuState>()
        .init_resource::<Bubbled>()
        .init_resource::<OverrideCursor>()
        .init_resource::<EditorHistoryLedger>()
        .init_resource::<MaterialProgramEditHistory>()
        .init_resource::<FunctionEditor>()
        .add_observer(execution::execute_timeline_action)
        .add_observer(action_controls::queue_timeline_action_activation)
        .add_observer(
            |e: On<HistoryAction>, routing: Routing, mut commands: Commands| {
                routing.execute(*e, &mut commands, |_, _, _, _| {
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
                .chain(),
        );
        let mut window = Window::default();
        window.resolution.set_scale_factor_override(Some(scale));
        let window = app
            .world_mut()
            .spawn((window, PrimaryWindow, CursorIcon::default()))
            .id();
        prepare(&mut app, markers[0], events[0]);
        let before = app.world().resource::<EditorSession>().effect.clone();
        before
            .save_ron(root.path().join("cues.aestra.ron"))
            .unwrap();
        let bytes = std::fs::read(root.path().join("cues.aestra.ron")).unwrap();
        app.update();
        Self {
            root,
            app,
            window,
            before,
            bytes,
            markers,
            events,
        }
    }
    fn cue(&mut self, kind: Kind, index: usize) -> Entity {
        let w = self.app.world_mut();
        match kind {
            Kind::Marker => w
                .query::<(Entity, &TimelineMarker)>()
                .iter(w)
                .find_map(|(e, c)| (c.marker == self.markers[index]).then_some(e))
                .unwrap(),
            Kind::Event => w
                .query::<(Entity, &TimelineChoreographyEvent)>()
                .iter(w)
                .find_map(|(e, c)| (c.event == self.events[index]).then_some(e))
                .unwrap(),
        }
    }
    fn event<E: Event>(&mut self, event: E)
    where
        for<'a> E::Trigger<'a>: Default,
    {
        self.app.world_mut().trigger(event);
        self.app.world_mut().flush();
    }
    fn present(&mut self) {
        let w = self.app.world_mut();
        w.run_system_once(presentation::update_timeline_visuals)
            .unwrap();
        w.run_system_once(presentation::update_timeline_marker_visuals)
            .unwrap();
        w.run_system_once(presentation::update_choreography_event_visuals)
            .unwrap();
    }
    fn begin(&mut self, e: Entity) {
        self.event(pointer::start(
            e,
            self.window,
            Vec2::ZERO,
            PointerId::Mouse,
            PointerButton::Primary,
        ));
    }
    fn motion(&mut self, e: Entity, x: f32) {
        self.event(pointer::motion_delta(e, self.window, Vec2::new(x, 0.0)));
    }
    fn end(&mut self, e: Entity) {
        self.event(pointer::end(
            e,
            self.window,
            Vec2::ZERO,
            PointerId::Mouse,
            PointerButton::Primary,
        ));
    }
    fn selected(&self, kind: Kind) -> SemanticTarget {
        match kind {
            Kind::Marker => SemanticTarget::Marker(self.markers[0]),
            Kind::Event => SemanticTarget::ChoreographyEvent(self.events[0]),
        }
    }
    fn preview(&self, kind: Kind) -> f32 {
        let s = self.app.world().resource::<TimelineState>();
        match kind {
            Kind::Marker => s.marker_drag.unwrap().current_time,
            Kind::Event => s.choreography_event_drag.unwrap().current_time,
        }
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
            std::fs::read(self.root.path().join("cues.aestra.ron")).unwrap(),
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
fn left(f: &Fixture, e: Entity) -> f32 {
    let Val::Percent(x) = f.app.world().get::<Node>(e).unwrap().left else {
        panic!("Expected percent")
    };
    x
}

#[test]
fn ruler_constructs_exact_retained_ticks_with_noninteractive_labels() {
    let mut f = Fixture::new();
    let w = f.app.world_mut();
    let mut ticks = w.query::<(Entity, &TimelineRulerTick, &Node, &Children)>();
    assert_eq!(ticks.iter(w).count(), 32);
    let mut ids = Vec::new();
    for (e, tick, node, children) in ticks.iter(w) {
        ids.push(tick.0);
        assert_eq!(node.display, Display::None);
        assert_eq!(node.width, Val::Px(1.0));
        assert_eq!(children.len(), 1);
        assert!(w.get::<Pickable>(e).is_some());
        let text = children[0];
        assert_eq!(w.get::<Text>(text).unwrap().0, "0.0");
        assert!(w.get::<Pickable>(text).is_some());
    }
    ids.sort();
    assert_eq!(ids, (0..32).collect::<Vec<_>>());
    f.clean();
}

#[test]
fn markers_and_events_keep_native_action_and_cursor_classification() {
    let mut f = Fixture::new();
    for kind in [Kind::Marker, Kind::Event] {
        let e = f.cue(kind, 0);
        let w = f.app.world();
        assert!(w.get::<Button>(e).is_some());
        assert!(w.get::<EditorNativeControl>(e).is_some());
        assert!(w.get::<FeathersActionButton>(e).is_none());
        assert_eq!(
            *w.get::<EntityCursor>(e).unwrap(),
            EntityCursor::System(SystemCursorIcon::Grab)
        );
        assert_eq!(w.get::<Node>(e).unwrap().display, Display::None);
        let expected = match kind {
            Kind::Marker => TimelineAction::SelectMarker(f.markers[0]),
            Kind::Event => TimelineAction::SelectChoreographyEvent(f.events[0]),
        };
        assert_eq!(w.get::<TimelineAction>(e), Some(&expected));
    }
    f.clean();
}

#[test]
fn localized_cue_labels_and_event_tooltip_are_real_production_components() {
    for locale in ["en-US", "fr-FR"] {
        let mut f = Fixture::configured(locale, 1.0, |_, _, _| {});
        for (kind, key, name) in [
            (Kind::Marker, "timeline-marker", "Launch"),
            (Kind::Event, "timeline-event", "Burst sound"),
        ] {
            let e = f.cue(kind, 0);
            let w = f.app.world();
            assert_eq!(
                w.get::<AccessibleLabel>(e).unwrap().0,
                format!("{} {name}", w.resource::<Localizer>().text(key))
            );
            if matches!(kind, Kind::Event) {
                assert!(w.get::<EditorTooltip>(e).is_some());
            }
        }
        f.clean();
    }
}

#[test]
fn selected_markers_preserve_fill_border_and_label_color() {
    let mut f = Fixture::configured("en-US", 1.0, |app, id, _| {
        app.world_mut()
            .resource_mut::<EditorSession>()
            .selection
            .primary = SemanticTarget::Marker(id)
    });
    let e = f.cue(Kind::Marker, 0);
    let other = f.cue(Kind::Marker, 1);
    let w = f.app.world();
    assert_eq!(
        w.get::<BackgroundColor>(e).unwrap().0,
        theme::ACCENT_DIM.with_alpha(0.8)
    );
    assert_eq!(
        w.get::<BackgroundColor>(other).unwrap().0,
        theme::PANEL_LIGHT.with_alpha(0.9)
    );
    assert_eq!(w.get::<Node>(e).unwrap().max_width, Val::Px(120.0));
    let label = w.get::<Children>(e).unwrap()[0];
    assert_eq!(w.get::<TextColor>(label).unwrap().0, theme::TEXT);
    f.clean();
}

#[test]
fn selected_events_preserve_diamond_fill_and_border_geometry() {
    let mut f = Fixture::configured("en-US", 1.0, |app, _, id| {
        app.world_mut()
            .resource_mut::<EditorSession>()
            .selection
            .primary = SemanticTarget::ChoreographyEvent(id)
    });
    let e = f.cue(Kind::Event, 0);
    let other = f.cue(Kind::Event, 1);
    let w = f.app.world();
    assert_eq!(w.get::<Text>(e).unwrap().0, "◆");
    assert_eq!(w.get::<BackgroundColor>(e).unwrap().0, theme::ACCENT);
    assert_eq!(
        w.get::<BackgroundColor>(other).unwrap().0,
        theme::ACCENT_DIM
    );
    let node = w.get::<Node>(e).unwrap();
    assert_eq!(node.width, Val::Px(18.0));
    assert_eq!(node.height, Val::Px(18.0));
    assert_eq!(node.margin.left, Val::Px(-9.0));
    assert_eq!(node.border.left, Val::Px(2.0));
    assert_eq!(w.get::<Node>(other).unwrap().border.left, Val::Px(1.0));
    f.clean();
}

#[test]
fn event_lane_and_drop_spacer_preserve_canvas_layout_contract() {
    let mut f = Fixture::new();
    let event = f.cue(Kind::Event, 0);
    let marker = f.cue(Kind::Marker, 0);
    let w = f.app.world();
    let built = w.resource::<Built>();
    assert_eq!(w.get::<ChildOf>(marker).unwrap().parent(), built.ruler);
    let lane = w.get::<ChildOf>(event).unwrap().parent();
    assert_eq!(w.get::<ChildOf>(lane).unwrap().parent(), built.canvas);
    assert_eq!(w.get::<Node>(lane).unwrap().top, Val::Px(25.0));
    assert_eq!(w.get::<Node>(lane).unwrap().height, Val::Px(28.0));
    let w = f.app.world_mut();
    let e = w
        .query_filtered::<Entity, With<TimelineEffectDropSpacer>>()
        .single(w)
        .unwrap();
    let node = w.get::<Node>(e).unwrap();
    assert_eq!(node.display, Display::None);
    assert_eq!(node.height, Val::Px(31.0));
    assert_eq!(node.grid_row, GridPlacement::start(1));
    assert!(w.get::<RelativeCursorPosition>(e).is_some());
    assert!(w.get::<Pickable>(e).is_some());
    f.clean();
}

#[test]
fn actual_presentation_positions_cues_without_respawning_controls() {
    let mut f = Fixture::new();
    let marker = f.cue(Kind::Marker, 0);
    let event = f.cue(Kind::Event, 0);
    f.present();
    for e in [marker, event] {
        assert_eq!(f.app.world().get::<Node>(e).unwrap().display, Display::Flex);
        near(left(&f, e), 12.5);
    }
    f.present();
    assert_eq!(f.cue(Kind::Marker, 0), marker);
    assert_eq!(f.cue(Kind::Event, 0), event);
    f.clean();
}

#[test]
fn ruler_tick_update_reuses_ids_and_formats_zoomed_times() {
    let mut f = Fixture::new();
    f.present();
    let w = f.app.world_mut();
    let ticks = w
        .query::<(Entity, &TimelineRulerTick)>()
        .iter(w)
        .map(|(e, t)| (t.0, e))
        .collect::<Vec<_>>();
    f.app.world_mut().resource_mut::<TimelineState>().view = TimelineView {
        start: 0.5,
        end: 1.5,
    };
    f.present();
    let w = f.app.world();
    for (index, e) in ticks {
        assert_eq!(w.get::<TimelineRulerTick>(e).unwrap().0, index);
        if index == 0 {
            let label = w.get::<Children>(e).unwrap()[0];
            assert_eq!(w.get::<Text>(label).unwrap().0, "0.50");
            near(left(&f, e), 0.0);
        }
    }
    f.clean();
}

#[test]
fn offscreen_cues_hide_and_reappear_with_same_identity() {
    let mut f = Fixture::new();
    let marker = f.cue(Kind::Marker, 0);
    let event = f.cue(Kind::Event, 0);
    f.app.world_mut().resource_mut::<TimelineState>().view = TimelineView {
        start: 2.0,
        end: 4.0,
    };
    f.present();
    for e in [marker, event] {
        assert_eq!(f.app.world().get::<Node>(e).unwrap().display, Display::None);
    }
    f.app.world_mut().resource_mut::<TimelineState>().view = TimelineView {
        start: 0.0,
        end: 2.0,
    };
    f.present();
    for e in [marker, event] {
        assert_eq!(f.app.world().get::<Node>(e).unwrap().display, Display::Flex);
        near(left(&f, e), 50.0);
    }
    f.clean();
}

#[test]
fn missing_authored_cues_hide_without_accepting_stale_drag_start() {
    let mut f = Fixture::new();
    let marker = f.cue(Kind::Marker, 0);
    let event = f.cue(Kind::Event, 0);
    {
        let mut s = f.app.world_mut().resource_mut::<EditorSession>();
        s.effect.markers.clear();
        s.effect.choreography_events.clear();
    }
    let before = f.app.world().resource::<EditorSession>().effect.clone();
    f.present();
    for e in [marker, event] {
        assert_eq!(f.app.world().get::<Node>(e).unwrap().display, Display::None);
        f.begin(e);
    }
    let state = f.app.world().resource::<TimelineState>();
    assert!(state.marker_drag.is_none());
    assert!(state.choreography_event_drag.is_none());
    assert_eq!(f.app.world().resource::<EditorSession>().effect, before);
    assert!(!f.app.world().resource::<EditorSession>().can_undo());
    f.disk();
}

#[test]
fn primary_click_selects_actual_cue_and_stops_parent_click() {
    for kind in [Kind::Marker, Kind::Event] {
        let mut f = Fixture::new();
        let e = f.cue(kind, 0);
        f.event(pointer::click(e, f.window, PointerButton::Primary, 1));
        assert_eq!(
            f.app.world().resource::<EditorSession>().selection.primary,
            f.selected(kind)
        );
        assert_eq!(f.app.world().resource::<Bubbled>().clicks, 0);
        f.clean();
    }
}

#[test]
fn secondary_click_and_injected_activate_do_not_select_native_cues() {
    let mut f = Fixture::new();
    let initial = f.app.world().resource::<EditorSession>().selection.primary;
    for kind in [Kind::Marker, Kind::Event] {
        let e = f.cue(kind, 0);
        f.event(pointer::click(e, f.window, PointerButton::Secondary, 1));
        f.event(Activate { entity: e });
        f.app.update();
        assert!(f.app.world().get::<PendingFeathersActivation>(e).is_none());
        assert_eq!(
            f.app.world().resource::<EditorSession>().selection.primary,
            initial
        );
    }
    f.clean();
}

#[test]
fn attached_control_press_stops_canvas_seek_on_both_cue_types() {
    let mut f = Fixture::new();
    for kind in [Kind::Marker, Kind::Event] {
        let e = f.cue(kind, 0);
        f.event(pointer::press(
            e,
            f.window,
            Vec2::ZERO,
            PointerId::Mouse,
            PointerButton::Primary,
        ));
    }
    assert_eq!(f.app.world().resource::<Bubbled>().presses, 0);
    near(f.app.world().resource::<EditorSession>().time(), 0.0);
    f.clean();
}

#[test]
fn marker_drag_attached_observers_preview_commit_and_roundtrip_one_edit() {
    let mut f = Fixture::new();
    let e = f.cue(Kind::Marker, 0);
    f.begin(e);
    f.motion(e, 50.0);
    f.present();
    near(f.preview(Kind::Marker), 2.0);
    near(left(&f, e), 25.0);
    f.clean();
    near(f.app.world().resource::<EditorSession>().time(), 0.0);
    assert_eq!(f.app.world().resource::<Bubbled>().drags, 0);
    f.end(e);
    near(
        f.app.world().resource::<EditorSession>().effect.markers[0].time,
        2.0,
    );
    assert_eq!(
        f.app.world().resource::<EditorSession>().selection.primary,
        f.selected(Kind::Marker)
    );
    assert!(
        f.app
            .world()
            .resource::<TimelineState>()
            .marker_drag
            .is_none()
    );
    assert!(f.app.world().resource::<OverrideCursor>().0.is_none());
    f.roundtrip();
}

#[test]
fn event_drag_attached_observers_preserve_payload_and_roundtrip_one_edit() {
    let mut f = Fixture::new();
    let e = f.cue(Kind::Event, 0);
    f.begin(e);
    f.motion(e, 50.0);
    f.present();
    near(f.preview(Kind::Event), 2.0);
    near(left(&f, e), 25.0);
    f.clean();
    f.end(e);
    let s = f.app.world().resource::<EditorSession>();
    near(s.effect.choreography_events[0].time, 2.0);
    assert_eq!(
        s.effect.choreography_events[0].payload,
        f.before.choreography_events[0].payload
    );
    assert_eq!(s.selection.primary, f.selected(Kind::Event));
    assert_eq!(f.app.world().resource::<Bubbled>().drags, 0);
    f.roundtrip();
}

#[test]
fn attached_cue_drags_scale_at_two_dpis_and_clamp_at_effect_bounds() {
    for scale in [1.0, 2.0] {
        for kind in [Kind::Marker, Kind::Event] {
            let mut f = Fixture::configured("en-US", scale, |_, _, _| {});
            let e = f.cue(kind, 0);
            f.begin(e);
            f.motion(e, 50.0 * scale);
            near(f.preview(kind), 2.0);
            f.motion(e, -10000.0);
            near(f.preview(kind), 0.0);
            f.motion(e, 10000.0);
            near(f.preview(kind), 8.0);
            f.clean();
        }
    }
}

#[test]
fn no_motion_release_selects_but_creates_no_history_and_restores_cursor() {
    for kind in [Kind::Marker, Kind::Event] {
        let mut f = Fixture::new();
        let e = f.cue(kind, 0);
        f.begin(e);
        assert_eq!(
            *f.app.world().get::<CursorIcon>(f.window).unwrap(),
            CursorIcon::System(SystemCursorIcon::Grabbing)
        );
        f.end(e);
        assert_eq!(
            *f.app.world().get::<CursorIcon>(f.window).unwrap(),
            CursorIcon::System(SystemCursorIcon::Grab)
        );
        assert_eq!(
            f.app.world().resource::<EditorSession>().selection.primary,
            f.selected(kind)
        );
        f.clean();
    }
}

#[test]
fn empty_document_keeps_ruler_and_event_lane_without_fake_controls() {
    let mut f = Fixture::configured("en-US", 1.0, |app, _, _| {
        let mut s = app.world_mut().resource_mut::<EditorSession>();
        s.effect.markers.clear();
        s.effect.choreography_events.clear();
    });
    let w = f.app.world_mut();
    assert_eq!(
        w.query_filtered::<Entity, With<TimelineMarker>>()
            .iter(w)
            .count(),
        0
    );
    assert_eq!(
        w.query_filtered::<Entity, With<TimelineChoreographyEvent>>()
            .iter(w)
            .count(),
        0
    );
    assert_eq!(
        w.query_filtered::<Entity, With<TimelineRulerTick>>()
            .iter(w)
            .count(),
        32
    );
    let canvas = w.resource::<Built>().canvas;
    let lanes = w
        .get::<Children>(canvas)
        .unwrap()
        .iter()
        .filter(|e| {
            w.get::<Node>(*e)
                .is_some_and(|n| n.top == Val::Px(25.0) && n.height == Val::Px(28.0))
        })
        .count();
    assert_eq!(lanes, 1);
    f.present();
    f.clean();
}
