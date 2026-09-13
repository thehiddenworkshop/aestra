use super::*;
use crate::document::DocumentKey;
use crate::feathers::node_graph::{GraphPresentationEdit, geometry::GraphGeometryPort};
use aestra_core::material::{MaterialFunction, MaterialFunctionOutput, MaterialSchemaVersion};

fn fixture(function: bool) -> (MaterialAuthoringDocument, Candidate) {
    let mut program = MaterialProgram::additive_sprite("Insertion");
    let inserted = MaterialExpressionId::new();
    program.expressions.push(MaterialExpression {
        id: inserted,
        kind: MaterialExpressionKind::DerivativeX {
            value: program.outputs.alpha,
        },
    });
    let function_asset = MaterialFunction {
        id: MaterialFunctionId::new(),
        name: "Insertion".into(),
        schema_version: MaterialSchemaVersion::CURRENT,
        inputs: vec![],
        outputs: vec![MaterialFunctionOutput {
            id: aestra_core::MaterialFunctionOutputId::new(),
            name: "Value".into(),
            value_type: MaterialValueType::Float,
            expression: program.outputs.alpha,
        }],
        expressions: program.expressions.clone(),
        custom_wesl: None,
    };
    let wire = if function {
        Wire {
            source: GraphNodeKey::Expression(program.outputs.alpha),
            target: GraphNodeKey::FunctionOutputs,
            port: GraphGeometryPort::FunctionOutput(function_asset.outputs[0].id),
        }
    } else {
        Wire::material(
            program.outputs.alpha,
            MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Alpha),
        )
    };
    let candidate = Candidate {
        view: GraphViewKey {
            document: GraphDocumentKey {
                project: "test".into(),
                asset: if function {
                    DocumentKey::MaterialFunction(function_asset.id)
                } else {
                    DocumentKey::MaterialProgram(program.id)
                },
            },
            view: None,
        },
        node: GraphNodeKey::Expression(inserted),
        wire,
        entity: Entity::PLACEHOLDER,
        inputs: vec![MaterialExpressionInput::Value],
    };
    (
        MaterialAuthoringDocument::standalone(vec![program])
            .with_material_functions(vec![function_asset]),
        candidate,
    )
}

#[test]
fn insertion_plans_both_graph_kinds_without_mutating_the_document() {
    for function in [false, true] {
        let (document, candidate) = fixture(function);
        let before = document.clone();
        let GraphNodeKey::Expression(inserted) = candidate.node else {
            unreachable!()
        };
        match plan(&document, &candidate).unwrap() {
            Replacement::Program(_, after) => assert_eq!(after.outputs.alpha, inserted),
            Replacement::Function(after) => assert_eq!(after.outputs[0].expression, inserted),
        }
        assert_eq!(document, before);
    }
}

#[test]
fn insertion_rejects_ambiguous_stale_and_connected_nodes() {
    for function in [false, true] {
        let (mut document, mut candidate) = fixture(function);
        let GraphNodeKey::Expression(source) = candidate.wire.source else {
            unreachable!()
        };
        let expressions = if function {
            &mut document.material_functions[0].expressions
        } else {
            &mut document.programs[0].expressions
        };
        expressions.last_mut().unwrap().kind = MaterialExpressionKind::Multiply(source, source);
        candidate.inputs = vec![
            MaterialExpressionInput::Left,
            MaterialExpressionInput::Right,
        ];
        assert!(
            plan(&document, &candidate)
                .err()
                .unwrap()
                .contains("Several inputs")
        );
        candidate.wire.source = GraphNodeKey::Expression(MaterialExpressionId::new());
        assert!(plan(&document, &candidate).is_err());
        let (mut document, candidate) = fixture(function);
        let GraphNodeKey::Expression(inserted) = candidate.node else {
            unreachable!()
        };
        let expressions = if function {
            &mut document.material_functions[0].expressions
        } else {
            &mut document.programs[0].expressions
        };
        expressions.push(MaterialExpression {
            id: MaterialExpressionId::new(),
            kind: MaterialExpressionKind::DerivativeX { value: inserted },
        });
        assert!(
            plan(&document, &candidate)
                .err()
                .unwrap()
                .contains("already connected")
        );
    }
}

#[test]
fn insertion_rejects_incompatible_inputs_without_changing_any_graph() {
    for function in [false, true] {
        let (mut document, candidate) = fixture(function);
        let expressions = if function {
            &mut document.material_functions[0].expressions
        } else {
            &mut document.programs[0].expressions
        };
        let vector = MaterialExpressionId::new();
        expressions.last_mut().unwrap().kind = MaterialExpressionKind::ExtractComponent {
            value: vector,
            component: aestra_core::material::MaterialVectorComponent::Z,
        };
        expressions.push(MaterialExpression {
            id: vector,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec3([1.0; 3])),
        });
        let before = document.clone();
        assert!(plan(&document, &candidate).is_err());
        assert_eq!(document, before);
    }
}

#[test]
fn insertion_preserves_existing_input_branches_and_rejects_cycle_candidates() {
    for function in [false, true] {
        let (mut document, candidate) = fixture(function);
        let GraphNodeKey::Expression(source) = candidate.wire.source else {
            unreachable!()
        };
        let expressions = if function {
            &mut document.material_functions[0].expressions
        } else {
            &mut document.programs[0].expressions
        };
        let branch = MaterialExpressionId::new();
        expressions.last_mut().unwrap().kind =
            MaterialExpressionKind::DerivativeX { value: branch };
        expressions.push(MaterialExpression {
            id: branch,
            kind: MaterialExpressionKind::DerivativeX { value: source },
        });
        let before = document.clone();
        assert!(plan(&document, &candidate).is_err());
        assert_eq!(document, before);

        let (mut document, candidate) = fixture(function);
        let expressions = if function {
            &mut document.material_functions[0].expressions
        } else {
            &mut document.programs[0].expressions
        };
        let GraphNodeKey::Expression(source) = candidate.wire.source else {
            unreachable!()
        };
        let GraphNodeKey::Expression(inserted) = candidate.node else {
            unreachable!()
        };
        let literal = MaterialExpressionId::new();
        expressions
            .iter_mut()
            .find(|e| e.id == inserted)
            .unwrap()
            .kind = MaterialExpressionKind::DerivativeX { value: literal };
        expressions
            .iter_mut()
            .find(|e| e.id == source)
            .unwrap()
            .kind = MaterialExpressionKind::DerivativeX { value: inserted };
        expressions.push(MaterialExpression {
            id: literal,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        });
        let before = document.clone();
        assert!(plan(&document, &candidate).is_err());
        assert_eq!(document, before);
    }
}

#[test]
fn insertion_drop_is_one_undo_redo_and_failed_drop_restores_placement() {
    for function in [false, true] {
        let (document, mut candidate) = fixture(function);
        let root = tempfile::tempdir().unwrap();
        candidate.view.document.project = root.path().into();
        document.programs[0]
            .save_ron(root.path().join("test.aestra.material.ron"))
            .unwrap();
        document.material_functions[0]
            .save_ron(root.path().join("test.aestra.material-function.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
        let effect = session.effect.clone();
        let graph = if function {
            session
                .open_material_function(&catalog, document.material_functions[0].id)
                .unwrap();
            function_graph_memory_key(root.path(), document.material_functions[0].id)
        } else {
            session
                .open_material_program(&catalog, document.programs[0].id)
                .unwrap();
            material_graph_view_key(document.programs[0].id)
        };
        let GraphNodeKey::Expression(inserted) = candidate.node else {
            unreachable!()
        };
        let edit = GraphPresentationEdit {
            graph: graph.clone(),
            node: if function {
                inserted.to_string()
            } else {
                material_graph_expression_node_key(inserted)
            },
            before: (Vec2::new(20.0, 350.0), false),
            after: (Vec2::new(260.0, 70.0), false),
        };
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(catalog)
            .init_resource::<GraphViewportMemory>()
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<crate::material_function_editor::FunctionEditor>()
            .add_observer(crate::history::execute_history_action)
            .add_observer(drop_node);
        app.world_mut()
            .resource_mut::<GraphViewportMemory>()
            .set_node(&graph, &edit.node, edit.after.0, false);
        app.world_mut().trigger(widget::Drop {
            candidate: candidate.clone(),
            edit: edit.clone(),
            before_offset: Vec2::ZERO,
        });
        assert_eq!(
            app.world().resource::<EditorSession>().status,
            "Inserted node on wire"
        );
        app.world_mut().trigger(crate::history::HistoryAction::Undo);
        app.world_mut().flush();
        assert_eq!(
            app.world()
                .resource::<GraphViewportMemory>()
                .node(&graph, &edit.node),
            Some(edit.before)
        );
        if function {
            assert_eq!(
                app.world()
                    .resource::<ProjectEffectCatalog>()
                    .material_functions()
                    .unwrap()[0]
                    .normalized(),
                document.material_functions[0].normalized()
            );
        } else {
            assert_eq!(
                app.world()
                    .resource::<ProjectEffectCatalog>()
                    .material_program(document.programs[0].id)
                    .unwrap(),
                document.programs[0].normalized()
            );
        }
        app.world_mut().trigger(crate::history::HistoryAction::Redo);
        app.world_mut().flush();
        assert_eq!(
            app.world()
                .resource::<GraphViewportMemory>()
                .node(&graph, &edit.node),
            Some(edit.after)
        );
        let before_failed = app
            .world()
            .resource::<EditorSession>()
            .graph_authoring_document(app.world().resource::<ProjectEffectCatalog>())
            .unwrap();
        let failed_edit = GraphPresentationEdit {
            before: edit.after,
            after: (Vec2::splat(900.0), false),
            ..edit
        };
        app.world_mut()
            .resource_mut::<GraphViewportMemory>()
            .set_node(&graph, &failed_edit.node, failed_edit.after.0, false);
        // The original wire no longer exists after insertion. Revalidate on drop, never reuse green feedback.
        app.world_mut().trigger(widget::Drop {
            candidate,
            edit: failed_edit.clone(),
            before_offset: Vec2::new(0.0, 40.0),
        });
        assert!(
            app.world()
                .resource::<EditorSession>()
                .status
                .starts_with("Insertion cancelled:")
        );
        assert_eq!(
            app.world()
                .resource::<GraphViewportMemory>()
                .node(&graph, &failed_edit.node),
            Some(failed_edit.before)
        );
        assert_eq!(
            app.world()
                .resource::<GraphViewportMemory>()
                .node_position(&graph, &failed_edit.node),
            Some(failed_edit.before.0 + Vec2::new(0.0, 40.0))
        );
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .graph_authoring_document(app.world().resource::<ProjectEffectCatalog>())
                .unwrap(),
            before_failed
        );
        assert_eq!(app.world().resource::<EditorSession>().effect, effect);
    }
}
