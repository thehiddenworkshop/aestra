//! Production outer headers and popup items; explicit geometry is not native layout acceptance.
use super::super::navigation_cursor::OverrideCursor;
use super::super::{action_controls, execution, timeline_pointer::tests as pointer};
use super::*;
use crate::feathers::{
    action_activation::{FeathersActionButton, PendingFeathersActivation},
    context_menu::{PointerContextMenuAnchor, PointerContextMenuItem, PointerContextMenuSurface},
    list_row::KeyboardNavigableList,
};
use crate::{
    CurvesState, ProjectEffectCatalog,
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    material_function_editor::FunctionEditor,
    menus::MenuState,
    persistence::DocumentAction,
};
use aestra_core::{EffectAsset, Emitter};
use bevy::{
    asset::AssetPlugin,
    math::Affine2,
    picking::pointer::{PointerButton, PointerId},
    scene::ScenePlugin,
    text::TextPlugin,
    ui_widgets::{Activate, ListBox, MenuPopup, ValueChange, popover::Popover},
    window::{CursorIcon, PrimaryWindow},
};

#[derive(Resource, Default)]
struct Intents(Vec<DocumentAction>);
#[derive(Resource, Default)]
struct Bubbled(usize);
#[derive(Resource)]
struct Built(Entity);

fn build(
    mut commands: Commands,
    session: Res<EditorSession>,
    state: Res<TimelineState>,
    localizer: Res<Localizer>,
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
            for (index, clip) in session.effect.effect_clips.iter().enumerate() {
                spawn_effect_clip_track_header(
                    parent,
                    &session,
                    &state,
                    &localizer,
                    clip,
                    "Source",
                    3,
                    (index + 1) as i16,
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
    before: EffectAsset,
    bytes: Vec<u8>,
    ids: [EffectClipId; 2],
}
impl Fixture {
    fn new() -> Self {
        Self::configured("en-US", false, |_, _| {})
    }
    fn menu() -> Self {
        Self::configured("en-US", true, |_, _| {})
    }
    fn configured(locale: &str, menu: bool, prepare: impl FnOnce(&mut App, EffectClipId)) -> Self {
        let root = tempfile::tempdir().unwrap();
        let source = EffectAsset::new("Source", 4.0);
        source
            .save_ron(root.path().join("source.aestra.ron"))
            .unwrap();
        let bytes = std::fs::read(root.path().join("source.aestra.ron")).unwrap();
        let mut effect = EffectAsset::new("Parent", 8.0);
        effect.emitters.push(Emitter::basic_sprite("Root", 8.0));
        effect.effect_clips.extend([
            EffectClip::new(source.id, 1.0, 2.0),
            EffectClip::new(source.id, 4.0, 2.0),
        ]);
        let ids = [effect.effect_clips[0].id, effect.effect_clips[1].id];
        effect.choreography_order = vec![
            ChoreographyTrackId::EffectClip(ids[0]),
            ChoreographyTrackId::Emitter(effect.emitters[0].id),
            ChoreographyTrackId::EffectClip(ids[1]),
        ];
        let mut state = TimelineState::framed(8.0);
        if menu {
            state.context_effect_clip = Some(ids[0]);
            state.context_menu_position = Vec2::new(50.0, 15.0);
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
        .insert_resource(EditorSession::from_test_effect(effect.clone()))
        .insert_resource(state)
        .insert_resource(Localizer::new(locale).unwrap())
        .insert_resource(ProjectEffectCatalog::scan(root.path()))
        .init_resource::<CurvesState>()
        .init_resource::<MenuState>()
        .init_resource::<Intents>()
        .init_resource::<Bubbled>()
        .init_resource::<OverrideCursor>()
        .init_resource::<EditorHistoryLedger>()
        .init_resource::<MaterialProgramEditHistory>()
        .init_resource::<FunctionEditor>()
        .add_observer(action_controls::queue_choreography_action_activation)
        .add_observer(action_controls::activate_timeline_track_entry)
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
        app.update();
        Self {
            root,
            app,
            window,
            before: effect,
            bytes,
            ids,
        }
    }
    fn header(&mut self, id: EffectClipId) -> Entity {
        let world = self.app.world_mut();
        world
            .query::<(Entity, &EffectClipTrackHeader)>()
            .iter(world)
            .find_map(|(entity, header)| (header.clip == id).then_some(entity))
            .unwrap()
    }
    fn control(&mut self, action: ChoreographyAction, menu: bool) -> Entity {
        let world = self.app.world_mut();
        world
            .query::<(
                Entity,
                &ChoreographyAction,
                Has<PointerContextMenuItem>,
                Has<TimelineTrackActionControl>,
            )>()
            .iter(world)
            .find_map(|(entity, found, item, inline)| {
                (*found == action && if menu { item } else { inline }).then_some(entity)
            })
            .unwrap()
    }
    fn handle(&mut self) -> Entity {
        let world = self.app.world_mut();
        world
            .query::<(Entity, &EffectClipTrackReorderHandle)>()
            .iter(world)
            .find_map(|(entity, handle)| (handle.clip == self.ids[0]).then_some(entity))
            .unwrap()
    }
    fn event<E: Event>(&mut self, event: E)
    where
        for<'a> E::Trigger<'a>: Default,
    {
        self.app.world_mut().trigger(event);
        self.app.world_mut().flush();
    }
    fn click(&mut self, entity: Entity, button: PointerButton, count: u8) {
        self.event(pointer::click_at(
            entity,
            self.window,
            button,
            count,
            Vec2::new(50.0, 55.0),
        ));
    }
    fn activate(&mut self, entity: Entity) {
        self.event(Activate { entity });
        self.app.update();
    }
    fn clean(&self) {
        let session = self.app.world().resource::<EditorSession>();
        assert_eq!(session.effect, self.before);
        assert!(!session.dirty);
        assert!(!session.can_undo());
        self.disk_unchanged();
    }
    fn disk_unchanged(&self) {
        assert_eq!(
            std::fs::read(self.root.path().join("source.aestra.ron")).unwrap(),
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
        self.disk_unchanged();
    }
}

#[test]
fn headers_retain_native_list_identity_and_grid_geometry() {
    let mut f = Fixture::new();
    for (index, id) in f.ids.into_iter().enumerate() {
        let header = f.header(id);
        let world = f.app.world();
        assert!(world.get::<Button>(header).is_some());
        assert!(world.get::<EditorNativeControl>(header).is_some());
        assert!(world.get::<ListItem>(header).is_some());
        assert!(world.get::<KeyboardNavigableListRow>(header).is_some());
        assert!(world.get::<FeathersActionButton>(header).is_none());
        assert_eq!(world.get::<Node>(header).unwrap().height, Val::Px(31.0));
        assert_eq!(
            world.get::<Node>(header).unwrap().grid_row,
            GridPlacement::start((index + 1) as i16)
        );
        assert_eq!(
            world.get::<ChoreographyAction>(header),
            Some(&ChoreographyAction::SelectEffectClip(id))
        );
    }
    f.clean();
}

#[test]
fn selected_header_uses_selected_marker_and_fill() {
    let mut f = Fixture::configured("en-US", false, |app, id| {
        app.world_mut()
            .resource_mut::<EditorSession>()
            .select_effect_clip(id);
    });
    let header = f.header(f.ids[0]);
    assert!(f.app.world().get::<Selected>(header).is_some());
    assert_eq!(
        f.app.world().get::<BackgroundColor>(header).unwrap().0,
        theme::SELECTION
    );
    let other = f.header(f.ids[1]);
    assert!(f.app.world().get::<Selected>(other).is_none());
    assert_eq!(
        f.app.world().get::<BackgroundColor>(other).unwrap().0,
        theme::PANEL_DARK
    );
    f.clean();
}

#[test]
fn source_name_count_and_accessibility_come_from_production_builder() {
    let mut f = Fixture::new();
    let header = f.header(f.ids[0]);
    assert!(
        f.app
            .world()
            .get::<AccessibleLabel>(header)
            .unwrap()
            .0
            .contains("Source")
    );
    let world = f.app.world_mut();
    let labels: Vec<_> = world
        .query::<(&Text, &ChildOf)>()
        .iter(world)
        .filter(|(_, parent)| parent.parent() == header)
        .map(|(text, _)| text.0.as_str())
        .collect();
    assert!(labels.contains(&"Source"));
    assert!(labels.contains(&"3"));
    f.clean();
}

#[test]
fn inline_controls_are_explicit_native_controls_not_feathers_activation_targets() {
    let mut f = Fixture::new();
    for action in [
        ChoreographyAction::ToggleEffectClipExpanded(EffectClipPath::root_path(f.ids[0])),
        ChoreographyAction::ToggleEffectClipMuted(f.ids[0]),
        ChoreographyAction::ToggleEffectClipSolo(f.ids[0]),
    ] {
        let entity = f.control(action, false);
        let world = f.app.world();
        assert!(world.get::<EditorNativeControl>(entity).is_some());
        assert!(world.get::<Button>(entity).is_none());
        assert!(world.get::<FeathersActionButton>(entity).is_none());
    }
    f.clean();
}

#[test]
fn inline_mute_click_changes_preview_once_and_stops_bubbling() {
    let mut f = Fixture::new();
    let id = f.ids[0];
    let control = f.control(ChoreographyAction::ToggleEffectClipMuted(id), false);
    f.click(control, PointerButton::Primary, 1);
    assert!(
        f.app
            .world()
            .resource::<TimelineState>()
            .muted_effect_clips
            .contains(&id)
    );
    assert_eq!(f.app.world().resource::<Bubbled>().0, 0);
    f.click(control, PointerButton::Primary, 1);
    assert!(
        !f.app
            .world()
            .resource::<TimelineState>()
            .muted_effect_clips
            .contains(&id)
    );
    f.clean();
}

#[test]
fn inline_solo_click_changes_only_preview_state() {
    let mut f = Fixture::new();
    let id = f.ids[0];
    let control = f.control(ChoreographyAction::ToggleEffectClipSolo(id), false);
    f.click(control, PointerButton::Primary, 1);
    assert_eq!(
        f.app.world().resource::<TimelineState>().solo_effect_clip,
        Some(id)
    );
    f.click(control, PointerButton::Primary, 1);
    assert_eq!(
        f.app.world().resource::<TimelineState>().solo_effect_clip,
        None
    );
    f.clean();
}

#[test]
fn disclosure_click_changes_actual_root_path_expansion() {
    let mut f = Fixture::new();
    let path = EffectClipPath::root_path(f.ids[0]);
    let control = f.control(
        ChoreographyAction::ToggleEffectClipExpanded(path.clone()),
        false,
    );
    f.click(control, PointerButton::Primary, 1);
    assert!(
        f.app
            .world()
            .resource::<TimelineState>()
            .expanded_effect_clips
            .contains(&path)
    );
    f.click(control, PointerButton::Primary, 1);
    assert!(
        !f.app
            .world()
            .resource::<TimelineState>()
            .expanded_effect_clips
            .contains(&path)
    );
    f.clean();
}

#[test]
fn injected_feathers_activation_does_not_double_dispatch_inline_controls() {
    let mut f = Fixture::new();
    let id = f.ids[0];
    for action in [
        ChoreographyAction::ToggleEffectClipMuted(id),
        ChoreographyAction::ToggleEffectClipSolo(id),
    ] {
        let entity = f.control(action, false);
        f.activate(entity);
        assert!(
            f.app
                .world()
                .get::<PendingFeathersActivation>(entity)
                .is_none()
        );
    }
    assert!(
        f.app
            .world()
            .resource::<TimelineState>()
            .muted_effect_clips
            .is_empty()
    );
    assert_eq!(
        f.app.world().resource::<TimelineState>().solo_effect_clip,
        None
    );
    f.clean();
}

#[test]
fn active_preview_controls_have_inverse_localized_labels_and_primary_style() {
    for locale in ["en-US", "fr-FR"] {
        let mut f = Fixture::configured(locale, true, |app, id| {
            let mut state = app.world_mut().resource_mut::<TimelineState>();
            state.muted_effect_clips.insert(id);
            state.solo_effect_clip = Some(id);
            state
                .expanded_effect_clips
                .insert(EffectClipPath::root_path(id));
        });
        for (action, key) in [
            (
                ChoreographyAction::ToggleEffectClipMuted(f.ids[0]),
                "timeline-unmute-effect-clip",
            ),
            (
                ChoreographyAction::ToggleEffectClipSolo(f.ids[0]),
                "timeline-unsolo-effect-clip",
            ),
        ] {
            let control = f.control(action, false);
            let world = f.app.world();
            assert!(world.get::<Selected>(control).is_some());
            assert_eq!(
                world.get::<ButtonVariant>(control),
                Some(&ButtonVariant::Primary)
            );
            assert_eq!(
                world.get::<AccessibleLabel>(control).unwrap().0,
                world.resource::<Localizer>().text(key)
            );
        }
        f.clean();
    }
}

#[test]
fn secondary_inline_click_opens_real_header_context_without_muting() {
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
    let control = f.control(ChoreographyAction::ToggleEffectClipMuted(id), false);
    f.click(control, PointerButton::Secondary, 1);
    let state = f.app.world().resource::<TimelineState>();
    assert_eq!(state.context_effect_clip, Some(id));
    assert_eq!(state.context_menu_position, Vec2::new(50.0, 15.0));
    assert!(state.muted_effect_clips.is_empty());
    assert_eq!(
        f.app.world().resource::<EditorSession>().selection.primary,
        SemanticTarget::EffectClip(id)
    );
    assert_eq!(f.app.world().resource::<Bubbled>().0, 0);
    f.clean();
}

#[test]
fn real_popup_retains_anchor_placement_and_window_margin_contract() {
    let mut f = Fixture::menu();
    let world = f.app.world_mut();
    let anchor = world
        .query_filtered::<Entity, (
            With<TimelineTrackContextMenuAnchor>,
            With<PointerContextMenuAnchor>,
        )>()
        .single(world)
        .unwrap();
    let node = world.get::<Node>(anchor).unwrap();
    assert_eq!(node.left, Val::Px(50.0));
    assert_eq!(node.top, Val::Px(15.0));
    let menu = world
        .query_filtered::<Entity, With<EffectClipTrackContextMenu>>()
        .single(world)
        .unwrap();
    assert!(world.get::<PointerContextMenuSurface>(menu).is_some());
    assert!(world.get::<MenuPopup>(menu).is_some());
    assert_eq!(world.get::<Node>(menu).unwrap().width, Val::Px(184.0));
    assert_eq!(world.get::<GlobalZIndex>(menu).unwrap().0, 250);
    let popover = world.get::<Popover>(menu).unwrap();
    assert_eq!(popover.window_margin, 8.0);
    assert_eq!(popover.positions.len(), 4);
    f.clean();
}

#[test]
fn popup_contains_exact_production_action_identities() {
    let mut f = Fixture::menu();
    let id = f.ids[0];
    for action in [
        ChoreographyAction::EditEffectClipSource(id),
        ChoreographyAction::ToggleEffectClipMuted(id),
        ChoreographyAction::ToggleEffectClipSolo(id),
        ChoreographyAction::DeleteEffectClip(id),
    ] {
        let entity = f.control(action, true);
        assert!(f.app.world().get::<FeathersActionButton>(entity).is_some());
        assert!(
            !f.app
                .world()
                .get::<AccessibleLabel>(entity)
                .unwrap()
                .0
                .is_empty()
        );
    }
    let world = f.app.world_mut();
    let items = world
        .query_filtered::<Entity, With<PointerContextMenuItem>>()
        .iter(world)
        .count();
    assert_eq!(items, 5);
    let explode = world
        .query::<&crate::asset_actions::AssetAction>()
        .single(world)
        .unwrap();
    assert_eq!(
        *explode,
        crate::asset_actions::AssetAction::ExplodeEffectClip(id)
    );
    f.clean();
}

#[test]
fn menu_click_does_not_bubble_into_header_source_opening() {
    let mut f = Fixture::menu();
    let control = f.control(ChoreographyAction::EditEffectClipSource(f.ids[0]), true);
    f.click(control, PointerButton::Primary, 2);
    assert_eq!(f.app.world().resource::<Bubbled>().0, 0);
    assert!(f.app.world().resource::<Intents>().0.is_empty());
    f.clean();
}

#[test]
fn menu_activation_consumes_pending_mute_once() {
    let mut f = Fixture::menu();
    let id = f.ids[0];
    let entity = f.control(ChoreographyAction::ToggleEffectClipMuted(id), true);
    f.activate(entity);
    assert!(
        f.app
            .world()
            .resource::<TimelineState>()
            .muted_effect_clips
            .contains(&id)
    );
    assert!(
        f.app
            .world()
            .get::<PendingFeathersActivation>(entity)
            .is_none()
    );
    f.app.update();
    assert!(
        f.app
            .world()
            .resource::<TimelineState>()
            .muted_effect_clips
            .contains(&id)
    );
    f.clean();
}

#[test]
fn menu_solo_activation_uses_real_dispatch() {
    let mut f = Fixture::menu();
    let id = f.ids[0];
    let entity = f.control(ChoreographyAction::ToggleEffectClipSolo(id), true);
    f.activate(entity);
    assert_eq!(
        f.app.world().resource::<TimelineState>().solo_effect_clip,
        Some(id)
    );
    f.clean();
}

#[test]
fn menu_source_activation_emits_real_document_intent_without_writing_source() {
    let mut f = Fixture::menu();
    let entity = f.control(ChoreographyAction::EditEffectClipSource(f.ids[0]), true);
    f.activate(entity);
    assert_eq!(
        f.app.world().resource::<Intents>().0,
        vec![DocumentAction::OpenSource(f.before.effect_clips[0].source)]
    );
    assert_eq!(
        f.app
            .world()
            .resource::<TimelineState>()
            .context_effect_clip,
        None
    );
    f.clean();
}

#[test]
fn delete_menu_updates_normalized_order_and_roundtrips_real_history() {
    let mut f = Fixture::configured("en-US", true, |app, id| {
        let mut state = app.world_mut().resource_mut::<TimelineState>();
        state.muted_effect_clips.insert(id);
        state.solo_effect_clip = Some(id);
        state
            .expanded_effect_clips
            .insert(EffectClipPath::root_path(id));
    });
    let id = f.ids[0];
    let entity = f.control(ChoreographyAction::DeleteEffectClip(id), true);
    f.activate(entity);
    let session = f.app.world().resource::<EditorSession>();
    assert_eq!(session.effect.effect_clips.len(), 1);
    assert!(session.dirty);
    assert!(session.can_undo());
    // Shipping removal preserves the authored order vector; its projection filters stale IDs.
    assert_eq!(
        session.effect.choreography_order,
        f.before.choreography_order
    );
    assert!(
        !super::super::state::normalized_choreography_order(&session.effect)
            .contains(&ChoreographyTrackId::EffectClip(id))
    );
    let state = f.app.world().resource::<TimelineState>();
    assert_eq!(state.context_effect_clip, None);
    assert_eq!(state.solo_effect_clip, None);
    assert!(!state.muted_effect_clips.contains(&id));
    assert!(
        !state
            .expanded_effect_clips
            .contains(&EffectClipPath::root_path(id))
    );
    f.roundtrip();
}

#[test]
fn list_selection_dispatches_actual_header_action_without_semantic_edit() {
    let mut f = Fixture::new();
    let id = f.ids[1];
    let entity = f.header(id);
    let root = f.app.world().resource::<Built>().0;
    f.event(ValueChange {
        source: root,
        value: entity,
        is_final: true,
    });
    assert_eq!(
        f.app.world().resource::<EditorSession>().selection.primary,
        SemanticTarget::EffectClip(id)
    );
    f.clean();
}

#[test]
fn double_click_header_routes_real_source_intent() {
    let mut f = Fixture::new();
    let header = f.header(f.ids[0]);
    f.click(header, PointerButton::Primary, 1);
    assert!(f.app.world().resource::<Intents>().0.is_empty());
    f.click(header, PointerButton::Primary, 2);
    assert_eq!(
        f.app.world().resource::<Intents>().0,
        vec![DocumentAction::OpenSource(f.before.effect_clips[0].source)]
    );
    f.clean();
}

#[test]
fn attached_reorder_handle_and_drop_header_make_one_reversible_mixed_order_edit() {
    let mut f = Fixture::new();
    let handle = f.handle();
    assert_eq!(
        f.app.world().get::<Node>(handle).unwrap().width,
        Val::Px(18.0)
    );
    assert_eq!(
        f.app.world().get::<Node>(handle).unwrap().height,
        Val::Px(23.0)
    );
    assert!(f.app.world().get::<EditorNativeControl>(handle).is_some());
    let icon = f.app.world().get::<Children>(handle).unwrap()[0];
    assert!(f.app.world().get::<UiSvg>(icon).is_some());
    assert_eq!(
        f.app.world().get::<Node>(icon).unwrap().height,
        Val::Px(20.0)
    );
    f.event(pointer::start(
        handle,
        f.window,
        Vec2::ZERO,
        PointerId::Mouse,
        PointerButton::Primary,
    ));
    assert_eq!(
        f.app
            .world()
            .resource::<TimelineState>()
            .effect_clip_reorder_drag,
        Some(f.ids[0])
    );
    assert_eq!(
        *f.app.world().get::<CursorIcon>(f.window).unwrap(),
        CursorIcon::System(SystemCursorIcon::Grabbing)
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
        Some(&ChoreographyTrackId::EffectClip(f.ids[0]))
    );
    f.event(pointer::end(
        handle,
        f.window,
        Vec2::ZERO,
        PointerId::Mouse,
        PointerButton::Primary,
    ));
    assert_eq!(
        f.app
            .world()
            .resource::<TimelineState>()
            .effect_clip_reorder_drag,
        None
    );
    f.roundtrip();
}
