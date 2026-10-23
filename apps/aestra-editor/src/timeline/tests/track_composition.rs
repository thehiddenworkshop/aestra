//! Actual assembled track lists and attached consumers, with explicit headless geometry.
use super::super::{
    action_controls, document_actions::TimelineSnapMode, execution,
    navigation_cursor::OverrideCursor, state::TimelineView, timeline_pointer::tests as pointer,
};
use super::*;
use crate::{
    feathers::list_row::KeyboardNavigableList,
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    material_function_editor::FunctionEditor,
    menus::MenuState,
};
use aestra_core::{
    EffectAsset, EffectClip, EffectClipId, Emitter, EmitterId, EmitterRegion, ModuleParameters,
};
use bevy::{
    asset::AssetPlugin,
    input_focus::InputFocus,
    picking::pointer::{PointerButton, PointerId},
    scene::ScenePlugin,
    text::TextPlugin,
    ui_widgets::ListBox,
    window::{CursorIcon, PrimaryWindow},
};

#[derive(Resource)]
struct Built {
    headers: Entity,
    rows: Entity,
}
#[derive(Resource, Default)]
struct Presses(usize);

#[allow(
    clippy::too_many_arguments,
    reason = "Explicit ECS resources for the production assembly fixture"
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
    let headers = commands
        .spawn((Node::default(), KeyboardNavigableList, ListBox))
        .with_children(|parent| {
            spawn_choreography_headers(
                parent, &session, &state, &catalog, &registry, &localizer, &assets,
            )
        })
        .id();
    let rows = commands
        .spawn((
            Node::default(),
            TimelineCanvas,
            ComputedNode {
                size: Vec2::new(400.0, 500.0),
                ..default()
            },
            RelativeCursorPosition::default(),
        ))
        .observe(
            |_: On<super::super::timeline_pointer::PointerPress>, mut presses: ResMut<Presses>| {
                presses.0 += 1
            },
        )
        .with_children(|parent| {
            spawn_choreography_rows(
                parent, &session, &state, &catalog, &registry, &curves, &localizer,
            )
        })
        .id();
    commands.insert_resource(Built { headers, rows });
}

struct Fixture {
    _root: tempfile::TempDir,
    app: App,
    window: Entity,
    emitters: [EmitterId; 2],
    clip: EffectClipId,
    nested: [EffectClipPath; 2],
    leaf_emitter: EmitterId,
    lanes: Vec<AutomationLaneId>,
    before: EffectAsset,
    files: Vec<(std::path::PathBuf, Vec<u8>)>,
}
impl Fixture {
    fn new() -> Self {
        Self::configured("en-US", 1.0, |_, _, _, _| {})
    }
    fn configured(
        locale: &str,
        scale: f32,
        prepare: impl FnOnce(&mut EditorSession, &mut TimelineState, EffectClipId, &[AutomationLaneId]),
    ) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut leaf = EffectAsset::new("Leaf", 4.0);
        leaf.emitters
            .push(Emitter::basic_sprite("Leaf emitter", 4.0));
        let leaf_emitter = leaf.emitters[0].id;
        let mut source = EffectAsset::new("Source", 6.0);
        source
            .emitters
            .push(Emitter::basic_sprite("Source emitter", 6.0));
        source.effect_clips.extend([
            EffectClip::new(leaf.id, 0.0, 2.0),
            EffectClip::new(leaf.id, 2.0, 2.0),
        ]);
        let mut effect = EffectAsset::new("Parent", 8.0);
        let mut emitter = Emitter::basic_sprite("First", 4.0);
        emitter.start_time = 1.0;
        for module in &mut emitter.modules {
            if let ModuleParameters::Appearance { size, .. } = &mut module.parameters {
                *size = aestra_core::Curve::new(vec![
                    aestra_core::CurveKey::new(0.0, 1.0),
                    aestra_core::CurveKey::new(1.0, 2.0),
                ]);
            }
        }
        emitter.regions = vec![
            EmitterRegion::new(1.0, 0.0, 1.0),
            EmitterRegion::new(3.0, 2.0, 1.0),
        ];
        effect
            .emitters
            .extend([emitter, Emitter::basic_sprite("Second", 4.0)]);
        let emitters = [effect.emitters[0].id, effect.emitters[1].id];
        let mut clip = EffectClip::new(source.id, 1.0, 3.0);
        clip.source_offset = 0.5;
        let clip_id = clip.id;
        let path = EffectClipPath::root_path(clip_id);
        let nested = [
            path.child(source.effect_clips[0].id),
            path.child(source.effect_clips[1].id),
        ];
        effect.effect_clips.push(clip);
        effect.choreography_order = vec![
            ChoreographyTrackId::Emitter(emitters[0]),
            ChoreographyTrackId::EffectClip(clip_id),
            ChoreographyTrackId::Emitter(emitters[1]),
        ];
        let localizer = Localizer::new(locale).unwrap();
        let registry = EditorModuleRegistry::default();
        let lanes = emitter_automation_lanes(&effect, &effect.emitters[0], &registry, &localizer)
            .into_iter()
            .filter(|lane| matches!(lane.id.parameter.as_str(), "size" | "color"))
            .map(|lane| lane.id)
            .collect::<Vec<_>>();
        assert_eq!(lanes.len(), 2);
        let mut state = TimelineState::framed(8.0);
        state.view = TimelineView {
            start: 0.0,
            end: 8.0,
        };
        state.snap = TimelineSnapMode::None;
        state
            .expanded_effect_clips
            .extend([path, nested[0].clone(), nested[1].clone()]);
        state.expanded_automation_emitters.insert(emitters[0]);
        state.visible_automation_lanes.extend(lanes.iter().cloned());
        state.automation_lane_heights.insert(lanes[0].clone(), 72.0);
        let mut session = EditorSession::from_test_effect(effect);
        prepare(&mut session, &mut state, clip_id, &lanes);
        let before = session.effect.clone();
        let mut files = Vec::new();
        for (name, asset) in [
            ("leaf.aestra.ron", &leaf),
            ("source.aestra.ron", &source),
            ("parent.aestra.ron", &before),
        ] {
            let path = root.path().join(name);
            asset.save_ron(&path).unwrap();
            files.push((path.clone(), std::fs::read(path).unwrap()));
        }
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
        .insert_resource(localizer)
        .insert_resource(registry)
        .insert_resource(ProjectEffectCatalog::scan(root.path()))
        .init_resource::<CurvesState>()
        .init_resource::<MenuState>()
        .init_resource::<InputFocus>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<OverrideCursor>()
        .init_resource::<EditorHistoryLedger>()
        .init_resource::<MaterialProgramEditHistory>()
        .init_resource::<FunctionEditor>()
        .init_resource::<Presses>()
        .add_observer(action_controls::queue_choreography_action_activation)
        .add_observer(action_controls::activate_timeline_track_entry)
        .add_observer(execution::execute_choreography_action)
        .add_observer(execution::execute_timeline_action)
        .add_observer(
            |event: On<HistoryAction>, routing: Routing, mut commands: Commands| {
                routing.execute(*event, &mut commands, |_, _, _, _| {
                    panic!("No filesystem history expected")
                });
            },
        )
        .add_systems(Startup, build);
        let mut window = Window::default();
        window.resolution.set_scale_factor_override(Some(scale));
        let window = app
            .world_mut()
            .spawn((window, PrimaryWindow, CursorIcon::default()))
            .id();
        app.update();
        Self {
            _root: root,
            app,
            window,
            emitters,
            clip: clip_id,
            nested,
            leaf_emitter,
            lanes,
            before,
            files,
        }
    }
    fn event<E: Event>(&mut self, event: E)
    where
        for<'a> E::Trigger<'a>: Default,
    {
        self.app.world_mut().trigger(event);
        self.app.world_mut().flush();
    }
    fn state(&self) -> &TimelineState {
        self.app.world().resource::<TimelineState>()
    }
    fn session(&self) -> &EditorSession {
        self.app.world().resource::<EditorSession>()
    }
    fn unchanged_files(&self) {
        for (path, bytes) in &self.files {
            assert_eq!(&std::fs::read(path).unwrap(), bytes);
        }
    }
    fn clean(&self) {
        assert_eq!(self.session().effect, self.before);
        assert!(!self.session().can_undo());
        self.unchanged_files();
    }
    fn roundtrip(&mut self) {
        let after = self.session().effect.clone();
        assert_ne!(after, self.before);
        self.event(HistoryAction::Undo);
        assert_eq!(self.session().effect, self.before);
        assert!(
            !self.session().can_undo(),
            "Exactly one semantic transaction per release"
        );
        self.event(HistoryAction::Redo);
        assert_eq!(self.session().effect, after);
        self.unchanged_files();
    }
    fn root_rows(&mut self) -> Vec<(ChoreographyTrackId, i16)> {
        let w = self.app.world_mut();
        w.query::<(&TimelineTrackDropRow, &TimelineChoreographyGridRow)>()
            .iter(w)
            .map(|(t, r)| (t.track, r.0))
            .collect()
    }
    fn grid_rows(&self, root: Entity) -> Vec<(i16, Val)> {
        let w = self.app.world();
        let mut rows = w
            .get::<Children>(root)
            .unwrap()
            .iter()
            .filter_map(|e| {
                Some((
                    w.get::<TimelineChoreographyGridRow>(e)?.0,
                    w.get::<Node>(e)?.height,
                ))
            })
            .collect::<Vec<_>>();
        rows.sort_by_key(|r| r.0);
        rows
    }
    fn effect_control(&mut self, kind: TimelineDragKind) -> Entity {
        let w = self.app.world_mut();
        w.query::<(Entity, &TimelineEffectClipInteraction)>()
            .iter(w)
            .find_map(|(e, c)| (c.clip == self.clip && c.kind == kind).then_some(e))
            .unwrap()
    }
    fn region_control(&mut self, kind: TimelineDragKind) -> Entity {
        let w = self.app.world_mut();
        let region = self.before.emitters[0].regions[0].id;
        w.query::<(Entity, &TimelineClipInteraction)>()
            .iter(w)
            .find_map(|(e, c)| {
                (c.emitter == self.emitters[0] && c.region == region && c.kind == kind).then_some(e)
            })
            .unwrap()
    }
    fn effect_bar(&mut self) -> Entity {
        let w = self.app.world_mut();
        w.query::<(Entity, &TimelineEffectClip)>()
            .iter(w)
            .find_map(|(e, c)| (c.clip == self.clip).then_some(e))
            .unwrap()
    }
    fn drag(&mut self, target: Entity, pixels: f32) {
        self.event(pointer::start(
            target,
            self.window,
            Vec2::ZERO,
            PointerId::Mouse,
            PointerButton::Primary,
        ));
        self.event(pointer::motion_delta(
            target,
            self.window,
            Vec2::new(pixels, 0.0),
        ));
        assert_eq!(
            self.session().effect,
            self.before,
            "Preview cannot edit the document"
        );
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
fn assembled_headers_and_rows_share_exact_mixed_grid_order_and_heights() {
    let mut f = Fixture::new();
    let roots = f.app.world().resource::<Built>();
    let headers = f.grid_rows(roots.headers);
    let rows = f.grid_rows(roots.rows);
    assert_eq!(headers, rows);
    assert_eq!(
        rows.iter().map(|r| r.0).collect::<Vec<_>>(),
        (1..=10).collect::<Vec<_>>()
    );
    assert_eq!(rows[0].1, Val::Px(31.0));
    assert_eq!(rows[1].1, Val::Px(72.0));
    assert_eq!(rows[3].1, Val::Px(31.0));
    assert_eq!(rows[4].1, Val::Px(27.0));
    assert_eq!(
        f.root_rows(),
        vec![
            (ChoreographyTrackId::EffectClip(f.clip), 4),
            (ChoreographyTrackId::Emitter(f.emitters[0]), 1),
            (ChoreographyTrackId::Emitter(f.emitters[1]), 10)
        ]
    );
    f.clean();
}

#[test]
fn collapsed_children_and_lanes_remove_both_sides_without_empty_rows() {
    let mut f = Fixture::configured("en-US", 1.0, |_, s, _, _| {
        s.expanded_effect_clips.clear();
        s.expanded_automation_emitters.clear();
    });
    let roots = f.app.world().resource::<Built>();
    assert_eq!(f.grid_rows(roots.headers), f.grid_rows(roots.rows));
    assert_eq!(
        f.grid_rows(roots.rows),
        vec![(1, Val::Px(31.0)), (2, Val::Px(31.0)), (3, Val::Px(31.0))]
    );
    assert_eq!(f.root_rows()[0].1, 2);
    f.clean();
}

#[test]
fn lane_visibility_and_custom_height_apply_to_both_assembled_sides() {
    let f = Fixture::configured("en-US", 1.0, |_, s, _, lanes| {
        s.visible_automation_lanes.remove(&lanes[1]);
    });
    let roots = f.app.world().resource::<Built>();
    let rows = f.grid_rows(roots.rows);
    assert_eq!(rows, f.grid_rows(roots.headers));
    assert_eq!(rows.len(), 9);
    assert_eq!(rows[1], (2, Val::Px(72.0)));
    f.clean();
}

#[test]
fn reused_nested_sources_keep_distinct_paths_in_the_assembled_scene() {
    let mut f = Fixture::new();
    let w = f.app.world_mut();
    let paths = w
        .query::<&ChoreographyAction>()
        .iter(w)
        .filter_map(|a| {
            if let ChoreographyAction::SelectEffectClipEmitter { path, emitter } = a {
                (*emitter == f.leaf_emitter).then_some(path.clone())
            } else {
                None
            }
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(paths, f.nested.iter().cloned().collect());
    f.unchanged_files();
}

#[test]
fn explicit_and_implicit_regions_build_three_native_controls_per_bar() {
    let mut f = Fixture::new();
    let w = f.app.world_mut();
    assert_eq!(w.query::<&TimelineClip>().iter(w).count(), 3);
    let controls = w
        .query::<(Entity, &TimelineClipInteraction)>()
        .iter(w)
        .map(|(e, _)| e)
        .collect::<Vec<_>>();
    assert_eq!(controls.len(), 9);
    for e in controls {
        assert!(w.get::<Button>(e).is_some());
        assert!(w.get::<EditorNativeControl>(e).is_some());
        assert!(w.get::<AccessibleLabel>(e).is_some());
    }
    f.clean();
}

#[test]
fn effect_bar_and_boundaries_use_clipped_view_geometry() {
    let mut f = Fixture::configured("en-US", 1.0, |_, s, _, _| {
        s.view = TimelineView {
            start: 2.0,
            end: 3.0,
        };
    });
    let bar = f.effect_bar();
    let node = f.app.world().get::<Node>(bar).unwrap();
    assert_eq!(
        (node.left, node.width),
        (Val::Percent(0.0), Val::Percent(100.0))
    );
    for kind in [TimelineDragKind::TrimStart, TimelineDragKind::TrimEnd] {
        let e = f.effect_control(kind);
        assert_eq!(f.app.world().get::<Node>(e).unwrap().display, Display::None);
    }
    f.clean();
}

#[test]
fn region_boundary_visibility_is_independent_for_each_edge() {
    let mut f = Fixture::configured("en-US", 1.0, |_, s, _, _| {
        s.view = TimelineView {
            start: 1.5,
            end: 3.5,
        };
    });
    let start = f.region_control(TimelineDragKind::TrimStart);
    let end = f.region_control(TimelineDragKind::TrimEnd);
    assert_eq!(
        f.app.world().get::<Node>(start).unwrap().display,
        Display::None
    );
    assert_eq!(
        f.app.world().get::<Node>(end).unwrap().display,
        Display::Flex
    );
    f.clean();
}

#[test]
fn selected_muted_and_suppressed_effect_bars_preserve_authored_style() {
    for (mode, alpha) in [(0, 0.25), (1, 0.42), (2, 0.10), (3, 0.10)] {
        let mut f = Fixture::configured("en-US", 1.0, |session, s, clip, _| {
            session.select_emitter(session.effect.emitters[0].id);
            if mode == 1 {
                session.select_effect_clip(clip);
            }
            if mode == 2 {
                s.muted_effect_clips.insert(clip);
            }
            if mode == 3 {
                session.solo_emitter = Some(session.effect.emitters[0].id);
            }
        });
        let e = f.effect_bar();
        let w = f.app.world();
        assert_eq!(w.get::<BackgroundColor>(e).unwrap().0.alpha(), alpha);
        assert_eq!(
            w.get::<Node>(e).unwrap().border.left,
            Val::Px(if mode == 1 { 2.0 } else { 1.0 })
        );
        f.clean();
    }
}

#[test]
fn localized_bar_controls_keep_labels_cursor_and_trim_hit_areas() {
    for locale in ["en-US", "fr-FR"] {
        let mut f = Fixture::configured(locale, 1.0, |_, _, _, _| {});
        for kind in [
            TimelineDragKind::Move,
            TimelineDragKind::TrimStart,
            TimelineDragKind::TrimEnd,
        ] {
            for e in [f.effect_control(kind), f.region_control(kind)] {
                let w = f.app.world();
                assert!(w.get::<EditorTooltip>(e).is_some());
                assert!(w.get::<AccessibleLabel>(e).is_some());
                assert!(
                    matches!(w.get::<EntityCursor>(e), Some(EntityCursor::System(c)) if *c == if kind == TimelineDragKind::Move { SystemCursorIcon::Grab } else { SystemCursorIcon::EwResize })
                );
                if kind != TimelineDragKind::Move {
                    assert_eq!(w.get::<Node>(e).unwrap().width, Val::Px(8.0));
                }
            }
        }
        f.clean();
    }
}

#[test]
fn both_assembled_panes_retain_the_hidden_drop_spacer() {
    let f = Fixture::new();
    let roots = f.app.world().resource::<Built>();
    let w = f.app.world();
    for root in [roots.headers, roots.rows] {
        let spacers = w
            .get::<Children>(root)
            .unwrap()
            .iter()
            .filter(|e| w.get::<TimelineEffectDropSpacer>(*e).is_some())
            .collect::<Vec<_>>();
        assert_eq!(spacers.len(), 1);
        assert_eq!(w.get::<Node>(spacers[0]).unwrap().display, Display::None);
    }
    f.clean();
}

#[test]
fn actual_assembled_effect_move_and_trims_commit_once_and_undo_globally() {
    for scale in [1.0, 2.0] {
        for kind in [
            TimelineDragKind::Move,
            TimelineDragKind::TrimStart,
            TimelineDragKind::TrimEnd,
        ] {
            let mut f = Fixture::configured("en-US", scale, |_, _, _, _| {});
            let target = f.effect_control(kind);
            f.drag(target, 25.0 * scale);
            let clip = &f.session().effect.effect_clips[0];
            let expected = match kind {
                TimelineDragKind::Move => (1.5, 0.5, 3.0),
                TimelineDragKind::TrimStart => (1.5, 1.0, 2.5),
                TimelineDragKind::TrimEnd => (1.0, 0.5, 3.5),
            };
            assert_eq!(
                (clip.start_time, clip.source_offset, clip.duration),
                expected
            );
            f.roundtrip();
            assert!(f.state().effect_clip_drag.is_none());
            assert!(f.app.world().resource::<OverrideCursor>().0.is_none());
        }
    }
}

#[test]
fn actual_assembled_region_move_and_trims_commit_once_and_undo_globally() {
    for scale in [1.0, 2.0] {
        for kind in [
            TimelineDragKind::Move,
            TimelineDragKind::TrimStart,
            TimelineDragKind::TrimEnd,
        ] {
            let mut f = Fixture::configured("en-US", scale, |_, _, _, _| {});
            let target = f.region_control(kind);
            f.drag(target, 12.5 * scale);
            let region = &f.session().effect.emitters[0].regions[0];
            let expected = match kind {
                TimelineDragKind::Move => (1.25, 0.0, 1.0),
                TimelineDragKind::TrimStart => (1.25, 0.25, 0.75),
                TimelineDragKind::TrimEnd => (1.0, 0.0, 1.25),
            };
            assert_eq!(
                (region.start_time, region.source_offset, region.duration),
                expected
            );
            f.roundtrip();
            assert!(f.state().drag.is_none());
            assert!(f.app.world().resource::<OverrideCursor>().0.is_none());
        }
    }
}

#[test]
fn no_motion_releases_of_built_controls_leave_document_and_history_clean() {
    let mut f = Fixture::new();
    for kind in [
        TimelineDragKind::Move,
        TimelineDragKind::TrimStart,
        TimelineDragKind::TrimEnd,
    ] {
        for target in [f.effect_control(kind), f.region_control(kind)] {
            f.drag(target, 0.0);
            f.clean();
        }
    }
}

#[test]
fn attached_primary_clicks_select_real_effect_and_region_without_editing() {
    let mut f = Fixture::new();
    let e = f.effect_control(TimelineDragKind::Move);
    f.event(pointer::click(e, f.window, PointerButton::Primary, 1));
    assert_eq!(
        f.session().selection.primary,
        SemanticTarget::EffectClip(f.clip)
    );
    let e = f.region_control(TimelineDragKind::Move);
    f.event(pointer::click(e, f.window, PointerButton::Primary, 1));
    assert_eq!(
        f.state().selected_emitter_region,
        Some((f.emitters[0], f.before.emitters[0].regions[0].id))
    );
    f.clean();
}

#[test]
fn attached_secondary_region_click_opens_its_real_header_context() {
    let mut f = Fixture::new();
    let e = f.region_control(TimelineDragKind::Move);
    f.event(pointer::click_at(
        e,
        f.window,
        PointerButton::Secondary,
        1,
        Vec2::new(20.0, 10.0),
    ));
    assert_eq!(f.state().context_emitter, Some(f.emitters[0]));
    assert_eq!(
        f.state().selected_emitter_region,
        Some((f.emitters[0], f.before.emitters[0].regions[0].id))
    );
    f.clean();
}

#[test]
fn attached_control_presses_do_not_reach_the_track_canvas() {
    let mut f = Fixture::new();
    for kind in [
        TimelineDragKind::Move,
        TimelineDragKind::TrimStart,
        TimelineDragKind::TrimEnd,
    ] {
        for target in [f.effect_control(kind), f.region_control(kind)] {
            f.event(pointer::press(
                target,
                f.window,
                Vec2::ZERO,
                PointerId::Mouse,
                PointerButton::Primary,
            ));
        }
    }
    assert_eq!(f.app.world().resource::<Presses>().0, 0);
    f.clean();
}

#[test]
fn assembled_automation_graphs_use_real_lane_identity_and_metadata() {
    let mut f = Fixture::new();
    let w = f.app.world_mut();
    let ids = w
        .query::<&TimelineAutomationLaneGraph>()
        .iter(w)
        .map(|g| g.0.clone())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids, f.lanes.iter().cloned().collect());
    assert!(w.query::<&TimelineAutomationKey>().iter(w).count() >= 4);
    f.clean();
}

#[test]
fn empty_effect_builds_only_spacers_and_no_editable_tracks() {
    let mut f = Fixture::configured("en-US", 1.0, |session, _, _, _| {
        session.effect.emitters.clear();
        session.effect.effect_clips.clear();
        session.effect.choreography_order.clear();
    });
    assert!(f.root_rows().is_empty());
    let roots = f.app.world().resource::<Built>();
    assert!(f.grid_rows(roots.headers).is_empty());
    assert!(f.grid_rows(roots.rows).is_empty());
    f.clean();
}
