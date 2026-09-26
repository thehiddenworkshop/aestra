use super::*;
use bevy::ecs::system::RunSystemOnce;

fn program() -> (MaterialProgram, BTreeSet<MaterialExpressionId>) {
    let mut program = MaterialProgram::additive_sprite("Clipboard").normalized();
    let constant = MaterialExpressionId::new();
    let multiply = MaterialExpressionId::new();
    program.expressions.extend([
        MaterialExpression {
            id: constant,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.75)),
        },
        MaterialExpression {
            id: multiply,
            kind: MaterialExpressionKind::Multiply(constant, constant),
        },
    ]);
    program.node_constants.push(constant);
    (program.normalized(), BTreeSet::from([constant, multiply]))
}

fn app(
    root: &std::path::Path,
    program: &MaterialProgram,
    selected: &BTreeSet<MaterialExpressionId>,
) -> App {
    program
        .save_ron(root.join("clipboard.aestra.material.ron"))
        .unwrap();
    let catalog = ProjectEffectCatalog::scan(root);
    let mut session = crate::test_support::session_with_timing_slack();
    session.open_material_program(&catalog, program.id).unwrap();
    let mut app = App::new();
    app.insert_resource(session)
        .insert_resource(catalog)
        .init_resource::<GraphClipboard>()
        .init_resource::<MaterialGraphSelectionState>()
        .init_resource::<MaterialGraphPreviewState>()
        .init_resource::<MaterialGraphPaletteState>()
        .init_resource::<MaterialStackInspectorState>()
        .init_resource::<GraphViewportMemory>()
        .init_resource::<MaterialProgramEditHistory>()
        .init_resource::<EditorHistoryLedger>()
        .init_resource::<crate::material_function_editor::FunctionEditor>()
        .init_resource::<ButtonInput<KeyCode>>()
        .add_systems(Update, material_graph_keyboard_input)
        .add_observer(crate::history::execute_history_action);
    let scope = Some(crate::docking::EditorViewId(7));
    app.world_mut()
        .resource_mut::<MaterialGraphSelectionState>()
        .select_material_expressions(scope, program.id, selected, GraphSelectionMode::Replace);
    for (index, id) in program
        .expressions
        .iter()
        .filter(|node| selected.contains(&node.id))
        .map(|node| node.id)
        .enumerate()
    {
        app.world_mut()
            .resource_mut::<GraphViewportMemory>()
            .place_node(
                material_graph_view_key(program.id),
                material_graph_expression_node_key(id),
                Vec2::new(80.0 + index as f32 * 250.0, 100.0),
            );
    }
    let marker = MaterialGraphViewport {
        program: program.id,
        scope,
        editing_target: crate::material_document::MaterialEditingTarget::Program {
            root: root.to_owned(),
            id: program.id,
        },
    };
    let program_id = program.id;
    app.world_mut()
        .run_system_once(move |mut commands: Commands| {
            commands.spawn(Node::default()).with_children(|parent| {
                let viewport = spawn_graph_viewport(
                    parent,
                    GraphViewportProps {
                        key: material_graph_view_key(program_id),
                        content_size: Vec2::splat(1000.0),
                        selection_bounds: None,
                        initial_view: Some((Vec2::ZERO, 1.0)),
                    },
                    (),
                    |_| {},
                    |_| {},
                );
                parent.commands().entity(viewport).insert((
                    marker.clone(),
                    RelativeCursorPosition {
                        cursor_over: true,
                        normalized: None,
                    },
                ));
            });
        })
        .unwrap();
    app
}

fn tap(app: &mut App, letter: KeyCode) {
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.reset_all();
    keys.press(KeyCode::ControlLeft);
    keys.press(letter);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
}

fn source(app: &App, id: MaterialProgramId) -> MaterialProgram {
    app.world()
        .resource::<ProjectEffectCatalog>()
        .material_program(id)
        .unwrap()
}

#[test]
fn node_menu_copy_cut_paste_preserve_multi_selection_links_and_undo() {
    let root = tempfile::tempdir().unwrap();
    let (program, selected) = program();
    let mut app = app(root.path(), &program, &selected);
    let scope = Some(crate::docking::EditorViewId(7));
    let invoke = |app: &mut App, action| {
        app.world_mut()
            .resource_mut::<MaterialGraphPaletteState>()
            .node_menu = Some(MaterialGraphNodeMenuOpen {
            program: program.id,
            scope,
            menu_position: Vec2::new(500.0, 400.0),
        });
        let item = app
            .world_mut()
            .spawn((
                MaterialGraphContextAction::Clipboard(program.id, action),
                FeathersActionButton,
                PendingFeathersActivation,
                Interaction::Pressed,
            ))
            .id();
        app.world_mut()
            .run_system_once(handle_material_graph_context_actions)
            .unwrap();
        app.world_mut().despawn(item);
        assert!(
            app.world()
                .resource::<MaterialGraphPaletteState>()
                .node_menu
                .is_none()
        );
    };
    invoke(&mut app, Shortcut::Copy);
    assert_eq!(source(&app, program.id), program);
    assert_eq!(
        app.world()
            .resource::<GraphClipboard>()
            .fragment
            .as_ref()
            .unwrap()
            .len(),
        2
    );
    invoke(&mut app, Shortcut::Cut);
    assert_eq!(
        source(&app, program.id).expressions.len(),
        program.expressions.len() - 2
    );
    invoke(&mut app, Shortcut::Paste);
    let pasted = source(&app, program.id);
    assert_eq!(pasted.expressions.len(), program.expressions.len());
    let created = &app
        .world()
        .resource::<MaterialGraphSelectionState>()
        .get(scope)
        .unwrap()
        .expressions;
    assert_eq!(created.len(), 2);
    assert!(created.is_disjoint(&selected));
    let new_constant = pasted
        .expressions
        .iter()
        .find(|node| {
            created.contains(&node.id)
                && node.kind == MaterialExpressionKind::Constant(MaterialValue::Float(0.75))
        })
        .unwrap()
        .id;
    assert!(
        pasted
            .expressions
            .iter()
            .any(|node| created.contains(&node.id)
                && node.kind == MaterialExpressionKind::Multiply(new_constant, new_constant))
    );
    app.world_mut().trigger(crate::history::HistoryAction::Undo);
    app.world_mut().flush();
    assert_eq!(
        source(&app, program.id).expressions.len(),
        program.expressions.len() - 2
    );
    app.world_mut().trigger(crate::history::HistoryAction::Undo);
    app.world_mut().flush();
    assert_eq!(source(&app, program.id), program);
}

#[test]
fn clipboard_remaps_internal_links_and_retains_metadata_and_relative_positions() {
    let (mut program, selected) = program();
    let a = program
        .expressions
        .iter()
        .find(|node| node.kind == MaterialExpressionKind::Constant(MaterialValue::Float(0.75)))
        .unwrap()
        .id;
    let b = program
        .expressions
        .iter()
        .find(|node| matches!(node.kind, MaterialExpressionKind::Multiply(..)))
        .unwrap()
        .id;
    program.disabled_expressions.push(b);
    let fragment = Fragment::capture(
        &program.expressions,
        &selected,
        BTreeMap::from([(a, Vec2::new(50.0, 20.0)), (b, Vec2::new(300.0, 100.0))]),
        &program.disabled_expressions,
        &program.node_constants,
    )
    .unwrap();
    let insert = fragment
        .instantiate(
            &BTreeSet::new(),
            fragment.offset_to(Vec2::new(500.0, 400.0)),
        )
        .unwrap();
    let new_a = *insert.constants.iter().next().unwrap();
    let new_b = *insert.disabled.iter().next().unwrap();
    assert!(!selected.contains(&new_a));
    assert!(!selected.contains(&new_b));
    assert_eq!(
        insert
            .expressions
            .iter()
            .find(|node| node.id == new_b)
            .unwrap()
            .kind,
        MaterialExpressionKind::Multiply(new_a, new_a)
    );
    assert_eq!(insert.positions[&new_a], Vec2::new(500.0, 400.0));
    assert_eq!(insert.positions[&new_b], Vec2::new(750.0, 480.0));
    assert!(insert.disabled.contains(&new_b));
    assert!(insert.constants.contains(&new_a));
    assert_ne!(
        insert.expressions[0].id,
        fragment
            .instantiate(&BTreeSet::new(), Vec2::ZERO)
            .unwrap()
            .expressions[0]
            .id
    );
}

#[test]
fn missing_external_source_is_rejected_without_silently_copying_extra_nodes() {
    let (program, _) = program();
    let node = program
        .expressions
        .iter()
        .find(|node| matches!(node.kind, MaterialExpressionKind::Multiply(..)))
        .unwrap();
    let fragment = Fragment::capture(
        &program.expressions,
        &BTreeSet::from([node.id]),
        BTreeMap::new(),
        &[],
        &[],
    )
    .unwrap();
    assert!(fragment.instantiate(&BTreeSet::new(), Vec2::ZERO).is_err());
    assert!(
        fragment
            .instantiate(
                &program.expressions.iter().map(|node| node.id).collect(),
                Vec2::ZERO
            )
            .is_ok()
    );
}

#[test]
fn multi_copy_paste_and_duplicate_select_only_new_nodes_and_are_undoable() {
    let root = tempfile::tempdir().unwrap();
    let (program, selected) = program();
    let mut app = app(root.path(), &program, &selected);
    let scope = Some(crate::docking::EditorViewId(7));
    let other = Some(crate::docking::EditorViewId(8));
    app.world_mut()
        .resource_mut::<MaterialGraphSelectionState>()
        .select_material_expressions(other, program.id, &selected, GraphSelectionMode::Replace);
    tap(&mut app, KeyCode::KeyC);
    assert_eq!(source(&app, program.id), program);
    // Paste into an empty selection must work, including a graph with no selected program.
    app.world_mut()
        .resource_mut::<MaterialGraphSelectionState>()
        .scopes
        .remove(&scope);
    tap(&mut app, KeyCode::KeyV);
    let pasted = source(&app, program.id);
    assert_eq!(pasted.expressions.len(), program.expressions.len() + 2);
    let original_ids = program
        .expressions
        .iter()
        .map(|node| node.id)
        .collect::<BTreeSet<_>>();
    let created = pasted
        .expressions
        .iter()
        .filter(|node| !original_ids.contains(&node.id))
        .map(|node| node.id)
        .collect::<BTreeSet<_>>();
    let selection = app.world().resource::<MaterialGraphSelectionState>();
    assert_eq!(selection.get(scope).unwrap().expressions, created);
    assert_eq!(selection.get(other).unwrap().expressions, selected);
    let memory = app.world().resource::<GraphViewportMemory>();
    for original in program
        .expressions
        .iter()
        .filter(|node| selected.contains(&node.id))
    {
        let new = pasted
            .expressions
            .iter()
            .find(|node| {
                created.contains(&node.id)
                    && std::mem::discriminant(&node.kind) == std::mem::discriminant(&original.kind)
            })
            .unwrap();
        let original_position = memory
            .node_position(
                &material_graph_view_key(program.id),
                &material_graph_expression_node_key(original.id),
            )
            .unwrap();
        assert_eq!(
            memory.node_position(
                &material_graph_view_key(program.id),
                &material_graph_expression_node_key(new.id)
            ),
            Some(original_position + Vec2::splat(24.0))
        );
    }
    app.world_mut().trigger(crate::history::HistoryAction::Undo);
    app.world_mut().flush();
    assert_eq!(source(&app, program.id), program);
    app.world_mut().trigger(crate::history::HistoryAction::Redo);
    app.world_mut().flush();
    assert_eq!(source(&app, program.id), pasted);
    tap(&mut app, KeyCode::KeyD);
    let duplicated = source(&app, program.id);
    let new_selection = &app
        .world()
        .resource::<MaterialGraphSelectionState>()
        .get(scope)
        .unwrap()
        .expressions;
    assert_eq!(new_selection.len(), 2);
    assert!(new_selection.is_disjoint(&created));
    assert_eq!(duplicated.expressions.len(), pasted.expressions.len() + 2);
    assert_eq!(
        app.world()
            .resource::<GraphClipboard>()
            .fragment
            .as_ref()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn multi_cut_keeps_snapshot_for_paste_after_sources_are_gone() {
    let root = tempfile::tempdir().unwrap();
    let (program, selected) = program();
    let mut app = app(root.path(), &program, &selected);
    tap(&mut app, KeyCode::KeyX);
    let cut = source(&app, program.id);
    assert!(
        cut.expressions
            .iter()
            .all(|node| !selected.contains(&node.id))
    );
    tap(&mut app, KeyCode::KeyV);
    let pasted = source(&app, program.id);
    assert_eq!(pasted.expressions.len(), program.expressions.len());
    assert!(
        pasted
            .expressions
            .iter()
            .all(|node| !selected.contains(&node.id))
    );
    assert_eq!(
        app.world()
            .resource::<MaterialGraphSelectionState>()
            .get(Some(crate::docking::EditorViewId(7)))
            .unwrap()
            .expressions
            .len(),
        2
    );
}

#[test]
fn editable_text_owns_graph_clipboard_shortcuts() {
    let root = tempfile::tempdir().unwrap();
    let (program, selected) = program();
    let mut app = app(root.path(), &program, &selected);
    let text = app.world_mut().spawn(EditableText::default()).id();
    let mut focus = InputFocus::default();
    focus.set(text, FocusCause::Pressed);
    app.insert_resource(focus);
    tap(&mut app, KeyCode::KeyX);
    tap(&mut app, KeyCode::KeyD);
    assert_eq!(source(&app, program.id), program);
    assert!(app.world().resource::<GraphClipboard>().fragment.is_none());
}

#[test]
fn copied_socket_defaults_are_independent_and_survive_cut_cleanup() {
    let mut program = MaterialProgram::additive_sprite("Defaults").normalized();
    let a = MaterialExpressionId::new();
    let b = MaterialExpressionId::new();
    let multiply = MaterialExpressionId::new();
    program.expressions.extend([
        MaterialExpression {
            id: a,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(2.0)),
        },
        MaterialExpression {
            id: b,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(3.0)),
        },
        MaterialExpression {
            id: multiply,
            kind: MaterialExpressionKind::Multiply(a, b),
        },
    ]);
    let fragment = Fragment::capture(
        &program.expressions,
        &BTreeSet::from([multiply]),
        BTreeMap::from([(multiply, Vec2::splat(50.0))]),
        &[],
        &[],
    )
    .unwrap()
    .with_inline_defaults(&program);
    let insert = fragment
        .instantiate(&BTreeSet::new(), Vec2::splat(24.0))
        .unwrap();
    assert_eq!(fragment.len(), 1);
    assert_eq!(insert.expressions.len(), 3);
    assert_eq!(
        insert.positions.len(),
        1,
        "only the explicit node is selected and placed"
    );
    let node = insert
        .expressions
        .iter()
        .find(|node| insert.positions.contains_key(&node.id))
        .unwrap();
    let dependencies = node.kind.dependencies();
    assert!(!dependencies.contains(&a));
    assert!(!dependencies.contains(&b));
    assert!(
        dependencies
            .iter()
            .all(|id| insert.expressions.iter().any(|node| node.id == *id))
    );
}

#[test]
fn duplicating_an_output_constant_keeps_the_new_node_visible_and_selected() {
    let root = tempfile::tempdir().unwrap();
    let program = MaterialProgram::additive_sprite("Constant").normalized();
    let selected = BTreeSet::from([program.outputs.color]);
    let mut app = app(root.path(), &program, &selected);
    tap(&mut app, KeyCode::KeyD);
    let after = source(&app, program.id);
    let selected = &app
        .world()
        .resource::<MaterialGraphSelectionState>()
        .get(Some(crate::docking::EditorViewId(7)))
        .unwrap()
        .expressions;
    assert_eq!(selected.len(), 1);
    assert!(!selected.contains(&program.outputs.color));
    let id = *selected.iter().next().unwrap();
    assert!(after.node_constants.contains(&id));
    assert!(
        after
            .normalized()
            .expressions
            .iter()
            .any(|node| node.id == id)
    );
    assert_eq!(after.outputs, program.outputs);
}
