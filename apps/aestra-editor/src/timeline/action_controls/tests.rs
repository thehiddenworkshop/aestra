//! Actual activation/list/color observers with production selection and global history.
use super::super::{TimelineState, control_input, execution};
use super::*;
use crate::{
    CurvesState, ProjectEffectCatalog,
    docking::ToolPanel,
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    material_function_editor::FunctionEditor,
    menus::{MenuKind, TabContextMenu},
};
use aestra_authoring::SemanticTarget;
use aestra_core::{EffectAsset, EffectClip, EmitterId, EmitterRegionId};
use bevy::ecs::system::RunSystemOnce;

struct Fixture {
    _root: tempfile::TempDir,
    app: App,
    before: EffectAsset,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let effect = crate::test_support::effect_with_timing_slack();
        let mut app = App::new();
        app.insert_resource(EditorSession::from_test_effect(effect.clone()))
            .insert_resource(TimelineState::framed(4.0))
            .insert_resource(ProjectEffectCatalog::scan(root.path()))
            .insert_resource(Localizer::new("en-US").unwrap())
            .init_resource::<CurvesState>()
            .init_resource::<MenuState>()
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<FunctionEditor>()
            .add_observer(queue_timeline_action_activation)
            .add_observer(queue_choreography_action_activation)
            .add_observer(activate_timeline_track_entry)
            .add_observer(handle_timeline_track_color_change)
            .add_observer(execution::execute_timeline_action)
            .add_observer(execution::execute_choreography_action)
            .add_observer(
                |event: On<HistoryAction>, routing: Routing, mut commands: Commands| {
                    routing.execute(*event, &mut commands, |_, _, _, _| {
                        panic!("No filesystem history expected")
                    });
                },
            )
            .add_systems(
                Update,
                (
                    handle_timeline_action_buttons,
                    handle_choreography_action_buttons,
                )
                    .chain(),
            );
        Self {
            _root: root,
            app,
            before: effect,
        }
    }
    fn session(&self) -> &EditorSession {
        self.app.world().resource::<EditorSession>()
    }
    fn emitter(&self) -> EmitterId {
        self.before.emitters[0].id
    }
    fn action<E: Event>(&mut self, event: E)
    where
        for<'a> E::Trigger<'a>: Default,
    {
        self.app.world_mut().flush();
        self.app.world_mut().trigger(event);
        self.app.world_mut().flush();
    }
    fn timeline(&mut self) -> Entity {
        let entity = self
            .app
            .world_mut()
            .spawn((TimelineAction::AddMarker, FeathersActionButton))
            .id();
        control_input::set_pressed(self.app.world_mut(), entity, false);
        entity
    }
    fn choreography(&mut self) -> Entity {
        let id = self.emitter();
        let entity = self
            .app
            .world_mut()
            .spawn((
                ChoreographyAction::SetEmitterEnabled {
                    emitter: id,
                    enabled: false,
                },
                FeathersActionButton,
            ))
            .id();
        control_input::set_pressed(self.app.world_mut(), entity, false);
        entity
    }
    fn picker(&mut self, emitter: EmitterId) -> Entity {
        self.app
            .world_mut()
            .spawn(EmitterTrackColorPicker { emitter })
            .id()
    }
    fn color(&mut self, picker: Entity, value: Option<[f32; 4]>, is_final: bool) {
        self.action(ValueChange {
            source: picker,
            value,
            is_final,
        });
    }
    fn clean(&self) {
        assert_eq!(self.session().effect, self.before);
        assert!(!self.session().can_undo());
        assert!(!self.session().dirty);
    }
    fn roundtrip(&mut self) {
        let after = self.session().effect.clone();
        assert_ne!(after, self.before);
        self.action(HistoryAction::Undo);
        self.clean();
        self.action(HistoryAction::Redo);
        assert_eq!(self.session().effect, after);
    }
    fn open_menus(&mut self) {
        let mut menu = self.app.world_mut().resource_mut::<MenuState>();
        menu.open = Some(MenuKind::File);
        menu.panels_open = true;
        menu.tab_context = Some(TabContextMenu {
            panel: ToolPanel::Timeline,
            position: [4.0, 5.0],
        });
        menu.show_about = true;
        menu.show_grid = false;
    }
}
const COLOR: [f32; 4] = [0.13, 0.62, 0.91, 0.7];
fn rgba() -> Color {
    Color::srgba(COLOR[0], COLOR[1], COLOR[2], COLOR[3])
}

#[test]
fn explicit_timeline_activate_queues_then_dispatches_one_real_edit() {
    let mut f = Fixture::new();
    let button = f.timeline();
    f.action(Activate { entity: button });
    assert!(control_input::is_pressed(f.app.world(), button));
    assert!(
        f.app
            .world()
            .get::<PendingFeathersActivation>(button)
            .is_some()
    );
    f.clean();
    f.app.update();
    assert_eq!(f.session().effect.markers.len(), f.before.markers.len() + 1);
    assert!(!control_input::is_pressed(f.app.world(), button));
    assert!(
        f.app
            .world()
            .get::<PendingFeathersActivation>(button)
            .is_none()
    );
    f.app.update();
    f.roundtrip();
}

#[test]
fn explicit_choreography_activate_dispatches_one_undoable_edit() {
    let mut f = Fixture::new();
    let button = f.choreography();
    f.action(Activate { entity: button });
    f.app.update();
    assert!(!f.session().effect.emitters[0].enabled);
    assert!(!control_input::is_pressed(f.app.world(), button));
    f.app.update();
    f.roundtrip();
}

#[test]
fn pressed_without_activate_cannot_edit_or_close_menus() {
    let mut f = Fixture::new();
    f.open_menus();
    let timeline = f.timeline();
    let choreography = f.choreography();
    control_input::set_pressed(f.app.world_mut(), timeline, true);
    control_input::set_pressed(f.app.world_mut(), choreography, true);
    f.app.update();
    f.clean();
    assert_eq!(
        f.app.world().resource::<MenuState>().open,
        Some(MenuKind::File)
    );
}

#[test]
fn pending_without_pressed_state_does_not_dispatch() {
    let mut f = Fixture::new();
    let timeline = f.timeline();
    let choreography = f.choreography();
    for button in [timeline, choreography] {
        f.app
            .world_mut()
            .entity_mut(button)
            .insert(PendingFeathersActivation);
    }
    f.app.update();
    f.clean();
}

#[test]
fn unclassified_and_native_actions_ignore_feathers_activate() {
    let mut f = Fixture::new();
    let plain = f.app.world_mut().spawn(TimelineAction::AddMarker).id();
    let native = f
        .app
        .world_mut()
        .spawn((TimelineAction::AddMarker, EditorNativeControl))
        .id();
    let other = f.app.world_mut().spawn(FeathersActionButton).id();
    for button in [plain, native, other] {
        f.action(Activate { entity: button });
        assert!(
            f.app
                .world()
                .get::<PendingFeathersActivation>(button)
                .is_none()
        );
    }
    f.app.update();
    f.clean();
}

#[test]
fn repeated_activation_before_dispatch_coalesces_but_later_activation_edits_again() {
    let mut f = Fixture::new();
    let button = f.timeline();
    f.action(Activate { entity: button });
    f.action(Activate { entity: button });
    f.app.update();
    let first = f.session().effect.clone();
    assert_eq!(first.markers.len(), f.before.markers.len() + 1);
    f.action(Activate { entity: button });
    f.app.update();
    assert_eq!(f.session().effect.markers.len(), f.before.markers.len() + 2);
    f.action(HistoryAction::Undo);
    assert_eq!(f.session().effect, first);
    f.action(HistoryAction::Undo);
    f.clean();
}

#[test]
fn pending_added_after_unchanged_press_waits_for_new_press_transition() {
    let mut f = Fixture::new();
    let button = f.timeline();
    control_input::set_pressed(f.app.world_mut(), button, true);
    f.app.update();
    f.app
        .world_mut()
        .entity_mut(button)
        .insert(PendingFeathersActivation);
    f.app.update();
    f.clean();
    control_input::set_pressed(f.app.world_mut(), button, true);
    f.app.update();
    f.roundtrip();
}

#[test]
fn dispatch_closes_only_action_menus_and_increments_tab_context_revision_once() {
    let mut f = Fixture::new();
    f.open_menus();
    let button = f
        .app
        .world_mut()
        .spawn((TimelineAction::FrameAll, FeathersActionButton))
        .id();
    let revision = f.session().ui_revision;
    f.action(Activate { entity: button });
    f.app.update();
    let menu = f.app.world().resource::<MenuState>();
    assert!(menu.open.is_none() && !menu.panels_open && menu.tab_context.is_none());
    assert!(menu.show_about && !menu.show_grid);
    assert_eq!(f.session().ui_revision, revision + 1);
    f.app.update();
    assert_eq!(f.session().ui_revision, revision + 1);
    f.clean();
}

#[test]
fn action_removal_before_dispatch_does_not_execute_stale_command() {
    let mut f = Fixture::new();
    let button = f.timeline();
    f.action(Activate { entity: button });
    f.app
        .world_mut()
        .entity_mut(button)
        .remove::<TimelineAction>();
    f.app.update();
    f.clean();
}

#[test]
fn list_value_selects_actual_emitter_and_clears_curve_selection() {
    let mut f = Fixture::new();
    let emitter = f.emitter();
    let list = f.app.world_mut().spawn(KeyboardNavigableList).id();
    let row = f
        .app
        .world_mut()
        .spawn((
            EmitterTrackHeader { emitter },
            ChoreographyAction::SelectEmitter(emitter),
        ))
        .id();
    f.app
        .world_mut()
        .resource_mut::<CurvesState>()
        .select_for_test(aestra_core::ModuleId::new(), 0, 0);
    f.action(ValueChange {
        source: list,
        value: row,
        is_final: true,
    });
    assert_eq!(
        f.session().selection.primary,
        SemanticTarget::Emitter(emitter)
    );
    assert!(!f.app.world().resource::<CurvesState>().has_selection());
    f.clean();
}

#[test]
fn list_value_preserves_existing_nonfinal_selection_policy() {
    let mut f = Fixture::new();
    let emitter = f.emitter();
    let list = f.app.world_mut().spawn(KeyboardNavigableList).id();
    let row = f
        .app
        .world_mut()
        .spawn((
            EmitterTrackHeader { emitter },
            ChoreographyAction::SelectEmitter(emitter),
        ))
        .id();
    f.action(ValueChange {
        source: list,
        value: row,
        is_final: false,
    });
    assert_eq!(
        f.session().selection.primary,
        SemanticTarget::Emitter(emitter)
    );
    f.clean();
}

#[test]
fn list_rejects_unmarked_source_row_without_header_and_stale_row() {
    let mut f = Fixture::new();
    let emitter = f.emitter();
    let initial = f.session().selection.primary;
    let list = f.app.world_mut().spawn(KeyboardNavigableList).id();
    let unmarked = f
        .app
        .world_mut()
        .spawn(crate::feathers::list_row::KeyboardNavigableListRow)
        .id();
    let row = f
        .app
        .world_mut()
        .spawn(ChoreographyAction::SelectEmitter(emitter))
        .id();
    f.action(ValueChange {
        source: list,
        value: row,
        is_final: true,
    });
    f.app
        .world_mut()
        .entity_mut(row)
        .insert(EmitterTrackHeader { emitter });
    f.action(ValueChange {
        source: unmarked,
        value: row,
        is_final: true,
    });
    f.app.world_mut().despawn(row);
    f.action(ValueChange {
        source: list,
        value: row,
        is_final: true,
    });
    assert_eq!(f.session().selection.primary, initial);
    f.clean();
}

#[test]
fn list_effect_clip_header_reaches_real_selection() {
    let mut f = Fixture::new();
    let clip = EffectClip::new(aestra_core::EffectId::new(), 0.0, 1.0);
    let id = clip.id;
    f.before.effect_clips.push(clip);
    f.app
        .insert_resource(EditorSession::from_test_effect(f.before.clone()));
    let list = f.app.world_mut().spawn(KeyboardNavigableList).id();
    let row = f
        .app
        .world_mut()
        .spawn((
            EffectClipTrackHeader { clip: id },
            ChoreographyAction::SelectEffectClip(id),
        ))
        .id();
    f.action(ValueChange {
        source: list,
        value: row,
        is_final: true,
    });
    assert_eq!(
        f.session().selection.primary,
        SemanticTarget::EffectClip(id)
    );
    f.clean();
}

#[test]
fn list_referenced_header_resolves_catalog_and_selects_child() {
    let mut f = Fixture::new();
    let mut source = crate::test_support::effect_with_timing_slack();
    source.id = aestra_core::EffectId::new();
    source
        .save_ron(f._root.path().join("child.aestra.ron"))
        .unwrap();
    let clip = EffectClip::new(source.id, 0.0, 1.0);
    let id = clip.id;
    f.before.effect_clips.push(clip);
    f.app
        .insert_resource(EditorSession::from_test_effect(f.before.clone()));
    f.app
        .insert_resource(ProjectEffectCatalog::scan(f._root.path()));
    let list = f.app.world_mut().spawn(KeyboardNavigableList).id();
    let path = EffectClipPath::root_path(id);
    let emitter = source.emitters[0].id;
    let row = f
        .app
        .world_mut()
        .spawn((
            ReferencedEmitterTrackHeader,
            ChoreographyAction::SelectEffectClipEmitter {
                path: path.clone(),
                emitter,
            },
        ))
        .id();
    f.action(ValueChange {
        source: list,
        value: row,
        is_final: true,
    });
    assert_eq!(
        f.app.world().resource::<TimelineState>().inspected_child,
        Some(EffectClipChildSelection::Emitter { path, emitter })
    );
    assert_eq!(
        f.session().selection.primary,
        SemanticTarget::EffectClip(id)
    );
    f.clean();
}

#[test]
fn color_commit_status_uses_current_locale() {
    let mut f = Fixture::new();
    f.app.insert_resource(Localizer::new("fr-FR").unwrap());
    let picker = f.picker(f.emitter());
    f.color(picker, Some(COLOR), true);
    assert_eq!(
        f.session().status,
        f.app
            .world()
            .resource::<Localizer>()
            .text("timeline-emitter-color-updated")
    );
    assert_ne!(
        f.session().status,
        Localizer::new("en-US")
            .unwrap()
            .text("timeline-emitter-color-updated")
    );
    f.roundtrip();
}

#[test]
fn live_color_updates_matching_swatches_and_all_regions_without_editing() {
    let mut f = Fixture::new();
    let emitter = f.emitter();
    let picker = f.picker(emitter);
    let swatch = f
        .app
        .world_mut()
        .spawn((
            EmitterTrackColorSwatch { emitter },
            BackgroundColor(Color::BLACK),
        ))
        .id();
    let clips: Vec<_> = (0..2)
        .map(|_| {
            f.app
                .world_mut()
                .spawn((
                    TimelineClip {
                        emitter,
                        region: EmitterRegionId::new(),
                    },
                    BackgroundColor(Color::BLACK),
                ))
                .id()
        })
        .collect();
    let other = f
        .app
        .world_mut()
        .spawn((
            EmitterTrackColorSwatch {
                emitter: f.before.emitters[1].id,
            },
            BackgroundColor(Color::BLACK),
        ))
        .id();
    f.color(picker, Some(COLOR), false);
    for entity in std::iter::once(swatch).chain(clips) {
        assert_eq!(
            f.app.world().get::<BackgroundColor>(entity).unwrap().0,
            rgba()
        );
    }
    assert_eq!(
        f.app.world().get::<BackgroundColor>(other).unwrap().0,
        Color::BLACK
    );
    f.clean();
}

#[test]
fn color_commit_uses_picker_emitter_not_current_selection_and_one_history_edit() {
    let mut f = Fixture::new();
    let emitter = f.emitter();
    let picker = f.picker(emitter);
    f.action(ChoreographyAction::SelectEmitter(f.before.emitters[1].id));
    f.color(picker, Some([0.2, 0.3, 0.4, 0.5]), false);
    f.color(picker, Some(COLOR), false);
    f.clean();
    f.color(picker, Some(COLOR), true);
    assert_eq!(f.session().effect.emitters[0].display_color, Some(COLOR));
    assert_eq!(
        f.session().effect.emitters[1].display_color,
        f.before.emitters[1].display_color
    );
    assert_eq!(
        f.session().status,
        f.app
            .world()
            .resource::<Localizer>()
            .text("timeline-emitter-color-updated")
    );
    f.roundtrip();
}

#[test]
fn unchanged_color_final_does_not_invent_history_or_status() {
    let mut f = Fixture::new();
    let emitter = f.emitter();
    let picker = f.picker(emitter);
    let status = f.session().status.clone();
    f.color(picker, None, true);
    assert_eq!(f.session().status, status);
    f.clean();
}

#[test]
fn missing_picker_and_missing_emitter_cannot_commit() {
    let mut f = Fixture::new();
    let unmarked = f.app.world_mut().spawn_empty().id();
    f.color(unmarked, Some(COLOR), true);
    let picker = f.picker(EmitterId::new());
    f.color(picker, Some(COLOR), true);
    f.clean();
}

#[test]
fn reset_color_none_commits_semantic_default_without_forcing_live_fill() {
    let mut f = Fixture::new();
    f.before.emitters[0].display_color = Some(COLOR);
    f.app
        .insert_resource(EditorSession::from_test_effect(f.before.clone()));
    let emitter = f.emitter();
    let picker = f.picker(emitter);
    let swatch = f
        .app
        .world_mut()
        .spawn((EmitterTrackColorSwatch { emitter }, BackgroundColor(rgba())))
        .id();
    f.color(picker, None, true);
    assert_eq!(f.session().effect.emitters[0].display_color, None);
    assert_eq!(
        f.app.world().get::<BackgroundColor>(swatch).unwrap().0,
        rgba()
    );
    f.roundtrip();
}

#[test]
fn nonfinal_none_preserves_existing_preview_until_a_later_update() {
    let mut f = Fixture::new();
    let emitter = f.emitter();
    let picker = f.picker(emitter);
    let swatch = f
        .app
        .world_mut()
        .spawn((
            EmitterTrackColorSwatch { emitter },
            BackgroundColor(Color::BLACK),
        ))
        .id();
    f.color(picker, Some(COLOR), false);
    f.color(picker, None, false);
    assert_eq!(
        f.app.world().get::<BackgroundColor>(swatch).unwrap().0,
        rgba()
    );
    f.clean();
}

#[test]
fn swatch_and_clip_queries_remain_disjoint_on_dual_tagged_entity() {
    let mut f = Fixture::new();
    let emitter = f.emitter();
    let picker = f.picker(emitter);
    let dual = f
        .app
        .world_mut()
        .spawn((
            EmitterTrackColorSwatch { emitter },
            TimelineClip {
                emitter: f.before.emitters[1].id,
                region: EmitterRegionId::new(),
            },
            BackgroundColor(Color::BLACK),
        ))
        .id();
    f.color(picker, Some(COLOR), false);
    assert_eq!(
        f.app.world().get::<BackgroundColor>(dual).unwrap().0,
        rgba()
    );
    f.clean();
}

#[test]
fn marker_and_track_color_share_global_undo_order() {
    let mut f = Fixture::new();
    let button = f.timeline();
    f.action(Activate { entity: button });
    f.app.update();
    let marked = f.session().effect.clone();
    let picker = f.picker(f.emitter());
    f.color(picker, Some(COLOR), true);
    let colored = f.session().effect.clone();
    f.action(HistoryAction::Undo);
    assert_eq!(f.session().effect, marked);
    f.action(HistoryAction::Undo);
    f.clean();
    f.action(HistoryAction::Redo);
    assert_eq!(f.session().effect, marked);
    f.action(HistoryAction::Redo);
    assert_eq!(f.session().effect, colored);
}

#[test]
fn classified_and_native_buttons_pass_actual_control_audit() {
    let mut f = Fixture::new();
    f.app.world_mut().spawn((
        super::super::timeline_pointer::TimelineButton,
        TimelineAction::AddMarker,
        FeathersActionButton,
    ));
    f.app.world_mut().spawn((
        super::super::timeline_pointer::TimelineButton,
        TimelineAction::AddMarker,
        EditorNativeControl,
    ));
    f.app
        .world_mut()
        .run_system_once(audit_timeline_controls)
        .unwrap();
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "must use FeathersActionButton")]
fn unclassified_timeline_button_fails_actual_audit() {
    let mut f = Fixture::new();
    f.app.world_mut().spawn((
        super::super::timeline_pointer::TimelineButton,
        TimelineAction::AddMarker,
    ));
    f.app
        .world_mut()
        .run_system_once(audit_timeline_controls)
        .unwrap();
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "must use FeathersActionButton")]
fn unclassified_choreography_button_fails_actual_audit() {
    let mut f = Fixture::new();
    let emitter = f.emitter();
    f.app.world_mut().spawn((
        super::super::timeline_pointer::TimelineButton,
        ChoreographyAction::SelectEmitter(emitter),
    ));
    f.app
        .world_mut()
        .run_system_once(audit_timeline_controls)
        .unwrap();
}
