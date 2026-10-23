//! Real material keyboard consumer, semantic publication and compound history on both engines.
use super::*;
use crate::{
    feathers::node_graph::GraphSelectionMode,
    history::{HistoryAction, routing::Routing},
    input::EditorKeyboardPlugin,
    material_function_editor::FunctionEditor,
    material_graph::{
        MaterialGraphPreviewTarget,
        keys::{material_graph_expression_node_key, material_graph_view_key},
        palette_state::MaterialGraphConnection,
    },
    material_target::MaterialEditingTarget,
    persistence::DocumentProtectionState,
};
use aestra_authoring::{MaterialConnectionTarget, MaterialOutputSocket};
use aestra_core::{
    MaterialExpressionId, MaterialProgramId,
    material::{MaterialExpression, MaterialExpressionKind, MaterialProgram, MaterialValue},
};
use bevy::{
    input::{
        ButtonState, InputPlugin,
        keyboard::{Key, KeyboardInput},
    },
    input_focus::{InputDispatchPlugin, InputFocusPlugin},
    time::TimeUpdateStrategy,
    window::PrimaryWindow,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

struct Fixture {
    root: tempfile::TempDir,
    app: App,
    window: Entity,
    viewport: Entity,
    nodes: Vec<Entity>,
    original: MaterialProgram,
    selected: BTreeSet<MaterialExpressionId>,
    scope: crate::material_graph::MaterialSelectionScope,
}
impl Fixture {
    fn new(bound: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut original = MaterialProgram::additive_sprite("Keyboard").normalized();
        let constant = MaterialExpressionId::new();
        let multiply = MaterialExpressionId::new();
        original.expressions.extend([
            MaterialExpression {
                id: constant,
                kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.75)),
            },
            MaterialExpression {
                id: multiply,
                kind: MaterialExpressionKind::Multiply(constant, constant),
            },
        ]);
        original.node_constants.push(constant);
        if bound {
            original.outputs.alpha = multiply;
        }
        original = original.normalized();
        original
            .save_ron(root.path().join("keyboard.aestra.material.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
        session
            .open_material_program(&catalog, original.id)
            .unwrap();
        let selected = BTreeSet::from([constant, multiply]);
        let scope = Some(crate::docking::EditorViewId(7));
        let graph_key = material_graph_view_key(original.id);
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
        .init_resource::<clipboard::GraphClipboard>()
        .init_resource::<MaterialGraphSelectionState>()
        .init_resource::<MaterialGraphPreviewState>()
        .init_resource::<MaterialGraphPaletteState>()
        .init_resource::<MaterialStackInspectorState>()
        .init_resource::<GraphViewportMemory>()
        .init_resource::<MaterialProgramEditHistory>()
        .init_resource::<EditorHistoryLedger>()
        .init_resource::<FunctionEditor>()
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
            .select_material_expressions(
                scope,
                original.id,
                &selected,
                GraphSelectionMode::Replace,
            );
        let mut nodes = vec![];
        for (index, expression) in selected.iter().copied().enumerate() {
            let position = Vec2::new(50.0 + index as f32 * 250.0, 80.0 + index as f32 * 40.0);
            let key = material_graph_expression_node_key(expression);
            app.world_mut()
                .resource_mut::<GraphViewportMemory>()
                .place_node(&graph_key, &key, position);
            nodes.push(
                app.world_mut()
                    .spawn((
                        MaterialGraphAction {
                            program: original.id,
                            expression,
                        },
                        FeathersGraphNode::clipboard_fixture(graph_key.clone(), key, position),
                    ))
                    .id(),
            );
        }
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        let viewport = app
            .world_mut()
            .spawn((
                MaterialGraphViewport {
                    program: original.id,
                    scope,
                    editing_target: MaterialEditingTarget::Program {
                        root: root.path().into(),
                        id: original.id,
                    },
                },
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
            nodes,
            original,
            selected,
            scope,
        }
    }
    fn current(&self) -> MaterialProgram {
        self.app
            .world()
            .resource::<EditorSession>()
            .graph_material_programs(self.app.world().resource::<ProjectEffectCatalog>())
            .unwrap()
            .into_iter()
            .find(|program| program.id == self.original.id)
            .unwrap()
    }
    fn selection(&self) -> BTreeSet<MaterialExpressionId> {
        self.app
            .world()
            .resource::<MaterialGraphSelectionState>()
            .get(self.scope)
            .unwrap()
            .expressions
            .clone()
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
    fn tap(&mut self, code: KeyCode, logical: Key) {
        self.key(code, logical.clone(), ButtonState::Pressed, false);
        self.key(code, logical, ButtonState::Released, false);
    }
    fn chord(&mut self, code: KeyCode, text: &str, shift: bool) {
        self.key(
            KeyCode::ControlLeft,
            Key::Control,
            ButtonState::Pressed,
            false,
        );
        if shift {
            self.key(KeyCode::ShiftLeft, Key::Shift, ButtonState::Pressed, false);
        }
        self.tap(code, Key::Character(text.into()));
        if shift {
            self.key(KeyCode::ShiftLeft, Key::Shift, ButtonState::Released, false);
        }
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
        assert_eq!(
            self.app
                .world()
                .resource::<EditorSession>()
                .operation_order
                .edit_serial(),
            0
        );
    }
    fn assert_disk_unchanged(&self) {
        assert_eq!(
            MaterialProgram::load_ron(self.root.path().join("keyboard.aestra.material.ron"))
                .unwrap(),
            self.original
        );
    }
    fn select_connection(&mut self) {
        let mut state = self
            .app
            .world_mut()
            .resource_mut::<MaterialGraphSelectionState>();
        let selected = state.entry(self.scope);
        selected.expressions.clear();
        selected.connection = Some(MaterialGraphConnection {
            program: self.original.id,
            source: self.original.outputs.alpha,
            target: MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Alpha),
        });
    }
}

#[test]
fn same_frame_copy_cut_paste_remap_wires_and_compound_history_restores_source() {
    let mut f = Fixture::new(false);
    for (key, text) in [
        (KeyCode::KeyC, "c"),
        (KeyCode::KeyX, "x"),
        (KeyCode::KeyV, "v"),
    ] {
        f.chord(key, text, false);
    }
    f.update();
    let pasted = f.current();
    assert_eq!(pasted.expressions.len(), f.original.expressions.len());
    assert_eq!(f.selection().len(), 2);
    assert!(f.selection().is_disjoint(&f.selected));
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
    f.step(HistoryAction::Undo);
    f.step(HistoryAction::Undo);
    assert_eq!(f.current(), f.original);
    f.step(HistoryAction::Redo);
    f.step(HistoryAction::Redo);
    assert_eq!(f.current(), pasted);
    f.assert_disk_unchanged();
}

#[test]
fn duplicate_selects_new_nodes_and_delete_undo_restores_preview_and_layout() {
    let mut f = Fixture::new(false);
    f.chord(KeyCode::KeyD, "d", false);
    f.update();
    let duplicated = f.current();
    let selection = f.selection();
    assert_eq!(
        duplicated.expressions.len(),
        f.original.expressions.len() + 2
    );
    assert!(selection.is_disjoint(&f.selected));
    f.app
        .world_mut()
        .resource_mut::<MaterialGraphPreviewState>()
        .set_visible(
            f.original.id,
            selection
                .iter()
                .copied()
                .map(MaterialGraphPreviewTarget::Expression),
            true,
        );
    f.tap(KeyCode::Delete, Key::Delete);
    f.update();
    assert_eq!(f.current(), f.original);
    assert!(f.selection().is_empty());
    for id in &selection {
        assert!(
            !f.app
                .world()
                .resource::<MaterialGraphPreviewState>()
                .is_visible(f.original.id, MaterialGraphPreviewTarget::Expression(*id))
        );
    }
    f.step(HistoryAction::Undo);
    assert_eq!(f.current(), duplicated);
    for id in selection {
        assert!(
            f.app
                .world()
                .resource::<MaterialGraphPreviewState>()
                .is_visible(f.original.id, MaterialGraphPreviewTarget::Expression(id))
        );
        assert!(
            f.app
                .world()
                .resource::<GraphViewportMemory>()
                .node_position(
                    &material_graph_view_key(f.original.id),
                    &material_graph_expression_node_key(id)
                )
                .is_some()
        );
    }
}

#[test]
fn extraction_chord_publishes_function_and_call_placement_as_one_undoable_edit() {
    let mut f = Fixture::new(true);
    f.chord(KeyCode::KeyE, "e", true);
    f.update();
    let extracted = f.current();
    let functions = f
        .app
        .world()
        .resource::<ProjectEffectCatalog>()
        .material_functions()
        .unwrap();
    assert_eq!(
        functions.len(),
        1,
        "{}",
        f.app.world().resource::<EditorSession>().status
    );
    assert!(
        extracted
            .expressions
            .iter()
            .any(|node| matches!(node.kind, MaterialExpressionKind::FunctionCall { .. }))
    );
    assert_eq!(f.selection().len(), 1);
    let call = *f.selection().first().unwrap();
    assert_eq!(
        f.app
            .world()
            .resource::<GraphViewportMemory>()
            .node_position(
                &material_graph_view_key(f.original.id),
                &material_graph_expression_node_key(call)
            ),
        Some(Vec2::new(175.0, 100.0))
    );
    f.step(HistoryAction::Undo);
    assert_eq!(f.current(), f.original);
    f.step(HistoryAction::Redo);
    assert_eq!(f.current(), extracted);
    f.assert_disk_unchanged();
}

#[test]
fn delete_selected_connection_resets_typed_output_and_undo_restores_wire() {
    let mut f = Fixture::new(true);
    f.select_connection();
    f.tap(KeyCode::Delete, Key::Delete);
    f.update();
    let disconnected = f.current();
    assert_ne!(disconnected.outputs.alpha, f.original.outputs.alpha);
    assert!(
        disconnected
            .expressions
            .iter()
            .any(|node| node.id == disconnected.outputs.alpha
                && matches!(
                    node.kind,
                    MaterialExpressionKind::Constant(MaterialValue::Float(_))
                ))
    );
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphSelectionState>()
            .selected_connection(f.scope)
            .is_none()
    );
    f.step(HistoryAction::Undo);
    assert_eq!(f.current(), f.original);
    f.step(HistoryAction::Redo);
    assert_eq!(f.current(), disconnected);
}

#[test]
fn extraction_requires_both_event_time_modifiers_and_never_uses_released_shift() {
    let mut f = Fixture::new(true);
    f.chord(KeyCode::KeyE, "e", false);
    f.update();
    f.assert_unchanged();
    f.key(KeyCode::ShiftLeft, Key::Shift, ButtonState::Pressed, false);
    f.key(KeyCode::ShiftLeft, Key::Shift, ButtonState::Released, false);
    f.chord(KeyCode::KeyE, "e", false);
    f.update();
    f.assert_unchanged();
    f.chord(KeyCode::KeyP, "E", true);
    f.update();
    assert_eq!(
        f.app
            .world()
            .resource::<ProjectEffectCatalog>()
            .material_functions()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn paste_uses_actual_node_positions_and_pointer_unprojection_not_stale_memory() {
    let mut f = Fixture::new(false);
    let graph = material_graph_view_key(f.original.id);
    for id in &f.selected {
        f.app
            .world_mut()
            .resource_mut::<GraphViewportMemory>()
            .place_node(
                &graph,
                material_graph_expression_node_key(*id),
                Vec2::splat(-500.0),
            );
    }
    f.app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(f.viewport)
        .unwrap()
        .normalized = Some(Vec2::new(0.1, -0.2));
    f.chord(KeyCode::KeyC, "c", false);
    f.chord(KeyCode::KeyV, "v", false);
    f.update();
    let positions: Vec<_> = f
        .selection()
        .iter()
        .map(|id| {
            f.app
                .world()
                .resource::<GraphViewportMemory>()
                .node_position(&graph, &material_graph_expression_node_key(*id))
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
fn invalid_live_node_positions_reject_selection_edits_without_partial_history() {
    let mut f = Fixture::new(false);
    let entity = f.nodes[0];
    let expression = f
        .app
        .world()
        .get::<MaterialGraphAction>(entity)
        .unwrap()
        .expression;
    f.app
        .world_mut()
        .entity_mut(entity)
        .insert(FeathersGraphNode::clipboard_fixture(
            material_graph_view_key(f.original.id),
            material_graph_expression_node_key(expression),
            Vec2::splat(f32::NAN),
        ));
    f.chord(KeyCode::KeyD, "d", false);
    f.update();
    f.assert_unchanged();
    assert_eq!(f.selection(), f.selected);
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .status
            .contains("invalid graph positions")
    );
}

#[test]
fn mismatched_tab_target_rejects_edits_without_touching_the_source() {
    let mut f = Fixture::new(false);
    f.app
        .world_mut()
        .get_mut::<MaterialGraphViewport>(f.viewport)
        .unwrap()
        .editing_target = MaterialEditingTarget::Program {
        root: f.root.path().into(),
        id: MaterialProgramId::new(),
    };
    f.chord(KeyCode::KeyD, "d", false);
    f.update();
    f.assert_unchanged();
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .status
            .contains("no longer matches this tab")
    );
}

#[test]
fn hovered_material_tab_activates_its_target_instead_of_the_previous_program() {
    let mut f = Fixture::new(false);
    let other = MaterialProgram::additive_sprite("Other").normalized();
    other
        .save_ron(f.root.path().join("other.aestra.material.ron"))
        .unwrap();
    let catalog = ProjectEffectCatalog::scan(f.root.path());
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .open_material_program(&catalog, other.id)
        .unwrap();
    f.app.insert_resource(catalog);
    f.chord(KeyCode::KeyD, "d", false);
    f.update();
    assert_eq!(
        f.app
            .world()
            .resource::<EditorSession>()
            .standalone_material(),
        Some(f.original.id)
    );
    assert_eq!(
        f.current().expressions.len(),
        f.original.expressions.len() + 2
    );
}

#[test]
fn no_hover_or_another_views_selection_cannot_edit_this_material() {
    let mut f = Fixture::new(false);
    f.app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(f.viewport)
        .unwrap()
        .cursor_over = false;
    f.chord(KeyCode::KeyX, "x", false);
    f.update();
    f.assert_unchanged();
    f.app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(f.viewport)
        .unwrap()
        .cursor_over = true;
    f.app
        .world_mut()
        .get_mut::<MaterialGraphViewport>(f.viewport)
        .unwrap()
        .scope = Some(crate::docking::EditorViewId(99));
    f.chord(KeyCode::KeyX, "x", false);
    f.update();
    f.assert_unchanged();
    assert!(
        f.app
            .world()
            .resource::<clipboard::GraphClipboard>()
            .fragment
            .is_none()
    );
}

#[test]
fn logical_letters_event_time_control_and_repeat_suppression_preserve_discrete_edits() {
    let mut f = Fixture::new(false);
    f.chord(KeyCode::KeyD, "p", false);
    f.update();
    f.assert_unchanged();
    f.chord(KeyCode::KeyP, "C", false);
    f.tap(KeyCode::KeyV, Key::Character("v".into()));
    f.update();
    f.assert_unchanged();
    f.chord(KeyCode::KeyP, "v", false);
    f.update();
    let pasted = f.current();
    assert_eq!(pasted.expressions.len(), f.original.expressions.len() + 2);
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
        true,
    );
    f.update();
    f.update();
    assert_eq!(f.current(), pasted);
}

#[test]
fn text_and_code_editor_ancestors_keep_clipboard_and_delete_keys() {
    for code in [false, true] {
        let mut f = Fixture::new(false);
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
        f.chord(KeyCode::KeyX, "x", false);
        f.tap(KeyCode::Delete, Key::Delete);
        f.update();
        f.assert_unchanged();
        assert!(
            f.app
                .world()
                .resource::<clipboard::GraphClipboard>()
                .fragment
                .is_none()
        );
    }
}

#[test]
fn palettes_and_document_modals_block_without_stale_replay_on_close() {
    let mut f = Fixture::new(false);
    for state in MaterialGraphPaletteState::blocking_states_for_test() {
        f.app.insert_resource(state);
        f.chord(KeyCode::KeyD, "d", false);
        f.update();
        f.assert_unchanged();
    }
    f.app.insert_resource(MaterialGraphPaletteState::default());
    for state in DocumentProtectionState::blocking_states_for_test() {
        f.app.insert_resource(state);
        f.chord(KeyCode::KeyD, "d", false);
        f.update();
        f.assert_unchanged();
    }
    f.app.insert_resource(DocumentProtectionState::default());
    f.update();
    f.assert_unchanged();
    f.chord(KeyCode::KeyD, "d", false);
    f.update();
    assert_eq!(
        f.current().expressions.len(),
        f.original.expressions.len() + 2
    );
}

#[test]
fn foreign_clipboard_dependencies_reject_paste_without_changing_selection_or_history() {
    let mut f = Fixture::new(false);
    let id = MaterialExpressionId::new();
    let foreign = MaterialExpressionId::new();
    f.app
        .world_mut()
        .resource_mut::<clipboard::GraphClipboard>()
        .fragment = Some(
        clipboard::Fragment::capture(
            &[MaterialExpression {
                id,
                kind: MaterialExpressionKind::Multiply(foreign, foreign),
            }],
            &BTreeSet::from([id]),
            BTreeMap::new(),
            &[],
            &[],
        )
        .unwrap(),
    );
    f.chord(KeyCode::KeyV, "v", false);
    f.update();
    f.assert_unchanged();
    assert_eq!(f.selection(), f.selected);
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .status
            .contains("input nodes are missing")
    );
}

#[test]
fn delete_without_selection_and_paste_without_payload_never_create_history() {
    let mut f = Fixture::new(false);
    f.app
        .world_mut()
        .resource_mut::<MaterialGraphSelectionState>()
        .select_material_expressions(
            f.scope,
            f.original.id,
            &BTreeSet::new(),
            GraphSelectionMode::Replace,
        );
    f.tap(KeyCode::Delete, Key::Delete);
    f.chord(KeyCode::KeyV, "v", false);
    f.update();
    f.assert_unchanged();
}
