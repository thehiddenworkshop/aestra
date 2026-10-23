use super::super::control_input::set_phase;
use super::*;
use crate::{
    document_registry::{DocumentKey, DocumentManager},
    editor_view_registry::{
        ActiveEditorContext, EditorViewKind, EditorViewManager, open_document_view,
    },
    feathers::action_activation::PendingFeathersActivation,
    feathers::code_editor::{ActiveCodeEditor, CodeEditor},
    history::{queue_history_action_activation, routing::Routing},
    material_target::MaterialEditingTarget,
    menus::{MenuKind, TabContextMenu},
    persistence::DocumentProtectionState,
    project_content::io,
};
use aestra_core::material::MaterialProgram;
use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    ui_widgets::Activate,
};

#[derive(Resource, Default)]
struct Events(Vec<HistoryAction>);
#[derive(Resource, Default)]
struct Blocked(bool);
#[derive(Resource, Default)]
struct Repeats(usize);

fn observe(action: On<HistoryAction>, routing: Routing, mut commands: Commands) {
    routing.execute(*action, &mut commands, |_, _, _, _| {
        panic!("filesystem execution is qualified separately")
    });
}
fn shortcuts(
    mut commands: Commands,
    keys: crate::input::ShortcutKeys,
    blocked: Res<Blocked>,
    mut repeats: ResMut<Repeats>,
) {
    repeats.0 = keys.repeats().map_or(0, <[_]>::len);
    crate::history::keyboard::dispatch(&keys, blocked.0, &mut commands);
}

struct Fixture {
    root: tempfile::TempDir,
    app: App,
    programs: [MaterialProgram; 2],
    undo: Entity,
    redo: Entity,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let programs = [
            MaterialProgram::additive_sprite("First").normalized(),
            MaterialProgram::additive_sprite("Second").normalized(),
        ];
        for (index, program) in programs.iter().enumerate() {
            program
                .save_ron(root.path().join(format!("{index}.aestra.material.ron")))
                .unwrap();
        }
        let mut session = crate::test_support::session_with_timing_slack();
        session.adjust_effect_duration(0.25);
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(ProjectEffectCatalog::scan(root.path()))
            .insert_resource(crate::localization::Localizer::new("en-US").unwrap())
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<crate::material_function_editor::FunctionEditor>()
            .init_resource::<MenuState>()
            .init_resource::<Events>()
            .add_observer(queue_history_action_activation)
            .add_observer(observe)
            .add_observer(|event: On<HistoryAction>, mut events: ResMut<Events>| {
                events.0.push(*event)
            })
            .add_systems(
                Update,
                (handle_history_buttons, update_history_availability).chain(),
            );
        let undo = app.world_mut().spawn(UndoMenuItem).id();
        let redo = app.world_mut().spawn(RedoMenuItem).id();
        Self {
            root,
            app,
            programs,
            undo,
            redo,
        }
    }
    fn enabled(&self, entity: Entity) -> bool {
        self.app
            .world()
            .get::<InteractionDisabled>(entity)
            .is_none()
    }
    fn step(&mut self, action: HistoryAction) {
        self.app.world_mut().trigger(action);
        self.app.update();
    }
    fn button(&mut self, feathers: bool) -> Entity {
        let mut entity = self.app.world_mut().spawn((
            HistoryAction::Undo,
            AuditedButton,
            BackgroundColor(theme::BUTTON),
        ));
        if feathers {
            entity.insert(FeathersActionButton);
        } else {
            entity.insert(EditorNativeControl);
        }
        let entity = entity.id();
        set_phase(self.app.world_mut(), entity, Phase::None);
        entity
    }
    fn edit(&mut self, index: usize) {
        let mut session = self
            .app
            .world_mut()
            .remove_resource::<EditorSession>()
            .unwrap();
        let mut catalog = self
            .app
            .world_mut()
            .remove_resource::<ProjectEffectCatalog>()
            .unwrap();
        let before = catalog.material_program(self.programs[index].id).unwrap();
        session.open_material_program(&catalog, before.id).unwrap();
        let mut after = before.clone();
        after.name = "Draft edit".into();
        self.app
            .world_mut()
            .resource_mut::<MaterialProgramEditHistory>()
            .execute_replacement(&mut session, &mut catalog, "Edit", before, after)
            .unwrap();
        self.app
            .world_mut()
            .resource_mut::<EditorHistoryLedger>()
            .record_material_edit(&mut session);
        self.app.insert_resource(session).insert_resource(catalog);
    }
    fn focus(&mut self, index: usize) {
        let mut documents = DocumentManager::default();
        let mut views = EditorViewManager::default();
        let mut active = ActiveEditorContext::default();
        open_document_view(
            &mut documents,
            &mut views,
            &mut active,
            DocumentKey::MaterialProgram(self.programs[index].id),
            EditorViewKind::MaterialGraph,
        );
        self.app
            .insert_resource(documents)
            .insert_resource(views)
            .insert_resource(active);
    }
    fn keyboard(&mut self) -> Entity {
        self.app.add_plugins((
            MinimalPlugins,
            bevy::input::InputPlugin,
            bevy::input_focus::InputFocusPlugin,
            bevy::input_focus::InputDispatchPlugin,
        ));
        crate::input::keypresses::register(&mut self.app);
        self.app
            .init_resource::<Blocked>()
            .init_resource::<Repeats>()
            .add_systems(Update, shortcuts.before(handle_history_buttons));
        self.app
            .world_mut()
            .spawn((Window::default(), bevy::window::PrimaryWindow))
            .id()
    }
    fn key(
        &mut self,
        window: Entity,
        code: KeyCode,
        logical: Key,
        state: ButtonState,
        repeat: bool,
    ) {
        self.app.world_mut().write_message(KeyboardInput {
            key_code: code,
            logical_key: logical,
            state,
            repeat,
            text: None,
            window,
        });
    }
    fn tap(&mut self, window: Entity, code: KeyCode, logical: &str) {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            self.key(window, code, Key::Character(logical.into()), state, false);
        }
    }
}

#[test]
fn only_history_controls_receive_disabled_state() {
    let root = tempfile::tempdir().unwrap();
    let mut app = App::new();
    app.insert_resource(crate::test_support::session_with_timing_slack())
        .insert_resource(ProjectEffectCatalog::scan(root.path()))
        .init_resource::<MaterialProgramEditHistory>()
        .init_resource::<EditorHistoryLedger>()
        .init_resource::<crate::material_function_editor::FunctionEditor>()
        .add_systems(Update, update_history_availability);
    let undo = app.world_mut().spawn(UndoMenuItem).id();
    let unrelated = app
        .world_mut()
        .spawn(BackgroundColor(theme::ACCENT_DIM))
        .id();
    app.update();
    assert!(app.world().get::<InteractionDisabled>(undo).is_some());
    assert!(app.world().get::<InteractionDisabled>(unrelated).is_none());
}

#[test]
fn effect_availability_round_trips_and_optional_catalog_is_not_required() {
    let mut f = Fixture::new();
    f.app.world_mut().remove_resource::<ProjectEffectCatalog>();
    f.app.update();
    assert!(f.enabled(f.undo));
    assert!(!f.enabled(f.redo));
    // Availability itself supports canvas-less/catalog-less hosts. Execution still requires catalog.
}

#[test]
fn real_effect_execution_and_availability_agree_after_undo_and_redo() {
    let mut f = Fixture::new();
    f.app.update();
    assert!(f.enabled(f.undo) && !f.enabled(f.redo));
    f.step(HistoryAction::Undo);
    assert!(!f.enabled(f.undo) && f.enabled(f.redo));
    f.step(HistoryAction::Redo);
    assert!(f.enabled(f.undo) && !f.enabled(f.redo));
}

#[test]
fn focused_material_view_selects_its_own_availability_and_execution() {
    let mut f = Fixture::new();
    f.edit(0);
    f.focus(1);
    f.app.update();
    assert!(!f.enabled(f.undo) && !f.enabled(f.redo));
    f.focus(0);
    f.app.update();
    assert!(f.enabled(f.undo));
    f.step(HistoryAction::Undo);
    assert!(!f.enabled(f.undo) && f.enabled(f.redo));
    assert_eq!(
        f.app
            .world()
            .resource::<ProjectEffectCatalog>()
            .material_program(f.programs[0].id)
            .unwrap(),
        f.programs[0]
    );
    let view = f
        .app
        .world()
        .resource::<ActiveEditorContext>()
        .active_view
        .unwrap();
    f.app
        .world_mut()
        .resource_mut::<EditorViewManager>()
        .close_view(view);
    f.app.update();
    assert!(f.enabled(f.redo)); // Stale focus falls back to the retained session target.
}

#[test]
fn focused_function_availability_uses_actual_function_history() {
    use aestra_core::material::MaterialFunction;
    let mut f = Fixture::new();
    let function = MaterialFunction::from_ron(include_str!(
        "../../../../../assets/test/materials/dissolve_edge.aestra.material-function.ron"
    ))
    .unwrap();
    function
        .save_ron(f.root.path().join("function.aestra.material-function.ron"))
        .unwrap();
    let mut catalog = f
        .app
        .world_mut()
        .remove_resource::<ProjectEffectCatalog>()
        .unwrap();
    catalog.refresh();
    let mut session = f
        .app
        .world_mut()
        .remove_resource::<EditorSession>()
        .unwrap();
    session
        .open_material_function(&catalog, function.id)
        .unwrap();
    let mut after = function.clone();
    after.name = "Function draft".into();
    f.app
        .world_mut()
        .resource_mut::<crate::material_function_editor::FunctionEditor>()
        .edit(&mut session, &mut catalog, after)
        .unwrap();
    f.app.insert_resource(session).insert_resource(catalog);
    f.app.update();
    assert!(f.enabled(f.undo) && !f.enabled(f.redo));
    f.step(HistoryAction::Undo);
    assert!(!f.enabled(f.undo) && f.enabled(f.redo));
    assert_eq!(
        f.app
            .world()
            .resource::<ProjectEffectCatalog>()
            .material_functions()
            .unwrap(),
        [function]
    );
}

#[test]
fn active_empty_and_stale_code_buffers_keep_availability_in_the_correct_domain() {
    let mut f = Fixture::new();
    let mut editor = CodeEditor::new("original".into());
    editor.insert_for_test(" edit");
    let entity = f.app.world_mut().spawn(editor).id();
    f.app.insert_resource(ActiveCodeEditor(Some(entity)));
    f.app.update();
    assert!(f.enabled(f.undo) && !f.enabled(f.redo));
    f.step(HistoryAction::Undo);
    assert!(!f.enabled(f.undo) && f.enabled(f.redo));
    assert!(f.app.world().resource::<EditorSession>().can_undo());
    f.app.world_mut().entity_mut(entity).despawn();
    f.app.update();
    assert!(f.enabled(f.undo) && !f.enabled(f.redo));
}

#[test]
fn every_protection_branch_disables_history_without_destroying_any_stack() {
    let mut f = Fixture::new();
    for state in DocumentProtectionState::blocking_states_for_test() {
        f.app.insert_resource(state);
        f.app.update();
        assert!(!f.enabled(f.undo) && !f.enabled(f.redo));
        assert!(f.app.world().resource::<EditorSession>().can_undo());
    }
    f.app.insert_resource(DocumentProtectionState::default());
    f.app.update();
    assert!(f.enabled(f.undo));
}

#[test]
fn unapplied_io_disables_history_until_actual_completion_publication() {
    let mut f = Fixture::new();
    let guard = io::IoGuard::capture(
        f.app.world().resource::<ProjectEffectCatalog>(),
        f.app.world().resource::<EditorSession>(),
    );
    io::enqueue(&mut f.app.world_mut().commands(), guard, || {
        io::completion(|_| {})
    });
    let mut completion = io::prepared_completion(f.app.world_mut());
    f.app.update();
    assert!(!f.enabled(f.undo) && !f.enabled(f.redo));
    completion.apply(f.app.world_mut());
    f.app.update();
    assert!(f.enabled(f.undo));
}

#[test]
fn global_empty_history_does_not_advertise_retained_local_undo() {
    let mut f = Fixture::new();
    f.edit(0);
    std::fs::write(f.root.path().join("unused.png"), b"recoverable").unwrap();
    let content = aestra_project::ProjectContent::scan(f.root.path());
    let source = content
        .source_tree()
        .at_relative_path("unused.png")
        .unwrap()
        .id;
    let item = content
        .plan_delete_source(source, &[], true)
        .unwrap()
        .apply()
        .unwrap();
    let mut order = asset_order::AssetOrder::default();
    order.record(
        item.clone(),
        f.app.world().resource::<EditorSession>(),
        f.app.world().resource::<ProjectEffectCatalog>(),
    );
    f.app.insert_resource(order);
    f.app.update();
    assert!(f.enabled(f.undo));
    let mut order = f.app.world_mut().resource_mut::<asset_order::AssetOrder>();
    let revision = order.revision;
    assert!(order.finish_delete(true, revision, item));
    order.clear_context(&asset_order::Context::Effect);
    order.clear_context(&asset_order::Context::Material(
        MaterialEditingTarget::Program {
            root: f.root.path().into(),
            id: f.programs[0].id,
        },
    ));
    f.app.update();
    assert!(!f.enabled(f.undo));
    assert!(f.enabled(f.redo)); // The filesystem restore remains the global redo item.
}

#[test]
fn layout_only_availability_and_real_execution_need_no_semantic_history_item() {
    use crate::{
        feathers::node_graph::GraphViewportMemory,
        material_graph::{
            keys::material_graph_view_key,
            presentation::{Snapshot, Transaction},
        },
    };
    let mut f = Fixture::new();
    let catalog = f.app.world().resource::<ProjectEffectCatalog>().clone();
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .open_material_program(&catalog, f.programs[0].id)
        .unwrap();
    let graph = material_graph_view_key(f.programs[0].id);
    let mut memory = GraphViewportMemory::default();
    memory.set_node(&graph, "output", Vec2::ZERO, false);
    let before = Snapshot::capture(
        &graph,
        &catalog,
        f.app.world().resource::<EditorSession>(),
        &memory,
    )
    .unwrap();
    memory.set_node(&graph, "output", Vec2::splat(42.0), true);
    let after = Snapshot::capture(
        &graph,
        &catalog,
        f.app.world().resource::<EditorSession>(),
        &memory,
    )
    .unwrap();
    let mut session = f.app.world_mut().resource_mut::<EditorSession>();
    let context = asset_order::Context::current(&session);
    session.operation_order.record_layout(
        context,
        Transaction::snapshots_for_test(before, after, None),
    );
    f.app.insert_resource(memory);
    f.app.update();
    assert!(f.enabled(f.undo));
    f.step(HistoryAction::Undo);
    assert!(!f.enabled(f.undo) && f.enabled(f.redo));
    assert_eq!(
        f.app
            .world()
            .resource::<GraphViewportMemory>()
            .node(&graph, "output"),
        Some((Vec2::ZERO, false))
    );
}

#[test]
fn feathers_activation_dispatches_once_and_closes_only_transient_menu_state() {
    for kind in [
        MenuKind::File,
        MenuKind::Edit,
        MenuKind::View,
        MenuKind::Help,
    ] {
        let mut f = Fixture::new();
        f.app.insert_resource(MenuState {
            open: Some(kind),
            panels_open: true,
            tab_context: Some(TabContextMenu {
                panel: crate::docking::ToolPanel::MaterialGraph,
                position: [12.0, 24.0],
            }),
            show_grid: false,
            show_about: true,
        });
        let button = f.button(true);
        let context = f.app.world().resource::<MenuState>().tab_context.unwrap();
        assert_eq!(context.panel, crate::docking::ToolPanel::MaterialGraph);
        assert_eq!(context.position, [12.0, 24.0]);
        f.app.update();
        f.app.world_mut().trigger(Activate { entity: button });
        f.app.update();
        assert_eq!(f.app.world().resource::<Events>().0, [HistoryAction::Undo]);
        assert!(
            f.app
                .world()
                .get::<PendingFeathersActivation>(button)
                .is_none()
        );
        let menu = f.app.world().resource::<MenuState>();
        assert!(menu.open.is_none() && !menu.panels_open && menu.tab_context.is_none());
        assert!(!menu.show_grid && menu.show_about);
        assert_eq!(
            f.app.world().get::<BackgroundColor>(button).unwrap().0,
            theme::BUTTON
        );
        f.app.update();
        assert_eq!(f.app.world().resource::<Events>().0.len(), 1);
    }
}

#[test]
fn pressed_without_activate_never_dispatches_a_feathers_action() {
    let mut f = Fixture::new();
    let button = f.button(true);
    set_phase(f.app.world_mut(), button, Phase::Pressed);
    f.app.update();
    assert!(f.app.world().resource::<Events>().0.is_empty());
}

#[test]
fn disabled_activation_remains_pending_without_touching_effect_or_menu() {
    let mut f = Fixture::new();
    let button = f.button(true);
    f.app
        .world_mut()
        .entity_mut(button)
        .insert(InteractionDisabled);
    f.app.world_mut().resource_mut::<MenuState>().open = Some(MenuKind::Edit);
    let effect = f.app.world().resource::<EditorSession>().effect.clone();
    f.app.world_mut().trigger(Activate { entity: button });
    f.app.update();
    assert!(f.app.world().resource::<Events>().0.is_empty());
    assert_eq!(f.app.world().resource::<EditorSession>().effect, effect);
    assert!(f.app.world().resource::<MenuState>().open.is_some());
    assert!(
        f.app
            .world()
            .get::<PendingFeathersActivation>(button)
            .is_some()
    );
}

#[test]
fn native_marker_removal_restores_hover_idle_colors_without_repeating_a_held_press() {
    let mut f = Fixture::new();
    let button = f.button(false);
    f.app.update();
    set_phase(f.app.world_mut(), button, Phase::Hovered);
    f.app.update();
    assert_eq!(
        f.app.world().get::<BackgroundColor>(button).unwrap().0,
        theme::BUTTON_HOVER
    );
    set_phase(f.app.world_mut(), button, Phase::Pressed);
    f.app.update();
    assert_eq!(
        f.app.world().get::<BackgroundColor>(button).unwrap().0,
        theme::ACCENT_DIM
    );
    f.app.update();
    assert_eq!(f.app.world().resource::<Events>().0.len(), 1);
    set_phase(f.app.world_mut(), button, Phase::Hovered);
    f.app.update();
    assert_eq!(
        f.app.world().get::<BackgroundColor>(button).unwrap().0,
        theme::BUTTON_HOVER
    );
    super::super::control_input::clear_hover(f.app.world_mut(), button);
    f.app.update();
    assert_eq!(
        f.app.world().get::<BackgroundColor>(button).unwrap().0,
        theme::BUTTON
    );
    assert_eq!(f.app.world().resource::<Events>().0.len(), 1);
}

#[test]
#[should_panic(expected = "history control")]
fn audit_still_rejects_unclassified_native_buttons() {
    let mut app = App::new();
    app.add_systems(Update, audit_history_controls);
    app.world_mut().spawn((HistoryAction::Undo, AuditedButton));
    app.update();
}

#[test]
fn audit_accepts_explicit_native_and_feathers_classification() {
    let mut app = App::new();
    app.add_systems(Update, audit_history_controls);
    app.world_mut()
        .spawn((HistoryAction::Undo, AuditedButton, EditorNativeControl));
    app.world_mut()
        .spawn((HistoryAction::Redo, AuditedButton, FeathersActionButton));
    app.update();
}

#[test]
fn same_frame_azerty_chords_keep_order_modifiers_and_no_next_frame_replay() {
    let mut f = Fixture::new();
    let window = f.keyboard();
    f.key(
        window,
        KeyCode::ControlRight,
        Key::Control,
        ButtonState::Pressed,
        false,
    );
    f.tap(window, KeyCode::KeyW, "z");
    f.key(
        window,
        KeyCode::ShiftLeft,
        Key::Shift,
        ButtonState::Pressed,
        false,
    );
    f.tap(window, KeyCode::KeyW, "Z");
    f.key(
        window,
        KeyCode::ShiftLeft,
        Key::Shift,
        ButtonState::Released,
        false,
    );
    f.tap(window, KeyCode::KeyY, "y");
    f.key(
        window,
        KeyCode::ControlRight,
        Key::Control,
        ButtonState::Released,
        false,
    );
    f.app.update();
    assert_eq!(
        f.app.world().resource::<Events>().0,
        [
            HistoryAction::Undo,
            HistoryAction::Redo,
            HistoryAction::Redo
        ]
    );
    f.app.update();
    assert_eq!(f.app.world().resource::<Events>().0.len(), 3);
}

#[test]
fn held_modifier_sides_survive_frames_but_release_disables_history_chords() {
    let mut f = Fixture::new();
    let window = f.keyboard();
    for code in [KeyCode::ControlLeft, KeyCode::ControlRight] {
        f.key(window, code, Key::Control, ButtonState::Pressed, false);
    }
    f.app.update();
    f.key(
        window,
        KeyCode::ControlLeft,
        Key::Control,
        ButtonState::Released,
        false,
    );
    f.tap(window, KeyCode::KeyZ, "z");
    f.app.update();
    assert_eq!(f.app.world().resource::<Events>().0, [HistoryAction::Undo]);
    f.key(
        window,
        KeyCode::ControlRight,
        Key::Control,
        ButtonState::Released,
        false,
    );
    f.tap(window, KeyCode::KeyZ, "z");
    f.app.update();
    assert_eq!(f.app.world().resource::<Events>().0.len(), 1);
}

#[test]
fn repeated_snapshots_are_retained_for_repeat_consumers_but_not_dispatched_as_history() {
    let mut f = Fixture::new();
    let window = f.keyboard();
    f.key(
        window,
        KeyCode::ControlLeft,
        Key::Control,
        ButtonState::Pressed,
        false,
    );
    f.key(
        window,
        KeyCode::KeyZ,
        Key::Character("z".into()),
        ButtonState::Pressed,
        true,
    );
    f.app.update();
    assert!(f.app.world().resource::<Events>().0.is_empty());
    assert_eq!(f.app.world().resource::<Repeats>().0, 1);
}

#[test]
fn blocked_dispatch_signal_drops_the_chord_without_replaying_it_when_unblocked() {
    let mut f = Fixture::new();
    let window = f.keyboard();
    f.app.world_mut().resource_mut::<Blocked>().0 = true;
    f.key(
        window,
        KeyCode::ControlLeft,
        Key::Control,
        ButtonState::Pressed,
        false,
    );
    f.tap(window, KeyCode::KeyZ, "z");
    f.app.update();
    assert!(f.app.world().resource::<Events>().0.is_empty());
    f.app.world_mut().resource_mut::<Blocked>().0 = false;
    f.app.update();
    assert!(f.app.world().resource::<Events>().0.is_empty());
    f.tap(window, KeyCode::KeyZ, "z");
    f.app.update();
    assert_eq!(f.app.world().resource::<Events>().0, [HistoryAction::Undo]);
}

#[test]
fn logical_punctuation_does_not_become_a_physical_history_shortcut() {
    let mut f = Fixture::new();
    let window = f.keyboard();
    f.key(
        window,
        KeyCode::ControlLeft,
        Key::Control,
        ButtonState::Pressed,
        false,
    );
    f.tap(window, KeyCode::KeyZ, ";");
    f.app.update();
    assert!(f.app.world().resource::<Events>().0.is_empty());
}

#[test]
fn direct_button_input_fallback_remains_available_without_snapshot_collection() {
    let mut f = Fixture::new();
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ControlLeft);
    keys.press(KeyCode::KeyZ);
    f.app
        .insert_resource(keys)
        .init_resource::<Blocked>()
        .init_resource::<Repeats>()
        .add_systems(Update, shortcuts.before(handle_history_buttons));
    f.app.update();
    assert_eq!(f.app.world().resource::<Events>().0, [HistoryAction::Undo]);
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    f.app.update();
    assert_eq!(f.app.world().resource::<Events>().0.len(), 1);
}
