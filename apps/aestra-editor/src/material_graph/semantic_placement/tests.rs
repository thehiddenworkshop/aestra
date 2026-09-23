use super::*;
use crate::{
    history::{HistoryAction, execute_history_action},
    material_function_editor::FunctionEditor,
};
use aestra_core::material::{MaterialExpression, MaterialExpressionKind};
use bevy::ecs::system::RunSystemOnce;

fn fixture() -> (tempfile::TempDir, App, MaterialProgram, MaterialFunction) {
    let root = tempfile::tempdir().unwrap();
    let program = MaterialProgram::additive_sprite("Semantic placement").normalized();
    program
        .save_ron(root.path().join("test.aestra.material.ron"))
        .unwrap();
    let function = MaterialFunction::from_ron(include_str!(
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
        .add_observer(execute_history_action);
    (root, app, program, function)
}

fn edit_program(
    app: &mut App,
    before: MaterialProgram,
    after: MaterialProgram,
) -> Result<(), String> {
    app.world_mut()
        .run_system_once(
            move |mut placement: Context,
                  mut session: ResMut<EditorSession>,
                  mut catalog: ResMut<ProjectEffectCatalog>,
                  mut history: ResMut<MaterialProgramEditHistory>,
                  mut ledger: ResMut<EditorHistoryLedger>| {
                placement.program(
                    &mut session,
                    &mut catalog,
                    &mut history,
                    "Semantic creation",
                    before.clone(),
                    after.clone(),
                )?;
                ledger.record_material_edit(&mut session);
                Ok(())
            },
        )
        .unwrap()
}

fn edit_function(app: &mut App, after: MaterialFunction) -> Result<(), String> {
    app.world_mut()
        .run_system_once(
            move |mut placement: Context,
                  mut session: ResMut<EditorSession>,
                  mut catalog: ResMut<ProjectEffectCatalog>,
                  mut editor: ResMut<FunctionEditor>| {
                placement.function(&mut session, &mut catalog, &mut editor, after.clone())
            },
        )
        .unwrap()
}

fn step(app: &mut App, undo: bool) {
    app.world_mut().trigger(if undo {
        HistoryAction::Undo
    } else {
        HistoryAction::Redo
    });
    app.world_mut().flush();
}

fn replacement(program: &MaterialProgram) -> MaterialProgram {
    let mut document = MaterialAuthoringDocument::standalone(vec![program.clone()]);
    let plan = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::CreateMaterialGraphNode {
            program: program.id,
            kind: MaterialGraphCreateKind::Function(aestra_compiler::MaterialGraphFunction::Remap),
            source: Some(program.outputs.color),
            target: Some(MaterialConnectionTarget::ProgramOutput(
                MaterialOutputSocket::Color,
            )),
        },
    )
    .unwrap();
    MaterialCommandExecutor::execute(&mut document, &plan.transaction).unwrap();
    let plan = MaterialToolPlanner::plan(
        &document,
        MaterialToolCommand::CreateMaterialGraphNode {
            program: program.id,
            kind: MaterialGraphCreateKind::Function(
                aestra_compiler::MaterialGraphFunction::Multiply,
            ),
            source: Some(program.outputs.alpha),
            target: Some(MaterialConnectionTarget::ProgramOutput(
                MaterialOutputSocket::Alpha,
            )),
        },
    )
    .unwrap();
    MaterialCommandExecutor::execute(&mut document, &plan.transaction).unwrap();
    document.programs.remove(0)
}

#[test]
fn semantic_program_batch_preserves_manual_and_bootstrap_bases_in_one_undo() {
    let (_root, mut app, before, _) = fixture();
    let after = replacement(&before);
    let graph = material_graph_view_key(before.id);
    let after_inline = after.inline_constants();
    let manual = MATERIAL_GRAPH_OUTPUT_NODE_KEY.to_owned();
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node(&graph, &manual, Vec2::new(-720.5, 910.25), true);
    let initial = app
        .world()
        .resource::<GraphViewportMemory>()
        .base_nodes(&graph);
    let effect = app.world().resource::<EditorSession>().effect.clone();
    let baseline = Model::program(
        &before,
        app.world().resource::<ProjectEffectCatalog>(),
        &default(),
    )
    .unwrap();
    edit_program(&mut app, before.clone(), after.clone()).unwrap();
    let bases = app
        .world()
        .resource::<GraphViewportMemory>()
        .base_nodes(&graph);
    assert_eq!(bases[&manual], initial[&manual]);
    for (key, node) in &baseline.nodes {
        if matches!(key, GraphNodeKey::Expression(id) if after_inline.contains(id)) {
            continue;
        }
        let key = baseline.node_key(*key);
        if key != manual {
            assert_eq!(bases[&key].0, node.initial);
        }
    }
    let created = after
        .expressions
        .iter()
        .filter(|node| {
            !before.expressions.iter().any(|old| old.id == node.id)
                && !after.inline_constants().contains(&node.id)
        })
        .collect::<Vec<_>>();
    assert!(created.len() > 1);
    let positions = created
        .iter()
        .map(|node| bases[&material_graph_expression_node_key(node.id)].0)
        .collect::<Vec<_>>();
    for (index, position) in positions.iter().enumerate() {
        assert!(position.is_finite());
        assert!(!positions[index + 1..].contains(position));
    }
    assert!(
        app.world()
            .resource::<EditorSession>()
            .status
            .contains("Local spacing unavailable")
    );
    step(&mut app, true);
    assert_eq!(
        app.world()
            .resource::<ProjectEffectCatalog>()
            .material_program(before.id)
            .unwrap(),
        before
    );
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph),
        initial
    );
    step(&mut app, false);
    assert_eq!(
        app.world()
            .resource::<ProjectEffectCatalog>()
            .material_program(before.id)
            .unwrap(),
        after.normalized()
    );
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph),
        bases
    );
    assert_eq!(app.world().resource::<EditorSession>().effect, effect);
}

#[test]
fn semantic_edit_impact_preserves_unaffected_nodes_for_replace_rewire_and_delete() {
    let (_root, mut app, before, _) = fixture();
    let graph = material_graph_view_key(before.id);
    let color = before.outputs.color;
    let alternate = MaterialExpressionId::new();
    let mut expanded = before.clone();
    expanded.expressions.push(MaterialExpression {
        id: alternate,
        kind: MaterialExpressionKind::Constant(MaterialValue::ColorSrgb([0.2, 0.4, 0.8, 1.0])),
    });
    // Keep both colors as explicit graph nodes while they are temporarily disconnected.
    expanded.node_constants.extend([color, alternate]);
    let expanded = expanded.normalized();
    let catalog = app.world().resource::<ProjectEffectCatalog>();
    let initial_model = Model::program(&before, catalog, &default()).unwrap();
    let expanded_model = Model::program(&expanded, catalog, &default()).unwrap();
    let impact = EditImpact::between(&initial_model, &expanded_model);
    assert_eq!(
        impact.created,
        BTreeSet::from([GraphNodeKey::Expression(alternate)])
    );
    assert!(impact.removed.is_empty());

    edit_program(&mut app, before, expanded.clone()).unwrap();
    let expanded_bases = app
        .world()
        .resource::<GraphViewportMemory>()
        .base_nodes(&graph);
    assert_eq!(expanded_bases.len(), expanded_model.nodes.len());

    let mut replaced = expanded.clone();
    replaced
        .expressions
        .iter_mut()
        .find(|expression| expression.id == color)
        .unwrap()
        .kind = MaterialExpressionKind::Constant(MaterialValue::ColorSrgb([0.9, 0.3, 0.1, 1.0]));
    let replaced = replaced.normalized();
    let replaced_model = Model::program(
        &replaced,
        app.world().resource::<ProjectEffectCatalog>(),
        &default(),
    )
    .unwrap();
    let impact = EditImpact::between(&expanded_model, &replaced_model);
    assert_eq!(
        impact.replaced,
        BTreeSet::from([GraphNodeKey::Expression(color)])
    );
    assert!(impact.created.is_empty() && impact.removed.is_empty() && impact.rewired.is_empty());
    edit_program(&mut app, expanded, replaced.clone()).unwrap();
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph),
        expanded_bases
    );

    let mut rewired = replaced.clone();
    rewired.outputs.color = alternate;
    let rewired = rewired.normalized();
    let rewired_model = Model::program(
        &rewired,
        app.world().resource::<ProjectEffectCatalog>(),
        &default(),
    )
    .unwrap();
    let impact = EditImpact::between(&replaced_model, &rewired_model);
    assert_eq!(
        impact.rewired,
        BTreeSet::from([GraphNodeKey::MaterialOutputs])
    );
    assert!(impact.created.is_empty() && impact.removed.is_empty() && impact.replaced.is_empty());
    edit_program(&mut app, replaced, rewired.clone()).unwrap();
    let rewired_bases = app
        .world()
        .resource::<GraphViewportMemory>()
        .base_nodes(&graph);
    assert_eq!(rewired_bases, expanded_bases);

    let mut deleted = rewired.clone();
    deleted
        .expressions
        .retain(|expression| expression.id != color);
    deleted
        .node_constants
        .retain(|expression| *expression != color);
    let deleted = deleted.normalized();
    let deleted_model = Model::program(
        &deleted,
        app.world().resource::<ProjectEffectCatalog>(),
        &default(),
    )
    .unwrap();
    let impact = EditImpact::between(&rewired_model, &deleted_model);
    assert_eq!(
        impact.removed,
        BTreeSet::from([GraphNodeKey::Expression(color)])
    );
    edit_program(&mut app, rewired.clone(), deleted.clone()).unwrap();
    let deleted_bases = app
        .world()
        .resource::<GraphViewportMemory>()
        .base_nodes(&graph);
    assert!(!deleted_bases.contains_key(&material_graph_expression_node_key(color)));
    for (key, position) in &rewired_bases {
        if key != &material_graph_expression_node_key(color) {
            assert_eq!(deleted_bases.get(key), Some(position));
        }
    }

    // The semantic command and its layout pruning remain one atomic history entry.
    step(&mut app, true);
    assert_eq!(
        app.world()
            .resource::<ProjectEffectCatalog>()
            .material_program(rewired.id)
            .unwrap(),
        rewired
    );
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph),
        rewired_bases
    );
    step(&mut app, false);
    assert_eq!(
        app.world()
            .resource::<ProjectEffectCatalog>()
            .material_program(deleted.id)
            .unwrap(),
        deleted
    );
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph),
        deleted_bases
    );
}

#[test]
fn semantic_creation_failure_and_noop_do_not_change_history_or_placement() {
    let (_root, mut app, before, _) = fixture();
    let after = replacement(&before);
    edit_program(&mut app, before.clone(), after.clone()).unwrap();
    let graph = material_graph_view_key(before.id);
    let bases = app
        .world()
        .resource::<GraphViewportMemory>()
        .base_nodes(&graph);
    let serial = app
        .world()
        .resource::<EditorSession>()
        .operation_order
        .edit_serial();
    assert!(
        edit_program(&mut app, before, after.clone()).is_err(),
        "stale command must fail"
    );
    let current = app
        .world()
        .resource::<ProjectEffectCatalog>()
        .material_program(after.id)
        .unwrap();
    let mut invalid = current.clone();
    invalid.outputs.color = MaterialExpressionId::new();
    assert!(edit_program(&mut app, current.clone(), invalid).is_err());
    edit_program(&mut app, current.clone(), current.clone()).unwrap();
    let mut normalized_noop = current.clone();
    normalized_noop.expressions.push(MaterialExpression {
        id: MaterialExpressionId::new(),
        kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
    });
    edit_program(&mut app, current, normalized_noop).unwrap();
    assert_eq!(
        app.world()
            .resource::<EditorSession>()
            .operation_order
            .edit_serial(),
        serial
    );
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph),
        bases
    );
}

fn select_function(app: &mut App, id: aestra_core::MaterialFunctionId) {
    let mut session = app.world_mut().remove_resource::<EditorSession>().unwrap();
    session
        .open_material_function(app.world().resource::<ProjectEffectCatalog>(), id)
        .unwrap();
    app.insert_resource(session);
}

#[test]
fn semantic_function_creation_uses_native_history_and_preserves_existing_nodes() {
    let (root, mut app, _, before) = fixture();
    select_function(&mut app, before.id);
    let graph = function_graph_memory_key(root.path(), before.id);
    let manual = before.expressions[0].id.to_string();
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node(&graph, &manual, Vec2::new(530.5, -94.25), false);
    let initial = app
        .world()
        .resource::<GraphViewportMemory>()
        .base_nodes(&graph);
    let mut after = before.clone();
    let id = MaterialExpressionId::new();
    after.expressions.push(MaterialExpression {
        id,
        kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
    });
    after
        .outputs
        .push(aestra_core::material::MaterialFunctionOutput {
            id: aestra_core::MaterialFunctionOutputId::new(),
            name: "New output".into(),
            value_type: MaterialValueType::Float,
            expression: id,
        });
    let effect = app.world().resource::<EditorSession>().effect.clone();
    edit_function(&mut app, after.clone()).unwrap();
    let bases = app
        .world()
        .resource::<GraphViewportMemory>()
        .base_nodes(&graph);
    assert_eq!(bases[&manual], initial[&manual]);
    assert!(bases[&id.to_string()].0.x < bases["outputs"].0.x);
    step(&mut app, true);
    assert_eq!(
        app.world()
            .resource::<EditorSession>()
            .graph_function(app.world().resource::<ProjectEffectCatalog>())
            .unwrap(),
        before
    );
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph),
        initial
    );
    step(&mut app, false);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph),
        bases
    );
    let mut invalid = after;
    invalid.outputs.last_mut().unwrap().expression = MaterialExpressionId::new();
    assert!(edit_function(&mut app, invalid).is_err());
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph),
        bases
    );
    assert_eq!(app.world().resource::<EditorSession>().effect, effect);
}

#[test]
fn semantic_placement_batch_is_deterministic_and_dependency_directed() {
    let (_root, app, program, _) = fixture();
    let mut after = replacement(&program);
    let catalog = app.world().resource::<ProjectEffectCatalog>();
    let before = Model::program(&program, catalog, &default()).unwrap();
    let run = |after: &MaterialProgram| {
        let model = Model::program(after, catalog, &default()).unwrap();
        let mut area = placement::Area::default();
        for (key, node) in &before.nodes {
            area.seed(*key, node.initial, node.size);
        }
        let created = model
            .nodes
            .keys()
            .filter(|key| !before.nodes.contains_key(key))
            .copied()
            .collect();
        place_batch(&mut area, &model, created)
            .into_iter()
            .map(|(key, value)| (key, value.position))
            .collect::<BTreeMap<_, _>>()
    };
    let first = run(&after);
    after.expressions.reverse();
    assert_eq!(run(&after), first);
    let output = before.nodes[&GraphNodeKey::MaterialOutputs].initial;
    assert!(first[&GraphNodeKey::Expression(after.outputs.color)].x < output.x);
}

#[test]
fn semantic_creation_without_ui_memory_still_uses_semantic_validation() {
    let (_root, mut app, before, _) = fixture();
    app.world_mut().remove_resource::<GraphViewportMemory>();
    let after = replacement(&before);
    edit_program(&mut app, before.clone(), after).unwrap();
    assert!(app.world().get_resource::<GraphViewportMemory>().is_none());
    step(&mut app, true);
    assert_eq!(
        app.world()
            .resource::<ProjectEffectCatalog>()
            .material_program(before.id)
            .unwrap(),
        before
    );
}

#[test]
fn oversized_semantic_batches_are_rejected_before_mutation() {
    let (_root, mut app, before, _) = fixture();
    let mut after = before.clone();
    after
        .expressions
        .extend((0..513).map(|_| MaterialExpression {
            id: MaterialExpressionId::new(),
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
        }));
    after.node_constants.extend(
        after
            .expressions
            .iter()
            .filter(|node| matches!(node.kind, MaterialExpressionKind::Constant(_)))
            .map(|node| node.id),
    );
    assert!(
        edit_program(&mut app, before.clone(), after)
            .unwrap_err()
            .contains("batch limit")
    );
    assert_eq!(
        app.world()
            .resource::<EditorSession>()
            .operation_order
            .edit_serial(),
        0
    );
    assert!(
        app.world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&material_graph_view_key(before.id))
            .is_empty()
    );
}

#[test]
fn semantic_function_creation_uses_measured_view_and_rejects_stale_geometry() {
    use crate::feathers::node_graph::geometry;
    for scale in [1.0, 1.25, 2.0] {
        for stale in [false, true] {
            let (root, mut fixture_app, _, before) = fixture();
            select_function(&mut fixture_app, before.id);
            let (mut app, host) = geometry::tests::layout_app(scale);
            app.world_mut()
                .get_mut::<bevy::prelude::Node>(host)
                .unwrap()
                .flex_direction = FlexDirection::Column;
            app.insert_resource(
                fixture_app
                    .world_mut()
                    .remove_resource::<EditorSession>()
                    .unwrap(),
            )
            .insert_resource(
                fixture_app
                    .world_mut()
                    .remove_resource::<ProjectEffectCatalog>()
                    .unwrap(),
            )
            .init_resource::<FunctionEditor>();
            let graph = function_graph_memory_key(root.path(), before.id);
            app.world_mut()
                .resource_mut::<GraphViewportMemory>()
                .set_view(format!("{graph}#view:9"), Vec2::new(31.0, 29.0), 0.75);
            app.world_mut()
                .run_system_once(
                    move |mut commands: Commands,
                          session: Res<EditorSession>,
                          catalog: Res<ProjectEffectCatalog>,
                          assets: Res<AssetServer>,
                          memory: Res<GraphViewportMemory>| {
                        commands.entity(host).with_children(|parent| {
                            crate::material_function_editor::spawn_graph(
                                parent,
                                &session,
                                &catalog,
                                &assets,
                                &memory,
                                &crate::material_graph::MaterialGraphSelectionState::default(),
                                &crate::material_function_editor::graph::FunctionGraphMenuState::default(),
                                &session.material_target,
                                Some(crate::docking::EditorViewId(9)),
                            );
                        });
                    },
                )
                .unwrap();
            for _ in 0..5 {
                app.update();
            }
            let manual = before.expressions[0].id.to_string();
            if stale {
                app.world_mut()
                    .resource_mut::<GraphViewportMemory>()
                    .set_node(&graph, &manual, Vec2::new(2200.0, 450.0), false);
            }
            let model =
                Model::function(&before, app.world().resource::<ProjectEffectCatalog>()).unwrap();
            let mut model = Some(model);
            let prepared = app
                .world_mut()
                .run_system_once(
                    move |context: Context,
                          session: Res<EditorSession>,
                          catalog: Res<ProjectEffectCatalog>| {
                        context
                            .prepare(model.take().unwrap(), &session, &catalog)
                            .unwrap()
                            .unwrap()
                    },
                )
                .unwrap();
            assert_eq!(
                prepared.view.as_ref().unwrap().view,
                Some(crate::docking::EditorViewId(9))
            );
            let old_rects = prepared
                .model
                .nodes
                .keys()
                .filter_map(|key| prepared.area.rect(*key))
                .collect::<Vec<_>>();
            let mut after = before.clone();
            let id = MaterialExpressionId::new();
            after.expressions.push(MaterialExpression {
                id,
                kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
            });
            after
                .outputs
                .push(aestra_core::material::MaterialFunctionOutput {
                    id: aestra_core::MaterialFunctionOutputId::new(),
                    name: "Measured output".into(),
                    value_type: MaterialValueType::Float,
                    expression: id,
                });
            let model =
                Model::function(&after, app.world().resource::<ProjectEffectCatalog>()).unwrap();
            edit_function(&mut app, after).unwrap();
            let position = app
                .world()
                .resource::<GraphViewportMemory>()
                .node(&graph, &id.to_string())
                .unwrap()
                .0;
            let rect = Rect::from_corners(
                position,
                position + model.nodes[&GraphNodeKey::Expression(id)].size,
            );
            for old in old_rects {
                assert!(rect.intersect(old).is_empty());
            }
            assert_eq!(
                app.world()
                    .resource::<EditorSession>()
                    .status
                    .contains("Local spacing unavailable"),
                stale
            );
            if stale {
                assert_eq!(
                    app.world()
                        .resource::<GraphViewportMemory>()
                        .node(&graph, &manual)
                        .unwrap()
                        .0,
                    Vec2::new(2200.0, 450.0)
                );
            }
        }
    }
}
