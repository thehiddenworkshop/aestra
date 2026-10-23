//! Real emitter controls and color/automation consumers on both engines.
use super::super::{
    action_controls, automation::emitter_automation_lanes, execution,
    navigation_cursor::OverrideCursor, timeline_pointer::tests as pointer,
};
use super::*;
use crate::feathers::{
    action_activation::PendingFeathersActivation, color_picker,
    context_menu::PointerContextMenuItem, list_row::KeyboardNavigableList,
};
use crate::{
    CurvesState, EditorModuleRegistry, ProjectEffectCatalog,
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    material_function_editor::FunctionEditor,
    menus::MenuState,
};
use aestra_core::{Diagnostic, DiagnosticCode, EffectAsset, EffectClip, EffectId, Emitter};
use bevy::{
    asset::AssetPlugin,
    input_focus::InputFocus,
    math::Affine2,
    picking::pointer::{PointerButton, PointerId},
    scene::ScenePlugin,
    text::TextPlugin,
    ui_widgets::{Activate, ListBox, ValueChange},
    window::{CursorIcon, PrimaryWindow},
};

#[derive(Resource)]
struct Built(Entity);
#[derive(Resource, Default)]
struct Bubbled(usize);
#[derive(Clone, Copy)]
enum Open {
    None,
    Context,
    Color,
    Automation,
}

fn build(
    mut commands: Commands,
    session: Res<EditorSession>,
    state: Res<TimelineState>,
    localizer: Res<Localizer>,
    registry: Res<EditorModuleRegistry>,
    assets: Res<AssetServer>,
) {
    let root = commands
        .spawn((Node::default(), KeyboardNavigableList, ListBox))
        .observe(
            |_: On<super::super::timeline_pointer::PointerClick>, mut bubbled: ResMut<Bubbled>| {
                bubbled.0 += 1
            },
        )
        .with_children(|parent| {
            for (index, emitter) in session.effect.emitters.iter().enumerate() {
                let lanes =
                    emitter_automation_lanes(&session.effect, emitter, &registry, &localizer);
                spawn_emitter_track_header(
                    parent,
                    &session,
                    &state,
                    &localizer,
                    index,
                    emitter.id,
                    &emitter.name,
                    emitter.enabled,
                    emitter.display_color,
                    (index + 1) as i16,
                    &lanes,
                    &assets,
                );
            }
        })
        .id();
    commands.insert_resource(Built(root));
}

struct Fixture {
    root: tempfile::TempDir,
    app: App,
    window: Entity,
    ids: [EmitterId; 2],
    before: EffectAsset,
    bytes: Vec<u8>,
}
impl Fixture {
    fn new() -> Self {
        Self::configured(Open::None, "en-US", |_, _| {})
    }
    fn open(open: Open) -> Self {
        Self::configured(open, "en-US", |_, _| {})
    }
    fn configured(open: Open, locale: &str, prepare: impl FnOnce(&mut App, EmitterId)) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut effect = EffectAsset::new("Headers", 8.0);
        effect.emitters.extend([
            Emitter::basic_sprite("First", 4.0),
            Emitter::basic_sprite("Second", 4.0),
        ]);
        let ids = [effect.emitters[0].id, effect.emitters[1].id];
        let clip = EffectClip::new(EffectId::new(), 0.0, 2.0);
        effect.choreography_order = vec![
            ChoreographyTrackId::Emitter(ids[0]),
            ChoreographyTrackId::EffectClip(clip.id),
            ChoreographyTrackId::Emitter(ids[1]),
        ];
        effect.effect_clips.push(clip);
        let mut state = TimelineState::framed(8.0);
        match open {
            Open::None => {}
            Open::Context => {
                state.context_emitter = Some(ids[0]);
                state.context_menu_position = Vec2::new(50.0, 15.0);
            }
            Open::Color => state.color_picker_emitter = Some(ids[0]),
            Open::Automation => state.automation_menu_emitter = Some(ids[0]),
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
        .insert_resource(EditorSession::from_test_effect(effect))
        .insert_resource(state)
        .insert_resource(Localizer::new(locale).unwrap())
        .insert_resource(ProjectEffectCatalog::scan(root.path()))
        .init_resource::<EditorModuleRegistry>()
        .init_resource::<CurvesState>()
        .init_resource::<MenuState>()
        .init_resource::<InputFocus>()
        .init_resource::<bevy::feathers::controls::UnitsRegistry>()
        .init_resource::<bevy::feathers::theme::UiTheme>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<Bubbled>()
        .init_resource::<OverrideCursor>()
        .init_resource::<EditorHistoryLedger>()
        .init_resource::<MaterialProgramEditHistory>()
        .init_resource::<FunctionEditor>()
        .add_observer(action_controls::queue_choreography_action_activation)
        .add_observer(action_controls::activate_timeline_track_entry)
        .add_observer(action_controls::handle_timeline_track_color_change)
        .add_observer(execution::execute_choreography_action)
        .add_observer(color_picker::handle_color_plane_change)
        .add_observer(color_picker::handle_color_lightness_change)
        .add_observer(color_picker::handle_color_alpha_change)
        .add_observer(color_picker::handle_color_channel_change)
        .add_observer(color_picker::handle_color_hex_change)
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
                action_controls::handle_choreography_action_buttons,
                action_controls::audit_timeline_controls,
            )
                .chain(),
        );
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow, CursorIcon::default()))
            .id();
        prepare(&mut app, ids[0]);
        let before = app.world().resource::<EditorSession>().effect.clone();
        before
            .save_ron(root.path().join("parent.aestra.ron"))
            .unwrap();
        let bytes = std::fs::read(root.path().join("parent.aestra.ron")).unwrap();
        app.update();
        Self {
            root,
            app,
            window,
            ids,
            before,
            bytes,
        }
    }
    fn event<E: Event>(&mut self, event: E)
    where
        for<'a> E::Trigger<'a>: Default,
    {
        self.app.world_mut().trigger(event);
        self.app.world_mut().flush();
    }
    fn header(&mut self, id: EmitterId) -> Entity {
        let w = self.app.world_mut();
        w.query::<(Entity, &EmitterTrackHeader)>()
            .iter(w)
            .find_map(|(e, h)| (h.emitter == id).then_some(e))
            .unwrap()
    }
    fn control(&mut self, action: ChoreographyAction, menu: bool) -> Entity {
        let w = self.app.world_mut();
        w.query::<(
            Entity,
            &ChoreographyAction,
            Has<TimelineTrackActionControl>,
            Has<PointerContextMenuItem>,
        )>()
        .iter(w)
        .find_map(|(e, a, inline, item)| {
            (*a == action && if menu { item } else { inline }).then_some(e)
        })
        .unwrap()
    }
    fn one<C: Component>(&mut self) -> Entity {
        let w = self.app.world_mut();
        w.query_filtered::<Entity, With<C>>().single(w).unwrap()
    }
    fn click(&mut self, entity: Entity, button: PointerButton) {
        self.event(pointer::click_at(
            entity,
            self.window,
            button,
            1,
            Vec2::new(50.0, 55.0),
        ));
    }
    fn activate(&mut self, entity: Entity) {
        self.event(Activate { entity });
        self.app.update();
    }
    fn lanes(&self) -> Vec<AutomationLaneProjection> {
        let w = self.app.world();
        let session = w.resource::<EditorSession>();
        emitter_automation_lanes(
            &session.effect,
            &session.effect.emitters[0],
            w.resource::<EditorModuleRegistry>(),
            w.resource::<Localizer>(),
        )
    }
    fn clean(&self) {
        let session = self.app.world().resource::<EditorSession>();
        assert_eq!(session.effect, self.before);
        assert!(!session.dirty);
        assert!(!session.can_undo());
        self.disk();
    }
    fn disk(&self) {
        assert_eq!(
            std::fs::read(self.root.path().join("parent.aestra.ron")).unwrap(),
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
        self.event(HistoryAction::Redo);
        assert_eq!(self.app.world().resource::<EditorSession>().effect, after);
        self.disk();
    }
}

#[test]
fn actual_headers_keep_list_native_action_and_grid_identity() {
    let mut f = Fixture::new();
    for (i, id) in f.ids.into_iter().enumerate() {
        let e = f.header(id);
        let w = f.app.world();
        assert!(w.get::<Button>(e).is_some());
        assert!(w.get::<EditorNativeControl>(e).is_some());
        assert!(w.get::<ListItem>(e).is_some());
        assert!(w.get::<KeyboardNavigableListRow>(e).is_some());
        assert_eq!(
            w.get::<ChoreographyAction>(e),
            Some(&ChoreographyAction::SelectEmitter(id))
        );
        assert_eq!(w.get::<Node>(e).unwrap().height, Val::Px(31.0));
        assert_eq!(
            w.get::<Node>(e).unwrap().grid_row,
            GridPlacement::start((i + 1) as i16)
        );
    }
    f.clean();
}

#[test]
fn current_multi_selection_and_stale_selection_use_production_fill_policy() {
    for current in [true, false] {
        let mut f = Fixture::configured(Open::None, "en-US", |app, id| {
            let second = app.world().resource::<EditorSession>().effect.emitters[1].id;
            app.world_mut()
                .resource_mut::<EditorSession>()
                .select_emitter(if current { id } else { second });
            app.world_mut()
                .resource_mut::<TimelineState>()
                .selected_emitters
                .insert(id);
        });
        let e = f.header(f.ids[0]);
        assert_eq!(f.app.world().get::<Selected>(e).is_some(), current);
        assert_eq!(
            f.app.world().get::<BackgroundColor>(e).unwrap().0,
            if current {
                theme::SELECTION
            } else {
                theme::PANEL_DARK
            }
        );
        f.clean();
    }
}

#[test]
fn disabled_solo_diagnostic_header_and_inverse_labels_follow_both_locales() {
    for locale in ["en-US", "fr-FR"] {
        let mut f = Fixture::configured(Open::None, locale, |app, id| {
            let mut session = app.world_mut().resource_mut::<EditorSession>();
            session.effect.emitters[0].enabled = false;
            session.toggle_preview_solo(id);
            session.diagnostics.push(Diagnostic::error(
                DiagnosticCode::InvalidTiming,
                "effect.emitters[0].duration",
                "Test",
            ));
        });
        let header = f.header(f.ids[0]);
        assert!(f.app.world().get::<EmitterTrackDisabled>(header).is_some());
        assert!(
            f.app
                .world()
                .get::<EmitterTrackDiagnostic>(header)
                .is_some()
        );
        let mut args = FluentArgs::new();
        args.set("name", "First");
        args.set(
            "state",
            f.app
                .world()
                .resource::<Localizer>()
                .text("timeline-emitter-muted-soloed-status"),
        );
        assert_eq!(
            f.app.world().get::<AccessibleLabel>(header).unwrap().0,
            f.app
                .world()
                .resource::<Localizer>()
                .text_with("timeline-select-emitter-status", &args)
        );
        for (action, key) in [
            (
                ChoreographyAction::SetEmitterEnabled {
                    emitter: f.ids[0],
                    enabled: true,
                },
                "timeline-unmute-emitter",
            ),
            (
                ChoreographyAction::ToggleEmitterSolo(f.ids[0]),
                "timeline-unsolo-emitter",
            ),
        ] {
            let e = f.control(action, false);
            assert!(f.app.world().get::<Selected>(e).is_some());
            assert_eq!(
                f.app.world().get::<ButtonVariant>(e),
                Some(&ButtonVariant::Primary)
            );
            assert_eq!(
                f.app.world().get::<AccessibleLabel>(e).unwrap().0,
                f.app.world().resource::<Localizer>().text(key)
            );
        }
        f.clean();
    }
}

#[test]
fn all_inline_actions_have_native_click_classification_not_feathers_activation() {
    let mut f = Fixture::new();
    let id = f.ids[0];
    for a in [
        ChoreographyAction::SetEmitterEnabled {
            emitter: id,
            enabled: false,
        },
        ChoreographyAction::ToggleEmitterSolo(id),
        ChoreographyAction::ToggleEmitterColorPicker(id),
        ChoreographyAction::ToggleEmitterAutomation(id),
    ] {
        let e = f.control(a, false);
        let w = f.app.world();
        assert!(w.get::<EditorNativeControl>(e).is_some());
        assert!(w.get::<FeathersActionButton>(e).is_none());
        assert!(w.get::<Button>(e).is_none());
    }
    f.clean();
}

#[test]
fn inline_mute_is_one_semantic_edit_with_global_undo() {
    let mut f = Fixture::new();
    let id = f.ids[0];
    let e = f.control(
        ChoreographyAction::SetEmitterEnabled {
            emitter: id,
            enabled: false,
        },
        false,
    );
    f.click(e, PointerButton::Primary);
    assert!(!f.app.world().resource::<EditorSession>().effect.emitters[0].enabled);
    assert_eq!(f.app.world().resource::<Bubbled>().0, 0);
    f.roundtrip();
}

#[test]
fn inline_solo_is_preview_only_and_clears_effect_clip_solo() {
    let mut f = Fixture::new();
    let id = f.ids[0];
    let clip = f.before.effect_clips[0].id;
    f.app
        .world_mut()
        .resource_mut::<TimelineState>()
        .solo_effect_clip = Some(clip);
    let e = f.control(ChoreographyAction::ToggleEmitterSolo(id), false);
    f.click(e, PointerButton::Primary);
    assert_eq!(
        f.app.world().resource::<EditorSession>().solo_emitter,
        Some(id)
    );
    assert_eq!(
        f.app.world().resource::<TimelineState>().solo_effect_clip,
        None
    );
    f.clean();
}

#[test]
fn color_and_automation_inline_clicks_open_the_actual_transient_state_only() {
    let mut f = Fixture::new();
    let id = f.ids[0];
    let color = f.control(ChoreographyAction::ToggleEmitterColorPicker(id), false);
    f.click(color, PointerButton::Primary);
    assert_eq!(
        f.app
            .world()
            .resource::<TimelineState>()
            .color_picker_emitter,
        Some(id)
    );
    let automation = f.control(ChoreographyAction::ToggleEmitterAutomation(id), false);
    f.click(automation, PointerButton::Primary);
    let state = f.app.world().resource::<TimelineState>();
    assert_eq!(state.color_picker_emitter, None);
    assert_eq!(state.automation_menu_emitter, Some(id));
    assert!(state.expanded_automation_emitters.contains(&id));
    f.clean();
}

#[test]
fn injected_activate_does_not_execute_native_inline_actions() {
    let mut f = Fixture::new();
    let id = f.ids[0];
    let e = f.control(
        ChoreographyAction::SetEmitterEnabled {
            emitter: id,
            enabled: false,
        },
        false,
    );
    f.activate(e);
    assert!(f.app.world().get::<PendingFeathersActivation>(e).is_none());
    f.clean();
}

#[test]
fn secondary_inline_click_resolves_header_geometry_selection_and_context() {
    let mut f = Fixture::new();
    let id = f.ids[0];
    let header = f.header(id);
    f.app.world_mut().entity_mut(header).insert((
        ComputedNode {
            size: Vec2::new(200.0, 40.0),
            ..default()
        },
        UiGlobalTransform::from(Affine2::from_translation(Vec2::new(100.0, 60.0))),
    ));
    let e = f.control(ChoreographyAction::ToggleEmitterSolo(id), false);
    f.click(e, PointerButton::Secondary);
    let state = f.app.world().resource::<TimelineState>();
    assert_eq!(state.context_emitter, Some(id));
    assert_eq!(state.context_menu_position, Vec2::new(50.0, 15.0));
    assert_eq!(
        f.app
            .world()
            .resource::<EditorSession>()
            .selection
            .emitter(&f.before),
        Some(id)
    );
    assert_eq!(f.app.world().resource::<Bubbled>().0, 0);
    f.clean();
}

#[test]
fn context_menu_contains_real_host_payload_and_region_sensitive_actions() {
    for regions in [false, true] {
        let mut f = Fixture::configured(Open::Context, "en-US", |app, id| {
            if regions {
                let region = aestra_core::EmitterRegion::new(0.0, 0.0, 1.0);
                let region_id = region.id;
                app.world_mut()
                    .resource_mut::<EditorSession>()
                    .effect
                    .emitters[0]
                    .regions
                    .push(region);
                app.world_mut()
                    .resource_mut::<TimelineState>()
                    .selected_emitter_regions
                    .insert((id, region_id));
            }
        });
        let id = f.ids[0];
        for a in [
            ChoreographyAction::SetEmitterEnabled {
                emitter: id,
                enabled: false,
            },
            ChoreographyAction::ToggleEmitterSolo(id),
            if regions {
                ChoreographyAction::DuplicateSelectedEmitterRegions
            } else {
                ChoreographyAction::DuplicateEmitter(Some(id))
            },
            if regions {
                ChoreographyAction::DeleteSelectedEmitterRegions
            } else {
                ChoreographyAction::DeleteEmitter(Some(id))
            },
        ] {
            let e = f.control(a, true);
            assert!(f.app.world().get::<FeathersActionButton>(e).is_some());
        }
        let w = f.app.world_mut();
        assert_eq!(
            *w.query::<&crate::asset_actions::AssetAction>()
                .single(w)
                .unwrap(),
            crate::asset_actions::AssetAction::CreateReusableEffectFromSelection
        );
        assert_eq!(
            w.query_filtered::<Entity, With<PointerContextMenuItem>>()
                .iter(w)
                .count(),
            5
        );
        f.clean();
    }
}

#[test]
fn menu_duplicate_uses_actual_semantics_and_one_undo() {
    let mut f = Fixture::open(Open::Context);
    let e = f.control(ChoreographyAction::DuplicateEmitter(Some(f.ids[0])), true);
    f.activate(e);
    assert_eq!(
        f.app
            .world()
            .resource::<EditorSession>()
            .effect
            .emitters
            .len(),
        3
    );
    f.roundtrip();
}

#[test]
fn context_mute_activation_is_consumed_once_and_does_not_replay() {
    let mut f = Fixture::open(Open::Context);
    let e = f.control(
        ChoreographyAction::SetEmitterEnabled {
            emitter: f.ids[0],
            enabled: false,
        },
        true,
    );
    f.activate(e);
    assert!(f.app.world().get::<PendingFeathersActivation>(e).is_none());
    let after = f.app.world().resource::<EditorSession>().effect.clone();
    f.app.update();
    assert_eq!(f.app.world().resource::<EditorSession>().effect, after);
    f.roundtrip();
}

#[test]
fn real_color_popover_contains_full_picker_and_preserves_placement_contract() {
    let mut f = Fixture::configured(Open::Color, "fr-FR", |app, _| {
        app.world_mut()
            .resource_mut::<EditorSession>()
            .effect
            .emitters[0]
            .display_color = Some([0.2, 0.4, 0.6, 0.8])
    });
    let popup = f.one::<EmitterTrackColorPickerPopover>();
    let root = f.one::<EmitterTrackColorPicker>();
    let w = f.app.world();
    assert!(w.get::<color_picker::ColorPickerRoot>(root).is_some());
    assert!(w.get::<color_picker::ColorPickerState>(root).is_some());
    assert_eq!(w.get::<Node>(popup).unwrap().width, Val::Px(258.0));
    assert_eq!(w.get::<GlobalZIndex>(popup).unwrap().0, 260);
    let p = w.get::<Popover>(popup).unwrap();
    assert_eq!(p.window_margin, 9.0);
    assert_eq!(p.positions.len(), 6);
    assert_eq!(p.positions[0].side, PopoverSide::Right);
    assert_eq!(p.positions[0].align, PopoverAlign::End);
    assert_eq!(
        w.get::<AccessibleLabel>(root).unwrap().0,
        w.resource::<Localizer>()
            .text("timeline-color-picker-accessible")
    );
    f.one::<color_picker::ColorPickerPlane>();
    f.one::<color_picker::ColorPickerHexInput>();
    f.one::<color_picker::ColorPickerAutomatic>();
    f.clean();
}

#[test]
fn real_picker_live_alpha_change_previews_only_then_final_commit_is_reversible() {
    let mut f = Fixture::open(Open::Color);
    let alpha = f.one::<color_picker::ColorPickerAlpha>();
    f.event(ValueChange {
        source: alpha,
        value: 0.3_f32,
        is_final: false,
    });
    f.clean();
    let w = f.app.world_mut();
    let swatch = w
        .query::<(&EmitterTrackColorSwatch, &BackgroundColor)>()
        .iter(w)
        .find(|(s, _)| s.emitter == f.ids[0])
        .unwrap()
        .1
        .0;
    assert!((swatch.alpha() - 0.3).abs() < 0.001);
    f.event(ValueChange {
        source: alpha,
        value: 0.3_f32,
        is_final: true,
    });
    assert!(
        (f.app.world().resource::<EditorSession>().effect.emitters[0]
            .display_color
            .unwrap()[3]
            - 0.3)
            .abs()
            < 0.001
    );
    f.roundtrip();
}

#[test]
fn actual_automatic_control_restores_none_with_real_history() {
    let mut f = Fixture::configured(Open::Color, "en-US", |app, _| {
        app.world_mut()
            .resource_mut::<EditorSession>()
            .effect
            .emitters[0]
            .display_color = Some([0.2, 0.4, 0.6, 0.8])
    });
    let automatic = f.one::<color_picker::ColorPickerAutomatic>();
    f.activate(automatic);
    assert_eq!(
        f.app.world().resource::<EditorSession>().effect.emitters[0].display_color,
        None
    );
    f.roundtrip();
}

#[test]
fn automation_menu_builds_exact_registry_lanes_and_action_ids() {
    let mut f = Fixture::open(Open::Automation);
    let lanes = f.lanes();
    assert!(!lanes.is_empty());
    let id = f.ids[0];
    f.one::<EmitterAutomationVisibilityMenu>();
    f.one::<EmitterAutomationVisibilityMenuAnchor>();
    for lane in &lanes {
        let e = f.control(
            ChoreographyAction::SetAutomationLaneVisibility {
                lane: lane.id.clone(),
                visible: true,
            },
            true,
        );
        assert!(
            f.app
                .world()
                .get::<AccessibleLabel>(e)
                .unwrap()
                .0
                .contains(&lane.label)
        );
    }
    let w = f.app.world_mut();
    assert_eq!(
        w.query_filtered::<Entity, With<PointerContextMenuItem>>()
            .iter(w)
            .count(),
        lanes.len() + 2
    );
    let _ = f.control(
        ChoreographyAction::SetEmitterAutomationVisibility {
            emitter: id,
            lanes: lanes.into_iter().map(|l| l.id).collect(),
            visible: true,
        },
        true,
    );
    f.clean();
}

#[test]
fn automation_show_all_and_single_hide_are_preview_only() {
    let mut f = Fixture::open(Open::Automation);
    let lanes = f.lanes();
    let ids = lanes.iter().map(|l| l.id.clone()).collect::<Vec<_>>();
    let e = f.control(
        ChoreographyAction::SetEmitterAutomationVisibility {
            emitter: f.ids[0],
            lanes: ids.clone(),
            visible: true,
        },
        true,
    );
    f.activate(e);
    assert!(ids.iter().all(|id| {
        f.app
            .world()
            .resource::<TimelineState>()
            .visible_automation_lanes
            .contains(id)
    }));
    // The existing popup is retained by this bounded fixture; use its real inverse action via a visibility-initialized build below.
    f.clean();
    let mut hidden = Fixture::configured(Open::Automation, "en-US", |app, _| {
        let lanes = {
            let w = app.world();
            let s = w.resource::<EditorSession>();
            emitter_automation_lanes(
                &s.effect,
                &s.effect.emitters[0],
                w.resource::<EditorModuleRegistry>(),
                w.resource::<Localizer>(),
            )
        };
        app.world_mut()
            .resource_mut::<TimelineState>()
            .visible_automation_lanes
            .extend(lanes.into_iter().map(|l| l.id));
    });
    let lane = hidden.lanes()[0].id.clone();
    let e = hidden.control(
        ChoreographyAction::SetAutomationLaneVisibility {
            lane: lane.clone(),
            visible: false,
        },
        true,
    );
    hidden.activate(e);
    assert!(
        !hidden
            .app
            .world()
            .resource::<TimelineState>()
            .visible_automation_lanes
            .contains(&lane)
    );
    hidden.clean();
}

#[test]
fn no_automation_lanes_keep_spacer_without_a_fake_button() {
    let mut f = Fixture::configured(Open::None, "en-US", |app, _| {
        let mut session = app.world_mut().resource_mut::<EditorSession>();
        for module in &mut session.effect.emitters[0].modules {
            for name in ["size", "opacity", "color"] {
                if module.parameter_value(name).is_some() {
                    module
                        .property_sources
                        .insert(name.into(), aestra_core::PropertySource::Constant);
                }
            }
        }
    });
    assert!(f.lanes().is_empty());
    let header = f.header(f.ids[0]);
    let w = f.app.world();
    assert!(
        w.get::<Children>(header)
            .unwrap()
            .iter()
            .all(|e| w.get::<EmitterAutomationMenuButton>(e).is_none())
    );
    f.clean();
}

#[test]
fn attached_reorder_controls_roundtrip_mixed_semantic_order() {
    let mut f = Fixture::new();
    let w = f.app.world_mut();
    let handle = w
        .query::<(Entity, &EmitterTrackReorderHandle)>()
        .iter(w)
        .find_map(|(e, h)| (h.emitter == f.ids[0]).then_some(e))
        .unwrap();
    assert_eq!(w.get::<Node>(handle).unwrap().width, Val::Px(18.0));
    let icon = w.get::<Children>(handle).unwrap()[0];
    assert!(w.get::<UiSvg>(icon).is_some());
    f.event(pointer::start(
        handle,
        f.window,
        Vec2::ZERO,
        PointerId::Mouse,
        PointerButton::Primary,
    ));
    assert_eq!(
        f.app.world().resource::<TimelineState>().reorder_drag,
        Some(f.ids[0])
    );
    let target = f.header(f.ids[1]);
    f.app
        .world_mut()
        .entity_mut(target)
        .insert(RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::new(0.0, 0.2)),
        });
    f.event(pointer::drop_on(
        target,
        handle,
        f.window,
        PointerButton::Primary,
    ));
    assert_eq!(
        f.app
            .world()
            .resource::<EditorSession>()
            .effect
            .choreography_order
            .last(),
        Some(&ChoreographyTrackId::Emitter(f.ids[0]))
    );
    f.event(pointer::end(
        handle,
        f.window,
        Vec2::ZERO,
        PointerId::Mouse,
        PointerButton::Primary,
    ));
    f.roundtrip();
}

#[test]
fn actual_list_value_change_selects_header_without_editing() {
    let mut f = Fixture::new();
    let header = f.header(f.ids[1]);
    let root = f.app.world().resource::<Built>().0;
    f.event(ValueChange {
        source: root,
        value: header,
        is_final: true,
    });
    assert_eq!(
        f.app
            .world()
            .resource::<EditorSession>()
            .selection
            .emitter(&f.before),
        Some(f.ids[1])
    );
    f.clean();
}
