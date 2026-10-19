//! Same production consumers on both engines. Event delivery is targeted; native picking,
//! function document opening and the presentation-history consumer are separate gates.
use super::*;
use crate::{
    docking::EditorViewId,
    history::{HistoryAction, routing::Routing},
    material_document::MaterialEditingTarget,
    material_function_editor::FunctionEditor,
    material_graph::{
        MaterialSelectionScope, click_input, keys::material_graph_view_key,
        sockets::MaterialGraphSocketKind,
    },
};
use aestra_core::{
    MaterialExpressionId, MaterialFunctionId, MaterialFunctionOutputId,
    material::{
        MaterialExpression, MaterialFunction, MaterialFunctionOutput, MaterialFunctionRef,
        MaterialProgram, MaterialSchemaVersion, MaterialValue, MaterialValueType,
    },
};

#[derive(Resource, Default)]
struct Observed {
    bubbled: usize,
    opened: Vec<(MaterialFunctionId, bool)>,
    batches: Vec<GraphPresentationBatchEdit>,
}
struct Fixture {
    root: tempfile::TempDir,
    app: App,
    original: MaterialProgram,
    function: MaterialFunction,
    ids: [MaterialExpressionId; 4],
    nodes: [Entity; 4],
    viewport: Entity,
    window: Entity,
    scope: MaterialSelectionScope,
    observed: BTreeSet<Entity>,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let ids = std::array::from_fn(|_| MaterialExpressionId::new());
        let body = MaterialExpressionId::new();
        let function = MaterialFunction {
            id: MaterialFunctionId::new(),
            schema_version: MaterialSchemaVersion::CURRENT,
            name: "Scalar function".into(),
            inputs: vec![],
            outputs: vec![MaterialFunctionOutput {
                id: MaterialFunctionOutputId::new(),
                name: "Value".into(),
                value_type: MaterialValueType::Float,
                expression: body,
            }],
            expressions: vec![MaterialExpression {
                id: body,
                kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.25)),
            }],
            custom_wesl: None,
        };
        function
            .save_ron(root.path().join("call.aestra.material-function.ron"))
            .unwrap();
        let mut original = MaterialProgram::additive_sprite("Node actions").normalized();
        original.expressions.extend([
            MaterialExpression {
                id: ids[0],
                kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
            },
            MaterialExpression {
                id: ids[1],
                kind: MaterialExpressionKind::Multiply(ids[0], ids[0]),
            },
            MaterialExpression {
                id: ids[2],
                kind: MaterialExpressionKind::Multiply(ids[1], ids[0]),
            },
            MaterialExpression {
                id: ids[3],
                kind: MaterialExpressionKind::FunctionCall {
                    function: MaterialFunctionRef::Project(function.id),
                    arguments: BTreeMap::new(),
                    output: function.outputs[0].id,
                },
            },
        ]);
        original.outputs.alpha = ids[2];
        original = original.normalized();
        original
            .save_ron(root.path().join("nodes.aestra.material.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
        session
            .open_material_program(&catalog, original.id)
            .unwrap();
        let mut layout = WorkspaceLayout::default();
        // Never persist the user's layout from this headless fixture.
        layout.show(ToolPanel::Properties);
        assert!(!layout.show(ToolPanel::Properties));
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(catalog)
            .insert_resource(layout)
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<FunctionEditor>()
            .init_resource::<GraphViewportMemory>()
            .init_resource::<MaterialGraphSelectionState>()
            .init_resource::<MaterialGraphPreviewState>()
            .init_resource::<MaterialStackInspectorState>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Observed>()
            .add_observer(select_material_graph_marquee)
            .add_observer(handle_modified_material_node_drag)
            .add_observer(
                |e: On<crate::asset_browser::OpenFunction>, mut o: ResMut<Observed>| {
                    o.opened.push((e.function, e.new_view));
                },
            )
            .add_observer(
                |e: On<GraphPresentationBatchEdit>, mut o: ResMut<Observed>| {
                    o.batches.push(e.event().clone());
                },
            )
            .add_observer(
                |e: On<HistoryAction>, routing: Routing, mut commands: Commands| {
                    routing.execute(*e, &mut commands, |_, _, _, _| {
                        panic!("No filesystem history")
                    });
                },
            );
        let window = app.world_mut().spawn(Window::default()).id();
        let parent = app
            .world_mut()
            .spawn_empty()
            .observe(|_: On<PointerClick>, mut o: ResMut<Observed>| o.bubbled += 1)
            .id();
        let scope = Some(EditorViewId(7));
        let viewport = app
            .world_mut()
            .spawn((
                ChildOf(parent),
                MaterialGraphViewport {
                    program: original.id,
                    scope,
                    editing_target: MaterialEditingTarget::Program {
                        root: root.path().into(),
                        id: original.id,
                    },
                },
            ))
            .id();
        let graph = material_graph_view_key(original.id);
        let nodes = std::array::from_fn(|i| {
            let position = Self::position(i);
            let key = material_graph_expression_node_key(ids[i]);
            app.world_mut()
                .resource_mut::<GraphViewportMemory>()
                .set_node(&graph, key.clone(), position, i == 0);
            app.world_mut()
                .spawn((
                    ChildOf(viewport),
                    MaterialGraphAction {
                        program: original.id,
                        expression: ids[i],
                    },
                    MaterialGraphNodeScope(scope),
                    FeathersGraphNode::clipboard_fixture(graph.clone(), key, position),
                ))
                .id()
        });
        app.world_mut()
            .resource_mut::<GraphViewportMemory>()
            .set_node(&graph, "output", Vec2::new(900.0, 100.0), true);
        Self {
            root,
            app,
            original,
            function,
            ids,
            nodes,
            viewport,
            window,
            scope,
            observed: BTreeSet::new(),
        }
    }
    fn position(i: usize) -> Vec2 {
        Vec2::new(i as f32 * 250.0, i as f32 * 40.0)
    }
    fn graph(&self) -> String {
        material_graph_view_key(self.original.id)
    }
    fn target(&self) -> MaterialEditingTarget {
        MaterialEditingTarget::Program {
            root: self.root.path().into(),
            id: self.original.id,
        }
    }
    fn revision(&self) -> u64 {
        self.app.world().resource::<EditorSession>().ui_revision
    }
    fn selection(&self) -> BTreeSet<MaterialExpressionId> {
        self.app
            .world()
            .resource::<MaterialGraphSelectionState>()
            .get(self.scope)
            .map(|s| s.expressions.clone())
            .unwrap_or_default()
    }
    fn inspector(&self) -> Option<(aestra_core::MaterialProgramId, MaterialExpressionId)> {
        self.app
            .world()
            .resource::<MaterialStackInspectorState>()
            .selected
    }
    fn select(&mut self, indices: &[usize]) {
        self.app
            .world_mut()
            .resource_mut::<MaterialGraphSelectionState>()
            .select_material_expressions(
                self.scope,
                self.original.id,
                &indices.iter().map(|i| self.ids[*i]).collect(),
                GraphSelectionMode::Replace,
            );
    }
    fn click(&mut self, entity: Entity, count: u8, button: PointerButton, open: bool) {
        if self.observed.insert(entity) {
            if open {
                self.app
                    .world_mut()
                    .entity_mut(entity)
                    .observe(open_material_graph_function_call);
            } else {
                self.app
                    .world_mut()
                    .entity_mut(entity)
                    .observe(select_material_graph_node);
            }
        }
        let click = click_input::with_click_count(
            click_input::click(entity, self.window, Vec2::ONE, button),
            count,
        );
        self.app.world_mut().trigger(click);
        self.app.world_mut().flush();
    }
    fn marquee(&mut self, indices: &[usize], mode: GraphSelectionMode) {
        self.app.world_mut().trigger(GraphMarqueeSelection {
            viewport: self.viewport,
            nodes: indices
                .iter()
                .map(|i| material_graph_expression_node_key(self.ids[*i]))
                .collect(),
            mode,
        });
        self.app.world_mut().flush();
    }
    fn drag(&mut self, index: usize, delta: Vec2, modifier: GraphNodeDragModifier) {
        let graph = self.graph();
        self.app.world_mut().trigger(GraphModifiedNodeDrag {
            graph,
            node: material_graph_expression_node_key(self.ids[index]),
            before: (Self::position(index), index == 0),
            after: (Self::position(index) + delta, index == 0),
            modifier,
        });
        self.app.world_mut().flush();
    }
    fn current(&self) -> MaterialProgram {
        let session = self.app.world().resource::<EditorSession>();
        session
            .graph_authoring_document_for(
                &self.target(),
                self.app.world().resource::<ProjectEffectCatalog>(),
            )
            .unwrap()
            .programs
            .into_iter()
            .find(|p| p.id == self.original.id)
            .unwrap()
    }
    fn unchanged(&self) {
        assert_eq!(self.current(), self.original);
        assert_eq!(
            self.app
                .world()
                .resource::<EditorSession>()
                .operation_order
                .edit_serial(),
            0
        );
        self.disk_unchanged();
    }
    fn disk_unchanged(&self) {
        assert_eq!(
            MaterialProgram::load_ron(self.root.path().join("nodes.aestra.material.ron")).unwrap(),
            self.original
        );
        assert_eq!(
            MaterialFunction::load_ron(self.root.path().join("call.aestra.material-function.ron"))
                .unwrap(),
            self.function
        );
    }
}

#[test]
fn primary_descendant_click_selects_once_updates_inspector_and_stops_bubbling() {
    let mut f = Fixture::new();
    let child = f.app.world_mut().spawn(ChildOf(f.nodes[1])).id();
    let before = f.revision();
    f.click(child, 1, PointerButton::Primary, false);
    assert_eq!(f.selection(), BTreeSet::from([f.ids[1]]));
    assert_eq!(f.inspector(), Some((f.original.id, f.ids[1])));
    assert_eq!(f.revision(), before + 1);
    assert_eq!(f.app.world().resource::<Observed>().bubbled, 0);
    f.unchanged();
}
#[test]
fn shift_adds_and_control_toggles_with_control_precedence_on_both_sides() {
    for (shift, control) in [
        (KeyCode::ShiftLeft, KeyCode::ControlLeft),
        (KeyCode::ShiftRight, KeyCode::ControlRight),
    ] {
        let mut f = Fixture::new();
        f.click(f.nodes[0], 1, PointerButton::Primary, false);
        f.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(shift);
        f.click(f.nodes[1], 1, PointerButton::Primary, false);
        assert_eq!(f.selection(), BTreeSet::from([f.ids[0], f.ids[1]]));
        f.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(control);
        f.click(f.nodes[1], 1, PointerButton::Primary, false);
        assert_eq!(f.selection(), BTreeSet::from([f.ids[0]]));
        assert_eq!(f.inspector(), Some((f.original.id, f.ids[0])));
        f.click(f.nodes[0], 1, PointerButton::Primary, false);
        assert!(f.selection().is_empty());
        assert_eq!(f.inspector(), None);
        f.unchanged();
    }
}
#[test]
fn secondary_and_space_primary_do_not_select_or_consume() {
    let mut f = Fixture::new();
    let before = f.revision();
    f.click(f.nodes[0], 1, PointerButton::Secondary, false);
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    f.click(f.nodes[0], 1, PointerButton::Primary, false);
    assert!(f.selection().is_empty());
    assert_eq!(f.revision(), before);
    assert_eq!(f.app.world().resource::<Observed>().bubbled, 2);
}
#[test]
fn socket_descendant_is_not_a_node_selection() {
    let mut f = Fixture::new();
    let socket = f
        .app
        .world_mut()
        .spawn((
            ChildOf(f.nodes[0]),
            MaterialGraphSocket {
                program: f.original.id,
                kind: MaterialGraphSocketKind::ExpressionOutput(f.ids[0]),
            },
        ))
        .id();
    let child = f.app.world_mut().spawn(ChildOf(socket)).id();
    f.click(child, 1, PointerButton::Primary, false);
    assert!(f.selection().is_empty());
    assert_eq!(f.app.world().resource::<Observed>().bubbled, 1);
}
#[test]
fn original_inline_control_records_exclude_numeric_and_toggle_descendants() {
    let mut f = Fixture::new();
    let number = MaterialGraphDefaultNumberControl {
        program: f.original.id,
        editing_target: f.target(),
        expression: f.ids[0],
        value: MaterialValue::Float(0.5),
        component: 0,
    };
    assert_eq!(
        (number.program, number.expression, number.component),
        (f.original.id, f.ids[0], 0)
    );
    assert_eq!(number.editing_target, f.target());
    assert_eq!(number.value, MaterialValue::Float(0.5));
    let toggle = MaterialGraphDefaultToggleControl {
        program: f.original.id,
        editing_target: f.target(),
        expression: f.ids[1],
        value: true,
    };
    assert_eq!(
        (toggle.program, toggle.expression, toggle.value),
        (f.original.id, f.ids[1], true)
    );
    assert_eq!(toggle.editing_target, f.target());
    let a = f.app.world_mut().spawn((ChildOf(f.nodes[0]), number)).id();
    let b = f.app.world_mut().spawn((ChildOf(f.nodes[1]), toggle)).id();
    for control in [a, b] {
        let child = f.app.world_mut().spawn(ChildOf(control)).id();
        f.click(child, 1, PointerButton::Primary, false);
    }
    assert!(f.selection().is_empty());
    f.unchanged();
}
#[test]
fn release_suppression_is_consumed_once_before_the_next_click_selects() {
    let mut f = Fixture::new();
    let graph = f.graph();
    f.app
        .world_mut()
        .entity_mut(f.nodes[0])
        .insert(FeathersGraphNode::completed_drag_fixture(
            graph,
            material_graph_expression_node_key(f.ids[0]),
            Fixture::position(0),
        ));
    let before = f.revision();
    f.click(f.nodes[0], 1, PointerButton::Primary, false);
    assert!(f.selection().is_empty());
    assert_eq!(f.revision(), before);
    assert_eq!(f.app.world().resource::<Observed>().bubbled, 0);
    f.click(f.nodes[0], 1, PointerButton::Primary, false);
    assert_eq!(f.selection(), BTreeSet::from([f.ids[0]]));
    assert_eq!(f.revision(), before + 1);
}
#[test]
fn missing_node_scope_uses_default_scope_without_touching_other_view() {
    let mut f = Fixture::new();
    f.select(&[1]);
    f.app
        .world_mut()
        .entity_mut(f.nodes[0])
        .remove::<MaterialGraphNodeScope>();
    f.click(f.nodes[0], 1, PointerButton::Primary, false);
    assert_eq!(f.selection(), BTreeSet::from([f.ids[1]]));
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphSelectionState>()
            .is_expression_selected(None, f.original.id, f.ids[0])
    );
}
#[test]
fn detached_click_without_action_is_ignored() {
    let mut f = Fixture::new();
    let entity = f.app.world_mut().spawn_empty().id();
    let before = f.revision();
    f.click(entity, 1, PointerButton::Primary, false);
    assert!(f.selection().is_empty());
    assert_eq!(f.revision(), before);
}
#[test]
fn double_click_project_call_emits_exact_open_intent_from_descendant() {
    let mut f = Fixture::new();
    let child = f.app.world_mut().spawn(ChildOf(f.nodes[3])).id();
    f.click(child, 2, PointerButton::Primary, true);
    assert_eq!(
        f.app.world().resource::<Observed>().opened,
        vec![(f.function.id, false)]
    );
    assert_eq!(f.app.world().resource::<Observed>().bubbled, 0);
    assert!(f.selection().is_empty());
    f.unchanged();
}
#[test]
fn single_secondary_and_non_call_double_clicks_do_not_open_functions() {
    let mut f = Fixture::new();
    f.click(f.nodes[3], 1, PointerButton::Primary, true);
    f.click(f.nodes[3], 2, PointerButton::Secondary, true);
    f.click(f.nodes[0], 2, PointerButton::Primary, true);
    assert!(f.app.world().resource::<Observed>().opened.is_empty());
    assert_eq!(f.app.world().resource::<Observed>().bubbled, 3);
    f.unchanged();
}
#[test]
fn retained_view_target_opens_call_without_changing_active_session_target() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .material_target = MaterialEditingTarget::EffectInstance;
    f.click(f.nodes[3], 2, PointerButton::Primary, true);
    assert_eq!(
        f.app.world().resource::<Observed>().opened,
        vec![(f.function.id, false)]
    );
    assert_eq!(
        f.app.world().resource::<EditorSession>().material_target,
        MaterialEditingTarget::EffectInstance
    );
    f.unchanged();
}
#[test]
fn stale_call_expression_or_scope_or_missing_view_does_not_emit_open_intent() {
    for stale in 0..3 {
        let mut f = Fixture::new();
        match stale {
            0 => {
                f.app
                    .world_mut()
                    .get_mut::<MaterialGraphAction>(f.nodes[3])
                    .unwrap()
                    .expression = MaterialExpressionId::new();
            }
            1 => {
                f.app
                    .world_mut()
                    .get_mut::<MaterialGraphNodeScope>(f.nodes[3])
                    .unwrap()
                    .0 = Some(EditorViewId(88));
            }
            _ => {
                f.app
                    .world_mut()
                    .entity_mut(f.viewport)
                    .remove::<MaterialGraphViewport>();
            }
        }
        f.click(f.nodes[3], 2, PointerButton::Primary, true);
        assert!(f.app.world().resource::<Observed>().opened.is_empty());
        f.unchanged();
    }
}
#[test]
fn marquee_replaces_filters_unknown_nodes_and_only_revises_on_change() {
    let mut f = Fixture::new();
    let before = f.revision();
    f.marquee(&[0, 1], GraphSelectionMode::Replace);
    assert_eq!(f.selection(), BTreeSet::from([f.ids[0], f.ids[1]]));
    assert_eq!(f.inspector(), None);
    assert_eq!(f.revision(), before + 1);
    f.marquee(&[0, 1], GraphSelectionMode::Replace);
    assert_eq!(f.revision(), before + 1);
    f.app.world_mut().trigger(GraphMarqueeSelection {
        viewport: f.viewport,
        nodes: BTreeSet::from(["missing-node".into()]),
        mode: GraphSelectionMode::Replace,
    });
    assert!(f.selection().is_empty());
    assert_eq!(f.revision(), before + 2);
    f.unchanged();
}
#[test]
fn marquee_add_toggle_inspector_follows_incoming_set_and_selection_is_scoped() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .resource_mut::<MaterialGraphSelectionState>()
        .select_single(Some(EditorViewId(88)), f.original.id, f.ids[3]);
    f.select(&[0]);
    f.marquee(&[1], GraphSelectionMode::Add);
    assert_eq!(f.selection(), BTreeSet::from([f.ids[0], f.ids[1]]));
    assert_eq!(f.inspector(), Some((f.original.id, f.ids[1])));
    f.marquee(&[1], GraphSelectionMode::Toggle);
    assert_eq!(f.selection(), BTreeSet::from([f.ids[0]]));
    // Preserve existing incoming-set inspector semantics, not a new selection policy.
    assert_eq!(f.inspector(), Some((f.original.id, f.ids[1])));
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphSelectionState>()
            .is_expression_selected(Some(EditorViewId(88)), f.original.id, f.ids[3])
    );
}
#[test]
fn missing_marquee_viewport_does_not_change_selection_or_revision() {
    let mut f = Fixture::new();
    f.select(&[0]);
    let before = f.revision();
    f.app
        .world_mut()
        .entity_mut(f.viewport)
        .remove::<MaterialGraphViewport>();
    f.marquee(&[1], GraphSelectionMode::Replace);
    assert_eq!(f.selection(), BTreeSet::from([f.ids[0]]));
    assert_eq!(f.revision(), before);
}
#[test]
fn duplicate_drag_remaps_selected_nodes_places_once_and_undo_redo_preserves_disk() {
    let mut f = Fixture::new();
    f.select(&[0, 1]);
    let delta = Vec2::new(125.0, 75.0);
    f.drag(1, delta, GraphNodeDragModifier::Duplicate);
    let changed = f.current();
    let selected = f.selection();
    assert_eq!(selected.len(), 2);
    assert!(selected.is_disjoint(&BTreeSet::from(f.ids)));
    assert_eq!(changed.expressions.len(), f.original.expressions.len() + 2);
    let constant = changed
        .expressions
        .iter()
        .find(|e| selected.contains(&e.id) && matches!(e.kind, MaterialExpressionKind::Constant(_)))
        .unwrap()
        .id;
    let multiply = changed
        .expressions
        .iter()
        .find(|e| {
            selected.contains(&e.id) && matches!(e.kind, MaterialExpressionKind::Multiply(_, _))
        })
        .unwrap();
    assert_eq!(
        multiply.kind,
        MaterialExpressionKind::Multiply(constant, constant)
    );
    let memory = f.app.world().resource::<GraphViewportMemory>();
    for (id, position) in [
        (constant, Fixture::position(0)),
        (multiply.id, Fixture::position(1)),
    ] {
        assert_eq!(
            memory.node_position(&f.graph(), &material_graph_expression_node_key(id)),
            Some(position + delta)
        );
    }
    for i in 0..4 {
        assert_eq!(
            memory.node_position(&f.graph(), &material_graph_expression_node_key(f.ids[i])),
            Some(Fixture::position(i))
        );
    }
    assert!(
        f.inspector()
            .is_some_and(|(p, e)| p == f.original.id && selected.contains(&e))
    );
    assert_eq!(
        f.app
            .world()
            .resource::<EditorSession>()
            .operation_order
            .edit_serial(),
        1
    );
    f.app.world_mut().trigger(HistoryAction::Undo);
    f.app.world_mut().flush();
    assert_eq!(f.current(), f.original);
    f.app.world_mut().trigger(HistoryAction::Redo);
    f.app.world_mut().flush();
    assert_eq!(f.current(), changed);
    f.disk_unchanged();
}
#[test]
fn duplicate_unselected_seed_replaces_selection_before_actual_edit() {
    let mut f = Fixture::new();
    f.select(&[0]);
    f.drag(1, Vec2::splat(20.0), GraphNodeDragModifier::Duplicate);
    assert_eq!(
        f.current().expressions.len(),
        f.original.expressions.len() + 1
    );
    assert_eq!(f.selection().len(), 1);
    assert!(!f.selection().contains(&f.ids[0]));
    f.disk_unchanged();
}
#[test]
fn invalid_duplicate_delta_cannot_publish_or_record_semantic_history() {
    let mut f = Fixture::new();
    f.drag(0, Vec2::splat(f32::NAN), GraphNodeDragModifier::Duplicate);
    f.unchanged();
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .status
            .starts_with("Could not duplicate material nodes:")
    );
}
#[test]
fn upstream_drag_moves_transitive_branch_once_preserving_collapsed_state() {
    let mut f = Fixture::new();
    let delta = Vec2::new(30.0, 50.0);
    let graph = f.graph();
    // The generic producer has already placed the seed at `after` on branch release.
    f.app
        .world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node(
            &graph,
            material_graph_expression_node_key(f.ids[1]),
            Fixture::position(1) + delta,
            false,
        );
    f.drag(1, delta, GraphNodeDragModifier::Upstream);
    assert_eq!(f.selection(), BTreeSet::from([f.ids[0], f.ids[1]]));
    let memory = f.app.world().resource::<GraphViewportMemory>();
    for i in 0..4 {
        assert_eq!(
            memory.node(&f.graph(), &material_graph_expression_node_key(f.ids[i])),
            Some((
                Fixture::position(i) + if i < 2 { delta } else { Vec2::ZERO },
                i == 0
            ))
        );
    }
    let o = f.app.world().resource::<Observed>();
    assert_eq!(o.batches.len(), 1);
    assert_eq!(o.batches[0].graph, f.graph());
    assert_eq!(o.batches[0].origin, None);
    assert_eq!(
        o.batches[0].before,
        BTreeMap::from([
            (
                material_graph_expression_node_key(f.ids[0]),
                (Fixture::position(0), true)
            ),
            (
                material_graph_expression_node_key(f.ids[1]),
                (Fixture::position(1), false)
            )
        ])
    );
    f.unchanged();
}
#[test]
fn downstream_drag_includes_material_output_but_not_unrelated_expression() {
    let mut f = Fixture::new();
    let delta = Vec2::splat(17.0);
    f.drag(1, delta, GraphNodeDragModifier::Downstream);
    assert_eq!(f.selection(), BTreeSet::from([f.ids[1], f.ids[2]]));
    let memory = f.app.world().resource::<GraphViewportMemory>();
    assert_eq!(
        memory.node(&f.graph(), "output"),
        Some((Vec2::new(900.0, 100.0) + delta, true))
    );
    assert_eq!(
        memory.node_position(&f.graph(), &material_graph_expression_node_key(f.ids[0])),
        Some(Fixture::position(0))
    );
    let o = f.app.world().resource::<Observed>();
    assert_eq!(o.batches.len(), 1);
    assert_eq!(o.batches[0].before.len(), 3);
    assert_eq!(
        o.batches[0].before["output"],
        (Vec2::new(900.0, 100.0), true)
    );
    f.unchanged();
}
#[test]
fn branch_missing_placement_is_not_manufactured_but_semantic_selection_is_complete() {
    let mut f = Fixture::new();
    let graph = f.graph();
    f.app
        .world_mut()
        .resource_mut::<GraphViewportMemory>()
        .remove_node(&graph, &material_graph_expression_node_key(f.ids[0]));
    f.drag(1, Vec2::ONE, GraphNodeDragModifier::Upstream);
    assert_eq!(f.selection(), BTreeSet::from([f.ids[0], f.ids[1]]));
    assert!(
        f.app
            .world()
            .resource::<GraphViewportMemory>()
            .node(&f.graph(), &material_graph_expression_node_key(f.ids[0]))
            .is_none()
    );
    assert_eq!(
        f.app.world().resource::<Observed>().batches[0].before.len(),
        1
    );
    f.unchanged();
}
#[test]
fn stale_modified_drag_graph_node_or_view_has_no_effect() {
    for stale in 0..3 {
        let mut f = Fixture::new();
        let before = f.revision();
        let mut event = GraphModifiedNodeDrag {
            graph: f.graph(),
            node: material_graph_expression_node_key(f.ids[0]),
            before: (Vec2::ZERO, true),
            after: (Vec2::ONE, true),
            modifier: GraphNodeDragModifier::Duplicate,
        };
        match stale {
            0 => event.graph = "missing-graph".into(),
            1 => event.node = "missing-node".into(),
            _ => {
                f.app
                    .world_mut()
                    .entity_mut(f.viewport)
                    .remove::<MaterialGraphViewport>();
            }
        }
        f.app.world_mut().trigger(event);
        f.app.world_mut().flush();
        assert_eq!(f.revision(), before);
        assert!(f.selection().is_empty());
        assert!(f.app.world().resource::<Observed>().batches.is_empty());
        f.unchanged();
    }
}
