//! Production systems with explicit computed geometry; not native layout/picking acceptance.
use super::super::projections::{
    ReferencedTimingContext, ReferencedTrackKind, map_referenced_interval,
};
use super::*;
use aestra_core::{
    ChoreographyEvent, ChoreographyEventPayload, EffectClip, EffectClipId, EffectMarker, Emitter,
    EmitterId, EmitterRegionId, MarkerId, ModuleParameters,
};

struct Fixture {
    _root: tempfile::TempDir,
    app: App,
    emitter: EmitterId,
    region: EmitterRegionId,
    clip: EffectClipId,
    before: EffectAsset,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut source = EffectAsset::new("Source", 4.0);
        source.emitters.push(Emitter::basic_sprite("Child", 4.0));
        source
            .save_ron(root.path().join("source.aestra.ron"))
            .unwrap();
        let mut effect = EffectAsset::new("Parent", 8.0);
        let mut emitter = Emitter::basic_sprite("Emitter", 4.0);
        emitter.start_time = 1.0;
        let emitter_id = emitter.id;
        let region = emitter.implicit_region_id();
        effect.emitters.push(emitter);
        let clip = EffectClip::new(source.id, 1.0, 3.0);
        let clip_id = clip.id;
        effect.effect_clips.push(clip);
        let mut app = App::new();
        app.insert_resource(EditorSession::from_test_effect(effect.clone()))
            .insert_resource(TimelineState::framed(8.0))
            .insert_resource(ProjectEffectCatalog::scan(root.path()))
            .init_resource::<InputFocus>()
            .init_resource::<InputFocusVisible>()
            .add_systems(
                Update,
                (
                    update_timeline_visuals,
                    update_timeline_marker_visuals,
                    update_choreography_event_visuals,
                    update_effect_clip_visuals,
                    update_effect_drop_insertion,
                    sync_effect_drop_track_gap,
                    update_effect_drop_preview,
                    sync_timeline_horizontal_scroll,
                    sync_timeline_vertical_scroll,
                    reveal_timeline_emitter,
                    update_timeline_time_label,
                    sync_effect_clip_reorder_hints,
                    sync_emitter_reorder_hints,
                    sync_timeline_track_drop_hints,
                )
                    .chain(),
            );
        Self {
            _root: root,
            app,
            emitter: emitter_id,
            region,
            clip: clip_id,
            before: effect,
        }
    }
    fn tick(&mut self) {
        self.app.update();
        assert_eq!(
            self.app.world().resource::<EditorSession>().effect,
            self.before,
            "Presentation must not edit the document"
        );
    }
    fn state(&mut self) -> Mut<'_, TimelineState> {
        self.app.world_mut().resource_mut()
    }
    fn node(&self, entity: Entity) -> &Node {
        self.app.world().get(entity).unwrap()
    }
    fn canvas(&mut self, x: f32) -> Entity {
        self.app
            .world_mut()
            .spawn((
                TimelineCanvas,
                RelativeCursorPosition {
                    cursor_over: true,
                    normalized: Some(Vec2::new(x, 0.0)),
                },
                ComputedNode {
                    size: Vec2::new(400.0, 100.0),
                    ..default()
                },
            ))
            .id()
    }
    fn preview(&mut self) {
        self.state().effect_drop_preview = Some(EffectDropPreview {
            source_duration: 2.0,
            display_name: "Dragged".into(),
        });
    }
    fn pane(&mut self, kind: TimelineVerticalPane, height: f32, content: f32) -> Entity {
        self.app
            .world_mut()
            .spawn((
                kind,
                ComputedNode {
                    size: Vec2::new(400.0, height),
                    content_size: Vec2::new(400.0, content),
                    ..default()
                },
                ScrollPosition::default(),
            ))
            .id()
    }
}
fn approx(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 0.001, "{actual} != {expected}");
}
fn percent(value: Val, expected: f32) {
    let Val::Percent(actual) = value else {
        panic!("Expected percent, got {value:?}")
    };
    approx(actual, expected);
}
fn cursor(y: f32) -> RelativeCursorPosition {
    RelativeCursorPosition {
        cursor_over: true,
        normalized: Some(Vec2::new(0.0, y)),
    }
}

#[test]
fn bars_clip_to_view_and_hide_zero_overlap() {
    let view = TimelineView {
        start: 2.0,
        end: 6.0,
    };
    let mut node = Node::default();
    apply_timeline_bar_geometry(&mut node, 1.0, 4.0, view);
    assert_eq!(node.display, Display::Flex);
    percent(node.left, 0.0);
    percent(node.width, 75.0);
    for (start, duration) in [(0.0, 2.0), (6.0, 1.0), (3.0, 0.0), (3.0, -1.0)] {
        apply_timeline_bar_geometry(&mut node, start, duration, view);
        assert_eq!(node.display, Display::None);
    }
    assert!(timeline_boundary_is_visible(2.0, view));
    assert!(timeline_boundary_is_visible(6.0, view));
    assert!(!timeline_boundary_is_visible(6.01, view));
}

#[test]
fn region_bar_uses_live_drag_without_editing_document() {
    let mut f = Fixture::new();
    let entity = f
        .app
        .world_mut()
        .spawn((
            TimelineClip {
                emitter: f.emitter,
                region: f.region,
            },
            Node::default(),
        ))
        .id();
    f.state().drag = Some(TimelineDrag {
        emitter: f.emitter,
        region: f.region,
        kind: TimelineDragKind::Move,
        pointer_start: 0.0,
        original_start: 1.0,
        original_duration: 4.0,
        original_source_offset: 0.0,
        current_start: 2.0,
        current_duration: 3.0,
        current_source_offset: 0.0,
        source_duration: 4.0,
    });
    f.tick();
    percent(f.node(entity).left, 25.0);
    percent(f.node(entity).width, 37.5);
}

#[test]
fn stale_regions_and_trim_handles_hide() {
    let mut f = Fixture::new();
    let stale = f
        .app
        .world_mut()
        .spawn((
            TimelineClip {
                emitter: f.emitter,
                region: EmitterRegionId::new(),
            },
            Node::default(),
        ))
        .id();
    let missing = f
        .app
        .world_mut()
        .spawn((
            TimelineClipInteraction {
                emitter: EmitterId::new(),
                region: f.region,
                kind: TimelineDragKind::Move,
            },
            Node::default(),
        ))
        .id();
    f.state().view = TimelineView {
        start: 2.0,
        end: 4.0,
    };
    let handles: Vec<_> = [
        TimelineDragKind::Move,
        TimelineDragKind::TrimStart,
        TimelineDragKind::TrimEnd,
    ]
    .into_iter()
    .map(|kind| {
        f.app
            .world_mut()
            .spawn((
                TimelineClipInteraction {
                    emitter: f.emitter,
                    region: f.region,
                    kind,
                },
                Node::default(),
            ))
            .id()
    })
    .collect();
    f.tick();
    assert_eq!(f.node(stale).display, Display::None);
    assert_eq!(f.node(missing).display, Display::None);
    assert_eq!(f.node(handles[0]).display, Display::Flex);
    assert_eq!(f.node(handles[1]).display, Display::None);
    assert_eq!(f.node(handles[2]).display, Display::None);
}

#[test]
fn playhead_and_guide_hide_outside_view_and_restore() {
    let mut f = Fixture::new();
    let play = f.app.world_mut().spawn((Playhead, Node::default())).id();
    let guide = f
        .app
        .world_mut()
        .spawn((TimelineSnapGuide, Node::default()))
        .id();
    f.state().view = TimelineView {
        start: 2.0,
        end: 6.0,
    };
    f.state().snap_guide = Some(7.0);
    f.tick();
    assert_eq!(f.node(play).display, Display::None);
    assert_eq!(f.node(guide).display, Display::None);
    f.state().view = TimelineView {
        start: 0.0,
        end: 8.0,
    };
    f.state().snap_guide = Some(4.0);
    f.tick();
    assert_eq!(f.node(play).display, Display::Flex);
    percent(f.node(guide).left, 50.0);
    f.state().snap_guide = None;
    f.tick();
    assert_eq!(f.node(guide).display, Display::None);
}

#[test]
fn ruler_uses_largest_canvas_and_formats_tick_precision() {
    let mut f = Fixture::new();
    f.canvas(0.0);
    f.app.world_mut().spawn((
        TimelineCanvas,
        ComputedNode {
            size: Vec2::new(960.0, 100.0),
            ..default()
        },
    ));
    let label = f.app.world_mut().spawn(Text::new("old")).id();
    let tick = f
        .app
        .world_mut()
        .spawn((TimelineRulerTick(2), Node::default()))
        .add_child(label)
        .id();
    let hidden = f
        .app
        .world_mut()
        .spawn((TimelineRulerTick(99), Node::default()))
        .with_child(Text::new("old"))
        .id();
    f.tick();
    percent(f.node(tick).left, 25.0);
    assert_eq!(f.app.world().get::<Text>(label).unwrap().0, "2.0");
    assert_eq!(f.node(hidden).display, Display::None);
    assert_eq!(format_timeline_tick(1.2345, 0.1), "1.23");
    assert_eq!(format_timeline_tick(1.2345, 0.01), "1.235");
}

#[test]
fn marker_and_event_positions_use_preview_and_stale_ids_hide() {
    let mut f = Fixture::new();
    let marker = EffectMarker {
        id: MarkerId::new(),
        name: "Cue".into(),
        time: 1.0,
    };
    let event = ChoreographyEvent::new(
        "Sound",
        1.0,
        ChoreographyEventPayload::PlaySound {
            cue: "burst".into(),
        },
    );
    let mid = marker.id;
    let eid = event.id;
    {
        let mut session = f.app.world_mut().resource_mut::<EditorSession>();
        session.effect.markers.push(marker);
        session.effect.choreography_events.push(event);
    }
    f.before = f.app.world().resource::<EditorSession>().effect.clone();
    f.state().marker_drag = Some(TimelineMarkerDrag {
        marker: mid,
        original_time: 1.0,
        current_time: 4.0,
    });
    f.state().choreography_event_drag = Some(TimelineChoreographyEventDrag {
        event: eid,
        original_time: 1.0,
        current_time: 6.0,
    });
    let m = f
        .app
        .world_mut()
        .spawn((TimelineMarker { marker: mid }, Node::default()))
        .id();
    let e = f
        .app
        .world_mut()
        .spawn((TimelineChoreographyEvent { event: eid }, Node::default()))
        .id();
    let stale = f
        .app
        .world_mut()
        .spawn((
            TimelineMarker {
                marker: MarkerId::new(),
            },
            Node::default(),
        ))
        .id();
    let stale_e = f
        .app
        .world_mut()
        .spawn((
            TimelineChoreographyEvent {
                event: aestra_core::ChoreographyEventId::new(),
            },
            Node::default(),
        ))
        .id();
    f.tick();
    percent(f.node(m).left, 50.0);
    percent(f.node(e).left, 75.0);
    assert_eq!(f.node(stale).display, Display::None);
    assert_eq!(f.node(stale_e).display, Display::None);
    f.state().view = TimelineView {
        start: 0.0,
        end: 2.0,
    };
    f.tick();
    assert_eq!(f.node(m).display, Display::None);
    assert_eq!(f.node(e).display, Display::None);
}

#[test]
fn effect_clip_and_referenced_child_follow_live_trim() {
    let mut f = Fixture::new();
    let clip = f
        .app
        .world_mut()
        .spawn((TimelineEffectClip { clip: f.clip }, Node::default()))
        .id();
    let child = f
        .app
        .world_mut()
        .spawn((
            TimelineReferencedEmitter {
                clip: f.clip,
                source_start: 0.5,
                source_duration: 2.0,
            },
            Node::default(),
        ))
        .id();
    f.state().effect_clip_drag = Some(EffectClipTimelineDrag {
        clip: f.clip,
        kind: TimelineDragKind::TrimStart,
        pointer_start: 0.0,
        original_start: 1.0,
        original_source_offset: 0.0,
        original_duration: 3.0,
        current_start: 2.0,
        current_source_offset: 1.0,
        current_duration: 2.0,
        source_duration: 4.0,
        source_looping: false,
    });
    f.tick();
    percent(f.node(clip).left, 25.0);
    percent(f.node(clip).width, 25.0);
    percent(f.node(child).left, 25.0);
    percent(f.node(child).width, 18.75);
    f.state()
        .effect_clip_drag
        .as_mut()
        .unwrap()
        .current_source_offset = 3.0;
    f.tick();
    assert_eq!(f.node(child).display, Display::None);
}

#[test]
fn missing_effect_clip_bars_children_and_handles_hide() {
    let mut f = Fixture::new();
    let id = EffectClipId::new();
    let bar = f
        .app
        .world_mut()
        .spawn((TimelineEffectClip { clip: id }, Node::default()))
        .id();
    let child = f
        .app
        .world_mut()
        .spawn((
            TimelineReferencedEmitter {
                clip: id,
                source_start: 0.0,
                source_duration: 1.0,
            },
            Node::default(),
        ))
        .id();
    let handle = f
        .app
        .world_mut()
        .spawn((
            TimelineEffectClipInteraction {
                clip: id,
                kind: TimelineDragKind::TrimEnd,
            },
            Node::default(),
        ))
        .id();
    f.tick();
    for entity in [bar, child, handle] {
        assert_eq!(f.node(entity).display, Display::None);
    }
}

#[test]
fn clip_handles_hide_only_clipped_boundaries() {
    let mut f = Fixture::new();
    f.state().view = TimelineView {
        start: 2.0,
        end: 4.0,
    };
    let handles: Vec<_> = [
        TimelineDragKind::Move,
        TimelineDragKind::TrimStart,
        TimelineDragKind::TrimEnd,
    ]
    .into_iter()
    .map(|kind| {
        f.app
            .world_mut()
            .spawn((
                TimelineEffectClipInteraction { clip: f.clip, kind },
                Node::default(),
            ))
            .id()
    })
    .collect();
    f.tick();
    assert_eq!(f.node(handles[0]).display, Display::Flex);
    assert_eq!(f.node(handles[1]).display, Display::None);
    assert_eq!(f.node(handles[2]).display, Display::Flex);
}

#[test]
fn horizontal_scroll_reads_user_change_and_writes_view_change() {
    let mut f = Fixture::new();
    f.state().view = TimelineView {
        start: 2.0,
        end: 6.0,
    };
    let pane = f
        .app
        .world_mut()
        .spawn((
            TimelineHorizontalScrollViewport,
            ComputedNode {
                size: Vec2::new(400.0, 100.0),
                content_size: Vec2::new(800.0, 100.0),
                ..default()
            },
            ScrollPosition::default(),
        ))
        .id();
    let content = f
        .app
        .world_mut()
        .spawn((TimelineHorizontalScrollContent, Node::default()))
        .id();
    let gutter = f
        .app
        .world_mut()
        .spawn((TimelineHorizontalGutter, Node::default()))
        .id();
    f.tick();
    percent(f.node(content).width, 200.0);
    assert_eq!(f.node(gutter).display, Display::Flex);
    approx(f.app.world().get::<ScrollPosition>(pane).unwrap().x, 200.0);
    f.app.world_mut().get_mut::<ScrollPosition>(pane).unwrap().0 = Vec2::new(100.0, 2.0);
    f.tick();
    approx(f.state().view.start, 1.0);
    approx(f.state().view.end, 5.0);
    f.tick(); // No new external scroll write on the following tick.
    f.state().view = TimelineView {
        start: 3.0,
        end: 7.0,
    };
    f.tick();
    approx(f.app.world().get::<ScrollPosition>(pane).unwrap().x, 300.0);
    approx(f.app.world().get::<ScrollPosition>(pane).unwrap().y, 0.0);
}

#[test]
fn horizontal_full_view_hides_both_gutters_and_resets_position() {
    let mut f = Fixture::new();
    let pane = f
        .app
        .world_mut()
        .spawn((
            TimelineHorizontalScrollViewport,
            ComputedNode::default(),
            ScrollPosition(Vec2::splat(5.0)),
        ))
        .id();
    let a = f
        .app
        .world_mut()
        .spawn((TimelineHorizontalGutter, Node::default()))
        .id();
    let b = f
        .app
        .world_mut()
        .spawn((TimelineHorizontalScrollbarGutter, Node::default()))
        .id();
    f.tick();
    assert_eq!(f.node(a).display, Display::None);
    assert_eq!(f.node(b).display, Display::None);
    assert_eq!(
        f.app.world().get::<ScrollPosition>(pane).unwrap().0,
        Vec2::ZERO
    );
}

#[test]
fn ambiguous_horizontal_viewports_do_not_pick_arbitrary_scroll_input() {
    let mut f = Fixture::new();
    f.state().view = TimelineView {
        start: 2.0,
        end: 6.0,
    };
    for x in [0.0, 200.0] {
        f.app.world_mut().spawn((
            TimelineHorizontalScrollViewport,
            ComputedNode {
                size: Vec2::splat(100.0),
                content_size: Vec2::splat(400.0),
                ..default()
            },
            ScrollPosition(Vec2::new(x, 0.0)),
        ));
    }
    f.tick();
    approx(f.state().view.start, 2.0);
}

#[test]
fn vertical_scroll_tracks_take_priority_and_clamp_both_panes() {
    let mut f = Fixture::new();
    let header = f.pane(TimelineVerticalPane::Headers, 100.0, 500.0);
    let tracks = f.pane(TimelineVerticalPane::Tracks, 100.0, 400.0);
    let gutter = f
        .app
        .world_mut()
        .spawn((TimelineVerticalScrollbarGutter, Node::default()))
        .id();
    f.tick();
    f.app
        .world_mut()
        .get_mut::<ScrollPosition>(header)
        .unwrap()
        .0 = Vec2::new(4.0, 50.0);
    f.app
        .world_mut()
        .get_mut::<ScrollPosition>(tracks)
        .unwrap()
        .0 = Vec2::new(2.0, 999.0);
    f.tick();
    approx(f.state().vertical_scroll, 300.0);
    assert_eq!(f.node(gutter).display, Display::Flex);
    for entity in [header, tracks] {
        assert_eq!(
            f.app.world().get::<ScrollPosition>(entity).unwrap().0,
            Vec2::new(0.0, 300.0)
        );
    }
}

#[test]
fn vertical_scroll_uses_headers_when_tracks_unmeasured() {
    let mut f = Fixture::new();
    let header = f.pane(TimelineVerticalPane::Headers, 100.0, 250.0);
    f.pane(TimelineVerticalPane::Tracks, 0.0, 500.0);
    f.tick();
    f.app
        .world_mut()
        .get_mut::<ScrollPosition>(header)
        .unwrap()
        .0
        .y = 80.0;
    f.tick();
    approx(f.state().vertical_scroll, 80.0);
}

#[test]
fn vertical_scroll_reclamps_after_content_shrinks() {
    let mut f = Fixture::new();
    let tracks = f.pane(TimelineVerticalPane::Tracks, 100.0, 400.0);
    let gutter = f
        .app
        .world_mut()
        .spawn((TimelineVerticalScrollbarGutter, Node::default()))
        .id();
    f.state().vertical_scroll = 250.0;
    f.tick();
    f.app
        .world_mut()
        .get_mut::<ComputedNode>(tracks)
        .unwrap()
        .content_size
        .y = 80.0;
    f.tick();
    approx(f.state().vertical_scroll, 0.0);
    assert_eq!(f.node(gutter).display, Display::None);
}

#[test]
fn reveal_waits_for_row_and_focuses_headers_once() {
    let mut f = Fixture::new();
    let emitter = f.emitter;
    f.state().reveal_emitter(emitter);
    f.tick();
    f.tick();
    assert_eq!(f.state().reveal_emitter, Some(emitter));
    let list = f
        .app
        .world_mut()
        .spawn((TimelineVerticalPane::Headers, ListBox))
        .id();
    let row = f
        .app
        .world_mut()
        .spawn((EmitterTrackHeader { emitter: f.emitter }, ChildOf(list)))
        .id();
    f.tick();
    assert_eq!(
        f.app.world().get::<ActiveDescendant>(list).unwrap().0,
        Some(row)
    );
    assert_eq!(f.app.world().resource::<InputFocus>().get(), Some(list));
    assert!(!f.app.world().resource::<InputFocusVisible>().0);
    assert_eq!(f.state().reveal_emitter, None);
}

#[test]
fn time_label_tracks_session_transport_not_document_edits() {
    let mut f = Fixture::new();
    let label = f.app.world_mut().spawn((TimeLabel, Text::new("old"))).id();
    f.tick();
    assert!(
        f.app
            .world()
            .get::<Text>(label)
            .unwrap()
            .0
            .starts_with("F00000")
    );
    f.app.world_mut().get_mut::<Text>(label).unwrap().0 = "unchanged".into();
    f.tick();
    assert_eq!(f.app.world().get::<Text>(label).unwrap().0, "unchanged");
}

#[test]
fn insertion_prefers_hovered_header_then_track_row() {
    let mut f = Fixture::new();
    let emitter = f.emitter;
    let clip = f.clip;
    f.preview();
    let header = f
        .app
        .world_mut()
        .spawn((
            TimelineTrackHeader {
                track: ChoreographyTrackId::Emitter(f.emitter),
            },
            cursor(-0.25),
        ))
        .id();
    f.app.world_mut().spawn((
        TimelineTrackDropRow {
            track: ChoreographyTrackId::EffectClip(f.clip),
        },
        Node::default(),
        BorderColor::default(),
        cursor(0.25),
    ));
    f.tick();
    assert_eq!(
        f.state().effect_drop_insertion,
        Some((ChoreographyTrackId::Emitter(emitter), true))
    );
    f.app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(header)
        .unwrap()
        .cursor_over = false;
    f.tick();
    assert_eq!(
        f.state().effect_drop_insertion,
        Some((ChoreographyTrackId::EffectClip(clip), false))
    );
}

#[test]
fn hovering_gap_keeps_insertion_then_leaving_clears_it() {
    let mut f = Fixture::new();
    f.preview();
    f.state().effect_drop_insertion = Some((ChoreographyTrackId::Emitter(f.emitter), false));
    let spacer = f
        .app
        .world_mut()
        .spawn((TimelineEffectDropSpacer, Node::default(), cursor(0.0)))
        .id();
    f.tick();
    assert!(f.state().effect_drop_insertion.is_some());
    f.app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(spacer)
        .unwrap()
        .cursor_over = false;
    f.tick();
    assert_eq!(f.state().effect_drop_insertion, None);
}

#[test]
fn cancelled_preview_resets_gap_grid_rows() {
    let mut f = Fixture::new();
    f.preview();
    let row = f
        .app
        .world_mut()
        .spawn((TimelineChoreographyGridRow(3), Node::default()))
        .id();
    let spacer = f
        .app
        .world_mut()
        .spawn((TimelineEffectDropSpacer, Node::default()))
        .id();
    f.tick();
    assert_eq!(f.node(row).grid_row, GridPlacement::start(4));
    assert_eq!(f.node(spacer).display, Display::Flex);
    f.state().effect_drop_preview = None;
    f.tick();
    assert_eq!(f.node(row).grid_row, GridPlacement::start(3));
    assert_eq!(f.node(spacer).display, Display::None);
}

#[test]
fn drop_preview_uses_placement_label_and_scroll_clamp() {
    let mut f = Fixture::new();
    f.preview();
    f.canvas(-0.25);
    let node = f
        .app
        .world_mut()
        .spawn((TimelineEffectDropPreview, Node::default()))
        .id();
    let label = f
        .app
        .world_mut()
        .spawn((TimelineEffectDropPreviewLabel, Text::new("old")))
        .id();
    f.state().vertical_scroll = 500.0;
    f.tick();
    percent(f.node(node).left, 25.0);
    percent(f.node(node).width, 25.0);
    assert_eq!(f.node(node).top, Val::Px(29.0));
    assert_eq!(f.app.world().get::<Text>(label).unwrap().0, "Dragged");
    f.state().effect_drop_preview = None;
    f.tick();
    assert_eq!(f.node(node).display, Display::None);
}

#[test]
fn drop_preview_rejects_ambiguous_canvas_cursor() {
    let mut f = Fixture::new();
    f.preview();
    f.canvas(0.0);
    f.canvas(0.25);
    let node = f
        .app
        .world_mut()
        .spawn((TimelineEffectDropPreview, Node::default()))
        .id();
    f.tick();
    assert_eq!(f.node(node).display, Display::None);
}

#[test]
fn reorder_hints_highlight_target_not_source_and_clear() {
    let mut f = Fixture::new();
    let emitter = f
        .app
        .world_mut()
        .spawn((
            EmitterTrackHeader { emitter: f.emitter },
            cursor(-0.25),
            Node::default(),
            BorderColor::default(),
        ))
        .id();
    let clip = f
        .app
        .world_mut()
        .spawn((
            EffectClipTrackHeader { clip: f.clip },
            cursor(0.25),
            Node::default(),
            BorderColor::default(),
        ))
        .id();
    f.state().reorder_drag = Some(f.emitter);
    f.tick();
    assert_eq!(f.node(emitter).border.top, Val::Px(0.0));
    assert_eq!(f.node(clip).border.bottom, Val::Px(3.0));
    assert_eq!(
        f.app.world().get::<BorderColor>(clip).unwrap().bottom,
        theme::DOCK_TARGET
    );
    f.state().reorder_drag = None;
    f.state().effect_clip_reorder_drag = Some(f.clip);
    f.tick();
    assert_eq!(f.node(emitter).border.top, Val::Px(3.0));
    assert_eq!(f.node(clip).border.bottom, Val::Px(1.0));
    f.preview();
    f.tick();
    assert_eq!(f.node(emitter).border.top, Val::Px(0.0));
}

#[test]
fn drop_track_rows_restore_neutral_border() {
    let mut f = Fixture::new();
    let row = f
        .app
        .world_mut()
        .spawn((
            TimelineTrackDropRow {
                track: ChoreographyTrackId::Emitter(f.emitter),
            },
            Node {
                border: UiRect::all(Val::Px(3.0)),
                ..default()
            },
            BorderColor::all(theme::DOCK_TARGET),
        ))
        .id();
    f.tick();
    assert_eq!(f.node(row).border.top, Val::Px(0.0));
    assert_eq!(f.node(row).border.bottom, Val::Px(1.0));
    assert_eq!(
        f.app.world().get::<BorderColor>(row).unwrap().bottom,
        theme::BORDER.with_alpha(0.45)
    );
}

#[test]
fn nested_projection_maps_trimmed_intervals_and_preserves_row_metadata() {
    let root = tempfile::tempdir().unwrap();
    let mut leaf = EffectAsset::new("Leaf", 4.0);
    leaf.emitters
        .push(Emitter::basic_sprite("Leaf emitter", 2.0));
    leaf.save_ron(root.path().join("leaf.aestra.ron")).unwrap();
    let mut source = EffectAsset::new("Source", 4.0);
    let mut nested = EffectClip::new(leaf.id, 0.25, 1.5);
    nested.source_offset = 0.2;
    let nested_id = nested.id;
    source.effect_clips.push(nested);
    source
        .emitters
        .push(Emitter::basic_sprite("Source emitter", 4.0));
    source
        .save_ron(root.path().join("source.aestra.ron"))
        .unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let mut clip = EffectClip::new(source.id, 1.0, 2.0);
    clip.source_offset = 0.5;
    let path = EffectClipPath::root_path(clip.id);
    let mut state = TimelineState::framed(8.0);
    assert!(referenced_track_projections(&catalog, &state, &clip).is_empty());
    state.expanded_effect_clips.insert(path.clone());
    state.expanded_effect_clips.insert(path.child(nested_id));
    let rows = referenced_track_projections(&catalog, &state, &clip);
    assert_eq!(rows.len(), 3);
    let child = rows.iter().find(|row| row.depth == 2).unwrap();
    assert_eq!(child.path, path.child(nested_id));
    let timing = child.timing.unwrap();
    approx(timing.root_source_start, 0.5);
    approx(timing.duration, 1.25);
    assert!(
        matches!(&child.kind,ReferencedTrackKind::Emitter {emitter} if emitter.name=="Leaf emitter")
    );
    assert!(rows.iter().any(|row| matches!(&row.kind,ReferencedTrackKind::EffectClip {clip,source_name,child_count} if clip.id==nested_id && source_name=="Leaf" && *child_count==1)));
    let mapped = map_referenced_interval(
        ReferencedTimingContext {
            root_source_start: 0.5,
            local_source_offset: 0.45,
            duration: 1.25,
        },
        0.4,
        1.0,
    )
    .unwrap();
    approx(mapped.0.duration, 0.95);
    approx(mapped.1, 0.05);
}

#[test]
fn expanded_reference_rows_contribute_to_gap_offset_and_height() {
    let mut f = Fixture::new();
    let clip = f.clip;
    f.state()
        .expanded_effect_clips
        .insert(EffectClipPath::root_path(clip));
    let session = f.app.world().resource::<EditorSession>();
    let state = f.app.world().resource::<TimelineState>();
    let catalog = f.app.world().resource::<ProjectEffectCatalog>();
    let (row, offset) = choreography_insertion_layout(&session.effect, state, catalog, None);
    assert_eq!(row, 4);
    approx(offset, 89.0);
    approx(
        timeline_vertical_content_height(&session.effect, state, catalog),
        89.0,
    );
    assert_eq!(
        choreography_grid_row(
            &session.effect,
            state,
            catalog,
            ChoreographyTrackId::Emitter(EmitterId::new())
        ),
        4
    );
}

#[test]
fn visible_automation_layout_ignores_stale_hidden_and_wrong_channel_lanes() {
    let mut f = Fixture::new();
    let emitter_id = f.emitter;
    let emitter = &f.before.emitters[0];
    let module = emitter
        .modules
        .iter()
        .find(|m| matches!(m.parameters, ModuleParameters::Appearance { .. }))
        .unwrap();
    let lane = AutomationLaneId {
        emitter: f.emitter,
        module: module.id,
        input: 0,
        parameter: "size".into(),
        channel: None,
    };
    let mut stale = lane.clone();
    stale.module = aestra_core::ModuleId::new();
    let mut invalid = lane.clone();
    invalid.channel = Some(1);
    let mut hidden = lane.clone();
    hidden.parameter = "opacity".into();
    {
        let mut state = f.state();
        state
            .visible_automation_lanes
            .extend([lane.clone(), stale.clone(), invalid]);
        state.automation_lane_heights.insert(lane, 180.0);
        state.automation_lane_heights.insert(stale, 300.0);
        state.automation_lane_heights.insert(hidden, 400.0);
        state.expanded_automation_emitters.insert(emitter_id);
    }
    let session = f.app.world().resource::<EditorSession>();
    let state = f.app.world().resource::<TimelineState>();
    let catalog = f.app.world().resource::<ProjectEffectCatalog>();
    assert_eq!(
        visible_automation_lane_count(state, &session.effect.emitters[0]),
        1
    );
    approx(
        automation_lanes_height(state, &session.effect.emitters[0]),
        180.0,
    );
    approx(
        timeline_vertical_content_height(&session.effect, state, catalog),
        242.0,
    );
    assert_eq!(
        choreography_insertion_layout(&session.effect, state, catalog, None),
        (4, 242.0)
    );
}
