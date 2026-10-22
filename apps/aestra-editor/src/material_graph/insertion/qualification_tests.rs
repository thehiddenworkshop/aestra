//! Registered semantic observers, using actual saved sources and global history.
use super::*;
use crate::feathers::node_graph::GraphPresentationEdit;
use crate::material_document::MaterialEditingTarget;
use crate::material_graph::keys::{
    function_graph_memory_key, material_graph_expression_node_key, material_graph_view_key,
};
use aestra_authoring::MaterialExpressionInput;
use aestra_core::material::{MaterialExpression, MaterialValue};

struct Fixture {
    app: App,
    _root: tempfile::TempDir,
    candidate: Candidate,
    edit: GraphPresentationEdit,
    viewport: Entity,
    target: MaterialEditingTarget,
}

impl Fixture {
    fn new(function: bool) -> Self {
        let (document, mut candidate) = super::tests::fixture(function);
        let root = tempfile::tempdir().unwrap();
        candidate.view.document.project = root.path().into();
        document.programs[0]
            .save_ron(root.path().join("insert.aestra.material.ron"))
            .unwrap();
        document.material_functions[0]
            .save_ron(root.path().join("insert.aestra.material-function.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
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
        let target = session.material_target.clone();
        let marker = if function {
            asset_drop::GraphDropTarget::function(&session, document.material_functions[0].id)
        } else {
            asset_drop::GraphDropTarget::program(&session, document.programs[0].id)
        };
        let GraphNodeKey::Expression(inserted) = candidate.node else {
            unreachable!()
        };
        let edit = GraphPresentationEdit {
            graph,
            node: if function {
                inserted.to_string()
            } else {
                material_graph_expression_node_key(inserted)
            },
            before: (Vec2::new(20.0, 350.0), false),
            after: (Vec2::new(260.0, 70.0), false),
            origin: None,
        };
        // A mounted unfocused source must route by its own identity, not the active effect.
        session.material_target = MaterialEditingTarget::EffectInstance;
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(catalog)
            .init_resource::<GraphViewportMemory>()
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<crate::material_function_editor::FunctionEditor>()
            .add_observer(crate::history::execute_history_action);
        register(&mut app);
        let viewport = app
            .world_mut()
            .spawn((
                GraphGeometryView {
                    key: candidate.view.clone(),
                    nodes: default(),
                },
                marker,
            ))
            .id();
        app.world_mut()
            .resource_mut::<GraphViewportMemory>()
            .set_node(&edit.graph, &edit.node, edit.after.0, false);
        app.update(); // Exercise the registrar's real Update refresh system as well.
        Self {
            app,
            _root: root,
            candidate,
            edit,
            viewport,
            target,
        }
    }

    fn document(&self) -> MaterialAuthoringDocument {
        self.app
            .world()
            .resource::<EditorSession>()
            .graph_authoring_document_for(
                &self.target,
                self.app.world().resource::<ProjectEffectCatalog>(),
            )
            .unwrap()
    }

    fn probe(&mut self, candidate: Option<Candidate>) {
        self.app.world_mut().trigger(widget::Probe(candidate));
        self.app.world_mut().flush();
    }

    fn drop(&mut self) {
        self.app.world_mut().trigger(widget::Drop {
            candidate: self.candidate.clone(),
            edit: self.edit.clone(),
            before_offset: Vec2::new(0.0, 40.0),
        });
        self.app.world_mut().flush();
    }

    fn history(&mut self, action: crate::history::HistoryAction) {
        self.app.world_mut().trigger(action);
        self.app.world_mut().flush();
    }
}

#[test]
fn registered_insertion_routes_unfocused_sources_and_creates_one_compound_edit() {
    for function in [false, true] {
        let mut fixture = Fixture::new(function);
        let before = fixture.document();
        let effect = fixture
            .app
            .world()
            .resource::<EditorSession>()
            .effect
            .clone();
        fixture.probe(Some(fixture.candidate.clone()));
        assert!(fixture.app.world().resource::<widget::State>().allowed);
        assert_eq!(
            fixture.app.world().resource::<EditorSession>().status,
            "Release to insert node · Alt: move only"
        );
        assert_eq!(
            fixture
                .app
                .world()
                .resource::<EditorSession>()
                .material_target,
            MaterialEditingTarget::EffectInstance
        );
        fixture.drop();
        assert_eq!(
            fixture.app.world().resource::<EditorSession>().status,
            "Inserted node on wire"
        );
        assert_eq!(
            fixture
                .app
                .world()
                .resource::<EditorSession>()
                .material_target,
            fixture.target
        );
        let after = fixture.document();
        assert_ne!(after, before);
        assert_eq!(
            fixture
                .app
                .world()
                .resource::<GraphViewportMemory>()
                .node(&fixture.edit.graph, &fixture.edit.node),
            Some(fixture.edit.after)
        );
        fixture.history(crate::history::HistoryAction::Undo);
        assert_eq!(fixture.document(), before);
        assert_eq!(
            fixture
                .app
                .world()
                .resource::<GraphViewportMemory>()
                .node(&fixture.edit.graph, &fixture.edit.node),
            Some(fixture.edit.before)
        );
        fixture.history(crate::history::HistoryAction::Undo);
        assert_eq!(fixture.document(), before);
        fixture.history(crate::history::HistoryAction::Redo);
        assert_eq!(fixture.document(), after);
        assert_eq!(
            fixture
                .app
                .world()
                .resource::<GraphViewportMemory>()
                .node(&fixture.edit.graph, &fixture.edit.node),
            Some(fixture.edit.after)
        );
        assert_eq!(
            fixture.app.world().resource::<EditorSession>().effect,
            effect
        );
    }
}

#[test]
fn registered_probe_rejects_wrong_project_view_asset_and_spacing_without_edits() {
    for function in [false, true] {
        let mut fixture = Fixture::new(function);
        let before = fixture.document();
        let mut wrong_project = fixture.candidate.clone();
        wrong_project.view.document.project = fixture._root.path().join("other");
        let mut wrong_view = fixture.candidate.clone();
        wrong_view.view.view = Some(crate::docking::EditorViewId(99));
        let mut wrong_asset = fixture.candidate.clone();
        wrong_asset.view.document.asset =
            crate::document::DocumentKey::MaterialProgram(aestra_core::MaterialProgramId::new());
        let mut blocked = fixture.candidate.clone();
        blocked.spacing = Err("No room beside a protected node".into());
        for (candidate, expected) in [
            (wrong_project, "Project changed"),
            (wrong_view, "Graph view is unavailable"),
            (wrong_asset, "Graph view is unavailable"),
            (blocked, "No room beside a protected node"),
        ] {
            fixture.probe(Some(fixture.candidate.clone()));
            assert!(fixture.app.world().resource::<widget::State>().allowed);
            fixture.probe(Some(candidate));
            assert!(!fixture.app.world().resource::<widget::State>().allowed);
            assert!(
                fixture
                    .app
                    .world()
                    .resource::<EditorSession>()
                    .status
                    .contains(expected)
            );
            assert_eq!(fixture.document(), before);
            assert_eq!(
                fixture
                    .app
                    .world()
                    .resource::<GraphViewportMemory>()
                    .node(&fixture.edit.graph, &fixture.edit.node),
                Some(fixture.edit.after)
            );
        }
        fixture.probe(None);
        assert_eq!(
            fixture.app.world().resource::<EditorSession>().status,
            "Move node"
        );
        fixture.history(crate::history::HistoryAction::Undo);
        assert_eq!(fixture.document(), before);
    }
}

#[test]
fn registered_drop_revalidates_retired_or_retargeted_views_after_green_feedback() {
    for function in [false, true] {
        for retire in [false, true] {
            let mut fixture = Fixture::new(function);
            let before = fixture.document();
            fixture.probe(Some(fixture.candidate.clone()));
            assert!(fixture.app.world().resource::<widget::State>().allowed);
            if retire {
                fixture.app.world_mut().despawn(fixture.viewport);
            } else {
                let session = fixture.app.world().resource::<EditorSession>();
                let marker = asset_drop::GraphDropTarget::program_for(
                    session,
                    aestra_core::MaterialProgramId::new(),
                    &MaterialEditingTarget::Program {
                        root: fixture._root.path().into(),
                        id: aestra_core::MaterialProgramId::new(),
                    },
                );
                fixture
                    .app
                    .world_mut()
                    .entity_mut(fixture.viewport)
                    .insert(marker);
            }
            fixture.drop();
            assert!(
                fixture
                    .app
                    .world()
                    .resource::<EditorSession>()
                    .status
                    .starts_with("Insertion cancelled:")
            );
            assert_eq!(fixture.document(), before);
            let memory = fixture.app.world().resource::<GraphViewportMemory>();
            assert_eq!(
                memory.node(&fixture.edit.graph, &fixture.edit.node),
                Some(fixture.edit.before)
            );
            assert_eq!(
                memory.node_position(&fixture.edit.graph, &fixture.edit.node),
                Some(fixture.edit.before.0 + Vec2::new(0.0, 40.0))
            );
            fixture.history(crate::history::HistoryAction::Undo);
            assert_eq!(fixture.document(), before);
        }
    }
}

#[test]
fn missing_function_editor_rolls_back_preserved_bootstrap_spacing() {
    let mut fixture = Fixture::new(true);
    let before = fixture.document();
    fixture.candidate.spacing = Ok(widget::Spacing::test_move(
        "outputs",
        None,
        Vec2::new(420.0, 70.0),
        Vec2::new(508.0, 70.0),
    ));
    fixture.probe(Some(fixture.candidate.clone()));
    assert!(fixture.app.world().resource::<widget::State>().allowed);
    assert!(
        fixture
            .app
            .world()
            .resource::<EditorSession>()
            .status
            .contains("move 1 nearby nodes")
    );
    fixture
        .app
        .world_mut()
        .remove_resource::<crate::material_function_editor::FunctionEditor>();
    fixture.drop();
    assert_eq!(
        fixture.app.world().resource::<EditorSession>().status,
        "Insertion cancelled: Function editor is unavailable"
    );
    assert_eq!(fixture.document(), before);
    let memory = fixture.app.world().resource::<GraphViewportMemory>();
    assert_eq!(memory.node(&fixture.edit.graph, "outputs"), None);
    assert_eq!(
        memory.node(&fixture.edit.graph, &fixture.edit.node),
        Some(fixture.edit.before)
    );
    // Global history requires its normal editor resources; restore the intentionally missing one
    // after exercising the insertion adapter's optional-resource failure path.
    fixture
        .app
        .init_resource::<crate::material_function_editor::FunctionEditor>();
    fixture.history(crate::history::HistoryAction::Undo);
    assert_eq!(fixture.document(), before);
}

#[test]
fn literal_input_replacement_deduplicates_choices_without_bypassing_the_bound() {
    for function in [false, true] {
        let (mut document, mut candidate) = super::tests::fixture(function);
        let GraphNodeKey::Expression(inserted) = candidate.node else {
            unreachable!()
        };
        let expressions = if function {
            &mut document.material_functions[0].expressions
        } else {
            &mut document.programs[0].expressions
        };
        let literal = MaterialExpressionId::new();
        expressions.last_mut().unwrap().kind =
            MaterialExpressionKind::DerivativeX { value: literal };
        expressions.push(MaterialExpression {
            id: literal,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
        });
        let before = document.clone();
        candidate.inputs = vec![MaterialExpressionInput::Value; 16];
        let replacement = plan(&document, &candidate).unwrap();
        let expressions = match &replacement {
            Replacement::Program(_, program) => &program.expressions,
            Replacement::Function(function) => &function.expressions,
        };
        let GraphNodeKey::Expression(source) = candidate.wire.source else {
            unreachable!()
        };
        assert!(
            matches!(expressions.iter().find(|expression| expression.id == inserted).unwrap().kind,
            MaterialExpressionKind::DerivativeX { value } if value == source)
        );
        candidate.inputs.push(MaterialExpressionInput::Value);
        assert!(
            plan(&document, &candidate)
                .err()
                .unwrap()
                .contains("Too many input choices")
        );
        assert_eq!(document, before);
    }
}
