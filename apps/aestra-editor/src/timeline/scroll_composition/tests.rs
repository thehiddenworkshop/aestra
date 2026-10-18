//! Constructed timeline gutters with real scene controls and production scroll consumers.
use super::super::{
    presentation::{sync_timeline_horizontal_scroll, sync_timeline_vertical_scroll},
    state::TimelineView,
    timeline_pointer::tests as pointer,
};
use super::*;
use crate::feathers::scroll::update_scrollbar_visibility;
use aestra_core::EffectAsset;
use bevy::{
    asset::AssetPlugin,
    picking::pointer::{PointerButton, PointerId},
    scene::ScenePlugin,
    ui_widgets::{ControlOrientation, ScrollArea, Scrollbar},
};

#[derive(Resource)]
struct Panes {
    headers: Entity,
    tracks: Entity,
}
#[derive(Resource, Default)]
struct ParentInput {
    presses: usize,
    drags: usize,
}
fn build(mut commands: Commands, session: Res<EditorSession>, state: Res<TimelineState>) {
    let mut headers = Entity::PLACEHOLDER;
    let mut tracks = Entity::PLACEHOLDER;
    commands
        .spawn(Node::default())
        .observe(
            |_: On<super::super::timeline_pointer::PointerPress>,
             mut events: ResMut<ParentInput>| events.presses += 1,
        )
        .observe(
            |_: On<super::super::timeline_pointer::PointerDrag>,
             mut events: ResMut<ParentInput>| events.drags += 1,
        )
        .with_children(|body| {
            headers = body
                .spawn((
                    Node::default(),
                    TimelineVerticalPane::Headers,
                    ScrollPosition::default(),
                    ComputedNode {
                        size: Vec2::new(200.0, 100.0),
                        content_size: Vec2::new(200.0, 400.0),
                        ..default()
                    },
                ))
                .id();
            tracks = body
                .spawn((
                    Node::default(),
                    TimelineVerticalPane::Tracks,
                    ScrollArea,
                    ScrollPosition::default(),
                    ComputedNode {
                        size: Vec2::new(400.0, 100.0),
                        content_size: Vec2::new(400.0, 400.0),
                        ..default()
                    },
                ))
                .id();
            spawn_horizontal_gutter(body, &session, &state);
            spawn_vertical_gutter(body, tracks);
            body.spawn((TimelineHorizontalGutter, Node::default()));
        });
    commands.insert_resource(Panes { headers, tracks });
}
struct Fixture {
    app: App,
    before: EffectAsset,
    window: Entity,
}
impl Fixture {
    fn new() -> Self {
        Self::configured(0.0, 4.0)
    }
    fn configured(start: f32, end: f32) -> Self {
        let effect = EffectAsset::new("Scroll", 8.0);
        let before = effect.clone();
        let mut state = TimelineState::framed(8.0);
        state.view = TimelineView { start, end };
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(), ScenePlugin))
            .insert_resource(EditorSession::from_test_effect(effect))
            .insert_resource(state)
            .init_resource::<ParentInput>()
            .add_systems(Startup, build)
            .add_systems(
                Update,
                (
                    sync_timeline_horizontal_scroll,
                    sync_timeline_vertical_scroll,
                    update_scrollbar_visibility,
                )
                    .chain(),
            );
        let window = app.world_mut().spawn(Window::default()).id();
        app.update();
        Self {
            app,
            before,
            window,
        }
    }
    fn one<C: Component>(&mut self) -> Entity {
        let w = self.app.world_mut();
        w.query_filtered::<Entity, With<C>>().single(w).unwrap()
    }
    fn node(&self, e: Entity) -> &Node {
        self.app.world().get(e).unwrap()
    }
    fn clean(&self) {
        let session = self.app.world().resource::<EditorSession>();
        assert_eq!(session.effect, self.before);
        assert!(!session.can_undo());
    }
    fn horizontal(&mut self, content: f32) -> Entity {
        let e = self.one::<TimelineHorizontalScrollViewport>();
        self.app.world_mut().entity_mut(e).insert(ComputedNode {
            size: Vec2::new(400.0, 100.0),
            content_size: Vec2::new(content, 100.0),
            ..default()
        });
        self.app.update();
        e
    }
    fn bar(&mut self, orientation: ControlOrientation) -> Entity {
        let w = self.app.world_mut();
        w.query::<(Entity, &Scrollbar)>()
            .iter(w)
            .find_map(|(e, b)| (b.orientation == orientation).then_some(e))
            .unwrap()
    }
    fn event<E: Event>(&mut self, event: E)
    where
        for<'a> E::Trigger<'a>: Default,
    {
        self.app.world_mut().trigger(event);
        self.app.world_mut().flush();
    }
}

#[test]
fn gutter_builds_actual_oriented_feathers_bars_for_correct_targets() {
    let mut f = Fixture::new();
    let viewport = f.one::<TimelineHorizontalScrollViewport>();
    for orientation in [ControlOrientation::Horizontal, ControlOrientation::Vertical] {
        let bar = f.bar(orientation);
        let w = f.app.world();
        let b = w.get::<Scrollbar>(bar).unwrap();
        assert_eq!(
            b.target,
            if orientation == ControlOrientation::Horizontal {
                viewport
            } else {
                w.resource::<Panes>().tracks
            }
        );
        let n = w.get::<Node>(bar).unwrap();
        assert_eq!(
            if orientation == ControlOrientation::Horizontal {
                n.height
            } else {
                n.width
            },
            Val::Px(10.0)
        );
        assert!(w.get::<Children>(bar).is_some());
    }
    f.clean();
}

#[test]
fn gutter_geometry_and_scroll_content_ratio_preserve_original_contract() {
    let mut f = Fixture::new();
    let h = f.one::<TimelineHorizontalScrollbarGutter>();
    let v = f.one::<TimelineVerticalScrollbarGutter>();
    assert_eq!(
        (f.node(h).height, f.node(h).padding.top),
        (Val::Px(15.0), Val::Px(4.0))
    );
    assert_eq!(
        (
            f.node(v).width,
            f.node(v).margin.top,
            f.node(v).margin.bottom
        ),
        (Val::Px(15.0), Val::Px(25.0), Val::Px(15.0))
    );
    let content = f.one::<TimelineHorizontalScrollContent>();
    assert_eq!(f.node(content).width, Val::Percent(200.0));
    f.clean();
}

#[test]
fn horizontal_view_changes_move_constructed_scroll_position_without_document_edits() {
    let mut f = Fixture::configured(2.0, 6.0);
    let viewport = f.horizontal(800.0);
    assert_eq!(
        f.app.world().get::<ScrollPosition>(viewport).unwrap().x,
        200.0
    );
    f.app.world_mut().resource_mut::<TimelineState>().view = TimelineView {
        start: 4.0,
        end: 8.0,
    };
    f.app.update();
    assert_eq!(
        f.app.world().get::<ScrollPosition>(viewport).unwrap().x,
        400.0
    );
    f.clean();
}

#[test]
fn constructed_horizontal_scroll_position_drives_view_and_clamps_at_end() {
    let mut f = Fixture::new();
    let viewport = f.horizontal(800.0);
    f.app
        .world_mut()
        .get_mut::<ScrollPosition>(viewport)
        .unwrap()
        .0 = Vec2::new(100.0, 7.0);
    f.app.update();
    let state = f.app.world().resource::<TimelineState>();
    assert_eq!((state.view.start, state.view.end), (1.0, 5.0));
    assert_eq!(
        f.app.world().get::<ScrollPosition>(viewport).unwrap().y,
        0.0
    );
    f.app
        .world_mut()
        .get_mut::<ScrollPosition>(viewport)
        .unwrap()
        .x = 900.0;
    f.app.update();
    let state = f.app.world().resource::<TimelineState>();
    assert_eq!((state.view.start, state.view.end), (4.0, 8.0));
    f.clean();
}

#[test]
fn no_horizontal_overflow_hides_gutters_and_resets_scroll_position() {
    let mut f = Fixture::new();
    let viewport = f.horizontal(800.0);
    f.app.world_mut().resource_mut::<TimelineState>().view = TimelineView {
        start: 0.0,
        end: 8.0,
    };
    f.app.update();
    let h = f.one::<TimelineHorizontalScrollbarGutter>();
    let peer = f.one::<TimelineHorizontalGutter>();
    assert_eq!(f.node(h).display, Display::None);
    assert_eq!(f.node(peer).display, Display::None);
    let c = f.one::<TimelineHorizontalScrollContent>();
    assert_eq!(f.node(c).width, Val::Percent(100.0));
    assert_eq!(
        f.app.world().get::<ScrollPosition>(viewport).unwrap().0,
        Vec2::ZERO
    );
    f.clean();
}

#[test]
fn vertical_header_and_tracks_scroll_together_with_tracks_winning_same_frame() {
    let mut f = Fixture::new();
    let p = f.app.world().resource::<Panes>();
    let (h, t) = (p.headers, p.tracks);
    f.app.world_mut().get_mut::<ScrollPosition>(h).unwrap().y = 40.0;
    f.app.update();
    assert_eq!(f.app.world().get::<ScrollPosition>(t).unwrap().y, 40.0);
    f.app.world_mut().get_mut::<ScrollPosition>(h).unwrap().y = 60.0;
    f.app.world_mut().get_mut::<ScrollPosition>(t).unwrap().0 = Vec2::new(3.0, 90.0);
    f.app.update();
    assert_eq!(
        f.app.world().resource::<TimelineState>().vertical_scroll,
        90.0
    );
    assert_eq!(
        f.app.world().get::<ScrollPosition>(h).unwrap().0,
        Vec2::new(0.0, 90.0)
    );
    f.clean();
}

#[test]
fn shrinking_vertical_content_clamps_both_panes_and_hides_gutter() {
    let mut f = Fixture::new();
    let p = f.app.world().resource::<Panes>();
    let (h, t) = (p.headers, p.tracks);
    f.app.world_mut().get_mut::<ScrollPosition>(t).unwrap().y = 200.0;
    f.app.update();
    for e in [h, t] {
        f.app
            .world_mut()
            .get_mut::<ComputedNode>(e)
            .unwrap()
            .content_size
            .y = 100.0;
    }
    f.app.update();
    assert_eq!(
        f.app.world().resource::<TimelineState>().vertical_scroll,
        0.0
    );
    for e in [h, t] {
        assert_eq!(f.app.world().get::<ScrollPosition>(e).unwrap().y, 0.0);
    }
    let gutter = f.one::<TimelineVerticalScrollbarGutter>();
    assert_eq!(f.node(gutter).display, Display::None);
    f.clean();
}

#[test]
fn actual_scrollbar_visibility_respects_half_pixel_overflow_threshold() {
    let mut f = Fixture::new();
    for (content, expected) in [(400.5, Display::None), (400.6, Display::Flex)] {
        f.horizontal(content);
        let bar = f.bar(ControlOrientation::Horizontal);
        assert_eq!(f.node(bar).display, expected);
    }
    f.clean();
}

#[test]
fn horizontal_gutter_and_bar_attached_observers_isolate_press_and_drag() {
    let mut f = Fixture::new();
    let gutter = f.one::<TimelineHorizontalScrollbarGutter>();
    let bar = f.bar(ControlOrientation::Horizontal);
    for e in [gutter, bar] {
        f.event(pointer::press(
            e,
            f.window,
            Vec2::ZERO,
            PointerId::Mouse,
            PointerButton::Primary,
        ));
        f.event(pointer::motion_delta(e, f.window, Vec2::new(20.0, 0.0)));
    }
    let input = f.app.world().resource::<ParentInput>();
    assert_eq!((input.presses, input.drags), (0, 0));
    f.clean();
}

#[test]
fn scroll_viewport_and_content_remain_non_pickable_with_no_duplicate_controls() {
    let mut f = Fixture::new();
    let viewport = f.one::<TimelineHorizontalScrollViewport>();
    let content = f.one::<TimelineHorizontalScrollContent>();
    for _ in 0..3 {
        f.app.update();
    }
    assert_eq!(f.one::<TimelineHorizontalScrollViewport>(), viewport);
    assert_eq!(f.one::<TimelineHorizontalScrollContent>(), content);
    let w = f.app.world_mut();
    assert_eq!(w.query::<&Scrollbar>().iter(w).count(), 2);
    assert!(w.get::<Pickable>(viewport).is_some());
    assert!(w.get::<Pickable>(content).is_some());
    f.clean();
}
