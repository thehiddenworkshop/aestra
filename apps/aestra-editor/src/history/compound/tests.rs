use super::*;
use crate::feathers::node_graph::GraphViewportMemory;
use crate::material_graph::{
    MaterialGraphPreviewState, MaterialGraphPreviewTarget,
    keys::{
        function_graph_memory_key, material_graph_expression_node_key, material_graph_view_key,
    },
    presentation::{Snapshot, State as PresentationState, Transaction},
};
use aestra_core::{
    MaterialExpressionId,
    material::{
        MaterialExpression, MaterialExpressionKind, MaterialFunction, MaterialProgram,
        MaterialValue,
    },
};
use bevy::prelude::*;
use std::{collections::BTreeSet, fs, sync::Arc};

#[derive(Resource, Default)]
struct ResultState(bool);

#[allow(
    clippy::too_many_arguments,
    reason = "Exercise the actual coupled state through Bevy system borrows"
)]
fn step(
    action: On<HistoryAction>,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<ProjectEffectCatalog>,
    mut history: ResMut<MaterialProgramEditHistory>,
    mut ledger: ResMut<EditorHistoryLedger>,
    mut functions: ResMut<FunctionEditor>,
    mut presentation: PresentationState,
    mut result: ResMut<ResultState>,
) {
    result.0 = execute(
        *action,
        &mut session,
        &mut catalog,
        &mut history,
        &mut ledger,
        &mut functions,
        &mut presentation,
    );
}

struct Fixture {
    root: tempfile::TempDir,
    app: App,
    program: MaterialProgram,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Compound history").normalized();
        program
            .save_ron(root.path().join("program.aestra.material.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
        session.open_material_program(&catalog, program.id).unwrap();
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(catalog)
            .init_resource::<GraphViewportMemory>()
            .init_resource::<MaterialGraphPreviewState>()
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<FunctionEditor>()
            .init_resource::<ResultState>()
            .add_observer(step);
        let mut f = Self { root, app, program };
        let graph = f.graph();
        f.app
            .world_mut()
            .resource_mut::<GraphViewportMemory>()
            .set_node(&graph, "output", Vec2::ZERO, false);
        f
    }
    fn graph(&self) -> String {
        material_graph_view_key(self.program.id)
    }
    fn snapshot(&self, graph: &str) -> Snapshot {
        Snapshot::capture(
            graph,
            self.app.world().resource::<ProjectEffectCatalog>(),
            self.app.world().resource::<EditorSession>(),
            self.app.world().resource::<GraphViewportMemory>(),
        )
        .unwrap()
    }
    fn step(&mut self, undo: bool) -> bool {
        self.app.world_mut().resource_mut::<ResultState>().0 = false;
        self.app.world_mut().trigger(if undo {
            HistoryAction::Undo
        } else {
            HistoryAction::Redo
        });
        self.app.world_mut().flush();
        self.app.world().resource::<ResultState>().0
    }
    fn current(&self) -> MaterialProgram {
        self.app
            .world()
            .resource::<ProjectEffectCatalog>()
            .material_program(self.program.id)
            .unwrap()
    }
    fn node(&self, key: &str) -> Option<(Vec2, bool)> {
        self.app
            .world()
            .resource::<GraphViewportMemory>()
            .node(&self.graph(), key)
    }
    fn edit(
        &mut self,
        after: MaterialProgram,
        placement: Option<(&str, Vec2, bool)>,
        attach: bool,
    ) {
        let graph = self.graph();
        let snapshot = self.snapshot(&graph);
        let before = self.current();
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
        self.app
            .world_mut()
            .resource_mut::<MaterialProgramEditHistory>()
            .execute_replacement(&mut session, &mut catalog, "Compound edit", before, after)
            .unwrap();
        self.app
            .world_mut()
            .resource_mut::<EditorHistoryLedger>()
            .record_material_edit(&mut session);
        if let Some((node, position, collapsed)) = placement {
            self.app
                .world_mut()
                .resource_mut::<GraphViewportMemory>()
                .set_node(&graph, node, position, collapsed);
        }
        if attach {
            let mut previews = self
                .app
                .world_mut()
                .remove_resource::<MaterialGraphPreviewState>()
                .unwrap();
            snapshot.attach_with_previews(
                &catalog,
                &mut session,
                &mut self.app.world_mut().resource_mut::<GraphViewportMemory>(),
                &mut previews,
                self.program.id,
            );
            self.app.insert_resource(previews);
        }
        self.app.insert_resource(session).insert_resource(catalog);
    }
    fn inserted(&self) -> (MaterialProgram, String) {
        let mut after = self.current();
        let expression = MaterialExpression {
            id: MaterialExpressionId::new(),
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
        };
        let key = material_graph_expression_node_key(expression.id);
        after.node_constants.push(expression.id);
        after.expressions.push(expression);
        (after.normalized(), key)
    }
    fn layout(
        &mut self,
        graph: &str,
        node: &str,
        position: Vec2,
        collapsed: bool,
        pinned: bool,
        preview_delta: bool,
    ) {
        let before = self.snapshot(graph);
        {
            let mut memory = self.app.world_mut().resource_mut::<GraphViewportMemory>();
            memory.set_node(graph, node, position, collapsed);
            memory.set_pinned(graph, node, pinned);
        }
        let after = self.snapshot(graph);
        let previews = preview_delta.then(|| {
            self.app
                .world_mut()
                .resource_mut::<MaterialGraphPreviewState>()
                .set_visible(self.program.id, [MaterialGraphPreviewTarget::Output], true);
            (
                self.program.id,
                BTreeSet::new(),
                BTreeSet::from([MaterialGraphPreviewTarget::Output]),
            )
        });
        let mut session = self.app.world_mut().resource_mut::<EditorSession>();
        let context = asset_order::Context::current(&session);
        session.operation_order.record_layout(
            context,
            Transaction::snapshots_for_test(before, after, previews),
        );
    }
}

#[test]
fn inserted_and_deleted_nodes_restore_exact_layout_with_one_semantic_step_each() {
    let mut f = Fixture::new();
    let original = f.program.clone();
    let (inserted, key) = f.inserted();
    let position = Vec2::new(-222.0, 517.0);
    f.edit(inserted.clone(), Some((&key, position, true)), true);
    assert!(f.step(true));
    assert_eq!(f.current(), original);
    assert_eq!(f.node(&key), None);
    assert!(
        !f.step(true),
        "compound layout must not create a second undo item"
    );
    assert!(f.step(false));
    assert_eq!(f.current(), inserted);
    assert_eq!(f.node(&key), Some((position, true)));
    f.edit(original.clone(), None, true);
    assert_eq!(f.node(&key), None);
    assert!(f.step(true));
    assert_eq!(f.current(), inserted);
    assert_eq!(f.node(&key), Some((position, true)));
    assert!(f.step(false));
    assert_eq!(f.current(), original);
    assert_eq!(f.node(&key), None);
}

#[test]
fn layout_only_round_trip_preserves_compiled_preview_semantics_and_view_camera() {
    let mut f = Fixture::new();
    let graph = f.graph();
    let preview = f
        .app
        .world()
        .resource::<EditorSession>()
        .preview()
        .unwrap()
        .effect()
        .clone();
    let effect = f.app.world().resource::<EditorSession>().effect.clone();
    let revision = f
        .app
        .world()
        .resource::<EditorSession>()
        .document_revision();
    let content = f
        .app
        .world()
        .resource::<ProjectEffectCatalog>()
        .content_revision();
    f.app
        .world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_view(format!("{graph}#view:1"), Vec2::splat(444.0), 0.75);
    f.app
        .world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node("other", "output", Vec2::splat(99.0), true);
    f.layout(&graph, "output", Vec2::new(40.0, -20.0), true, true, true);
    assert!(f.step(true));
    assert_eq!(f.node("output"), Some((Vec2::ZERO, false)));
    assert!(
        !f.app
            .world()
            .resource::<GraphViewportMemory>()
            .is_pinned(&graph, "output")
    );
    assert!(
        !f.app
            .world()
            .resource::<MaterialGraphPreviewState>()
            .is_visible(f.program.id, MaterialGraphPreviewTarget::Output)
    );
    assert!(f.step(false));
    assert_eq!(f.node("output"), Some((Vec2::new(40.0, -20.0), true)));
    assert!(
        f.app
            .world()
            .resource::<GraphViewportMemory>()
            .is_pinned(&graph, "output")
    );
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphPreviewState>()
            .is_visible(f.program.id, MaterialGraphPreviewTarget::Output)
    );
    assert_eq!(f.current(), f.program);
    let session = f.app.world().resource::<EditorSession>();
    assert_eq!(session.effect, effect);
    assert_eq!(session.document_revision(), revision);
    assert!(Arc::ptr_eq(&preview, session.preview().unwrap().effect()));
    assert_eq!(
        f.app
            .world()
            .resource::<ProjectEffectCatalog>()
            .content_revision(),
        content
    );
    assert_eq!(
        f.app
            .world()
            .resource::<GraphViewportMemory>()
            .node("other", "output"),
        Some((Vec2::splat(99.0), true))
    );
    assert_eq!(
        f.app
            .world()
            .resource::<GraphViewportMemory>()
            .view(&format!("{graph}#view:1")),
        Some((Vec2::splat(444.0), 0.75))
    );
}

#[test]
fn missing_layout_state_rejects_before_semantic_undo_and_remains_retryable() {
    let mut f = Fixture::new();
    let (inserted, key) = f.inserted();
    f.edit(inserted.clone(), Some((&key, Vec2::ONE, false)), true);
    let memory = f
        .app
        .world_mut()
        .remove_resource::<GraphViewportMemory>()
        .unwrap();
    assert!(!f.step(true));
    assert_eq!(f.current(), inserted);
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .status
            .contains("layout is unavailable")
    );
    f.app.insert_resource(memory);
    assert!(f.step(true));
    assert_eq!(f.current(), f.program);
}

#[test]
fn preview_delta_requires_matching_visibility_and_state_before_any_mutation() {
    let mut f = Fixture::new();
    let graph = f.graph();
    f.layout(&graph, "output", Vec2::ONE, false, false, true);
    let mut previews = f
        .app
        .world_mut()
        .remove_resource::<MaterialGraphPreviewState>()
        .unwrap();
    assert!(!f.step(true));
    assert_eq!(f.node("output"), Some((Vec2::ONE, false)));
    previews.set_visible(f.program.id, [MaterialGraphPreviewTarget::Output], false);
    f.app.insert_resource(previews);
    assert!(!f.step(true));
    assert_eq!(f.node("output"), Some((Vec2::ONE, false)));
    f.app
        .world_mut()
        .resource_mut::<MaterialGraphPreviewState>()
        .set_visible(f.program.id, [MaterialGraphPreviewTarget::Output], true);
    assert!(f.step(true));
}

#[test]
fn canvasless_semantic_host_can_apply_layout_without_a_preview_delta() {
    let mut f = Fixture::new();
    let graph = f.graph();
    f.layout(&graph, "output", Vec2::ONE, false, false, false);
    f.app
        .world_mut()
        .remove_resource::<MaterialGraphPreviewState>();
    assert!(f.step(true));
    assert_eq!(f.node("output"), Some((Vec2::ZERO, false)));
    assert!(
        !f.app
            .world()
            .contains_resource::<MaterialGraphPreviewState>()
    );
}

#[test]
fn manual_move_or_pin_rejects_compound_undo_without_consuming_semantic_history() {
    for pin in [false, true] {
        let mut f = Fixture::new();
        let graph = f.graph();
        let (inserted, key) = f.inserted();
        f.edit(inserted.clone(), Some((&key, Vec2::ONE, false)), true);
        {
            let mut memory = f.app.world_mut().resource_mut::<GraphViewportMemory>();
            if pin {
                memory.set_pinned(&graph, &key, true);
            } else {
                memory.set_node(&graph, &key, Vec2::splat(88.0), false);
            }
        }
        assert!(!f.step(true));
        assert_eq!(f.current(), inserted);
        {
            let mut memory = f.app.world_mut().resource_mut::<GraphViewportMemory>();
            memory.set_node(&graph, &key, Vec2::ONE, false);
            memory.set_pinned(&graph, &key, false);
        }
        assert!(f.step(true));
        assert_eq!(f.current(), f.program);
    }
}

#[test]
fn external_source_comments_survive_in_memory_undo_and_layout_restoration() {
    let mut f = Fixture::new();
    let (inserted, key) = f.inserted();
    f.edit(inserted.clone(), Some((&key, Vec2::ONE, true)), true);
    let path = f.root.path().join("program.aestra.material.ron");
    let original_bytes = fs::read(&path).unwrap();
    let external = format!(
        "// external change\n{}",
        String::from_utf8(original_bytes.clone()).unwrap()
    );
    fs::write(&path, &external).unwrap();
    assert!(f.step(true));
    assert_eq!(f.current(), f.program);
    assert_eq!(f.node(&key), None);
    assert_eq!(fs::read_to_string(&path).unwrap(), external);
    assert!(f.step(false));
    assert_eq!(f.current(), inserted);
    assert_eq!(f.node(&key), Some((Vec2::ONE, true)));
    assert_eq!(fs::read_to_string(&path).unwrap(), external);
}

#[test]
fn a_newer_shared_draft_rejects_compound_undo_and_leaves_the_step_retryable() {
    let mut f = Fixture::new();
    let (inserted, key) = f.inserted();
    f.edit(inserted.clone(), Some((&key, Vec2::ONE, true)), true);
    let mut newer = inserted.clone();
    newer.name = "Newer shared edit".into();
    f.app
        .world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .replace_material_program(&inserted, &newer)
        .unwrap();
    assert!(!f.step(true));
    assert_eq!(f.current(), newer);
    assert_eq!(f.node(&key), Some((Vec2::ONE, true)));
    f.app
        .world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .replace_material_program(&newer, &inserted)
        .unwrap();
    assert!(f.step(true));
    assert_eq!(f.current(), f.program);
}

#[test]
fn failed_effect_scoped_semantic_undo_preserves_layout_draft_and_installed_preview() {
    use aestra_core::{
        MaterialId,
        material::{MaterialInstance, MaterialProgramRef, MaterialRenderState},
    };
    let mut f = Fixture::new();
    let catalog = f.app.world().resource::<ProjectEffectCatalog>().clone();
    {
        let mut session = f.app.world_mut().resource_mut::<EditorSession>();
        session.material_target = crate::material_target::MaterialEditingTarget::EffectInstance;
        session.material_history_active = false;
        session.effect.material_instances.push(MaterialInstance {
            id: MaterialId::new(),
            program: MaterialProgramRef::Project(f.program.id),
            values: default(),
            render_state: MaterialRenderState::additive_sprite(),
        });
        let compiled = catalog.compile_project(&session.effect).unwrap().root;
        session.install_compiled_project_root(compiled).unwrap();
    }
    let (inserted, key) = f.inserted();
    f.edit(inserted.clone(), Some((&key, Vec2::ONE, false)), true);
    let preview = f
        .app
        .world()
        .resource::<EditorSession>()
        .preview()
        .unwrap()
        .effect()
        .clone();
    let capacity = f.app.world().resource::<EditorSession>().effect.emitters[0].max_particles;
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .effect
        .emitters[0]
        .max_particles = 0;
    assert!(!f.step(true));
    assert_eq!(f.current(), inserted);
    assert_eq!(f.node(&key), Some((Vec2::ONE, false)));
    assert!(Arc::ptr_eq(
        &preview,
        f.app
            .world()
            .resource::<EditorSession>()
            .preview()
            .unwrap()
            .effect()
    ));
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .effect
        .emitters[0]
        .max_particles = capacity;
    assert!(f.step(true));
    assert_eq!(f.current(), f.program);
    assert_eq!(f.node(&key), None);
}

#[test]
fn deletion_and_undo_restore_a_removed_nodes_preview_visibility_as_one_step() {
    let mut f = Fixture::new();
    let (inserted, key) = f.inserted();
    let id = inserted.node_constants[0];
    f.edit(inserted.clone(), Some((&key, Vec2::ONE, false)), true);
    f.app
        .world_mut()
        .resource_mut::<MaterialGraphPreviewState>()
        .set_visible(
            f.program.id,
            [MaterialGraphPreviewTarget::Expression(id)],
            true,
        );
    f.edit(f.program.clone(), None, true);
    assert!(
        !f.app
            .world()
            .resource::<MaterialGraphPreviewState>()
            .is_visible(f.program.id, MaterialGraphPreviewTarget::Expression(id))
    );
    assert!(f.step(true));
    assert_eq!(f.current(), inserted);
    assert_eq!(f.node(&key), Some((Vec2::ONE, false)));
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphPreviewState>()
            .is_visible(f.program.id, MaterialGraphPreviewTarget::Expression(id))
    );
    assert!(f.step(false));
    assert_eq!(f.node(&key), None);
}

#[test]
fn stale_project_generation_or_explicit_invalidation_cannot_apply_layout_history() {
    for switch in [false, true] {
        let mut f = Fixture::new();
        let graph = f.graph();
        f.layout(&graph, "output", Vec2::ONE, false, false, false);
        if switch {
            f.app
                .insert_resource(ProjectEffectCatalog::scan(f.root.path()));
        } else {
            f.app
                .world_mut()
                .resource_mut::<EditorSession>()
                .operation_order
                .invalidate_layouts(Some(&graph));
        }
        assert!(!f.step(true));
        assert_eq!(f.node("output"), Some((Vec2::ONE, false)));
    }
}

#[test]
fn semantic_undo_without_layout_does_not_require_canvas_resources() {
    let mut f = Fixture::new();
    let mut after = f.program.clone();
    after.name = "Semantic only".into();
    f.edit(after, None, false);
    f.app.world_mut().remove_resource::<GraphViewportMemory>();
    f.app
        .world_mut()
        .remove_resource::<MaterialGraphPreviewState>();
    assert!(f.step(true));
    assert_eq!(f.current(), f.program);
}

#[test]
fn function_semantics_and_layout_restore_together_and_noop_does_not_replace_attachment() {
    use aestra_core::{
        MaterialFunctionId, MaterialFunctionOutputId,
        material::{MaterialFunctionOutput, MaterialSchemaVersion, MaterialValueType},
    };
    let mut f = Fixture::new();
    let expression = MaterialExpressionId::new();
    let function = MaterialFunction {
        schema_version: MaterialSchemaVersion::CURRENT,
        id: MaterialFunctionId::new(),
        name: "Before".into(),
        inputs: vec![],
        outputs: vec![MaterialFunctionOutput {
            id: MaterialFunctionOutputId::new(),
            name: "Value".into(),
            value_type: MaterialValueType::Float,
            expression,
        }],
        expressions: vec![MaterialExpression {
            id: expression,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        }],
        custom_wesl: None,
    };
    function
        .save_ron(f.root.path().join("function.aestra.material-function.ron"))
        .unwrap();
    f.app
        .world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .refresh();
    let mut catalog = f
        .app
        .world_mut()
        .remove_resource::<ProjectEffectCatalog>()
        .unwrap();
    let mut session = f
        .app
        .world_mut()
        .remove_resource::<EditorSession>()
        .unwrap();
    session
        .open_material_function(&catalog, function.id)
        .unwrap();
    let graph = function_graph_memory_key(catalog.root(), function.id);
    f.app
        .world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node(&graph, "outputs", Vec2::ZERO, false);
    let snapshot = Snapshot::capture(
        &graph,
        &catalog,
        &session,
        f.app.world().resource::<GraphViewportMemory>(),
    )
    .unwrap();
    let mut renamed = function.clone();
    renamed.name = "After".into();
    f.app
        .world_mut()
        .resource_mut::<FunctionEditor>()
        .edit(&mut session, &mut catalog, renamed.clone())
        .unwrap();
    f.app
        .world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node(&graph, "outputs", Vec2::new(88.0, 24.0), true);
    snapshot.attach(
        &catalog,
        &mut session,
        &mut f.app.world_mut().resource_mut::<GraphViewportMemory>(),
    );
    let no_op = Snapshot::capture(
        &graph,
        &catalog,
        &session,
        f.app.world().resource::<GraphViewportMemory>(),
    )
    .unwrap();
    let serial = session.operation_order.edit_serial();
    f.app
        .world_mut()
        .resource_mut::<FunctionEditor>()
        .edit(&mut session, &mut catalog, renamed.clone())
        .unwrap();
    assert_eq!(session.operation_order.edit_serial(), serial);
    no_op.attach(
        &catalog,
        &mut session,
        &mut f.app.world_mut().resource_mut::<GraphViewportMemory>(),
    );
    f.app.insert_resource(catalog).insert_resource(session);
    assert!(f.step(true));
    assert_eq!(
        f.app
            .world()
            .resource::<ProjectEffectCatalog>()
            .material_functions()
            .unwrap(),
        [function]
    );
    assert_eq!(
        f.app
            .world()
            .resource::<GraphViewportMemory>()
            .node(&graph, "outputs"),
        Some((Vec2::ZERO, false))
    );
    assert_eq!(f.node("output"), Some((Vec2::ZERO, false)));
    assert!(f.step(false));
    assert_eq!(
        f.app
            .world()
            .resource::<ProjectEffectCatalog>()
            .material_functions()
            .unwrap(),
        [renamed]
    );
    assert_eq!(
        f.app
            .world()
            .resource::<GraphViewportMemory>()
            .node(&graph, "outputs"),
        Some((Vec2::new(88.0, 24.0), true))
    );
    assert!(!f.step(false));
}

#[test]
fn effect_semantic_and_layout_history_share_one_step_without_modifying_project_material() {
    let mut f = Fixture::new();
    let graph = f.graph();
    let mut session = f
        .app
        .world_mut()
        .remove_resource::<EditorSession>()
        .unwrap();
    session.material_target = crate::material_target::MaterialEditingTarget::EffectInstance;
    session.material_history_active = false;
    let original = session.effect.clone();
    let snapshot = Snapshot::capture(
        &graph,
        f.app.world().resource::<ProjectEffectCatalog>(),
        &session,
        f.app.world().resource::<GraphViewportMemory>(),
    )
    .unwrap();
    session.set_effect_name("Effect compound edit");
    f.app
        .world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node(&graph, "output", Vec2::new(55.0, 66.0), true);
    let catalog = f.app.world().resource::<ProjectEffectCatalog>().clone();
    snapshot.attach(
        &catalog,
        &mut session,
        &mut f.app.world_mut().resource_mut::<GraphViewportMemory>(),
    );
    f.app.insert_resource(session);
    assert!(f.step(true));
    assert_eq!(f.app.world().resource::<EditorSession>().effect, original);
    assert_eq!(f.node("output"), Some((Vec2::ZERO, false)));
    assert!(!f.step(true));
    assert!(f.step(false));
    assert_eq!(
        f.app.world().resource::<EditorSession>().effect.name,
        "Effect compound edit"
    );
    assert_eq!(f.node("output"), Some((Vec2::new(55.0, 66.0), true)));
    assert_eq!(f.current(), f.program);
}
