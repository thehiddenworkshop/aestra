use super::*;
use crate::history::{
    EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, execute_history_action,
};
use crate::material_function_editor::FunctionEditor;

fn fixture() -> (
    tempfile::TempDir,
    App,
    MaterialProgram,
    aestra_core::material::MaterialFunction,
) {
    let root = tempfile::tempdir().unwrap();
    let program = MaterialProgram::additive_sprite("History").normalized();
    program
        .save_ron(root.path().join("test.aestra.material.ron"))
        .unwrap();
    let function = aestra_core::material::MaterialFunction::from_ron(include_str!(
        "../../../../../assets/test/materials/dissolve_edge.aestra.material-function.ron"
    ))
    .unwrap();
    function
        .save_ron(root.path().join("test.aestra.material-function.ron"))
        .unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let mut session = crate::test_support::session_with_timing_slack();
    session.open_material_program(&catalog, program.id).unwrap();
    let mut app = App::new();
    app.insert_resource(catalog)
        .insert_resource(session)
        .init_resource::<GraphViewportMemory>()
        .init_resource::<MaterialGraphPreviewState>()
        .init_resource::<MaterialProgramEditHistory>()
        .init_resource::<EditorHistoryLedger>()
        .init_resource::<FunctionEditor>()
        .add_observer(execute_history_action)
        .add_observer(node_edit)
        .add_observer(pin_edit);
    (root, app, program, function)
}

#[test]
fn arrangement_uses_projected_canvas_nodes_not_inline_semantic_constants() {
    let root = tempfile::tempdir().unwrap();
    let mut program = MaterialProgram::additive_sprite("Inline arrangement");
    let old_alpha = program.outputs.alpha;
    let inline = MaterialExpressionId::new();
    let multiply = MaterialExpressionId::new();
    program.expressions.extend([
        MaterialExpression {
            id: inline,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
        },
        MaterialExpression {
            id: multiply,
            kind: MaterialExpressionKind::Multiply(old_alpha, inline),
        },
    ]);
    program.outputs.alpha = multiply;
    let program = program.normalized();
    program
        .save_ron(root.path().join("inline.aestra.material.ron"))
        .unwrap();
    let catalog = ProjectEffectCatalog::scan(root.path());
    let mut session = crate::test_support::session_with_timing_slack();
    session.open_material_program(&catalog, program.id).unwrap();
    let graph = material_graph_view_key(program.id);
    let (_, keys) = semantic(&graph, &catalog, &session).unwrap();
    let projection = MaterialCompiler.project_graph(&program, None);
    let projected = projection
        .nodes
        .iter()
        .map(|node| material_graph_expression_node_key(node.expression))
        .chain([MATERIAL_GRAPH_OUTPUT_NODE_KEY.into()])
        .collect::<BTreeSet<_>>();

    assert!(!keys.contains(&material_graph_expression_node_key(inline)));
    assert!(!keys.contains(&material_graph_expression_node_key(old_alpha)));
    assert_eq!(keys, projected);
}

#[test]
fn arrangement_is_one_exact_undo_and_redo() {
    let (_root, mut app, program, _) = fixture();
    let graph = material_graph_view_key(program.id);
    let before = Snapshot::capture(
        &graph,
        app.world().resource::<ProjectEffectCatalog>(),
        app.world().resource::<EditorSession>(),
        app.world().resource::<GraphViewportMemory>(),
    )
    .unwrap();
    let revision = app
        .world()
        .resource::<GraphViewportMemory>()
        .placement_revision(&graph);
    let arranged = before
        .keys
        .iter()
        .enumerate()
        .map(|(index, key)| (key.clone(), Vec2::new(index as f32 * 240.0, 80.0)))
        .collect::<BTreeMap<_, _>>();
    arrange(app.world_mut(), before, revision, arranged.clone()).unwrap();
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph)
            .into_iter()
            .map(|(key, (position, _))| (key, position))
            .collect::<BTreeMap<_, _>>(),
        arranged
    );

    step(&mut app, true);
    assert!(
        app.world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph)
            .is_empty()
    );
    step(&mut app, false);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph)
            .into_iter()
            .map(|(key, (position, _))| (key, position))
            .collect::<BTreeMap<_, _>>(),
        arranged
    );
}

#[test]
fn explicit_pin_is_one_presentation_undo_and_manual_move_keeps_it() {
    let (_root, mut app, program, _) = fixture();
    let graph = material_graph_view_key(program.id);
    let node = MATERIAL_GRAPH_OUTPUT_NODE_KEY;
    let original = Vec2::new(320.0, 96.0);
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node(&graph, node, original, false);
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_pinned(&graph, node, true);
    app.world_mut().trigger(GraphPinEdit {
        graph: graph.clone(),
        node: node.into(),
        before: false,
        after: true,
    });
    app.world_mut().flush();

    assert!(
        app.world()
            .resource::<GraphViewportMemory>()
            .is_pinned(&graph, node)
    );
    step(&mut app, true);
    assert!(
        !app.world()
            .resource::<GraphViewportMemory>()
            .is_pinned(&graph, node)
    );
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node_position(&graph, node),
        Some(original)
    );
    step(&mut app, false);
    assert!(
        app.world()
            .resource::<GraphViewportMemory>()
            .is_pinned(&graph, node)
    );
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node_position(&graph, node),
        Some(original)
    );

    let moved = Vec2::new(480.0, 128.0);
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node(&graph, node, moved, false);
    assert!(
        app.world()
            .resource::<GraphViewportMemory>()
            .is_pinned(&graph, node)
    );
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node_position(&graph, node),
        Some(moved)
    );
}

#[test]
fn arrangement_rejects_manual_placement_changes_without_mutating_other_nodes() {
    let (_root, mut app, program, _) = fixture();
    let graph = material_graph_view_key(program.id);
    let before = Snapshot::capture(
        &graph,
        app.world().resource::<ProjectEffectCatalog>(),
        app.world().resource::<EditorSession>(),
        app.world().resource::<GraphViewportMemory>(),
    )
    .unwrap();
    let revision = app
        .world()
        .resource::<GraphViewportMemory>()
        .placement_revision(&graph);
    let changed = before.keys.iter().next().unwrap().clone();
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .place_node(&graph, &changed, Vec2::new(17.0, 31.0));
    let arranged = before
        .keys
        .iter()
        .map(|key| (key.clone(), Vec2::new(500.0, 500.0)))
        .collect();
    assert!(arrange(app.world_mut(), before, revision, arranged).is_err());
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph),
        BTreeMap::from([(changed, (Vec2::new(17.0, 31.0), false))])
    );
}

#[test]
fn palette_creation_placement_is_one_undo_and_failed_creation_leaves_it_unchanged() {
    use bevy::ecs::system::RunSystemOnce;
    let (_root, mut app, program, _) = fixture();
    app.init_resource::<MaterialStackInspectorState>()
        .init_resource::<MaterialGraphPaletteState>()
        .init_resource::<MaterialGraphSelectionState>();
    let graph = material_graph_view_key(program.id);
    let effect = app.world().resource::<EditorSession>().effect.clone();
    let editing_target = app
        .world()
        .resource::<EditorSession>()
        .material_target
        .clone();
    let action = MaterialGraphPaletteAction {
        program: program.id,
        editing_target,
        scope: None,
        kind: MaterialGraphCreateKind::Function(aestra_compiler::MaterialGraphFunction::Multiply),
        source: None,
        target: None,
        label: "Multiply".into(),
        graph_position: Vec2::new(300.0, 200.0),
        graph_key: graph.clone(),
        searchable: "multiply".into(),
    };
    app.world_mut().spawn((
        action.clone(),
        FeathersActionButton,
        Interaction::Pressed,
        PendingFeathersActivation,
    ));
    app.world_mut()
        .run_system_once(handle_material_graph_palette_actions)
        .unwrap();
    let after = app
        .world()
        .resource::<GraphViewportMemory>()
        .base_nodes(&graph);
    assert!(!after.is_empty());
    let positions = after
        .values()
        .map(|(position, _)| *position)
        .collect::<Vec<_>>();
    for (index, position) in positions.iter().enumerate() {
        assert!(!positions[index + 1..].contains(position));
    }
    step(&mut app, true);
    assert_eq!(
        app.world()
            .resource::<ProjectEffectCatalog>()
            .material_program(program.id)
            .unwrap(),
        program
    );
    assert!(
        app.world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph)
            .is_empty()
    );
    step(&mut app, false);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph),
        after
    );
    let mut invalid = action;
    invalid.program = MaterialProgramId::new();
    app.world_mut().spawn((
        invalid,
        FeathersActionButton,
        Interaction::Pressed,
        PendingFeathersActivation,
    ));
    app.world_mut()
        .run_system_once(handle_material_graph_palette_actions)
        .unwrap();
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph),
        after
    );
    assert_eq!(app.world().resource::<EditorSession>().effect, effect);
}

fn step(app: &mut App, undo: bool) {
    app.world_mut().trigger(if undo {
        HistoryAction::Undo
    } else {
        HistoryAction::Redo
    });
    app.world_mut().flush();
}

fn move_node(app: &mut App, graph: &str, node: &str, before: (Vec2, bool), after: (Vec2, bool)) {
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node(graph, node, after.0, after.1);
    app.world_mut().trigger(GraphPresentationEdit {
        graph: graph.into(),
        node: node.into(),
        before,
        after,
    });
    app.world_mut().flush();
}

fn edit_material(
    app: &mut App,
    before: &MaterialProgram,
    after: MaterialProgram,
    placement: Option<(&str, &str, Vec2)>,
) {
    let mut session = app.world_mut().remove_resource::<EditorSession>().unwrap();
    let mut catalog = app
        .world_mut()
        .remove_resource::<ProjectEffectCatalog>()
        .unwrap();
    let snapshot = Snapshot::capture(
        &material_graph_view_key(before.id),
        &catalog,
        &session,
        app.world().resource::<GraphViewportMemory>(),
    )
    .unwrap();
    app.world_mut()
        .resource_mut::<MaterialProgramEditHistory>()
        .execute_replacement(
            &mut session,
            &mut catalog,
            "Semantic edit",
            before.clone(),
            after,
        )
        .unwrap();
    app.world_mut()
        .resource_mut::<EditorHistoryLedger>()
        .record_material_edit(&mut session);
    if let Some((graph, node, position)) = placement {
        app.world_mut()
            .resource_mut::<GraphViewportMemory>()
            .place_node(graph, node, position);
    }
    let mut previews = app
        .world_mut()
        .remove_resource::<MaterialGraphPreviewState>()
        .unwrap();
    snapshot.attach_with_previews(
        &catalog,
        &mut session,
        &mut app.world_mut().resource_mut::<GraphViewportMemory>(),
        &mut previews,
        before.id,
    );
    app.insert_resource(session)
        .insert_resource(catalog)
        .insert_resource(previews);
}

#[test]
fn mixed_semantic_move_collapse_and_preview_follow_one_chronology_without_compilation() {
    let (_root, mut app, original, _) = fixture();
    let graph = material_graph_view_key(original.id);
    let node = material_graph_expression_node_key(original.expressions[0].id);
    let before_effect = app.world().resource::<EditorSession>().effect.clone();
    let mut renamed = original.clone();
    renamed.name = "Edited shader".into();
    edit_material(&mut app, &original, renamed.clone(), None);
    let revision = app
        .world()
        .resource::<ProjectEffectCatalog>()
        .content_revision();
    let document_revision = app.world().resource::<EditorSession>().document_revision();
    let base = Vec2::new(30.0, 45.0);
    let moved = Vec2::new(270.0, -92.0);
    move_node(&mut app, &graph, &node, (base, false), (moved, false));
    move_node(&mut app, &graph, &node, (moved, false), (moved, true));
    let snapshot = Snapshot::capture(
        &graph,
        app.world().resource::<ProjectEffectCatalog>(),
        app.world().resource::<EditorSession>(),
        app.world().resource::<GraphViewportMemory>(),
    )
    .unwrap();
    let visible = BTreeSet::from([MaterialGraphPreviewTarget::Output]);
    app.world_mut()
        .resource_mut::<MaterialGraphPreviewState>()
        .visible
        .insert((original.id, MaterialGraphPreviewTarget::Output));
    record(
        app.world_mut(),
        Transaction {
            before: snapshot.clone(),
            after: snapshot,
            previews: Some((original.id, BTreeSet::new(), visible)),
            invalidated: false,
        },
    );
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_view(format!("{graph}#view:1"), Vec2::splat(444.0), 0.75);
    for expected in [(moved, true), (moved, false), (base, false)] {
        step(&mut app, true);
        assert_eq!(
            app.world()
                .resource::<GraphViewportMemory>()
                .node(&graph, &node),
            Some(expected)
        );
        assert_eq!(
            app.world()
                .resource::<ProjectEffectCatalog>()
                .content_revision(),
            revision
        );
        assert_eq!(
            app.world().resource::<EditorSession>().document_revision(),
            document_revision
        );
        assert_eq!(
            app.world()
                .resource::<ProjectEffectCatalog>()
                .material_program(original.id)
                .unwrap(),
            renamed
        );
    }
    assert!(
        !app.world()
            .resource::<MaterialGraphPreviewState>()
            .is_visible(original.id, MaterialGraphPreviewTarget::Output)
    );
    step(&mut app, true);
    assert_eq!(
        app.world()
            .resource::<ProjectEffectCatalog>()
            .material_program(original.id)
            .unwrap(),
        original
    );
    for _ in 0..4 {
        step(&mut app, false);
    }
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node(&graph, &node),
        Some((moved, true))
    );
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .view(&format!("{graph}#view:1")),
        Some((Vec2::splat(444.0), 0.75))
    );
    assert_eq!(
        app.world().resource::<EditorSession>().effect,
        before_effect
    );
    assert!(
        app.world()
            .resource::<MaterialGraphPreviewState>()
            .is_visible(original.id, MaterialGraphPreviewTarget::Output)
    );
}

#[test]
fn function_and_material_placement_history_are_independent_and_new_move_forks_redo() {
    let (_root, mut app, program, function) = fixture();
    let material_key = material_graph_view_key(program.id);
    let function_key = function_graph_memory_key(
        app.world().resource::<ProjectEffectCatalog>().root(),
        function.id,
    );
    move_node(
        &mut app,
        &material_key,
        "output",
        (Vec2::ZERO, false),
        (Vec2::ONE, true),
    );
    let catalog = app.world().resource::<ProjectEffectCatalog>().clone();
    app.world_mut()
        .resource_mut::<EditorSession>()
        .open_material_function(&catalog, function.id)
        .unwrap();
    move_node(
        &mut app,
        &function_key,
        "outputs",
        (Vec2::ZERO, false),
        (Vec2::splat(50.0), false),
    );
    step(&mut app, true);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node(&material_key, "output"),
        Some((Vec2::ONE, true))
    );
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node(&function_key, "outputs"),
        Some((Vec2::ZERO, false))
    );
    move_node(
        &mut app,
        &function_key,
        "outputs",
        (Vec2::ZERO, false),
        (Vec2::splat(80.0), false),
    );
    step(&mut app, false);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node(&function_key, "outputs"),
        Some((Vec2::splat(80.0), false))
    );
    app.world_mut()
        .resource_mut::<EditorSession>()
        .open_material_program(&catalog, program.id)
        .unwrap();
    step(&mut app, true);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node(&material_key, "output"),
        Some((Vec2::ZERO, false))
    );
}

#[test]
fn compound_insertion_and_deletion_restore_exact_authored_positions() {
    let (_root, mut app, program, _) = fixture();
    let graph = material_graph_view_key(program.id);
    let expression = aestra_core::material::MaterialExpression {
        id: MaterialExpressionId::new(),
        kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
    };
    let node = material_graph_expression_node_key(expression.id);
    let mut inserted = program.clone();
    inserted.expressions.push(expression.clone());
    inserted.node_constants.push(expression.id);
    let position = Vec2::new(-222.0, 517.0);
    edit_material(
        &mut app,
        &program,
        inserted.clone(),
        Some((&graph, &node, position)),
    );
    step(&mut app, true);
    assert_eq!(
        app.world()
            .resource::<ProjectEffectCatalog>()
            .material_program(program.id)
            .unwrap(),
        program
    );
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node(&graph, &node),
        None
    );
    step(&mut app, false);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node(&graph, &node),
        Some((position, false))
    );
    edit_material(&mut app, &inserted, program.clone(), None);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node(&graph, &node),
        None
    );
    step(&mut app, true);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node(&graph, &node),
        Some((position, false))
    );
    assert!(
        app.world()
            .resource::<ProjectEffectCatalog>()
            .material_program(program.id)
            .unwrap()
            .expressions
            .iter()
            .any(|e| e.id == expression.id)
    );
}

#[test]
fn stale_history_rejects_reload_missing_nodes_and_project_switch_without_mutation() {
    let (_root, mut app, program, _) = fixture();
    let graph = material_graph_view_key(program.id);
    move_node(
        &mut app,
        &graph,
        "output",
        (Vec2::ZERO, false),
        (Vec2::ONE, false),
    );
    app.world_mut()
        .resource_mut::<EditorSession>()
        .operation_order
        .invalidate_layouts(Some(&graph));
    step(&mut app, true);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node(&graph, "output"),
        Some((Vec2::ONE, false))
    );
    assert!(
        app.world()
            .resource::<EditorSession>()
            .status
            .contains("stale")
    );
    let other = tempfile::tempdir().unwrap();
    program
        .save_ron(other.path().join("same.aestra.material.ron"))
        .unwrap();
    app.insert_resource(ProjectEffectCatalog::scan(other.path()));
    step(&mut app, true);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node(&graph, "output"),
        Some((Vec2::ONE, false))
    );
}

#[test]
fn failed_compound_semantic_undo_keeps_layout_and_history_intact() {
    let (_root, mut app, original, _) = fixture();
    let graph = material_graph_view_key(original.id);
    let mut renamed = original.clone();
    renamed.name = "Changed".into();
    edit_material(
        &mut app,
        &original,
        renamed.clone(),
        Some((&graph, "output", Vec2::ONE)),
    );
    let mut external = renamed.clone();
    external.name = "External replacement".into();
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .replace_material_program(&renamed, &external)
        .unwrap();
    step(&mut app, true);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node(&graph, "output"),
        Some((Vec2::ONE, false))
    );
    assert_eq!(
        app.world()
            .resource::<ProjectEffectCatalog>()
            .material_program(original.id)
            .unwrap(),
        external
    );
    assert!(
        app.world()
            .resource::<EditorSession>()
            .operation_order
            .layout(
                &Context::current(app.world().resource::<EditorSession>()),
                true
            )
            .is_some()
    );
}

#[test]
fn deleting_a_previewed_node_restores_its_visibility_before_undoing_the_toggle() {
    let (_root, mut app, program, _) = fixture();
    let graph = material_graph_view_key(program.id);
    let expression = aestra_core::material::MaterialExpression {
        id: MaterialExpressionId::new(),
        kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
    };
    let target = MaterialGraphPreviewTarget::Expression(expression.id);
    let mut inserted = program.clone();
    inserted.expressions.push(expression.clone());
    inserted.node_constants.push(expression.id);
    edit_material(&mut app, &program, inserted.clone(), None);
    let snapshot = Snapshot::capture(
        &graph,
        app.world().resource::<ProjectEffectCatalog>(),
        app.world().resource::<EditorSession>(),
        app.world().resource::<GraphViewportMemory>(),
    )
    .unwrap();
    app.world_mut()
        .resource_mut::<MaterialGraphPreviewState>()
        .visible
        .insert((program.id, target));
    record(
        app.world_mut(),
        Transaction {
            before: snapshot.clone(),
            after: snapshot,
            previews: Some((program.id, BTreeSet::new(), BTreeSet::from([target]))),
            invalidated: false,
        },
    );
    edit_material(&mut app, &inserted, program.clone(), None);
    assert!(
        !app.world()
            .resource::<MaterialGraphPreviewState>()
            .is_visible(program.id, target)
    );
    step(&mut app, true);
    assert!(
        app.world()
            .resource::<MaterialGraphPreviewState>()
            .is_visible(program.id, target)
    );
    step(&mut app, true);
    assert!(
        !app.world()
            .resource::<MaterialGraphPreviewState>()
            .is_visible(program.id, target)
    );
}

#[test]
fn function_semantics_and_layout_share_history_and_noop_does_not_replace_compound() {
    let (_root, mut app, _, function) = fixture();
    let mut session = app.world_mut().remove_resource::<EditorSession>().unwrap();
    let mut catalog = app
        .world_mut()
        .remove_resource::<ProjectEffectCatalog>()
        .unwrap();
    session
        .open_material_function(&catalog, function.id)
        .unwrap();
    let graph = function_graph_memory_key(catalog.root(), function.id);
    let snapshot = Snapshot::capture(
        &graph,
        &catalog,
        &session,
        app.world().resource::<GraphViewportMemory>(),
    )
    .unwrap();
    let mut changed = function.clone();
    changed.name = "Renamed function".into();
    app.world_mut()
        .resource_mut::<FunctionEditor>()
        .edit(&mut session, &mut catalog, changed.clone())
        .unwrap();
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node(&graph, "outputs", Vec2::splat(100.0), true);
    snapshot.attach(
        &catalog,
        &mut session,
        &mut app.world_mut().resource_mut::<GraphViewportMemory>(),
    );
    let snapshot = Snapshot::capture(
        &graph,
        &catalog,
        &session,
        app.world().resource::<GraphViewportMemory>(),
    )
    .unwrap();
    app.world_mut()
        .resource_mut::<FunctionEditor>()
        .edit(&mut session, &mut catalog, changed)
        .unwrap();
    snapshot.attach(
        &catalog,
        &mut session,
        &mut app.world_mut().resource_mut::<GraphViewportMemory>(),
    );
    app.insert_resource(session).insert_resource(catalog);
    move_node(
        &mut app,
        &graph,
        "outputs",
        (Vec2::splat(100.0), true),
        (Vec2::splat(220.0), true),
    );
    step(&mut app, true);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node(&graph, "outputs"),
        Some((Vec2::splat(100.0), true))
    );
    step(&mut app, true);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node(&graph, "outputs"),
        None
    );
    assert_eq!(
        app.world()
            .resource::<EditorSession>()
            .graph_function(app.world().resource::<ProjectEffectCatalog>())
            .unwrap(),
        function
    );
    step(&mut app, false);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node(&graph, "outputs"),
        Some((Vec2::splat(100.0), true))
    );
}

#[test]
fn source_removal_and_discard_do_not_resurrect_old_layout_history() {
    let (root, mut app, program, _) = fixture();
    let graph = material_graph_view_key(program.id);
    move_node(
        &mut app,
        &graph,
        "output",
        (Vec2::ZERO, false),
        (Vec2::ONE, true),
    );
    std::fs::remove_file(root.path().join("test.aestra.material.ron")).unwrap();
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .refresh();
    step(&mut app, true);
    assert!(
        app.world()
            .resource::<EditorSession>()
            .status
            .contains("stale")
    );
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node(&graph, "output"),
        Some((Vec2::ONE, true))
    );
    let target = app
        .world()
        .resource::<EditorSession>()
        .material_target
        .clone();
    crate::history::clear_document_order(app.world_mut(), target);
    assert!(
        app.world()
            .resource::<EditorSession>()
            .operation_order
            .layout(
                &Context::current(app.world().resource::<EditorSession>()),
                true
            )
            .is_none()
    );
}
