//! Actual click/menu observers with real catalog, selection, source intents and history.
use super::super::{execution, timeline_pointer::tests as pointer};
use super::*;
use crate::{
    ProjectEffectCatalog,
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    localization::Localizer,
    material_function_editor::FunctionEditor,
    persistence::DocumentAction,
};
use aestra_authoring::SemanticTarget;
use aestra_core::{
    ChoreographyEvent, ChoreographyEventPayload, EffectAsset, EffectClip, EffectMarker, MarkerId,
    ModuleId,
};
use bevy::ecs::system::RunSystemOnce;
use bevy::{input_focus::FocusCause, math::Affine2};

#[derive(Resource, Default)]
struct Intents(Vec<DocumentAction>);
#[derive(Resource, Default)]
struct Bubbled(usize);
struct Fixture {
    root: tempfile::TempDir,
    app: App,
    window: Entity,
    emitters: [EmitterId; 2],
    clip: aestra_core::EffectClipId,
    source: EffectAsset,
    source_bytes: Vec<u8>,
    before: EffectAsset,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut source = crate::test_support::effect_with_timing_slack();
        source.id = aestra_core::EffectId::new();
        source.name = "Source".into();
        source
            .save_ron(root.path().join("source.aestra.ron"))
            .unwrap();
        let source_bytes = std::fs::read(root.path().join("source.aestra.ron")).unwrap();
        let mut effect = crate::test_support::effect_with_timing_slack();
        let emitters = [effect.emitters[0].id, effect.emitters[1].id];
        let clip = EffectClip::new(source.id, 0.0, 2.0);
        let clip_id = clip.id;
        effect.effect_clips.push(clip);
        effect.markers.push(EffectMarker {
            id: MarkerId::new(),
            name: "Cue".into(),
            time: 1.0,
        });
        effect.choreography_events.push(ChoreographyEvent::new(
            "Sound",
            1.0,
            ChoreographyEventPayload::PlaySound {
                cue: "burst".into(),
            },
        ));
        let mut app = App::new();
        app.insert_resource(EditorSession::from_test_effect(effect.clone()))
            .insert_resource(TimelineState::framed(4.0))
            .insert_resource(ProjectEffectCatalog::scan(root.path()))
            .insert_resource(Localizer::new("en-US").unwrap())
            .init_resource::<CurvesState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<InputFocus>()
            .init_resource::<Time<Real>>()
            .init_resource::<Intents>()
            .init_resource::<Bubbled>()
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<FunctionEditor>()
            .add_observer(execution::execute_timeline_action)
            .add_observer(execution::execute_choreography_action)
            .add_observer(|event: On<DocumentAction>, mut intents: ResMut<Intents>| {
                intents.0.push(*event)
            })
            .add_observer(
                |event: On<HistoryAction>, routing: Routing, mut commands: Commands| {
                    routing.execute(*event, &mut commands, |_, _, _, _| {
                        panic!("No filesystem history expected")
                    });
                },
            )
            .add_systems(Update, open_focused_timeline_context_menu);
        // PointerTraversal queries Window even while walking ChildOf ancestry.
        let window = app.world_mut().spawn(Window::default()).id();
        Self {
            root,
            app,
            window,
            emitters,
            clip: clip_id,
            source,
            source_bytes,
            before: effect,
        }
    }
    fn session(&self) -> &EditorSession {
        self.app.world().resource::<EditorSession>()
    }
    fn state(&self) -> &TimelineState {
        self.app.world().resource::<TimelineState>()
    }
    fn action<E: Event>(&mut self, event: E)
    where
        for<'a> E::Trigger<'a>: Default,
    {
        self.app.world_mut().flush();
        self.app.world_mut().trigger(event);
        self.app.world_mut().flush();
    }
    fn click(&mut self, entity: Entity, button: PointerButton, count: u8) {
        self.action(pointer::click_at(
            entity,
            self.window,
            button,
            count,
            Vec2::new(50.0, 55.0),
        ));
    }
    fn geometry() -> (ComputedNode, UiGlobalTransform) {
        (
            ComputedNode {
                size: Vec2::new(200.0, 40.0),
                ..default()
            },
            UiGlobalTransform::from(Affine2::from_translation(Vec2::new(100.0, 60.0))),
        )
    }
    fn emitter_header(&mut self, id: EmitterId) -> Entity {
        self.app
            .world_mut()
            .spawn((EmitterTrackHeader { emitter: id }, Self::geometry()))
            .observe(open_timeline_track_context_menu)
            .observe(collapse_emitter_multi_selection_on_primary_click)
            .id()
    }
    fn clip_header(&mut self) -> Entity {
        self.app
            .world_mut()
            .spawn((EffectClipTrackHeader { clip: self.clip }, Self::geometry()))
            .observe(open_effect_clip_track_context_menu)
            .observe(open_effect_clip_source_from_header)
            .id()
    }
    fn group(&mut self) {
        self.app
            .world_mut()
            .resource_mut::<TimelineState>()
            .selected_emitters
            .extend(self.emitters);
        self.app
            .world_mut()
            .resource_mut::<EditorSession>()
            .select_emitter(self.emitters[0]);
    }
    fn select_curve(&mut self) {
        self.app
            .world_mut()
            .resource_mut::<CurvesState>()
            .select_for_test(ModuleId::new(), 0, 1);
    }
    fn clean(&self) {
        assert_eq!(self.session().effect, self.before);
        assert!(!self.session().dirty);
        assert!(!self.session().can_undo());
        assert_eq!(
            std::fs::read(self.root.path().join("source.aestra.ron")).unwrap(),
            self.source_bytes
        );
    }
    fn focus_key(&mut self, entity: Entity, key: KeyCode) {
        self.app
            .world_mut()
            .resource_mut::<InputFocus>()
            .set(entity, FocusCause::Navigated);
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        self.app.update();
    }
    fn referenced(&self) -> ChoreographyAction {
        ChoreographyAction::SelectEffectClipEmitter {
            path: EffectClipPath::root_path(self.clip),
            emitter: self.source.emitters[0].id,
        }
    }
    fn expect_source(&self) {
        assert_eq!(
            self.app.world().resource::<Intents>().0,
            [DocumentAction::OpenSource(self.source.id.into())]
        );
    }
    fn expect_emitter_source(&self) {
        assert_eq!(
            self.app.world().resource::<Intents>().0,
            [DocumentAction::OpenSourceEmitter(
                self.source.id.into(),
                self.source.emitters[0].id
            )]
        );
    }
}

#[test]
fn primary_marker_click_selects_real_marker_and_stops_propagation() {
    let mut f = Fixture::new();
    let parent = f
        .app
        .world_mut()
        .spawn_empty()
        .observe(|_: On<PointerClick>, mut seen: ResMut<Bubbled>| seen.0 += 1)
        .id();
    let id = f.before.markers[0].id;
    let marker = f
        .app
        .world_mut()
        .spawn((TimelineMarker { marker: id }, ChildOf(parent)))
        .observe(select_timeline_marker)
        .id();
    f.click(marker, PointerButton::Primary, 1);
    assert_eq!(f.session().selection.primary, SemanticTarget::Marker(id));
    assert_eq!(f.app.world().resource::<Bubbled>().0, 0);
    f.clean();
}

#[test]
fn primary_event_click_selects_real_choreography_event() {
    let mut f = Fixture::new();
    let id = f.before.choreography_events[0].id;
    let event = f
        .app
        .world_mut()
        .spawn(TimelineChoreographyEvent { event: id })
        .observe(select_choreography_event)
        .id();
    f.click(event, PointerButton::Primary, 1);
    assert_eq!(
        f.session().selection.primary,
        SemanticTarget::ChoreographyEvent(id)
    );
    f.clean();
}

#[test]
fn secondary_and_missing_marker_controls_leave_selection_unchanged() {
    let mut f = Fixture::new();
    let initial = f.session().selection.primary;
    let target = f
        .app
        .world_mut()
        .spawn(TimelineMarker {
            marker: f.before.markers[0].id,
        })
        .observe(select_timeline_marker)
        .id();
    f.click(target, PointerButton::Secondary, 1);
    f.app
        .world_mut()
        .entity_mut(target)
        .remove::<TimelineMarker>();
    f.click(target, PointerButton::Primary, 1);
    assert_eq!(f.session().selection.primary, initial);
    f.clean();
}

#[test]
fn region_click_routes_actual_region_selection() {
    let mut f = Fixture::new();
    let emitter = f.emitters[0];
    let region = f.before.emitters[0].implicit_region_id();
    let target = f
        .app
        .world_mut()
        .spawn((
            TimelineClipInteraction {
                emitter,
                region,
                kind: TimelineDragKind::Move,
            },
            ChoreographyAction::SelectEmitterRegion { emitter, region },
        ))
        .observe(select_timeline_clip)
        .id();
    f.click(target, PointerButton::Primary, 1);
    assert_eq!(f.session().selected_emitter_region, Some(region));
    assert_eq!(f.state().selected_emitter_region, Some((emitter, region)));
    f.clean();
}

#[test]
fn effect_clip_single_click_selects_and_double_click_opens_source() {
    let mut f = Fixture::new();
    let target = f
        .app
        .world_mut()
        .spawn((
            TimelineEffectClipControl,
            TimelineEffectClipInteraction {
                clip: f.clip,
                kind: TimelineDragKind::Move,
            },
            ChoreographyAction::SelectEffectClip(f.clip),
        ))
        .observe(select_timeline_effect_clip)
        .id();
    f.click(target, PointerButton::Primary, 1);
    assert_eq!(
        f.session().selection.primary,
        SemanticTarget::EffectClip(f.clip)
    );
    assert!(f.app.world().resource::<Intents>().0.is_empty());
    f.click(target, PointerButton::Primary, 2);
    f.expect_source();
    f.clean();
}

#[test]
fn trim_double_click_and_secondary_click_do_not_open_source() {
    let mut f = Fixture::new();
    let target = f
        .app
        .world_mut()
        .spawn((
            TimelineEffectClipControl,
            TimelineEffectClipInteraction {
                clip: f.clip,
                kind: TimelineDragKind::TrimStart,
            },
            ChoreographyAction::SelectEffectClip(f.clip),
        ))
        .observe(select_timeline_effect_clip)
        .id();
    f.click(target, PointerButton::Secondary, 2);
    f.click(target, PointerButton::Primary, 2);
    assert_eq!(
        f.session().selection.primary,
        SemanticTarget::EffectClip(f.clip)
    );
    assert!(f.app.world().resource::<Intents>().0.is_empty());
    f.clean();
}

#[test]
fn stale_clip_and_unclassified_controls_cannot_open_a_source() {
    let mut f = Fixture::new();
    let target = f.clip_header();
    f.app
        .world_mut()
        .get_mut::<EffectClipTrackHeader>(target)
        .unwrap()
        .clip = aestra_core::EffectClipId::new();
    f.click(target, PointerButton::Primary, 2);
    let unclassified = f
        .app
        .world_mut()
        .spawn((
            TimelineEffectClipInteraction {
                clip: f.clip,
                kind: TimelineDragKind::Move,
            },
            ChoreographyAction::SelectEffectClip(f.clip),
        ))
        .observe(select_timeline_effect_clip)
        .id();
    f.click(unclassified, PointerButton::Primary, 2);
    assert!(f.app.world().resource::<Intents>().0.is_empty());
    f.clean();
}

#[test]
fn header_source_open_requires_primary_double_click() {
    let mut f = Fixture::new();
    let header = f.clip_header();
    f.click(header, PointerButton::Primary, 1);
    f.click(header, PointerButton::Secondary, 2);
    assert!(f.app.world().resource::<Intents>().0.is_empty());
    f.click(header, PointerButton::Primary, 2);
    f.expect_source();
    f.clean();
}

#[test]
fn referenced_click_selects_child_then_repeated_click_opens_actual_source() {
    let mut f = Fixture::new();
    let action = f.referenced();
    let target = f
        .app
        .world_mut()
        .spawn((TimelineReferencedEmitterControl, action))
        .observe(select_timeline_referenced_emitter)
        .id();
    f.click(target, PointerButton::Primary, 1);
    assert_eq!(
        f.state().inspected_child,
        Some(EffectClipChildSelection::Emitter {
            path: EffectClipPath::root_path(f.clip),
            emitter: f.source.emitters[0].id
        })
    );
    f.app
        .world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_millis(200));
    f.click(target, PointerButton::Primary, 1);
    f.expect_emitter_source();
    assert!(f.state().referenced_emitter_click.is_none());
    f.clean();
}

#[test]
fn referenced_header_first_click_only_arms_source_opening() {
    let mut f = Fixture::new();
    let action = f.referenced();
    let target = f
        .app
        .world_mut()
        .spawn((ReferencedEmitterTrackHeader, action))
        .observe(open_referenced_emitter_source_from_header)
        .id();
    let selection = f.session().selection.primary;
    f.click(target, PointerButton::Primary, 1);
    assert_eq!(f.session().selection.primary, selection);
    assert!(f.app.world().resource::<Intents>().0.is_empty());
    f.click(target, PointerButton::Primary, 1);
    f.expect_emitter_source();
    f.clean();
}

#[test]
fn referenced_repeat_expires_and_secondary_click_does_not_arm() {
    let mut f = Fixture::new();
    let action = f.referenced();
    let target = f
        .app
        .world_mut()
        .spawn((TimelineReferencedEmitterControl, action))
        .observe(select_timeline_referenced_emitter)
        .id();
    f.click(target, PointerButton::Secondary, 1);
    assert!(f.state().referenced_emitter_click.is_none());
    f.click(target, PointerButton::Primary, 1);
    f.app
        .world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_millis(501));
    f.click(target, PointerButton::Primary, 1);
    assert!(f.app.world().resource::<Intents>().0.is_empty());
    f.clean();
}

#[test]
fn referenced_explicit_double_click_opens_without_prior_click() {
    let mut f = Fixture::new();
    let action = f.referenced();
    let target = f
        .app
        .world_mut()
        .spawn((TimelineReferencedEmitterControl, action))
        .observe(select_timeline_referenced_emitter)
        .id();
    f.click(target, PointerButton::Primary, 2);
    f.expect_emitter_source();
    f.clean();
}

#[test]
fn referenced_fallback_action_clears_repeat_memory() {
    let mut f = Fixture::new();
    let action = f.referenced();
    referenced_emitter_click_action(
        &mut f.app.world_mut().resource_mut::<TimelineState>(),
        Duration::ZERO,
        1,
        &action,
    );
    let fallback = ChoreographyAction::SelectEffectClip(f.clip);
    assert_eq!(
        referenced_emitter_click_action(
            &mut f.app.world_mut().resource_mut::<TimelineState>(),
            Duration::ZERO,
            2,
            &fallback
        ),
        fallback
    );
    assert!(f.state().referenced_emitter_click.is_none());
}

#[test]
fn emitter_context_walks_child_ancestry_and_preserves_selected_group() {
    #[derive(Resource, Default)]
    struct Traversed(Vec<Entity>);
    let mut f = Fixture::new();
    f.app.init_resource::<Traversed>().add_observer(
        |event: On<PointerClick>, mut visited: ResMut<Traversed>| {
            visited.0.push(event.event_target())
        },
    );
    f.group();
    f.select_curve();
    let header = f.emitter_header(f.emitters[0]);
    let child = f.app.world_mut().spawn(ChildOf(header)).id();
    assert_eq!(
        f.app.world().get::<ChildOf>(child).unwrap().parent(),
        header
    );
    {
        let mut state = f.app.world_mut().resource_mut::<TimelineState>();
        state.color_picker_emitter = Some(f.emitters[1]);
        state.automation_menu_emitter = Some(f.emitters[1]);
        state.context_effect_clip = Some(f.clip);
    }
    let revision = f.session().ui_revision;
    f.click(child, PointerButton::Secondary, 1);
    assert_eq!(
        f.state().context_emitter,
        Some(f.emitters[0]),
        "header {header:?}; child {child:?}; visited {:?}",
        f.app.world().resource::<Traversed>().0
    );
    assert_eq!(f.state().selected_emitters.len(), 2);
    assert_eq!(f.state().context_menu_position, Vec2::new(50.0, 15.0));
    assert!(
        f.state().color_picker_emitter.is_none()
            && f.state().automation_menu_emitter.is_none()
            && f.state().context_effect_clip.is_none()
    );
    assert!(!f.app.world().resource::<CurvesState>().has_selection());
    assert_eq!(f.session().ui_revision, revision + 1);
    f.clean();
}

#[test]
fn emitter_context_outside_group_selects_only_its_target() {
    let mut f = Fixture::new();
    let header = f.emitter_header(f.emitters[1]);
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .select_only_emitter(f.emitters[0]);
    f.click(header, PointerButton::Secondary, 1);
    assert_eq!(
        f.state().selected_emitters,
        std::collections::BTreeSet::from([f.emitters[1]])
    );
    assert_eq!(
        f.session().selection.primary,
        SemanticTarget::Emitter(f.emitters[1])
    );
    f.clean();
}

#[test]
fn region_context_preserves_multi_regions_and_uses_header_coordinates() {
    let mut f = Fixture::new();
    let emitter = f.emitters[0];
    let region = f.before.emitters[0].implicit_region_id();
    let other = (f.emitters[1], f.before.emitters[1].implicit_region_id());
    f.emitter_header(emitter);
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .selected_emitter_regions
        .extend([(emitter, region), other]);
    let target = f
        .app
        .world_mut()
        .spawn(TimelineClipInteraction {
            emitter,
            region,
            kind: TimelineDragKind::Move,
        })
        .observe(open_timeline_region_context_menu)
        .id();
    f.click(target, PointerButton::Secondary, 1);
    assert_eq!(f.state().selected_emitter_regions.len(), 2);
    assert_eq!(f.state().selected_emitter_region, Some((emitter, region)));
    assert_eq!(
        f.state().emitter_region_selection_anchor,
        Some((emitter, region))
    );
    assert_eq!(f.state().context_menu_position, Vec2::new(50.0, 15.0));
    f.clean();
}

#[test]
fn region_context_without_header_uses_zero_position_and_single_selection() {
    let mut f = Fixture::new();
    let emitter = f.emitters[0];
    let region = f.before.emitters[0].implicit_region_id();
    let target = f
        .app
        .world_mut()
        .spawn(TimelineClipInteraction {
            emitter,
            region,
            kind: TimelineDragKind::Move,
        })
        .observe(open_timeline_region_context_menu)
        .id();
    f.click(target, PointerButton::Secondary, 1);
    assert_eq!(f.state().selected_emitter_regions.len(), 1);
    assert_eq!(f.state().context_menu_position, Vec2::ZERO);
    f.clean();
}

#[test]
fn effect_context_clears_emitter_and_child_selection_and_other_popovers() {
    let mut f = Fixture::new();
    f.group();
    f.select_curve();
    {
        let mut state = f.app.world_mut().resource_mut::<TimelineState>();
        state.inspected_child = Some(EffectClipChildSelection::EffectClip {
            path: EffectClipPath::root_path(f.clip),
        });
        state.context_emitter = Some(f.emitters[0]);
        state.color_picker_emitter = Some(f.emitters[0]);
        state.automation_menu_emitter = Some(f.emitters[0]);
    }
    let header = f.clip_header();
    let child = f.app.world_mut().spawn(ChildOf(header)).id();
    f.click(child, PointerButton::Secondary, 1);
    assert_eq!(
        f.session().selection.primary,
        SemanticTarget::EffectClip(f.clip)
    );
    assert!(f.state().selected_emitters.is_empty() && f.state().inspected_child.is_none());
    assert!(
        f.state().context_emitter.is_none()
            && f.state().color_picker_emitter.is_none()
            && f.state().automation_menu_emitter.is_none()
    );
    assert_eq!(f.state().context_effect_clip, Some(f.clip));
    assert_eq!(f.state().context_menu_position, Vec2::new(50.0, 15.0));
    assert!(!f.app.world().resource::<CurvesState>().has_selection());
    f.clean();
}

#[test]
fn primary_and_unrelated_context_targets_do_not_open_menus() {
    let mut f = Fixture::new();
    let emitter = f.emitter_header(f.emitters[0]);
    let clip = f.clip_header();
    f.click(emitter, PointerButton::Primary, 1);
    f.click(clip, PointerButton::Primary, 1);
    let unrelated = f
        .app
        .world_mut()
        .spawn_empty()
        .observe(open_timeline_track_context_menu)
        .id();
    f.click(unrelated, PointerButton::Secondary, 1);
    assert!(f.state().context_emitter.is_none() && f.state().context_effect_clip.is_none());
    f.clean();
}

#[test]
fn stale_header_keeps_legacy_menu_identity_without_selecting_missing_emitter() {
    let mut f = Fixture::new();
    let missing = EmitterId::new();
    let header = f.emitter_header(missing);
    let selection = f.session().selection.primary;
    f.click(header, PointerButton::Secondary, 1);
    assert_eq!(f.state().context_emitter, Some(missing));
    assert_eq!(f.session().selection.primary, selection);
    f.clean();
}

#[test]
fn keyboard_menu_uses_active_descendant_and_nearest_header() {
    let mut f = Fixture::new();
    let header = f.emitter_header(f.emitters[0]);
    let child = f.app.world_mut().spawn(ChildOf(header)).id();
    let list = f.app.world_mut().spawn(ActiveDescendant(Some(child))).id();
    f.focus_key(list, KeyCode::ContextMenu);
    assert_eq!(f.state().context_emitter, Some(f.emitters[0]));
    assert_eq!(f.state().context_menu_position, Vec2::new(192.0, 20.0));
    f.clean();
}

#[test]
fn keyboard_shift_f10_opens_but_plain_f10_does_not() {
    for shift in [KeyCode::ShiftLeft, KeyCode::ShiftRight] {
        let mut f = Fixture::new();
        let header = f.emitter_header(f.emitters[0]);
        f.focus_key(header, KeyCode::F10);
        assert!(f.state().context_emitter.is_none());
        f.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        f.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::F10);
        f.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(shift);
        f.focus_key(header, KeyCode::F10);
        assert_eq!(f.state().context_emitter, Some(f.emitters[0]));
        f.clean();
    }
}

#[test]
fn keyboard_effect_context_clamps_small_header_and_clears_emitter_group() {
    let mut f = Fixture::new();
    f.group();
    let header = f.clip_header();
    f.app
        .world_mut()
        .get_mut::<ComputedNode>(header)
        .unwrap()
        .size = Vec2::new(4.0, 20.0);
    f.focus_key(header, KeyCode::ContextMenu);
    assert_eq!(f.state().context_effect_clip, Some(f.clip));
    assert_eq!(f.state().context_menu_position, Vec2::new(0.0, 10.0));
    assert!(f.state().selected_emitters.is_empty());
    f.clean();
}

#[test]
fn keyboard_no_focus_unrelated_focus_and_existing_menu_do_not_open_or_replace() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ContextMenu);
    f.app.update();
    assert!(f.state().context_emitter.is_none());
    let unrelated = f.app.world_mut().spawn_empty().id();
    f.app
        .world_mut()
        .resource_mut::<InputFocus>()
        .set(unrelated, FocusCause::Navigated);
    f.app.update();
    assert!(f.state().context_emitter.is_none());
    let header = f.emitter_header(f.emitters[0]);
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .context_effect_clip = Some(f.clip);
    f.app
        .world_mut()
        .resource_mut::<InputFocus>()
        .set(header, FocusCause::Navigated);
    f.app.update();
    assert!(f.state().context_emitter.is_none());
    assert_eq!(f.state().context_effect_clip, Some(f.clip));
    f.clean();
}

#[test]
fn configured_action_control_is_native_dispatches_once_and_undoes() {
    let mut f = Fixture::new();
    let emitter = f.emitters[0];
    let control = f
        .app
        .world_mut()
        .spawn((
            TimelineButton,
            FeathersActionButton,
            ChoreographyAction::SetEmitterEnabled {
                emitter,
                enabled: false,
            },
        ))
        .id();
    f.app
        .world_mut()
        .run_system_once(move |commands: Commands| {
            configure_timeline_track_action_control(commands, control)
        })
        .unwrap();
    f.app.world_mut().flush();
    assert!(f.app.world().get::<TimelineButton>(control).is_none());
    assert!(f.app.world().get::<FeathersActionButton>(control).is_none());
    assert!(f.app.world().get::<EditorNativeControl>(control).is_some());
    assert!(
        f.app
            .world()
            .get::<TimelineTrackActionControl>(control)
            .is_some()
    );
    f.click(control, PointerButton::Primary, 1);
    assert!(!f.session().effect.emitters[0].enabled);
    f.action(HistoryAction::Undo);
    f.clean();
}

#[test]
fn native_action_secondary_or_missing_action_is_ignored() {
    let mut f = Fixture::new();
    let control = f
        .app
        .world_mut()
        .spawn((
            TimelineTrackActionControl,
            ChoreographyAction::SetEmitterEnabled {
                emitter: f.emitters[0],
                enabled: false,
            },
        ))
        .observe(activate_timeline_track_action_control)
        .id();
    f.click(control, PointerButton::Secondary, 1);
    f.app
        .world_mut()
        .entity_mut(control)
        .remove::<ChoreographyAction>();
    f.click(control, PointerButton::Primary, 1);
    f.clean();
}

#[test]
fn unmodified_primary_header_click_collapses_group_and_clears_curves() {
    let mut f = Fixture::new();
    f.group();
    f.select_curve();
    let header = f.emitter_header(f.emitters[0]);
    f.click(header, PointerButton::Primary, 1);
    assert_eq!(
        f.state().selected_emitters,
        std::collections::BTreeSet::from([f.emitters[0]])
    );
    assert!(!f.app.world().resource::<CurvesState>().has_selection());
    f.clean();
}

#[test]
fn modifiers_and_native_action_controls_preserve_group() {
    for key in [
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
    ] {
        let mut f = Fixture::new();
        f.group();
        let header = f.emitter_header(f.emitters[0]);
        f.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        f.click(header, PointerButton::Primary, 1);
        assert_eq!(f.state().selected_emitters.len(), 2);
        f.clean();
    }
    let mut f = Fixture::new();
    f.group();
    let header = f.emitter_header(f.emitters[0]);
    let control = f
        .app
        .world_mut()
        .spawn((
            TimelineTrackActionControl,
            ChoreographyAction::ToggleEmitterSolo(f.emitters[0]),
            ChildOf(header),
        ))
        .observe(activate_timeline_track_action_control)
        .id();
    f.click(control, PointerButton::Primary, 1);
    assert_eq!(f.state().selected_emitters.len(), 2);
    f.clean();
}

#[test]
fn unhandled_reorder_click_keeps_legacy_bubbled_target_collapse_policy() {
    let mut f = Fixture::new();
    f.group();
    let header = f.emitter_header(f.emitters[0]);
    let handle = f
        .app
        .world_mut()
        .spawn((
            EmitterTrackReorderHandle {
                emitter: f.emitters[0],
            },
            ChildOf(header),
        ))
        .id();
    f.click(handle, PointerButton::Primary, 1);
    // The existing header observer sees the propagated header target, not the handle.
    assert_eq!(f.state().selected_emitters.len(), 1);
    f.clean();
}

#[test]
fn changing_referenced_row_does_not_open_the_previous_emitters_source() {
    let mut f = Fixture::new();
    let first_action = f.referenced();
    let first = f
        .app
        .world_mut()
        .spawn((TimelineReferencedEmitterControl, first_action))
        .observe(select_timeline_referenced_emitter)
        .id();
    let second_action = ChoreographyAction::SelectEffectClipEmitter {
        path: EffectClipPath::root_path(f.clip),
        emitter: f.source.emitters[1].id,
    };
    let second = f
        .app
        .world_mut()
        .spawn((TimelineReferencedEmitterControl, second_action))
        .observe(select_timeline_referenced_emitter)
        .id();
    f.click(first, PointerButton::Primary, 1);
    f.click(second, PointerButton::Primary, 1);
    assert!(f.app.world().resource::<Intents>().0.is_empty());
    assert_eq!(
        f.state().referenced_emitter_click.as_ref().unwrap().emitter,
        f.source.emitters[1].id
    );
    f.clean();
}

#[test]
fn keyboard_menu_without_focus_resource_is_a_safe_noop() {
    let mut f = Fixture::new();
    f.app.world_mut().remove_resource::<InputFocus>();
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ContextMenu);
    f.app.update();
    assert!(f.state().context_emitter.is_none() && f.state().context_effect_clip.is_none());
    f.clean();
}

#[test]
fn popup_coordinates_preserve_existing_translation_and_size_policy() {
    let (node, transform) = Fixture::geometry();
    assert_eq!(
        pointer_position_in_node(Vec2::new(-10.0, 35.0), &node, &transform),
        Vec2::new(-10.0, -5.0)
    );
}
