//! Actual application ownership queries and plugin ingress, not a blocking-signal substitute.
use super::*;
use crate::{
    asset_actions::AssetOperationState,
    asset_browser::BrowserSurface,
    feathers::code_editor::CodeEditor,
    material_function_editor::graph::FunctionGraphMenuState,
    material_graph::MaterialGraphPaletteState,
    menus::{MenuKind, MenuState, TabContextMenu},
    persistence::DocumentProtectionState,
};
use bevy::ecs::system::RunSystemOnce;

fn decision(app: &mut App) -> (bool, bool) {
    app.world_mut()
        .run_system_once(|context: ShortcutContext| {
            (context.blocked(), context.asset_browser_focused())
        })
        .unwrap()
}

#[test]
fn tab_strip_focus_owns_panel_navigation_but_not_global_save_or_history() {
    let mut app = App::new();
    let strip = app
        .world_mut()
        .spawn(bevy::ui_widgets::TabList::default())
        .id();
    let tab = app
        .world_mut()
        .spawn((bevy::ui_widgets::Tab, ChildOf(strip)))
        .id();
    app.insert_resource(InputFocus::from_entity(tab));
    assert_eq!(
        app.world_mut()
            .run_system_once(|context: ShortcutContext| (
                context.blocked(),
                context.tab_strip_focused()
            ))
            .unwrap(),
        (false, true)
    );
    let outside = app.world_mut().spawn_empty().id();
    app.insert_resource(InputFocus::from_entity(outside));
    assert!(
        !app.world_mut()
            .run_system_once(|context: ShortcutContext| context.tab_strip_focused())
            .unwrap()
    );
}

#[test]
fn absent_optional_resources_and_stale_or_empty_focus_do_not_capture_shortcuts() {
    let mut app = App::new();
    assert_eq!(decision(&mut app), (false, false));
    app.init_resource::<InputFocus>();
    assert_eq!(decision(&mut app), (false, false));
    let stale = app.world_mut().spawn_empty().id();
    app.world_mut().despawn(stale);
    app.insert_resource(InputFocus::from_entity(stale));
    assert_eq!(decision(&mut app), (false, false));
}

#[test]
fn native_select_listbox_focus_blocks_authoring_shortcuts_through_the_popup() {
    let mut app = App::new();
    let popup = app
        .world_mut()
        .spawn(bevy::ui_widgets::MenuPopup::default())
        .id();
    let listbox = app
        .world_mut()
        .spawn((bevy::ui_widgets::ListBox, ChildOf(popup)))
        .id();
    app.insert_resource(InputFocus::from_entity(listbox));
    assert_eq!(decision(&mut app), (true, false));
    let trigger = app.world_mut().spawn_empty().id();
    app.insert_resource(InputFocus::from_entity(trigger));
    assert_eq!(decision(&mut app), (false, false));
}

#[test]
fn closed_actual_modal_and_menu_models_do_not_block() {
    let mut app = App::new();
    app.init_resource::<DocumentProtectionState>()
        .init_resource::<AssetOperationState>()
        .init_resource::<MaterialGraphPaletteState>()
        .init_resource::<FunctionGraphMenuState>()
        .init_resource::<MenuState>();
    assert_eq!(decision(&mut app), (false, false));
    // Grid display is not shortcut ownership.
    app.world_mut().resource_mut::<MenuState>().show_grid = false;
    assert_eq!(decision(&mut app), (false, false));
}

#[test]
fn every_document_protection_branch_blocks_with_no_resource_mutation() {
    let mut app = App::new();
    for state in DocumentProtectionState::blocking_states_for_test() {
        app.insert_resource(state.clone());
        assert_eq!(decision(&mut app), (true, false));
        assert_eq!(app.world().resource::<DocumentProtectionState>(), &state);
    }
    app.insert_resource(DocumentProtectionState::default());
    assert_eq!(decision(&mut app), (false, false));
}

#[test]
fn actual_asset_extraction_dialog_blocks_even_with_a_retryable_error() {
    let mut app = App::new();
    let state = AssetOperationState::open_for_test();
    app.insert_resource(state.clone());
    assert_eq!(decision(&mut app), (true, false));
    assert_eq!(app.world().resource::<AssetOperationState>(), &state);
    app.insert_resource(AssetOperationState::default());
    assert_eq!(decision(&mut app), (false, false));
}

#[test]
fn each_material_palette_node_and_connection_menu_blocks() {
    let mut app = App::new();
    for state in MaterialGraphPaletteState::blocking_states_for_test() {
        let before = format!("{state:?}");
        app.insert_resource(state);
        assert_eq!(decision(&mut app), (true, false));
        assert_eq!(
            format!("{:?}", app.world().resource::<MaterialGraphPaletteState>()),
            before
        );
    }
    app.insert_resource(MaterialGraphPaletteState::default());
    assert_eq!(decision(&mut app), (false, false));
}

#[test]
fn each_function_node_and_connection_menu_blocks() {
    let mut app = App::new();
    for state in FunctionGraphMenuState::blocking_states_for_test() {
        app.insert_resource(state);
        assert_eq!(decision(&mut app), (true, false));
        assert!(app.world().resource::<FunctionGraphMenuState>().is_open());
    }
    app.insert_resource(FunctionGraphMenuState::default());
    assert_eq!(decision(&mut app), (false, false));
}

fn menu_states() -> Vec<MenuState> {
    let mut states: Vec<_> = [
        MenuKind::File,
        MenuKind::Edit,
        MenuKind::View,
        MenuKind::Help,
    ]
    .into_iter()
    .map(|kind| MenuState {
        open: Some(kind),
        ..default()
    })
    .collect();
    states.extend([
        MenuState {
            panels_open: true,
            ..default()
        },
        MenuState {
            tab_context: Some(TabContextMenu {
                panel: crate::docking::ToolPanel::MaterialGraph,
                position: [12.0, 24.0],
            }),
            ..default()
        },
        MenuState {
            show_about: true,
            ..default()
        },
    ]);
    states
}

#[test]
fn all_top_level_panel_tab_context_and_about_menus_block() {
    let mut app = App::new();
    for state in menu_states() {
        app.insert_resource(state);
        assert_eq!(decision(&mut app), (true, false));
    }
    app.insert_resource(MenuState::default());
    assert_eq!(decision(&mut app), (false, false));
}

#[test]
fn window_focus_requires_any_real_window_but_supports_headless_and_floating_contexts() {
    let mut app = App::new();
    assert_eq!(decision(&mut app), (false, false));
    let main = app
        .world_mut()
        .spawn(Window {
            focused: false,
            ..default()
        })
        .id();
    assert_eq!(decision(&mut app), (true, false));
    let floating = app
        .world_mut()
        .spawn(Window {
            focused: true,
            ..default()
        })
        .id();
    assert_eq!(decision(&mut app), (false, false));
    app.world_mut().get_mut::<Window>(floating).unwrap().focused = false;
    assert_eq!(decision(&mut app), (true, false));
    app.world_mut().get_mut::<Window>(main).unwrap().focused = true;
    assert_eq!(decision(&mut app), (false, false));
    app.world_mut().despawn(main);
    app.world_mut().despawn(floating);
    assert_eq!(decision(&mut app), (false, false));
}

#[test]
fn editable_text_and_descendant_focus_block_without_an_active_code_buffer_resource() {
    let mut app = App::new();
    let field = app
        .world_mut()
        .spawn(EditableText::new("Do not edit the timeline"))
        .id();
    let child = app.world_mut().spawn(ChildOf(field)).id();
    let leaf = app.world_mut().spawn(ChildOf(child)).id();
    for entity in [field, child, leaf] {
        app.insert_resource(InputFocus::from_entity(entity));
        assert_eq!(decision(&mut app), (true, false));
    }
    app.world_mut().entity_mut(field).remove::<EditableText>();
    assert_eq!(decision(&mut app), (false, false));
}

#[test]
fn code_buffer_and_menu_item_ancestors_own_shortcuts_not_unrelated_siblings() {
    let mut app = App::new();
    for owner in [
        app.world_mut().spawn(CodeEditor::new("shader".into())).id(),
        app.world_mut().spawn(bevy::ui_widgets::MenuItem).id(),
    ] {
        let child = app.world_mut().spawn(ChildOf(owner)).id();
        app.insert_resource(InputFocus::from_entity(child));
        assert_eq!(decision(&mut app), (true, false));
        let sibling = app.world_mut().spawn_empty().id();
        app.insert_resource(InputFocus::from_entity(sibling));
        assert_eq!(decision(&mut app), (false, false));
    }
}

#[test]
fn assets_isolates_panel_local_shortcuts_without_blocking_global_history() {
    let mut app = App::new();
    let surface = app.world_mut().spawn(BrowserSurface).id();
    let middle = app.world_mut().spawn(ChildOf(surface)).id();
    let leaf = app.world_mut().spawn(ChildOf(middle)).id();
    for entity in [surface, middle, leaf] {
        app.insert_resource(InputFocus::from_entity(entity));
        assert_eq!(decision(&mut app), (false, true));
    }
    app.world_mut()
        .entity_mut(middle)
        .insert(EditableText::new("Filter"));
    assert_eq!(decision(&mut app), (true, true));
    app.world_mut().entity_mut(middle).remove::<EditableText>();
    app.world_mut().entity_mut(leaf).remove::<ChildOf>();
    assert_eq!(decision(&mut app), (false, false));
}

#[test]
fn reparenting_and_despawning_focused_descendants_release_ownership_immediately() {
    let mut app = App::new();
    let owner = app.world_mut().spawn(EditableText::new("Field")).id();
    let neutral = app.world_mut().spawn_empty().id();
    let leaf = app.world_mut().spawn(ChildOf(owner)).id();
    app.insert_resource(InputFocus::from_entity(leaf));
    assert_eq!(decision(&mut app), (true, false));
    app.world_mut().entity_mut(leaf).insert(ChildOf(neutral));
    assert_eq!(decision(&mut app), (false, false));
    app.world_mut().entity_mut(leaf).insert(ChildOf(owner));
    assert_eq!(decision(&mut app), (true, false));
    app.world_mut().despawn(owner);
    assert_eq!(decision(&mut app), (false, false));
}

#[derive(Resource, Default)]
struct Actions(Vec<crate::history::HistoryAction>);

struct KeyboardFixture {
    app: App,
    window: Entity,
    _root: tempfile::TempDir,
    edited_duration: f32,
}

impl KeyboardFixture {
    fn new() -> Self {
        use crate::history::{EditorHistoryLedger, MaterialProgramEditHistory, routing::Routing};
        let root = tempfile::tempdir().unwrap();
        let mut session = crate::test_support::session_with_timing_slack();
        session.adjust_effect_duration(0.25);
        let edited_duration = session.effect.duration;
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            bevy::input::InputPlugin,
            bevy::input_focus::InputFocusPlugin,
            bevy::input_focus::InputDispatchPlugin,
            EditorKeyboardPlugin,
        ))
        .insert_resource(session)
        .insert_resource(crate::ProjectEffectCatalog::scan(root.path()))
        .init_resource::<crate::ModulePaletteState>()
        .init_resource::<EditorHistoryLedger>()
        .init_resource::<MaterialProgramEditHistory>()
        .init_resource::<crate::material_function_editor::FunctionEditor>()
        .init_resource::<Actions>()
        .add_systems(Update, crate::history::history_keyboard_input)
        .add_observer(
            |action: On<crate::history::HistoryAction>,
             routing: Routing,
             mut commands: Commands,
             mut actions: ResMut<Actions>| {
                actions.0.push(*action);
                routing.execute(*action, &mut commands, |_, _, _, _| {
                    panic!("no filesystem operation in keyboard fixture")
                });
            },
        );
        let window = app
            .world_mut()
            .spawn((Window::default(), bevy::window::PrimaryWindow))
            .id();
        Self {
            app,
            window,
            _root: root,
            edited_duration,
        }
    }
    fn chord(&mut self, letter: &str) {
        use bevy::input::{ButtonState, keyboard::Key};
        super::tests::key(
            &mut self.app,
            self.window,
            KeyCode::ControlRight,
            Key::Control,
            ButtonState::Pressed,
        );
        // AZERTY physical W with logical Z must reach the real application dispatcher.
        super::tests::tap(
            &mut self.app,
            self.window,
            KeyCode::KeyW,
            Key::Character(letter.into()),
        );
        super::tests::key(
            &mut self.app,
            self.window,
            KeyCode::ControlRight,
            Key::Control,
            ButtonState::Released,
        );
        self.app.update();
    }
    fn duration(&self) -> f32 {
        self.app
            .world()
            .resource::<crate::session::EditorSession>()
            .effect
            .duration
    }
    fn assert_blocked(&mut self) {
        self.chord("z");
        assert_eq!(self.duration(), self.edited_duration);
        assert!(self.app.world().resource::<Actions>().0.is_empty());
    }
    fn assert_released(&mut self) {
        self.app.update();
        assert!(
            self.app.world().resource::<Actions>().0.is_empty(),
            "blocked snapshots must not replay when ownership clears"
        );
        self.chord("z");
        assert!(self.duration() < self.edited_duration);
        assert_eq!(
            self.app.world().resource::<Actions>().0,
            [crate::history::HistoryAction::Undo]
        );
        self.app.update();
        assert_eq!(self.app.world().resource::<Actions>().0.len(), 1);
    }
}

#[test]
fn real_history_shortcuts_respect_every_document_modal_then_resume_on_a_fresh_chord() {
    for state in DocumentProtectionState::blocking_states_for_test() {
        let mut fixture = KeyboardFixture::new();
        fixture.app.insert_resource(state);
        fixture.assert_blocked();
        fixture
            .app
            .insert_resource(DocumentProtectionState::default());
        fixture.assert_released();
    }
}

#[test]
fn real_history_shortcuts_respect_all_actual_graph_and_asset_dialog_states() {
    let mut fixture = KeyboardFixture::new();
    fixture
        .app
        .insert_resource(AssetOperationState::open_for_test());
    fixture.assert_blocked();
    fixture.app.insert_resource(AssetOperationState::default());
    fixture.assert_released();
    for state in MaterialGraphPaletteState::blocking_states_for_test() {
        let mut fixture = KeyboardFixture::new();
        fixture.app.insert_resource(state);
        fixture.assert_blocked();
        fixture
            .app
            .insert_resource(MaterialGraphPaletteState::default());
        fixture.assert_released();
    }
    for state in FunctionGraphMenuState::blocking_states_for_test() {
        let mut fixture = KeyboardFixture::new();
        fixture.app.insert_resource(state);
        fixture.assert_blocked();
        fixture
            .app
            .insert_resource(FunctionGraphMenuState::default());
        fixture.assert_released();
    }
}

#[test]
fn real_history_shortcuts_respect_every_menu_and_module_palette_without_closing_them() {
    for state in menu_states() {
        let mut fixture = KeyboardFixture::new();
        fixture.app.insert_resource(state);
        fixture.assert_blocked();
        assert!(
            decision(&mut fixture.app).0,
            "dispatch cannot close its owner"
        );
        fixture.app.insert_resource(MenuState::default());
        fixture.assert_released();
    }
    let mut fixture = KeyboardFixture::new();
    fixture
        .app
        .world_mut()
        .resource_mut::<crate::ModulePaletteState>()
        .open = true;
    fixture.assert_blocked();
    assert!(
        fixture
            .app
            .world()
            .resource::<crate::ModulePaletteState>()
            .open
    );
    fixture
        .app
        .world_mut()
        .resource_mut::<crate::ModulePaletteState>()
        .open = false;
    fixture.assert_released();
}

#[test]
fn real_history_shortcuts_are_available_from_assets_but_not_its_search_descendants() {
    let mut fixture = KeyboardFixture::new();
    let surface = fixture.app.world_mut().spawn(BrowserSurface).id();
    let field = fixture
        .app
        .world_mut()
        .spawn((EditableText::new("Filter"), ChildOf(surface)))
        .id();
    let leaf = fixture.app.world_mut().spawn(ChildOf(field)).id();
    fixture.app.insert_resource(InputFocus::from_entity(leaf));
    fixture.assert_blocked();
    fixture
        .app
        .insert_resource(InputFocus::from_entity(surface));
    fixture.assert_released();
}

#[test]
fn real_history_shortcuts_are_suppressed_for_unfocused_windows_and_not_deferred() {
    let mut fixture = KeyboardFixture::new();
    fixture
        .app
        .world_mut()
        .get_mut::<Window>(fixture.window)
        .unwrap()
        .focused = false;
    fixture.assert_blocked();
    fixture
        .app
        .world_mut()
        .get_mut::<Window>(fixture.window)
        .unwrap()
        .focused = true;
    fixture.assert_released();
}

#[test]
fn real_history_shortcuts_do_not_capture_code_or_menu_descendants() {
    for menu in [false, true] {
        let mut fixture = KeyboardFixture::new();
        let owner = if menu {
            fixture
                .app
                .world_mut()
                .spawn(bevy::ui_widgets::MenuItem)
                .id()
        } else {
            fixture
                .app
                .world_mut()
                .spawn(CodeEditor::new("source".into()))
                .id()
        };
        let leaf = fixture.app.world_mut().spawn(ChildOf(owner)).id();
        fixture.app.insert_resource(InputFocus::from_entity(leaf));
        fixture.assert_blocked();
        fixture.app.world_mut().resource_mut::<InputFocus>().clear();
        fixture.assert_released();
    }
}
