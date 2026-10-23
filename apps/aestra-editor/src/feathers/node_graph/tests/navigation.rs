use super::*;

struct Fixture {
    app: App,
    window: Entity,
    viewport: Entity,
}

fn fixture(ui_scale: f32, display_scale: f32, zoom: f32) -> Fixture {
    let mut app = App::new();
    app.add_message::<CursorMoved>()
        .add_message::<MouseWheel>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<GraphPanGesture>()
        .init_resource::<OverrideCursor>()
        .insert_resource(UiScale(ui_scale))
        .add_systems(
            Update,
            (navigate_graph_viewports, sync_graph_viewport_transforms).chain(),
        );
    let mut window = Window::default();
    window
        .resolution
        .set_scale_factor_override(Some(display_scale));
    let window = app.world_mut().spawn((window, PrimaryWindow)).id();
    let viewport = app
        .world_mut()
        .spawn((
            FeathersGraphViewport {
                key: "material/view-a".into(),
                pan: Vec2::new(60.0, -30.0),
                zoom,
                content_size: Vec2::new(960.0, 640.0),
                selection_bounds: None,
                frame_request: Some(GraphFrameTarget::All),
                measured_frame: None,
            },
            ComputedNode {
                size: Vec2::new(800.0, 600.0) * ui_scale * display_scale,
                inverse_scale_factor: 1.0 / (ui_scale * display_scale),
                ..default()
            },
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::new(-0.2, 0.1)),
            },
        ))
        .id();
    Fixture {
        app,
        window,
        viewport,
    }
}

impl Fixture {
    fn cursor(&mut self, position: Vec2) {
        self.app.world_mut().write_message(CursorMoved {
            window: self.window,
            position,
            delta: None,
        });
    }
    fn wheel(&mut self, window: Entity, unit: MouseScrollUnit, y: f32) {
        self.app.world_mut().write_message(MouseWheel {
            window,
            unit,
            x: 0.0,
            y,
            phase: bevy::input::touch::TouchPhase::Moved,
        });
    }
    fn view(&self) -> &FeathersGraphViewport {
        self.app
            .world()
            .get::<FeathersGraphViewport>(self.viewport)
            .unwrap()
    }
    fn press(&mut self, button: MouseButton) {
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(button);
    }
    fn tick(&mut self) {
        self.app.update();
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
    }
}

fn close(actual: Vec2, expected: Vec2) {
    assert!(
        actual.distance(expected) < 0.001,
        "{actual:?} != {expected:?}"
    );
}

#[test]
fn pan_converts_ui_units_once_and_keeps_final_release_motion_outside_viewport() {
    for ui_scale in [1.0, 1.5, 2.0] {
        for display_scale in [1.0, 1.5, 2.0] {
            for zoom in [MIN_ZOOM, 0.5, MAX_ZOOM] {
                let mut f = fixture(ui_scale, display_scale, zoom);
                let before = f.view().pan;
                f.press(MouseButton::Middle);
                f.cursor(Vec2::new(100.0, 100.0));
                f.tick();
                close(f.view().pan, before);
                f.app
                    .world_mut()
                    .get_mut::<RelativeCursorPosition>(f.viewport)
                    .unwrap()
                    .cursor_over = false;
                f.cursor(Vec2::new(140.0, 120.0));
                f.tick();
                close(f.view().pan, before + Vec2::new(40.0, 20.0) / ui_scale);
                assert_eq!(
                    f.app.world().resource::<OverrideCursor>().0,
                    Some(EntityCursor::System(SystemCursorIcon::Grabbing))
                );
                f.app
                    .world_mut()
                    .resource_mut::<ButtonInput<MouseButton>>()
                    .release(MouseButton::Middle);
                f.cursor(Vec2::new(160.0, 130.0));
                f.tick();
                close(f.view().pan, before + Vec2::new(60.0, 30.0) / ui_scale);
                assert_eq!(f.view().zoom, zoom);
                assert!(f.view().frame_request.is_none());
                assert!(
                    f.app
                        .world()
                        .resource::<GraphPanGesture>()
                        .viewport
                        .is_none()
                );
                assert!(f.app.world().resource::<OverrideCursor>().0.is_none());
            }
        }
    }
}

#[test]
fn only_middle_or_space_left_captures_navigation_and_space_release_ends_it() {
    for (button, space) in [
        (MouseButton::Left, false),
        (MouseButton::Right, false),
        (MouseButton::Right, true),
        (MouseButton::Left, true),
        (MouseButton::Middle, false),
    ] {
        let mut f = fixture(1.0, 1.0, 1.0);
        if space {
            f.app
                .world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Space);
        }
        f.press(button);
        f.cursor(Vec2::ZERO);
        f.tick();
        let expected = button == MouseButton::Middle || (space && button == MouseButton::Left);
        assert_eq!(
            f.app.world().resource::<GraphPanGesture>().viewport,
            expected.then_some(f.viewport)
        );
        f.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::Space);
        if button != MouseButton::Left {
            f.app
                .world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .release(button);
        }
        f.tick();
        assert!(
            f.app
                .world()
                .resource::<GraphPanGesture>()
                .viewport
                .is_none()
        );
        assert!(f.app.world().resource::<OverrideCursor>().0.is_none());
    }
}

#[test]
fn cursor_centered_wheel_zoom_matches_line_and_pixel_events_at_all_scales() {
    for ui_scale in [1.0, 1.5, 2.0] {
        for display_scale in [1.0, 1.5, 2.0] {
            for (unit, delta) in [(MouseScrollUnit::Line, 1.0), (MouseScrollUnit::Pixel, 50.0)] {
                let mut f = fixture(ui_scale, display_scale, 0.7);
                let cursor = Vec2::new(240.0, 360.0);
                let anchor = f.view().unproject_viewport_point(cursor);
                f.wheel(f.window, unit, delta);
                f.tick();
                close(f.view().project_graph_point(anchor), cursor);
                assert!((f.view().zoom - 0.7 * 0.12_f32.exp()).abs() < 0.001);
                assert!(f.view().frame_request.is_none());
                let after = f.view().zoom;
                f.tick();
                assert_eq!(f.view().zoom, after, "wheel is consumed only once");
            }
        }
    }
}

#[test]
fn other_window_wheel_and_cursor_do_not_drive_the_primary_graph() {
    let mut f = fixture(1.0, 1.0, 0.7);
    let other = f.app.world_mut().spawn(Window::default()).id();
    let before = f.view().pan;
    f.wheel(other, MouseScrollUnit::Line, 5.0);
    f.app.world_mut().write_message(CursorMoved {
        window: other,
        position: Vec2::splat(400.0),
        delta: None,
    });
    f.tick();
    assert_eq!(f.view().zoom, 0.7);
    close(f.view().pan, before);
}

#[test]
fn hovered_overlay_blocks_wheel_pan_and_frame_shortcuts() {
    let mut f = fixture(1.0, 1.0, 0.7);
    let before = f.view().pan;
    f.app.world_mut().spawn((
        FeathersGraphNavigationBlocker,
        RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::ZERO),
        },
    ));
    f.press(MouseButton::Middle);
    f.cursor(Vec2::ZERO);
    f.wheel(f.window, MouseScrollUnit::Line, 5.0);
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyF);
    f.tick();
    assert!(
        f.app
            .world()
            .resource::<GraphPanGesture>()
            .viewport
            .is_none()
    );
    close(f.view().pan, before);
    assert_eq!(f.view().zoom, 0.7);
    assert_eq!(f.view().frame_request, Some(GraphFrameTarget::All));
}

#[test]
fn vanished_pan_owner_releases_cursor_and_does_not_capture_another_view() {
    let mut f = fixture(1.0, 1.0, 0.7);
    f.press(MouseButton::Middle);
    f.cursor(Vec2::ZERO);
    f.tick();
    assert_eq!(
        f.app.world().resource::<GraphPanGesture>().viewport,
        Some(f.viewport)
    );
    let view = f.view().clone();
    let other = f
        .app
        .world_mut()
        .spawn((
            view,
            ComputedNode::default(),
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::ZERO),
            },
        ))
        .id();
    f.app.world_mut().despawn(f.viewport);
    f.cursor(Vec2::splat(60.0));
    f.tick();
    assert!(
        f.app
            .world()
            .resource::<GraphPanGesture>()
            .viewport
            .is_none()
    );
    assert!(f.app.world().resource::<OverrideCursor>().0.is_none());
    assert_eq!(
        f.app
            .world()
            .get::<FeathersGraphViewport>(other)
            .unwrap()
            .pan,
        Vec2::new(60.0, -30.0)
    );
}

#[test]
fn frame_shortcuts_are_hover_gated_and_do_not_change_placement() {
    let mut f = fixture(1.0, 1.0, 0.7);
    let before = f.view().pan;
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyF);
    f.tick();
    assert_eq!(f.view().frame_request, Some(GraphFrameTarget::Selection));
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Home);
    f.app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(f.viewport)
        .unwrap()
        .cursor_over = false;
    f.tick();
    assert_eq!(f.view().frame_request, Some(GraphFrameTarget::Selection));
    close(f.view().pan, before);
}

#[test]
fn staged_framing_is_atomic_for_owned_canvases_and_navigation_cancels_stale_results() {
    let mut f = fixture(1.0, 1.0, 0.7);
    let canvas = f
        .app
        .world_mut()
        .spawn((
            FeathersGraphCanvas {
                viewport: f.viewport,
            },
            UiTransform::IDENTITY,
        ))
        .id();
    let other = f
        .app
        .world_mut()
        .spawn((
            FeathersGraphCanvas {
                viewport: Entity::PLACEHOLDER,
            },
            UiTransform::IDENTITY,
        ))
        .id();
    let view = GraphView {
        pan: Vec2::new(5.0, 7.0),
        zoom: 0.5,
    };
    f.app
        .world_mut()
        .get_mut::<FeathersGraphViewport>(f.viewport)
        .unwrap()
        .measured_frame = Some((GraphFrameTarget::All, view));
    f.tick();
    close(f.view().pan, view.pan);
    assert_eq!(f.view().zoom, view.zoom);
    assert_eq!(
        *f.app.world().get::<UiTransform>(canvas).unwrap(),
        graph_canvas_transform(view.pan, view.zoom, f.view().content_size)
    );
    assert_eq!(
        *f.app.world().get::<UiTransform>(other).unwrap(),
        UiTransform::IDENTITY
    );
    {
        let mut viewport = f
            .app
            .world_mut()
            .get_mut::<FeathersGraphViewport>(f.viewport)
            .unwrap();
        viewport.frame_request = Some(GraphFrameTarget::All);
        viewport.measured_frame = Some((
            GraphFrameTarget::All,
            GraphView {
                pan: Vec2::splat(1000.0),
                zoom: 2.0,
            },
        ));
    }
    f.wheel(f.window, MouseScrollUnit::Line, 1.0);
    f.tick();
    assert_ne!(f.view().pan, Vec2::splat(1000.0));
    assert!(f.view().measured_frame.is_none());
}

#[test]
fn overview_zoom_clamps_without_losing_pointer_anchor() {
    let before = GraphView {
        pan: Vec2::new(30.0, 50.0),
        zoom: 1.0,
    };
    let cursor = Vec2::new(200.0, 120.0);
    for (scroll, zoom) in [(-100.0, MIN_ZOOM), (100.0, MAX_ZOOM)] {
        let after = zoomed_graph_view_at(before, cursor, scroll);
        assert_eq!(after.zoom, zoom);
        close(
            (cursor - after.pan) / after.zoom,
            (cursor - before.pan) / before.zoom,
        );
    }
}

#[test]
fn framing_and_canvas_transform_keep_unzoomed_graph_coordinates() {
    let bounds = Rect::from_corners(Vec2::new(-100.0, -60.0), Vec2::new(900.0, 500.0));
    let viewport_size = Vec2::new(800.0, 600.0);
    let view = framed_graph_view(bounds, viewport_size, 1.0);
    close(view.pan + bounds.center() * view.zoom, viewport_size * 0.5);
    for point in [bounds.min, bounds.max] {
        let projected = view.pan + point * view.zoom;
        assert!(projected.min_element() >= FRAME_PADDING - 0.001);
        assert!(projected.x <= viewport_size.x - FRAME_PADDING + 0.001);
        assert!(projected.y <= viewport_size.y - FRAME_PADDING + 0.001);
    }
    let transform = graph_canvas_transform(view.pan, view.zoom, bounds.size());
    assert_eq!(transform.scale, Vec2::splat(view.zoom));
    let expected = view.pan + bounds.size() * (view.zoom - 1.0) * 0.5;
    assert_eq!(transform.translation, Val2::px(expected.x, expected.y));
}
