//! Complete production timeline/choreography observers with actual session and history routing.
use super::*;
use crate::{
    CurvesState, ProjectEffectCatalog,
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    localization::Localizer,
    material_function_editor::FunctionEditor,
    persistence::DocumentAction,
};
use aestra_authoring::SemanticTarget;
use aestra_core::{EffectId, MarkerId, ModuleParameters};
use bevy::{
    input::{
        ButtonState, InputPlugin,
        keyboard::{Key, KeyboardInput},
    },
    input_focus::{InputDispatchPlugin, InputFocusPlugin},
    time::TimeUpdateStrategy,
    window::PrimaryWindow,
};
use std::time::Duration;

#[derive(Resource, Default)]
struct SourceIntents(Vec<DocumentAction>);
struct Fixture {
    root: tempfile::TempDir,
    app: App,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let session = crate::test_support::session_with_timing_slack();
        let duration = session.playback_duration();
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(TimelineState::framed(duration))
            .insert_resource(ProjectEffectCatalog::scan(root.path()))
            .insert_resource(Localizer::new("en-US").unwrap())
            .init_resource::<CurvesState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<SourceIntents>()
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<FunctionEditor>()
            .add_observer(execution::execute_timeline_action)
            .add_observer(execution::execute_choreography_action)
            .add_observer(
                |action: On<DocumentAction>, mut intents: ResMut<SourceIntents>| {
                    intents.0.push(*action);
                },
            )
            .add_observer(
                |action: On<HistoryAction>, routing: Routing, mut commands: Commands| {
                    routing.execute(*action, &mut commands, |_, _, _, _| {
                        panic!("No filesystem history expected")
                    });
                },
            );
        Self { root, app }
    }
    fn action<E: Event>(&mut self, action: E)
    where
        for<'a> E::Trigger<'a>: Default,
    {
        self.app.world_mut().trigger(action);
        self.app.world_mut().flush();
    }
    fn session(&self) -> &EditorSession {
        self.app.world().resource::<EditorSession>()
    }
    fn state(&self) -> &TimelineState {
        self.app.world().resource::<TimelineState>()
    }
    fn baseline(&mut self, effect: EffectAsset) {
        let duration = effect.duration;
        self.app
            .insert_resource(EditorSession::from_test_effect(effect));
        self.app.insert_resource(TimelineState::framed(duration));
    }
    fn emitter(&self) -> EmitterId {
        self.session().effect.emitters[0].id
    }
    fn regions(&mut self) -> (EmitterId, Vec<EmitterRegionId>) {
        let mut effect = self.session().effect.clone();
        let emitter = &mut effect.emitters[0];
        let id = emitter.id;
        let first = emitter.implicit_region_id();
        let second = EmitterRegionId::new();
        emitter.regions = emitter.split_timeline_region(first, 1.5, second).unwrap();
        let third = EmitterRegionId::new();
        emitter.regions = emitter.split_timeline_region(second, 2.25, third).unwrap();
        self.baseline(effect);
        (id, vec![first, second, third])
    }
    fn nested(&mut self) -> (EffectClipId, EffectClipPath, EffectAsset, EffectAsset) {
        let mut grand = crate::test_support::effect_with_timing_slack();
        grand.id = EffectId::new();
        grand.name = "Grandchild".into();
        let mut child = crate::test_support::effect_with_timing_slack();
        child.id = EffectId::new();
        child.name = "Child".into();
        let inner = EffectClip::new(grand.id, 0.0, 1.0);
        let inner_id = inner.id;
        child.effect_clips.push(inner);
        child
            .save_ron(self.root.path().join("child.aestra.ron"))
            .unwrap();
        grand
            .save_ron(self.root.path().join("grand.aestra.ron"))
            .unwrap();
        let mut effect = self.session().effect.clone();
        let clip = EffectClip::new(child.id, 0.0, 1.0);
        let root_id = clip.id;
        effect.effect_clips.push(clip);
        self.baseline(effect);
        self.app
            .insert_resource(ProjectEffectCatalog::scan(self.root.path()));
        (
            root_id,
            EffectClipPath::root_path(root_id).child(inner_id),
            child,
            grand,
        )
    }
    fn assert_clean(&self, effect: &EffectAsset) {
        assert_eq!(&self.session().effect, effect);
        assert!(!self.session().can_undo());
        assert!(!self.session().dirty);
    }
    fn keyboard(&mut self) -> Entity {
        self.app
            .add_plugins((
                MinimalPlugins,
                InputPlugin,
                InputFocusPlugin,
                InputDispatchPlugin,
                crate::input::EditorKeyboardPlugin,
            ))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO))
            .init_resource::<ModulePaletteState>()
            .add_systems(
                Update,
                keyboard::choreography_keyboard_input.in_set(TimelineSet::Input),
            );
        self.app.world_mut().spawn(TimelineCanvas);
        let window = self
            .app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        self.app.update();
        window
    }
    fn key(&mut self, window: Entity, code: KeyCode, logical: Key, state: ButtonState) {
        self.app.world_mut().write_message(KeyboardInput {
            key_code: code,
            logical_key: logical,
            state,
            repeat: false,
            text: None,
            window,
        });
    }
}

#[test]
fn add_and_duplicate_emitters_create_fresh_identity_and_one_history_step_each() {
    let mut f = Fixture::new();
    let before = f.session().effect.clone();
    f.action(ChoreographyAction::AddEmitter);
    let added = f.session().effect.clone();
    assert_eq!(added.emitters.len(), before.emitters.len() + 1);
    let selected = f.session().selection.emitter(&added).unwrap();
    assert_eq!(f.state().selected_emitters, BTreeSet::from([selected]));
    f.action(ChoreographyAction::DuplicateEmitter(Some(selected)));
    let duplicated = f.session().effect.clone();
    assert_eq!(duplicated.emitters.len(), before.emitters.len() + 2);
    assert_ne!(f.session().selection.emitter(&duplicated), Some(selected));
    f.action(HistoryAction::Undo);
    assert_eq!(f.session().effect, added);
    f.action(HistoryAction::Undo);
    f.assert_clean(&before);
    f.action(HistoryAction::Redo);
    f.action(HistoryAction::Redo);
    assert_eq!(f.session().effect, duplicated);
}

#[test]
fn emitter_deletion_previews_then_confirmation_commits_one_reversible_edit() {
    let mut f = Fixture::new();
    let before = f.session().effect.clone();
    let id = f.emitter();
    f.action(ChoreographyAction::DeleteEmitter(Some(id)));
    f.assert_clean(&before);
    assert!(f.session().pending_change.as_ref().unwrap().can_apply);
    assert!(
        f.app
            .world_mut()
            .resource_mut::<EditorSession>()
            .apply_pending_change()
    );
    let deleted = f.session().effect.clone();
    assert!(!deleted.emitters.iter().any(|emitter| emitter.id == id));
    f.action(HistoryAction::Undo);
    f.assert_clean(&before);
    f.action(HistoryAction::Redo);
    assert_eq!(f.session().effect, deleted);
}

#[test]
fn canceling_emitter_deletion_preserves_document_and_history() {
    let mut f = Fixture::new();
    let before = f.session().effect.clone();
    f.action(ChoreographyAction::DeleteEmitter(None));
    assert!(
        f.app
            .world_mut()
            .resource_mut::<EditorSession>()
            .discard_pending_change()
    );
    f.assert_clean(&before);
}

#[test]
fn last_emitter_and_stale_or_non_emitter_targets_are_protected() {
    let mut f = Fixture::new();
    let mut effect = f.session().effect.clone();
    effect.emitters.truncate(1);
    effect.choreography_order.clear();
    f.baseline(effect.clone());
    f.action(ChoreographyAction::DeleteEmitter(None));
    f.assert_clean(&effect);
    assert!(f.session().pending_change.is_none());
    for action in [
        ChoreographyAction::DeleteEmitter(Some(EmitterId::new())),
        ChoreographyAction::DuplicateEmitter(Some(EmitterId::new())),
        ChoreographyAction::SetEmitterEnabled {
            emitter: EmitterId::new(),
            enabled: false,
        },
    ] {
        f.action(action);
        f.assert_clean(&effect);
    }
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .selection
        .primary = SemanticTarget::Effect(effect.id);
    f.action(ChoreographyAction::DuplicateEmitter(None));
    f.assert_clean(&effect);
}

#[test]
fn enabled_changes_are_undoable_but_preview_solo_is_nonsemantic() {
    let mut f = Fixture::new();
    let before = f.session().effect.clone();
    let id = f.emitter();
    f.action(ChoreographyAction::SetEmitterEnabled {
        emitter: id,
        enabled: false,
    });
    assert!(!f.session().effect.emitters[0].enabled);
    f.action(HistoryAction::Undo);
    f.assert_clean(&before);
    f.action(ChoreographyAction::ToggleEmitterSolo(id));
    assert_eq!(f.session().solo_emitter, Some(id));
    f.assert_clean(&before);
    f.action(ChoreographyAction::ToggleEmitterSolo(id));
    assert_eq!(f.session().solo_emitter, None);
    f.assert_clean(&before);
}

#[test]
fn emitter_selection_honors_control_shift_and_clears_nested_curve_state() {
    let mut f = Fixture::new();
    let before = f.session().effect.clone();
    let ids = before.emitters.iter().map(|e| e.id).collect::<Vec<_>>();
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .inspected_child = Some(EffectClipChildSelection::EffectClip {
        path: EffectClipPath::root_path(EffectClipId::new()),
    });
    f.app
        .world_mut()
        .resource_mut::<CurvesState>()
        .select_for_test(before.emitters[0].modules[0].id, 0, 0);
    f.action(ChoreographyAction::SelectEmitter(ids[0]));
    assert!(f.state().inspected_child.is_none());
    assert!(!f.app.world().resource::<CurvesState>().has_selection());
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    f.action(ChoreographyAction::SelectEmitter(ids[2]));
    assert_eq!(
        f.state().selected_emitters,
        BTreeSet::from([ids[0], ids[2]])
    );
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::ControlLeft);
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftRight);
    f.action(ChoreographyAction::SelectEmitter(ids[1]));
    assert_eq!(
        f.state().selected_emitters,
        BTreeSet::from([ids[1], ids[2]])
    );
    f.assert_clean(&before);
}

#[test]
fn region_selection_honors_toggle_and_range_without_authoring_edits() {
    let mut f = Fixture::new();
    let (id, regions) = f.regions();
    let before = f.session().effect.clone();
    f.action(ChoreographyAction::SelectEmitterRegion {
        emitter: id,
        region: regions[0],
    });
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);
    f.action(ChoreographyAction::SelectEmitterRegion {
        emitter: id,
        region: regions[2],
    });
    assert_eq!(f.state().selected_emitter_regions.len(), 3);
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlRight);
    f.action(ChoreographyAction::SelectEmitterRegion {
        emitter: id,
        region: regions[1],
    });
    assert_eq!(f.state().selected_emitter_regions.len(), 2);
    f.assert_clean(&before);
}

#[test]
fn split_and_join_regions_preserve_source_timing_and_undo_as_transactions() {
    let mut f = Fixture::new();
    let before = f.session().effect.clone();
    let id = f.emitter();
    let first = before.emitters[0].implicit_region_id();
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .select_only_emitter_region(id, first);
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .seek_time(1.5);
    f.action(TimelineAction::SplitEmitterRegion);
    let split = f.session().effect.clone();
    let regions = split.emitters[0].timeline_regions();
    assert_eq!(regions.len(), 2);
    assert_eq!(regions[0].id, first);
    assert_eq!(regions[0].end_time(), regions[1].start_time);
    assert_eq!(regions[1].source_offset, regions[0].duration);
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .select_only_emitter_regions(id, &regions.iter().map(|r| r.id).collect::<Vec<_>>());
    f.action(TimelineAction::JoinEmitterRegion);
    let joined = f.session().effect.clone();
    assert_eq!(joined.emitters[0].timeline_regions().len(), 1);
    f.action(HistoryAction::Undo);
    assert_eq!(f.session().effect, split);
    f.action(HistoryAction::Undo);
    f.assert_clean(&before);
}

#[test]
fn split_boundary_and_cross_emitter_join_reject_without_partial_edits() {
    let mut f = Fixture::new();
    let before = f.session().effect.clone();
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .seek_time(before.emitters[0].start_time);
    f.action(TimelineAction::SplitEmitterRegion);
    f.assert_clean(&before);
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .selected_emitter_regions
        .extend(
            before.emitters[..2]
                .iter()
                .map(|e| (e.id, e.implicit_region_id())),
        );
    f.action(TimelineAction::JoinEmitterRegion);
    f.assert_clean(&before);
}

#[test]
fn duplicate_regions_remaps_identity_and_retains_source_offsets() {
    let mut f = Fixture::new();
    let (id, regions) = f.regions();
    let before = f.session().effect.clone();
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .select_only_emitter_regions(id, &regions[..2]);
    f.action(ChoreographyAction::DuplicateSelectedEmitterRegions);
    let edited = f.session().effect.clone();
    let selected = &f.state().selected_emitter_regions;
    assert_eq!(selected.len(), 2);
    assert!(selected.iter().all(|(_, region)| !regions.contains(region)));
    let sources = before.emitters[0].timeline_regions();
    let mut copies = selected
        .iter()
        .map(|(_, region)| edited.emitters[0].timeline_region(*region).unwrap())
        .collect::<Vec<_>>();
    copies.sort_by(|left, right| left.start_time.total_cmp(&right.start_time));
    for (copy, original) in copies.iter().zip(sources.iter()) {
        assert_eq!(copy.source_offset, original.source_offset);
    }
    f.action(HistoryAction::Undo);
    f.assert_clean(&before);
    f.action(HistoryAction::Redo);
    assert_eq!(f.session().effect, edited);
}

#[test]
fn deleting_a_region_selects_a_survivor_and_restores_exact_regions_on_undo() {
    let mut f = Fixture::new();
    let (id, regions) = f.regions();
    let before = f.session().effect.clone();
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .select_only_emitter_region(id, regions[1]);
    f.action(ChoreographyAction::DeleteSelectedEmitterRegions);
    assert_eq!(f.session().effect.emitters[0].timeline_regions().len(), 2);
    assert_eq!(f.state().selected_emitter_region, Some((id, regions[2])));
    f.action(HistoryAction::Undo);
    f.assert_clean(&before);
}

#[test]
fn deleting_all_regions_requires_emitter_confirmation_and_cancel_is_safe() {
    let mut f = Fixture::new();
    let (id, regions) = f.regions();
    let before = f.session().effect.clone();
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .select_only_emitter_regions(id, &regions);
    f.action(ChoreographyAction::DeleteSelectedEmitterRegions);
    f.assert_clean(&before);
    assert!(f.session().pending_change.is_some());
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .discard_pending_change();
    f.assert_clean(&before);
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .select_only_emitter_regions(id, &regions);
    f.action(ChoreographyAction::DeleteSelectedEmitterRegions);
    assert!(
        f.app
            .world_mut()
            .resource_mut::<EditorSession>()
            .apply_pending_change()
    );
    assert!(!f.session().effect.emitters.iter().any(|e| e.id == id));
    f.action(HistoryAction::Undo);
    f.assert_clean(&before);
}

#[test]
fn stale_region_duplicate_and_delete_reject_without_partial_semantics() {
    for action in [
        ChoreographyAction::DuplicateSelectedEmitterRegions,
        ChoreographyAction::DeleteSelectedEmitterRegions,
    ] {
        let mut f = Fixture::new();
        let before = f.session().effect.clone();
        let id = f.emitter();
        f.app
            .world_mut()
            .resource_mut::<TimelineState>()
            .select_only_emitter_region(id, EmitterRegionId::new());
        f.action(action);
        f.assert_clean(&before);
        assert!(f.session().pending_change.is_none());
    }
}

#[test]
fn marker_selection_and_stale_deletion_use_the_complete_timeline_observer() {
    let mut f = Fixture::new();
    let id = f.emitter();
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .select_only_emitter(id);
    f.action(TimelineAction::AddMarker);
    let marker = f.session().effect.markers[0].id;
    assert!(f.state().selected_emitters.is_empty());
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .inspected_child = Some(EffectClipChildSelection::EffectClip {
        path: EffectClipPath::root_path(EffectClipId::new()),
    });
    f.action(TimelineAction::SelectMarker(marker));
    assert!(f.state().inspected_child.is_none());
    let effect = f.session().effect.clone();
    f.action(TimelineAction::DeleteMarker(MarkerId::new()));
    assert_eq!(f.session().effect, effect);
    f.action(HistoryAction::Undo);
    assert!(f.session().effect.markers.is_empty() && !f.session().can_undo());
}

#[test]
fn duration_snap_and_frame_actions_keep_view_changes_separate_from_document_history() {
    let mut f = Fixture::new();
    let before = f.session().effect.clone();
    let revision = f.session().ui_revision;
    f.action(TimelineAction::SetSnap(TimelineSnapMode::Frames));
    assert_eq!(f.session().ui_revision, revision + 1);
    f.action(TimelineAction::SetSnap(TimelineSnapMode::Frames));
    assert_eq!(f.session().ui_revision, revision + 1);
    f.assert_clean(&before);
    f.action(TimelineAction::AdjustEffectDuration(1.0));
    assert!(f.session().effect.duration > before.duration);
    f.action(TimelineAction::FrameAll);
    assert_eq!(f.state().view.start, 0.0);
    assert_eq!(f.state().view.end, f.session().playback_duration());
    f.action(HistoryAction::Undo);
    f.assert_clean(&before);
}

#[test]
fn selecting_local_clip_clears_nested_inspection_emitter_and_curve_selection() {
    let mut f = Fixture::new();
    let (root, path, _, _) = f.nested();
    let before = f.session().effect.clone();
    let id = f.emitter();
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .select_only_emitter(id);
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .inspected_child = Some(EffectClipChildSelection::EffectClip { path });
    f.app
        .world_mut()
        .resource_mut::<CurvesState>()
        .select_for_test(before.emitters[0].modules[0].id, 0, 0);
    f.action(ChoreographyAction::SelectEffectClip(root));
    assert_eq!(
        f.session().selection.primary,
        SemanticTarget::EffectClip(root)
    );
    assert!(f.state().inspected_child.is_none() && f.state().selected_emitters.is_empty());
    assert!(!f.app.world().resource::<CurvesState>().has_selection());
    f.assert_clean(&before);
}

#[test]
fn clip_and_emitter_solo_are_mutually_exclusive_and_do_not_dirty_document() {
    let mut f = Fixture::new();
    let (root, _, _, _) = f.nested();
    let before = f.session().effect.clone();
    let id = f.emitter();
    f.action(ChoreographyAction::ToggleEmitterSolo(id));
    f.action(ChoreographyAction::ToggleEffectClipSolo(root));
    assert_eq!(f.session().solo_emitter, None);
    assert_eq!(f.state().solo_effect_clip, Some(root));
    f.action(ChoreographyAction::ToggleEmitterSolo(id));
    assert_eq!(f.state().solo_effect_clip, None);
    assert_eq!(f.session().solo_emitter, Some(id));
    f.action(ChoreographyAction::ToggleEffectClipMuted(root));
    assert!(!f.state().effect_clip_is_audible(root));
    f.action(ChoreographyAction::ToggleEffectClipMuted(root));
    assert!(f.state().effect_clip_is_audible(root));
    f.assert_clean(&before);
}

#[test]
fn clip_deletion_retires_nested_preview_state_and_is_reversible() {
    let mut f = Fixture::new();
    let (root, path, _, _) = f.nested();
    let before = f.session().effect.clone();
    {
        let mut state = f.app.world_mut().resource_mut::<TimelineState>();
        state.expanded_effect_clips.insert(path.clone());
        state.muted_effect_clips.insert(root);
        state.solo_effect_clip = Some(root);
        state.inspected_child = Some(EffectClipChildSelection::EffectClip { path });
    }
    f.action(ChoreographyAction::DeleteEffectClip(root));
    assert!(f.session().effect.effect_clips.is_empty());
    assert!(
        f.state().expanded_effect_clips.is_empty()
            && f.state().muted_effect_clips.is_empty()
            && f.state().solo_effect_clip.is_none()
            && f.state().inspected_child.is_none()
    );
    f.action(HistoryAction::Undo);
    f.assert_clean(&before);
    f.action(HistoryAction::Redo);
    assert!(f.session().effect.effect_clips.is_empty());
}

#[test]
fn nested_selection_resolves_actual_catalog_and_never_edits_source_files() {
    let mut f = Fixture::new();
    let (root, path, _, grand) = f.nested();
    let before = f.session().effect.clone();
    let source = std::fs::read(f.root.path().join("grand.aestra.ron")).unwrap();
    f.action(ChoreographyAction::SelectReferencedEffectClip(path.clone()));
    assert_eq!(
        f.session().selection.primary,
        SemanticTarget::EffectClip(root)
    );
    assert_eq!(
        f.state().inspected_child,
        Some(EffectClipChildSelection::EffectClip { path: path.clone() })
    );
    f.action(ChoreographyAction::SelectEffectClipEmitter {
        path: path.clone(),
        emitter: grand.emitters[0].id,
    });
    assert_eq!(
        f.state().inspected_child,
        Some(EffectClipChildSelection::Emitter {
            path,
            emitter: grand.emitters[0].id
        })
    );
    f.assert_clean(&before);
    assert_eq!(
        std::fs::read(f.root.path().join("grand.aestra.ron")).unwrap(),
        source
    );
}

#[test]
fn collapsing_an_ancestor_clears_descendant_inspection_but_not_other_paths() {
    let mut f = Fixture::new();
    let (root, path, _, _) = f.nested();
    let before = f.session().effect.clone();
    let ancestor = EffectClipPath::root_path(root);
    f.action(ChoreographyAction::ToggleEffectClipExpanded(
        ancestor.clone(),
    ));
    f.action(ChoreographyAction::SelectReferencedEffectClip(path));
    f.action(ChoreographyAction::ToggleEffectClipExpanded(ancestor));
    assert!(f.state().inspected_child.is_none());
    f.assert_clean(&before);
}

#[test]
fn source_opening_emits_exact_document_intents_for_local_and_nested_sources() {
    let mut f = Fixture::new();
    let (root, path, child, grand) = f.nested();
    let before = f.session().effect.clone();
    f.action(ChoreographyAction::EditEffectClipSource(root));
    f.action(ChoreographyAction::EditEffectClipEmitterSource {
        path,
        emitter: grand.emitters[0].id,
    });
    assert_eq!(
        f.app.world().resource::<SourceIntents>().0,
        [
            DocumentAction::OpenSource(child.id.into()),
            DocumentAction::OpenSourceEmitter(grand.id.into(), grand.emitters[0].id)
        ]
    );
    f.assert_clean(&before);
}

#[test]
fn missing_paths_sources_and_emitters_do_not_open_or_replace_inspection() {
    let mut f = Fixture::new();
    let (_, path, _, _) = f.nested();
    let before = f.session().effect.clone();
    let old = f.state().inspected_child.clone();
    for action in [
        ChoreographyAction::SelectReferencedEffectClip(path.child(EffectClipId::new())),
        ChoreographyAction::SelectEffectClipEmitter {
            path: path.clone(),
            emitter: EmitterId::new(),
        },
        ChoreographyAction::EditEffectClipEmitterSource {
            path,
            emitter: EmitterId::new(),
        },
        ChoreographyAction::EditEffectClipSource(EffectClipId::new()),
    ] {
        f.action(action);
        assert_eq!(f.state().inspected_child, old);
        f.assert_clean(&before);
    }
    assert!(f.app.world().resource::<SourceIntents>().0.is_empty());
}

#[test]
fn context_cleanup_and_color_picker_toggle_preserve_native_revision_policy() {
    let mut f = Fixture::new();
    let id = f.emitter();
    let before = f.session().effect.clone();
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .context_emitter = Some(id);
    let revision = f.session().ui_revision;
    f.action(ChoreographyAction::DuplicateEmitter(Some(EmitterId::new())));
    assert!(f.state().context_emitter.is_none());
    assert_eq!(f.state().restore_context_emitter_focus, Some(id));
    assert_eq!(f.session().ui_revision, revision + 1);
    f.action(ChoreographyAction::ToggleEmitterColorPicker(id));
    assert_eq!(f.state().color_picker_emitter, Some(id));
    f.action(ChoreographyAction::ToggleEmitterColorPicker(id));
    assert_eq!(f.state().color_picker_emitter, None);
    f.assert_clean(&before);
}

#[test]
fn full_choreography_observer_delegates_automation_without_double_execution() {
    let mut f = Fixture::new();
    let before = f.session().effect.clone();
    let emitter = &before.emitters[0];
    let module = emitter
        .modules
        .iter()
        .find(|m| matches!(m.parameters, ModuleParameters::Appearance { .. }))
        .unwrap();
    let lane = AutomationLaneId {
        emitter: emitter.id,
        module: module.id,
        input: 0,
        parameter: "size".into(),
        channel: None,
    };
    f.action(ChoreographyAction::AddAutomationKeyAt {
        lane: lane.clone(),
        normalized_time_bits: 0.6_f32.to_bits(),
        value_bits: Some(2.0_f32.to_bits()),
    });
    let keys = automation_data::automation_lane_keys(&f.session().effect, &lane).unwrap();
    assert_eq!(
        keys.len(),
        automation_data::automation_lane_keys(&before, &lane)
            .unwrap()
            .len()
            + 1
    );
    f.action(HistoryAction::Undo);
    f.assert_clean(&before);
}

#[test]
fn keyboard_create_duplicate_and_delete_reach_real_choreography_confirmation() {
    let mut f = Fixture::new();
    let before = f.session().effect.clone();
    let window = f.keyboard();
    f.key(
        window,
        KeyCode::ControlLeft,
        Key::Control,
        ButtonState::Pressed,
    );
    f.key(window, KeyCode::Enter, Key::Enter, ButtonState::Pressed);
    f.key(window, KeyCode::Enter, Key::Enter, ButtonState::Released);
    f.key(
        window,
        KeyCode::KeyD,
        Key::Character("d".into()),
        ButtonState::Pressed,
    );
    f.key(
        window,
        KeyCode::KeyD,
        Key::Character("d".into()),
        ButtonState::Released,
    );
    f.key(
        window,
        KeyCode::ControlLeft,
        Key::Control,
        ButtonState::Released,
    );
    f.app.update();
    assert_eq!(f.session().effect.emitters.len(), before.emitters.len() + 2);
    f.key(window, KeyCode::Delete, Key::Delete, ButtonState::Pressed);
    f.key(window, KeyCode::Delete, Key::Delete, ButtonState::Released);
    f.app.update();
    assert!(f.session().pending_change.is_some());
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .discard_pending_change();
    f.action(HistoryAction::Undo);
    f.action(HistoryAction::Undo);
    f.assert_clean(&before);
}

#[test]
fn mixed_timeline_and_emitter_edits_undo_in_global_order_and_fork_redo() {
    let mut f = Fixture::new();
    let before = f.session().effect.clone();
    f.action(ChoreographyAction::AddEmitter);
    let added = f.session().effect.clone();
    f.action(TimelineAction::AddMarker);
    let both = f.session().effect.clone();
    f.action(HistoryAction::Undo);
    assert_eq!(f.session().effect, added);
    f.action(HistoryAction::Undo);
    f.assert_clean(&before);
    f.action(HistoryAction::Redo);
    f.action(HistoryAction::Redo);
    assert_eq!(f.session().effect, both);
    f.action(HistoryAction::Undo);
    f.action(TimelineAction::AddChoreographyEvent);
    let fork = f.session().effect.clone();
    f.action(HistoryAction::Redo);
    assert_eq!(f.session().effect, fork);
}
