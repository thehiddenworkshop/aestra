//! Actual host header/lane controls and their existing action, gesture and history consumers.
use super::super::{
    document_actions::TimelineSnapMode,
    interaction_state::TimelineCanvas,
    motion_execution, motion_gestures,
    state::{TimelineState, TimelineView},
    timeline_pointer::tests as pointer,
};
use super::*;
use crate::{
    CurvesState, Localizer, ProjectEffectCatalog,
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    material_function_editor::FunctionEditor,
};
use aestra_core::{EffectAsset, EmitterTransform, HostTransformKey, HostTransformTrack};
use bevy::{
    asset::AssetPlugin,
    picking::pointer::{PointerButton, PointerId},
    scene::ScenePlugin,
    text::TextPlugin,
};

#[derive(Resource)]
struct Root(Entity);
#[derive(Resource, Default)]
struct Presses(usize);
fn build(mut commands: Commands, session: Res<EditorSession>) {
    commands.spawn(Node::default()).with_children(spawn_header);
    let canvas = commands
        .spawn((
            Node::default(),
            TimelineCanvas,
            ComputedNode {
                size: Vec2::new(400.0, 100.0),
                ..default()
            },
        ))
        .observe(
            |_: On<super::super::timeline_pointer::PointerPress>, mut presses: ResMut<Presses>| {
                presses.0 += 1
            },
        )
        .with_children(|parent| spawn_lane(parent, &session))
        .id();
    commands.insert_resource(Root(canvas));
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
    window: Entity,
    before: EffectAsset,
    path: std::path::PathBuf,
    bytes: Vec<u8>,
}
impl Fixture {
    fn new() -> Self {
        Self::configured(true, 1.0)
    }
    fn configured(with_track: bool, scale: f32) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut effect = EffectAsset::new("Motion", 4.0);
        effect.host_transform_track = with_track.then(track);
        let path = root.path().join("motion.aestra.ron");
        effect.save_ron(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let before = effect.clone();
        let mut state = TimelineState::framed(4.0);
        state.view = TimelineView {
            start: 0.0,
            end: 4.0,
        };
        state.snap = TimelineSnapMode::None;
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
        ))
        .init_asset::<Image>()
        .init_asset::<crate::feathers::icon::SvgFile>()
        .insert_resource(EditorSession::from_test_effect(effect))
        .insert_resource(state)
        .insert_resource(ProjectEffectCatalog::scan(root.path()))
        .insert_resource(Localizer::new("en-US").unwrap())
        .init_resource::<CurvesState>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<Presses>()
        .init_resource::<EditorHistoryLedger>()
        .init_resource::<MaterialProgramEditHistory>()
        .init_resource::<FunctionEditor>()
        .add_observer(activate)
        .add_observer(motion_execution::execute)
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
                buttons,
                motion_execution::cancel_drag,
                motion_gestures::update_keys,
            )
                .chain(),
        );
        let mut window = Window::default();
        window.resolution.set_scale_factor_override(Some(scale));
        let window = app.world_mut().spawn(window).id();
        app.update();
        Self {
            _root: root,
            app,
            window,
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
    fn control(&mut self, inspect: bool) -> Entity {
        let w = self.app.world_mut();
        w.query::<(Entity, &Action)>()
            .iter(w)
            .find_map(|(e, a)| {
                (if inspect {
                    matches!(a, Action::Inspect)
                } else {
                    matches!(a, Action::Add)
                })
                .then_some(e)
            })
            .unwrap()
    }
    fn key(&mut self, index: usize) -> Entity {
        let w = self.app.world_mut();
        w.query::<(Entity, &KeyControl)>()
            .iter(w)
            .find_map(|(e, k)| (k.index == index).then_some(e))
            .unwrap()
    }
    fn unchanged(&self) {
        assert_eq!(std::fs::read(&self.path).unwrap(), self.bytes);
    }
    fn clean(&self) {
        assert_eq!(self.session().effect, self.before);
        assert!(!self.session().can_undo());
        self.unchanged();
    }
    fn roundtrip(&mut self) {
        let after = self.session().effect.clone();
        assert_ne!(after, self.before);
        self.event(HistoryAction::Undo);
        assert_eq!(self.session().effect, self.before);
        assert!(!self.session().can_undo());
        self.event(HistoryAction::Redo);
        assert_eq!(self.session().effect, after);
        self.unchanged();
    }
    fn activate(&mut self, target: Entity) {
        self.event(Activate { entity: target });
        self.app.update();
    }
    fn begin(&mut self, target: Entity) {
        self.event(pointer::start(
            target,
            self.window,
            Vec2::ZERO,
            PointerId::Mouse,
            PointerButton::Primary,
        ));
    }
    fn motion(&mut self, target: Entity, distance: f32) {
        self.event(pointer::motion_delta(
            target,
            self.window,
            Vec2::new(distance, 0.0),
        ));
    }
    fn end(&mut self, target: Entity) {
        self.event(pointer::end(
            target,
            self.window,
            Vec2::ZERO,
            PointerId::Mouse,
            PointerButton::Primary,
        ));
    }
}

#[test]
fn header_preserves_real_feathers_actions_and_motion_ownership_markers() {
    let mut f = Fixture::new();
    for inspect in [true, false] {
        let e = f.control(inspect);
        let w = f.app.world();
        assert!(w.get::<FeathersActionButton>(e).is_some());
        assert!(w.get::<HostMotionControl>(e).is_some());
        assert!(w.get::<RelativeCursorPosition>(e).is_some());
        assert!(w.get::<AccessibleLabel>(e).is_some());
        if inspect {
            let n = w.get::<Node>(e).unwrap();
            assert_eq!(
                (n.width, n.height, n.flex_shrink),
                (Val::Auto, Val::Px(24.0), 0.0)
            );
        } else {
            assert!(w.get::<EditorTooltip>(e).is_some());
        }
    }
    f.clean();
}

#[test]
fn pose_lane_has_exact_native_key_identity_geometry_cursor_and_labels() {
    let mut f = Fixture::new();
    for index in 0..3 {
        let e = f.key(index);
        let w = f.app.world();
        let n = w.get::<Node>(e).unwrap();
        assert_eq!(
            (n.width, n.height, n.margin.left),
            (Val::Px(18.0), Val::Px(20.0), Val::Px(-9.0))
        );
        assert!(w.get::<EditorNativeControl>(e).is_some());
        assert!(w.get::<Button>(e).is_some());
        assert!(w.get::<HostMotionControl>(e).is_some());
        assert_eq!(
            w.get::<AccessibleLabel>(e).unwrap().0,
            format!("Host pose key at {:.3} seconds", index as f32)
        );
        assert!(
            matches!(w.get::<EntityCursor>(e), Some(EntityCursor::System(c)) if *c == if index == 0 { SystemCursorIcon::Pointer } else { SystemCursorIcon::EwResize })
        );
        let lane = w.get::<ChildOf>(e).unwrap().parent();
        let lane = w.get::<Node>(lane).unwrap();
        assert_eq!((lane.top, lane.height), (Val::Px(53.0), Val::Px(28.0)));
    }
    f.clean();
}

#[test]
fn absent_track_builds_no_dummy_keys_and_header_can_create_the_real_track() {
    let mut f = Fixture::configured(false, 1.0);
    let w = f.app.world_mut();
    assert_eq!(w.query::<&KeyControl>().iter(w).count(), 0);
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .seek_time(0.75);
    let add = f.control(false);
    f.activate(add);
    assert_eq!(
        f.session()
            .effect
            .host_transform_track
            .as_ref()
            .unwrap()
            .curves
            .key_times(),
        vec![0.0, 0.75]
    );
    f.roundtrip();
}

#[test]
fn inspect_activation_uses_real_consumer_without_semantic_history() {
    let mut f = Fixture::new();
    let e = f.control(true);
    f.activate(e);
    assert!(super::super::motion_state::active(f.session(), f.state()));
    assert!(f.app.world().get::<PendingFeathersActivation>(e).is_none());
    assert!(!control_input::is_pressed(f.app.world(), e));
    f.clean();
}

#[test]
fn add_activation_at_existing_key_selects_without_duplication_or_history() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .seek_time(1.0);
    let e = f.control(false);
    f.activate(e);
    assert_eq!(f.state().host_motion.selected, Some(1));
    f.clean();
}

#[test]
fn add_activation_is_consumed_once_and_global_undo_restores_the_track() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .seek_time(0.5);
    let e = f.control(false);
    f.activate(e);
    let after = f.session().effect.clone();
    for _ in 0..3 {
        f.app.update();
    }
    assert_eq!(f.session().effect, after);
    assert!(f.app.world().get::<PendingFeathersActivation>(e).is_none());
    f.roundtrip();
}

#[test]
fn raw_pressed_state_without_activate_cannot_dispatch_a_header_action() {
    let mut f = Fixture::new();
    let e = f.control(false);
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .seek_time(0.5);
    control_input::set_pressed(f.app.world_mut(), e, true);
    f.app.update();
    f.clean();
}

#[test]
fn activate_rejects_an_action_without_feathers_classification() {
    let mut f = Fixture::new();
    let e = f.app.world_mut().spawn(Action::Add).id();
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .seek_time(0.5);
    f.activate(e);
    assert!(f.app.world().get::<PendingFeathersActivation>(e).is_none());
    f.clean();
}

#[test]
fn built_key_click_selects_through_the_attached_observer() {
    let mut f = Fixture::new();
    let e = f.key(2);
    f.event(pointer::click(e, f.window, PointerButton::Primary, 1));
    assert_eq!(f.state().host_motion.selected, Some(2));
    assert!(super::super::motion_state::active(f.session(), f.state()));
    f.clean();
}

#[test]
fn built_anchor_cannot_be_retimed() {
    let mut f = Fixture::new();
    let e = f.key(0);
    f.begin(e);
    f.motion(e, 100.0);
    f.end(e);
    assert!(f.state().host_motion.drag.is_none());
    f.clean();
}

#[test]
fn built_key_drag_previews_exact_time_and_globally_commits_once_at_two_dpi_scales() {
    for scale in [1.0, 2.0] {
        let mut f = Fixture::configured(true, scale);
        let e = f.key(1);
        f.begin(e);
        f.motion(e, 50.0 / scale);
        let drag = f.state().host_motion.drag.as_ref().unwrap();
        assert_eq!(drag.candidate.keys()[drag.selected].time, 1.5);
        f.clean();
        f.end(e);
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
        f.roundtrip();
    }
}

#[test]
fn crossing_key_and_trailing_click_preserve_reordered_selection() {
    let mut f = Fixture::new();
    let e = f.key(1);
    f.begin(e);
    f.motion(e, 200.0);
    f.end(e);
    f.event(pointer::click(e, f.window, PointerButton::Primary, 1));
    assert_eq!(f.state().host_motion.selected, Some(2));
    assert_eq!(
        f.session()
            .effect
            .host_transform_track
            .as_ref()
            .unwrap()
            .keys()[2]
            .time,
        3.0
    );
    f.roundtrip();
}

#[test]
fn built_key_presses_do_not_seek_parent_canvas() {
    let mut f = Fixture::new();
    for index in 0..3 {
        let e = f.key(index);
        f.event(pointer::press(
            e,
            f.window,
            Vec2::ZERO,
            PointerId::Mouse,
            PointerButton::Primary,
        ));
    }
    assert_eq!(f.app.world().resource::<Presses>().0, 0);
    f.clean();
}

#[test]
fn built_key_visuals_follow_zoom_selection_and_keep_entity_identity() {
    let mut f = Fixture::new();
    let keys = [f.key(0), f.key(1), f.key(2)];
    let selected = keys[1];
    f.event(pointer::click(
        selected,
        f.window,
        PointerButton::Primary,
        1,
    ));
    f.app.world_mut().resource_mut::<TimelineState>().view = TimelineView {
        start: 0.5,
        end: 1.5,
    };
    f.app.update();
    assert_eq!(
        f.app.world().get::<Node>(keys[0]).unwrap().display,
        Display::None
    );
    assert_eq!(
        f.app.world().get::<Node>(selected).unwrap().left,
        Val::Percent(50.0)
    );
    assert_eq!(
        f.app.world().get::<BackgroundColor>(selected).unwrap().0,
        theme::ACCENT
    );
    assert_eq!(
        f.app.world().get::<Node>(keys[2]).unwrap().display,
        Display::None
    );
    for (i, e) in keys.into_iter().enumerate() {
        assert_eq!(f.key(i), e);
    }
    let canvas = f.app.world().resource::<Root>().0;
    assert!(f.app.world().get_entity(canvas).is_ok());
    f.clean();
}
