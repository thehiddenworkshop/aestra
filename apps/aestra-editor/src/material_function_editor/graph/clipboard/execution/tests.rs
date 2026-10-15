//! Actual function clipboard, keyboard ownership, catalog publication and compound history.
use super::*;
use crate::{
    history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, routing::Routing},
    input::EditorKeyboardPlugin,
    material_graph::{MaterialGraphSelectionState, MaterialSelectionScope},
    persistence::DocumentProtectionState,
};
use aestra_core::{
    MaterialExpressionId, MaterialFunctionOutputId,
    material::{
        MaterialExpression, MaterialExpressionKind, MaterialFunction, MaterialFunctionOutput,
        MaterialSchemaVersion, MaterialValue, MaterialValueType,
    },
};
use bevy::{
    input::{
        ButtonState, InputPlugin,
        keyboard::{Key, KeyboardInput},
    },
    input_focus::{InputDispatchPlugin, InputFocus, InputFocusPlugin},
    text::EditableText,
    time::TimeUpdateStrategy,
    window::PrimaryWindow,
};
use std::time::Duration;

fn function(name: &str) -> MaterialFunction {
    let output = MaterialExpressionId::new();
    let constant = MaterialExpressionId::new();
    let multiply = MaterialExpressionId::new();
    MaterialFunction {
        id: MaterialFunctionId::new(),
        name: name.into(),
        schema_version: MaterialSchemaVersion::CURRENT,
        inputs: vec![],
        outputs: vec![MaterialFunctionOutput {
            id: MaterialFunctionOutputId::new(),
            name: "Value".into(),
            value_type: MaterialValueType::Float,
            expression: output,
        }],
        expressions: vec![
            MaterialExpression {
                id: output,
                kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
            },
            MaterialExpression {
                id: constant,
                kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.75)),
            },
            MaterialExpression {
                id: multiply,
                kind: MaterialExpressionKind::Multiply(constant, constant),
            },
        ],
        custom_wesl: None,
    }
    .normalized()
}

struct Fixture {
    root: tempfile::TempDir,
    app: App,
    window: Entity,
    viewport: Entity,
    original: MaterialFunction,
    selected: BTreeSet<MaterialExpressionId>,
    scope: MaterialSelectionScope,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let original = function("Clipboard");
        original
            .save_ron(root.path().join("clipboard.aestra.material-function.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
        session
            .open_material_function(&catalog, original.id)
            .unwrap();
        let selected: BTreeSet<_> = original
            .expressions
            .iter()
            .filter(|node| node.id != original.outputs[0].expression)
            .map(|node| node.id)
            .collect();
        let scope = Some(crate::docking::EditorViewId(42));
        let graph_key =
            crate::material_graph::function_graph_memory_key(catalog.root(), original.id);
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            InputPlugin,
            InputFocusPlugin,
            InputDispatchPlugin,
            EditorKeyboardPlugin,
        ))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO))
        .insert_resource(session)
        .insert_resource(catalog)
        .init_resource::<GraphClipboard>()
        .init_resource::<MaterialGraphSelectionState>()
        .init_resource::<GraphViewportMemory>()
        .init_resource::<FunctionEditor>()
        .init_resource::<MaterialProgramEditHistory>()
        .init_resource::<EditorHistoryLedger>()
        .add_observer(
            |action: On<HistoryAction>, routing: Routing, mut commands: Commands| {
                routing.execute(*action, &mut commands, |_, _, _, _| {
                    panic!("No filesystem history in this fixture")
                });
            },
        );
        register(&mut app);
        app.world_mut()
            .resource_mut::<MaterialGraphSelectionState>()
            .select_function_expressions(
                scope,
                original.id,
                &selected,
                GraphSelectionMode::Replace,
            );
        for (index, id) in selected.iter().enumerate() {
            app.world_mut()
                .resource_mut::<GraphViewportMemory>()
                .place_node(
                    &graph_key,
                    id.to_string(),
                    Vec2::new(50.0 + 250.0 * index as f32, 80.0 + 40.0 * index as f32),
                );
        }
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        let viewport = app
            .world_mut()
            .spawn((
                View(original.id),
                ViewScope(scope),
                FeathersGraphViewport::clipboard_fixture(graph_key, Vec2::new(40.0, 20.0), 2.0),
                ComputedNode {
                    size: Vec2::new(800.0, 600.0),
                    ..default()
                },
                RelativeCursorPosition {
                    cursor_over: true,
                    normalized: None,
                },
            ))
            .id();
        app.update();
        Self {
            root,
            app,
            window,
            viewport,
            original,
            selected,
            scope,
        }
    }

    fn current(&self) -> MaterialFunction {
        self.app
            .world()
            .resource::<EditorSession>()
            .graph_function(self.app.world().resource::<ProjectEffectCatalog>())
            .unwrap()
    }

    fn selection(&self) -> BTreeSet<GraphNodeKey> {
        self.app
            .world()
            .resource::<MaterialGraphSelectionState>()
            .function_arrange_seeds(self.scope, self.original.id)
    }

    fn key(&mut self, code: KeyCode, logical: Key, state: ButtonState, repeat: bool) {
        self.app.world_mut().write_message(KeyboardInput {
            key_code: code,
            logical_key: logical,
            state,
            repeat,
            text: None,
            window: self.window,
        });
    }

    fn tap(&mut self, code: KeyCode, text: &str) {
        self.key(
            code,
            Key::Character(text.into()),
            ButtonState::Pressed,
            false,
        );
        self.key(
            code,
            Key::Character(text.into()),
            ButtonState::Released,
            false,
        );
    }

    fn chord(&mut self, code: KeyCode, text: &str) {
        self.key(
            KeyCode::ControlLeft,
            Key::Control,
            ButtonState::Pressed,
            false,
        );
        self.tap(code, text);
        self.key(
            KeyCode::ControlLeft,
            Key::Control,
            ButtonState::Released,
            false,
        );
    }

    fn update(&mut self) {
        self.app.update();
    }

    fn step(&mut self, action: HistoryAction) {
        self.app.world_mut().trigger(action);
        self.app.world_mut().flush();
    }

    fn assert_unchanged(&self) {
        assert_eq!(self.current(), self.original);
        assert!(
            !self
                .app
                .world()
                .resource::<FunctionEditor>()
                .available_for_function(self.root.path(), self.original.id, true)
        );
    }
}

#[test]
fn same_frame_copy_cut_paste_preserve_wires_and_compound_undo_redo() {
    let mut f = Fixture::new();
    for (key, text) in [
        (KeyCode::KeyC, "c"),
        (KeyCode::KeyX, "x"),
        (KeyCode::KeyV, "v"),
    ] {
        f.chord(key, text);
    }
    f.update();
    let pasted = f.current();
    assert_eq!(pasted.expressions.len(), 3);
    let constant = pasted
        .expressions
        .iter()
        .find(|node| node.kind == MaterialExpressionKind::Constant(MaterialValue::Float(0.75)))
        .unwrap()
        .id;
    assert!(
        pasted
            .expressions
            .iter()
            .any(|node| node.kind == MaterialExpressionKind::Multiply(constant, constant))
    );
    assert_eq!(f.selection().len(), 2);
    assert!(
        f.selected
            .iter()
            .all(|id| !f.selection().contains(&GraphNodeKey::Expression(*id)))
    );
    f.step(HistoryAction::Undo);
    assert_eq!(f.current().expressions.len(), 1);
    f.step(HistoryAction::Undo);
    assert_eq!(f.current(), f.original);
    f.step(HistoryAction::Redo);
    f.step(HistoryAction::Redo);
    assert_eq!(f.current(), pasted);
}

#[test]
fn repeated_pastes_allocate_disjoint_nodes_and_do_not_replay_on_idle() {
    let mut f = Fixture::new();
    f.chord(KeyCode::KeyC, "c");
    f.chord(KeyCode::KeyV, "v");
    f.chord(KeyCode::KeyV, "v");
    f.update();
    let current = f.current();
    let ids: BTreeSet<_> = current.expressions.iter().map(|node| node.id).collect();
    assert_eq!(ids.len(), 7);
    assert_eq!(f.selection().len(), 2);
    f.update();
    assert_eq!(f.current(), current);
}

#[test]
fn duplicate_then_delete_use_fresh_selection_and_undo_restores_layout() {
    let mut f = Fixture::new();
    f.chord(KeyCode::KeyD, "d");
    f.update();
    let duplicate = f.current();
    let selected = f.selection();
    assert_eq!(duplicate.expressions.len(), 5);
    f.key(KeyCode::Delete, Key::Delete, ButtonState::Pressed, false);
    f.key(KeyCode::Delete, Key::Delete, ButtonState::Released, false);
    f.update();
    assert_eq!(f.current().expressions.len(), 3);
    assert!(f.selection().is_empty());
    f.step(HistoryAction::Undo);
    assert_eq!(f.current(), duplicate);
    let key = crate::material_graph::function_graph_memory_key(f.root.path(), f.original.id);
    for node in selected {
        let GraphNodeKey::Expression(id) = node else {
            panic!("Expression expected")
        };
        assert!(
            f.app
                .world()
                .resource::<GraphViewportMemory>()
                .node_position(&key, &id.to_string())
                .is_some()
        );
    }
}

#[test]
fn paste_unprojects_the_pointer_using_actual_pan_zoom_and_relative_placement() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(f.viewport)
        .unwrap()
        .normalized = Some(Vec2::new(0.1, -0.2));
    f.chord(KeyCode::KeyC, "c");
    f.chord(KeyCode::KeyV, "v");
    f.update();
    let key = crate::material_graph::function_graph_memory_key(f.root.path(), f.original.id);
    let positions: Vec<_> = f
        .selection()
        .iter()
        .map(|node| {
            let GraphNodeKey::Expression(id) = node else {
                panic!("Expression expected")
            };
            f.app
                .world()
                .resource::<GraphViewportMemory>()
                .node_position(&key, &id.to_string())
                .unwrap()
        })
        .collect();
    assert_eq!(positions.len(), 2);
    for expected in [Vec2::new(220.0, 80.0), Vec2::new(470.0, 120.0)] {
        assert!(
            positions
                .iter()
                .any(|position| position.abs_diff_eq(expected, 0.001)),
            "{positions:?}"
        );
    }
}

#[test]
fn selection_is_per_view_even_when_two_views_display_the_same_function() {
    let mut f = Fixture::new();
    let other = Some(crate::docking::EditorViewId(99));
    f.app
        .world_mut()
        .get_mut::<ViewScope>(f.viewport)
        .unwrap()
        .0 = other;
    f.chord(KeyCode::KeyX, "x");
    f.update();
    f.assert_unchanged();
    assert_eq!(f.selection().len(), 2);
    assert!(
        f.app
            .world()
            .resource::<GraphClipboard>()
            .fragment
            .is_none()
    );
}

#[test]
fn hovered_view_activates_its_real_function_not_the_previously_active_target() {
    let mut f = Fixture::new();
    let other = function("Other");
    other
        .save_ron(f.root.path().join("other.aestra.material-function.ron"))
        .unwrap();
    let catalog = ProjectEffectCatalog::scan(f.root.path());
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .open_material_function(&catalog, other.id)
        .unwrap();
    f.app.insert_resource(catalog);
    f.chord(KeyCode::KeyD, "d");
    f.update();
    assert_eq!(f.current().id, f.original.id);
    assert_eq!(f.current().expressions.len(), 5);
    assert!(
        !f.app
            .world()
            .resource::<FunctionEditor>()
            .available_for_function(f.root.path(), other.id, true)
    );
}

#[test]
fn missing_hover_or_despawned_view_never_edit_or_overwrite_clipboard() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(f.viewport)
        .unwrap()
        .cursor_over = false;
    f.chord(KeyCode::KeyX, "x");
    f.update();
    f.assert_unchanged();
    f.app.world_mut().despawn(f.viewport);
    f.chord(KeyCode::KeyD, "d");
    f.update();
    f.assert_unchanged();
    assert!(
        f.app
            .world()
            .resource::<GraphClipboard>()
            .fragment
            .is_none()
    );
}

#[test]
fn logical_letters_and_event_time_control_drive_clipboard_not_scan_codes() {
    let mut f = Fixture::new();
    f.chord(KeyCode::KeyD, "p");
    f.update();
    f.assert_unchanged();
    f.chord(KeyCode::KeyP, "C");
    f.tap(KeyCode::KeyV, "v"); // Ctrl was released before this event.
    f.update();
    f.assert_unchanged();
    assert!(
        f.app
            .world()
            .resource::<GraphClipboard>()
            .fragment
            .is_some()
    );
    f.chord(KeyCode::KeyP, "v");
    f.update();
    assert_eq!(f.current().expressions.len(), 5);
}

#[test]
fn os_repeats_and_held_control_chords_do_not_repeat_function_edits() {
    let mut f = Fixture::new();
    f.key(
        KeyCode::ControlLeft,
        Key::Control,
        ButtonState::Pressed,
        false,
    );
    f.key(
        KeyCode::KeyD,
        Key::Character("d".into()),
        ButtonState::Pressed,
        false,
    );
    f.update();
    let current = f.current();
    assert_eq!(current.expressions.len(), 5);
    f.key(
        KeyCode::KeyD,
        Key::Character("d".into()),
        ButtonState::Pressed,
        true,
    );
    f.update();
    f.update();
    assert_eq!(f.current(), current);
}

#[test]
fn editable_and_code_buffer_ancestors_keep_function_clipboard_shortcuts() {
    for code in [false, true] {
        let mut f = Fixture::new();
        let parent = if code {
            f.app
                .world_mut()
                .spawn(crate::feathers::code_editor::CodeEditor::new(
                    "Shader".into(),
                ))
                .id()
        } else {
            f.app.world_mut().spawn(EditableText::new("Name")).id()
        };
        let child = f.app.world_mut().spawn(ChildOf(parent)).id();
        f.app.insert_resource(InputFocus::from_entity(child));
        f.chord(KeyCode::KeyX, "x");
        f.update();
        f.assert_unchanged();
        assert!(
            f.app
                .world()
                .resource::<GraphClipboard>()
                .fragment
                .is_none()
        );
    }
}

#[test]
fn document_protection_and_graph_menus_block_without_replaying_after_close() {
    let mut f = Fixture::new();
    for state in DocumentProtectionState::blocking_states_for_test() {
        f.app.insert_resource(state);
        f.chord(KeyCode::KeyD, "d");
        f.update();
        f.assert_unchanged();
    }
    f.app.insert_resource(DocumentProtectionState::default());
    for state in
        crate::material_function_editor::graph::FunctionGraphMenuState::blocking_states_for_test()
    {
        f.app.insert_resource(state);
        f.chord(KeyCode::KeyD, "d");
        f.update();
        f.assert_unchanged();
    }
    f.app
        .insert_resource(crate::material_function_editor::graph::FunctionGraphMenuState::default());
    f.update();
    f.assert_unchanged();
    f.chord(KeyCode::KeyD, "d");
    f.update();
    assert_eq!(f.current().expressions.len(), 5);
}

#[test]
fn output_cut_is_atomic_and_retains_a_copy_without_corrupting_history() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .resource_mut::<MaterialGraphSelectionState>()
        .select_function_expressions(
            f.scope,
            f.original.id,
            &BTreeSet::from([f.original.outputs[0].expression]),
            GraphSelectionMode::Replace,
        );
    f.chord(KeyCode::KeyX, "x");
    f.update();
    f.assert_unchanged();
    assert_eq!(
        f.app
            .world()
            .resource::<GraphClipboard>()
            .fragment
            .as_ref()
            .unwrap()
            .len(),
        1
    );
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .status
            .contains("Could not edit function nodes")
    );
}

#[test]
fn foreign_external_dependency_rejects_paste_without_partial_semantics_or_layout() {
    let mut f = Fixture::new();
    let foreign = MaterialExpressionId::new();
    let node = MaterialExpressionId::new();
    f.app.world_mut().resource_mut::<GraphClipboard>().fragment = Some(
        Fragment::capture(
            &[MaterialExpression {
                id: node,
                kind: MaterialExpressionKind::Multiply(foreign, foreign),
            }],
            &BTreeSet::from([node]),
            BTreeMap::new(),
            &[],
            &[],
        )
        .unwrap(),
    );
    let selection = f.selection();
    f.chord(KeyCode::KeyV, "v");
    f.update();
    f.assert_unchanged();
    assert_eq!(f.selection(), selection);
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .status
            .contains("input nodes are missing")
    );
}

#[test]
fn empty_clipboard_and_empty_selection_never_create_a_history_step() {
    let mut f = Fixture::new();
    f.chord(KeyCode::KeyV, "v");
    f.update();
    f.assert_unchanged();
    f.app
        .world_mut()
        .resource_mut::<MaterialGraphSelectionState>()
        .clear_function_selection(f.scope, f.original.id);
    f.chord(KeyCode::KeyX, "x");
    f.chord(KeyCode::KeyD, "d");
    f.update();
    f.assert_unchanged();
    assert!(
        f.app
            .world()
            .resource::<GraphClipboard>()
            .fragment
            .is_none()
    );
}
