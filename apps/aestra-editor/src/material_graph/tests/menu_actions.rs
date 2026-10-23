//! Same real Activate consumers, semantic mutations and history under both engine versions.
use super::*;
use crate::{
    docking::EditorViewId,
    feathers::node_graph::GraphSelectionMode,
    history::{HistoryAction, routing::Routing},
    material_document::MaterialEditingTarget,
    material_function_editor::FunctionEditor,
    material_graph::{
        keys::material_graph_view_key,
        palette_state::{
            MaterialGraphConnectionMenuOpen, MaterialGraphNodeMenuOpen, MaterialGraphPaletteOpen,
        },
    },
};
use aestra_authoring::MaterialOutputSocket;
use aestra_compiler::MaterialGraphCreateKind;
use aestra_core::{
    MaterialExpressionId,
    material::{
        MaterialExpression, MaterialExpressionKind, MaterialProgram, MaterialValue,
        MaterialValueType,
    },
};

#[derive(Resource, Default)]
struct Arrangements(Vec<arrange::ArrangeGraph>);
struct Fixture {
    root: tempfile::TempDir,
    app: App,
    original: MaterialProgram,
    scope: MaterialSelectionScope,
    selected: BTreeSet<MaterialExpressionId>,
    viewport: Entity,
}
impl Fixture {
    fn new(bound: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut original = MaterialProgram::additive_sprite("Menu dispatch").normalized();
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
            .save_ron(root.path().join("menu.aestra.material.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
        session
            .open_material_program(&catalog, original.id)
            .unwrap();
        let scope = Some(EditorViewId(7));
        let selected = BTreeSet::from([constant, multiply]);
        let mut app = App::new();
        app.insert_resource(session)
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
            .init_resource::<Arrangements>()
            .add_observer(queue_material_graph_menu_action_activation)
            .add_observer(
                |event: On<arrange::ArrangeGraph>, mut events: ResMut<Arrangements>| {
                    events.0.push(event.event().clone())
                },
            )
            .add_observer(
                |event: On<HistoryAction>, routing: Routing, mut commands: Commands| {
                    routing.execute(*event, &mut commands, |_, _, _, _| {
                        panic!("No filesystem history in menu fixture")
                    });
                },
            )
            .add_systems(
                Update,
                (
                    handle_material_graph_palette_actions,
                    handle_material_graph_context_actions,
                )
                    .chain(),
            );
        app.world_mut()
            .resource_mut::<MaterialGraphSelectionState>()
            .select_material_expressions(
                scope,
                original.id,
                &selected,
                GraphSelectionMode::Replace,
            );
        let key = material_graph_view_key(original.id);
        for (index, expression) in selected.iter().copied().enumerate() {
            let position = Vec2::new(50.0 + index as f32 * 250.0, 80.0);
            app.world_mut()
                .resource_mut::<GraphViewportMemory>()
                .place_node(
                    &key,
                    material_graph_expression_node_key(expression),
                    position,
                );
            app.world_mut().spawn((
                MaterialGraphAction {
                    program: original.id,
                    expression,
                },
                FeathersGraphNode::clipboard_fixture(
                    key.clone(),
                    material_graph_expression_node_key(expression),
                    position,
                ),
            ));
        }
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
                FeathersGraphViewport::clipboard_fixture(key, Vec2::new(40.0, 20.0), 2.0),
            ))
            .id();
        app.update();
        Self {
            root,
            app,
            original,
            scope,
            selected,
            viewport,
        }
    }
    fn current(&self) -> MaterialProgram {
        self.app
            .world()
            .resource::<EditorSession>()
            .graph_material_programs(self.app.world().resource::<ProjectEffectCatalog>())
            .unwrap()
            .into_iter()
            .find(|p| p.id == self.original.id)
            .unwrap()
    }
    fn target(&self) -> MaterialEditingTarget {
        MaterialEditingTarget::Program {
            root: self.root.path().into(),
            id: self.original.id,
        }
    }
    fn action(&self) -> MaterialGraphPaletteAction {
        MaterialGraphPaletteAction {
            program: self.original.id,
            editing_target: self.target(),
            scope: self.scope,
            kind: MaterialGraphCreateKind::Constant(MaterialValueType::Float),
            source: None,
            target: None,
            label: "Float".into(),
            graph_position: Vec2::new(500.0, 250.0),
            graph_key: material_graph_view_key(self.original.id),
            searchable: "float".into(),
        }
    }
    fn open_palette(&mut self) {
        let a = self.action();
        let mut palette = self
            .app
            .world_mut()
            .resource_mut::<MaterialGraphPaletteState>();
        palette.open = Some(MaterialGraphPaletteOpen {
            program: a.program,
            editing_target: a.editing_target,
            scope: a.scope,
            menu_position: Vec2::ONE,
            graph_position: a.graph_position,
            graph_key: a.graph_key,
            connection: None,
        });
        palette.query = "float".into();
    }
    fn open_context(&mut self) {
        self.app
            .world_mut()
            .resource_mut::<MaterialGraphPaletteState>()
            .node_menu = Some(MaterialGraphNodeMenuOpen {
            program: self.original.id,
            scope: self.scope,
            menu_position: Vec2::new(240.0, 180.0),
        });
    }
    fn button<A: Component>(&mut self, action: A) -> Entity {
        self.app
            .world_mut()
            .spawn((FeathersActionButton, action))
            .id()
    }
    fn activate(&mut self, entity: Entity) {
        self.app.world_mut().trigger(Activate { entity });
        self.app.update();
        assert!(
            self.app
                .world()
                .get::<PendingFeathersActivation>(entity)
                .is_none()
        );
        assert!(!control_input::is_pressed(self.app.world(), entity));
    }
    fn context(&mut self, action: MaterialGraphContextAction) {
        self.open_context();
        let button = self.button(action);
        self.activate(button);
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
    fn step(&mut self, action: HistoryAction) {
        self.app.world_mut().trigger(action);
        self.app.world_mut().flush();
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
    }
    fn disk_unchanged(&self) {
        assert_eq!(
            MaterialProgram::load_ron(self.root.path().join("menu.aestra.material.ron")).unwrap(),
            self.original
        );
    }
}
#[test]
fn palette_activate_creates_once_places_selects_and_undo_redo_preserves_disk() {
    let mut f = Fixture::new(false);
    f.open_palette();
    let selected_before = f.selection();
    let button = f.button(f.action());
    f.app.world_mut().trigger(Activate { entity: button });
    f.app.world_mut().trigger(Activate { entity: button });
    f.app.update();
    let changed = f.current();
    assert_eq!(changed.expressions.len(), f.original.expressions.len() + 1);
    assert_eq!(f.selection().len(), 1);
    let id = *f.selection().first().unwrap();
    assert!(!selected_before.contains(&id));
    assert_eq!(
        f.app
            .world()
            .resource::<MaterialStackInspectorState>()
            .selected,
        Some((f.original.id, id))
    );
    assert_eq!(
        f.app
            .world()
            .resource::<GraphViewportMemory>()
            .node_position(
                &material_graph_view_key(f.original.id),
                &material_graph_expression_node_key(id)
            ),
        Some(Vec2::new(388.0, 235.0))
    );
    for old in &f.selected {
        assert!(
            f.app
                .world()
                .resource::<GraphViewportMemory>()
                .node_position(
                    &material_graph_view_key(f.original.id),
                    &material_graph_expression_node_key(*old)
                )
                .is_some()
        );
    }
    let palette = f.app.world().resource::<MaterialGraphPaletteState>();
    assert!(palette.open.is_none());
    assert!(palette.query.is_empty());
    f.app.update();
    assert_eq!(f.current(), changed);
    f.step(HistoryAction::Undo);
    assert_eq!(f.current(), f.original);
    f.step(HistoryAction::Redo);
    assert_eq!(f.current(), changed);
    f.disk_unchanged();
}
#[test]
fn pointer_press_without_activate_does_not_edit_palette_or_context() {
    let mut f = Fixture::new(false);
    f.open_palette();
    f.open_context();
    let a = f.button(f.action());
    let b = f.button(MaterialGraphContextAction::Delete(f.original.id));
    control_input::set_pressed(f.app.world_mut(), a, true);
    control_input::set_pressed(f.app.world_mut(), b, true);
    f.app.update();
    f.unchanged();
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphPaletteState>()
            .is_open()
    );
}
#[test]
fn invalid_position_consumes_activation_but_keeps_palette_for_retry() {
    for position in [Vec2::splat(f32::NAN), Vec2::splat(f32::INFINITY)] {
        let mut f = Fixture::new(false);
        f.open_palette();
        let mut a = f.action();
        a.graph_position = position;
        let b = f.button(a);
        f.activate(b);
        f.unchanged();
        assert!(
            f.app
                .world()
                .resource::<MaterialGraphPaletteState>()
                .open
                .is_some()
        );
        assert_eq!(
            f.app.world().resource::<EditorSession>().status,
            "Cannot insert a node at an invalid graph position"
        );
    }
}
#[test]
fn missing_program_rejects_creation_without_history_or_file_write() {
    let mut f = Fixture::new(false);
    f.open_palette();
    let mut a = f.action();
    a.program = MaterialProgramId::new();
    let b = f.button(a);
    f.activate(b);
    f.unchanged();
    f.disk_unchanged();
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphPaletteState>()
            .open
            .is_none()
    );
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .status
            .starts_with("Could not add material node:")
    );
}
#[test]
fn planner_failure_closes_palette_without_mutating_document() {
    let mut f = Fixture::new(false);
    f.open_palette();
    let mut a = f.action();
    a.source = Some(MaterialExpressionId::new());
    a.kind = MaterialGraphCreateKind::Function(aestra_core::material::MaterialGraphFunction::Add);
    let b = f.button(a);
    f.activate(b);
    f.unchanged();
    f.disk_unchanged();
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphPaletteState>()
            .open
            .is_none()
    );
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .status
            .starts_with("Could not add material node:")
    );
}
#[test]
fn palette_wires_requested_output_and_undo_restores_original() {
    let mut f = Fixture::new(false);
    let mut a = f.action();
    a.target = Some(MaterialConnectionTarget::ProgramOutput(
        MaterialOutputSocket::Alpha,
    ));
    let b = f.button(a);
    f.activate(b);
    assert_ne!(f.current().outputs.alpha, f.original.outputs.alpha);
    assert_eq!(f.current().outputs.alpha, *f.selection().first().unwrap());
    f.step(HistoryAction::Undo);
    assert_eq!(f.current(), f.original);
    f.disk_unchanged();
}
#[test]
fn duplicate_then_delete_and_compound_history_restore_selection() {
    let mut f = Fixture::new(false);
    f.context(MaterialGraphContextAction::Duplicate(f.original.id));
    let duplicated = f.current();
    assert_eq!(
        duplicated.expressions.len(),
        f.original.expressions.len() + 2
    );
    assert!(f.selection().is_disjoint(&f.selected));
    f.context(MaterialGraphContextAction::Delete(f.original.id));
    assert_eq!(f.current(), f.original);
    assert!(f.selection().is_empty());
    f.step(HistoryAction::Undo);
    assert_eq!(f.current(), duplicated);
    f.step(HistoryAction::Undo);
    assert_eq!(f.current(), f.original);
    f.disk_unchanged();
}
#[test]
fn copy_cut_paste_uses_fragment_and_round_trips_history() {
    let mut f = Fixture::new(false);
    f.context(MaterialGraphContextAction::Clipboard(
        f.original.id,
        clipboard::Shortcut::Copy,
    ));
    f.unchanged();
    assert_eq!(
        f.app
            .world()
            .resource::<clipboard::GraphClipboard>()
            .fragment
            .as_ref()
            .unwrap()
            .len(),
        2
    );
    f.context(MaterialGraphContextAction::Clipboard(
        f.original.id,
        clipboard::Shortcut::Cut,
    ));
    assert_eq!(
        f.current().expressions.len() + 2,
        f.original.expressions.len()
    );
    // Source entities would normally be rebuilt by the native host; paste reads only its fragment.
    f.context(MaterialGraphContextAction::Clipboard(
        f.original.id,
        clipboard::Shortcut::Paste,
    ));
    let pasted = f.current();
    assert_eq!(pasted.expressions.len(), f.original.expressions.len());
    assert!(f.selection().is_disjoint(&f.selected));
    f.step(HistoryAction::Undo);
    f.step(HistoryAction::Undo);
    assert_eq!(f.current(), f.original);
    f.disk_unchanged();
}
#[test]
fn empty_clipboard_closes_popup_without_an_edit() {
    let mut f = Fixture::new(false);
    f.context(MaterialGraphContextAction::Clipboard(
        f.original.id,
        clipboard::Shortcut::Paste,
    ));
    f.unchanged();
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphPaletteState>()
            .node_menu
            .is_none()
    );
    assert_eq!(
        f.app.world().resource::<EditorSession>().status,
        "No graph nodes to paste"
    );
}

#[test]
fn palette_reactivates_its_retained_target_without_switching_the_effect() {
    let mut f = Fixture::new(false);
    let effect = f.app.world().resource::<EditorSession>().effect.clone();
    let action = f.action();
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .material_target = MaterialEditingTarget::EffectInstance;
    let button = f.button(action);
    f.activate(button);
    assert_eq!(
        f.app.world().resource::<EditorSession>().material_target,
        f.target()
    );
    assert_eq!(f.app.world().resource::<EditorSession>().effect, effect);
    assert_eq!(
        f.current().expressions.len(),
        f.original.expressions.len() + 1
    );
    f.disk_unchanged();
}

#[test]
fn context_edits_leave_other_view_selection_untouched() {
    let mut f = Fixture::new(false);
    let other = Some(EditorViewId(88));
    f.app
        .world_mut()
        .resource_mut::<MaterialGraphSelectionState>()
        .select_material_expressions(
            other,
            f.original.id,
            &f.selected,
            GraphSelectionMode::Replace,
        );
    f.context(MaterialGraphContextAction::Duplicate(f.original.id));
    assert_eq!(
        f.app
            .world()
            .resource::<MaterialGraphSelectionState>()
            .get(other)
            .unwrap()
            .expressions,
        f.selected
    );
    assert!(f.selection().is_disjoint(&f.selected));
}
#[test]
fn stale_scope_and_missing_mounted_target_do_not_edit() {
    for missing in [true, false] {
        let mut f = Fixture::new(false);
        f.open_context();
        if missing {
            f.app.world_mut().despawn(f.viewport);
        } else {
            f.app
                .world_mut()
                .resource_mut::<MaterialGraphPaletteState>()
                .node_menu
                .as_mut()
                .unwrap()
                .scope = Some(EditorViewId(999));
        }
        let b = f.button(MaterialGraphContextAction::Delete(f.original.id));
        f.activate(b);
        f.unchanged();
        assert!(
            f.app
                .world()
                .resource::<MaterialGraphPaletteState>()
                .node_menu
                .is_some()
        );
        assert_eq!(
            f.app.world().resource::<EditorSession>().status,
            "The material graph is no longer available"
        );
    }
}
#[test]
fn disconnect_selects_exact_connection_and_is_undoable() {
    let mut f = Fixture::new(true);
    let connection = MaterialGraphConnection {
        program: f.original.id,
        source: f.original.outputs.alpha,
        target: MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Alpha),
    };
    f.app
        .world_mut()
        .resource_mut::<MaterialGraphPaletteState>()
        .connection_menu = Some(MaterialGraphConnectionMenuOpen {
        program: f.original.id,
        scope: f.scope,
        menu_position: Vec2::ZERO,
        connections: vec![connection],
    });
    let b = f.button(MaterialGraphContextAction::Disconnect(connection));
    f.activate(b);
    assert_ne!(f.current().outputs.alpha, f.original.outputs.alpha);
    assert!(
        f.app
            .world()
            .resource::<MaterialGraphPaletteState>()
            .connection_menu
            .is_none()
    );
    f.step(HistoryAction::Undo);
    assert_eq!(f.current(), f.original);
    f.disk_unchanged();
}
#[test]
fn extraction_creates_function_through_actual_semantic_publication() {
    let mut f = Fixture::new(true);
    f.context(MaterialGraphContextAction::ExtractFunction(f.original.id));
    assert_ne!(f.current(), f.original);
    assert_eq!(
        f.app
            .world()
            .resource::<ProjectEffectCatalog>()
            .material_functions()
            .unwrap()
            .len(),
        1
    );
    f.step(HistoryAction::Undo);
    assert_eq!(f.current(), f.original);
    f.disk_unchanged();
}
#[test]
fn arrange_dispatches_actual_scoped_request_without_rebuilding_graph() {
    for scope in [
        arrange::ArrangeScope::Graph,
        arrange::ArrangeScope::Selection,
        arrange::ArrangeScope::Upstream,
        arrange::ArrangeScope::Downstream,
    ] {
        let mut f = Fixture::new(false);
        f.open_context();
        let popup = f.app.world_mut().spawn_empty().id();
        let surface = f
            .app
            .world_mut()
            .spawn((MaterialGraphNodeMenu, ChildOf(popup)))
            .id();
        let unrelated = f.app.world_mut().spawn_empty().id();
        let revision = f.app.world().resource::<EditorSession>().ui_revision;
        let b = f.button(MaterialGraphContextAction::Arrange(f.original.id, scope));
        f.activate(b);
        f.unchanged();
        let events = &f.app.world().resource::<Arrangements>().0;
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.scope, scope);
        assert_eq!(event.editing_target, f.target());
        assert_eq!(event.view.view, f.scope);
        assert_eq!(event.view.document.project, f.root.path());
        assert_eq!(
            event.view.document.asset,
            crate::document::DocumentKey::MaterialProgram(f.original.id)
        );
        assert_eq!(
            event.seeds,
            f.selected
                .iter()
                .copied()
                .map(GraphNodeKey::Expression)
                .collect()
        );
        assert_eq!(
            f.app.world().resource::<EditorSession>().ui_revision,
            revision
        );
        assert!(f.app.world().get_entity(popup).is_err());
        assert!(f.app.world().get_entity(surface).is_err());
        assert!(f.app.world().get_entity(unrelated).is_ok());
        assert!(f.app.world().get_entity(f.viewport).is_ok());
        assert!(
            f.app
                .world()
                .resource::<MaterialGraphPaletteState>()
                .node_menu
                .is_none()
        );
    }
}
#[test]
fn mismatched_context_program_does_not_queue_arrangement_or_change_selection() {
    let mut f = Fixture::new(false);
    f.open_context();
    let before = f.selection();
    let b = f.button(MaterialGraphContextAction::Arrange(
        MaterialProgramId::new(),
        arrange::ArrangeScope::Graph,
    ));
    f.activate(b);
    f.unchanged();
    assert_eq!(f.selection(), before);
    assert!(f.app.world().resource::<Arrangements>().0.is_empty());
}
#[test]
fn activation_queue_owns_only_classified_material_buttons() {
    let mut f = Fixture::new(false);
    let foreign = f.app.world_mut().spawn(FeathersActionButton).id();
    let action = f.action();
    let unmarked = f.app.world_mut().spawn(action).id();
    for entity in [foreign, unmarked] {
        f.app.world_mut().trigger(Activate { entity });
        f.app.world_mut().flush();
        assert!(
            f.app
                .world()
                .get::<PendingFeathersActivation>(entity)
                .is_none()
        );
    }
    // Toolbar and preview actions are owned by this queue, consumed by their own host systems.
    let preview = MaterialGraphPreviewToggle {
        program: f.original.id,
        editing_target: f.target(),
        target: MaterialGraphPreviewTarget::Expression(f.original.outputs.alpha),
    };
    assert_eq!(preview.program, f.original.id);
    assert_eq!(preview.editing_target, f.target());
    assert_eq!(
        preview.target,
        MaterialGraphPreviewTarget::Expression(f.original.outputs.alpha)
    );
    let b = f.button(preview);
    f.app.world_mut().trigger(Activate { entity: b });
    f.app.world_mut().flush();
    assert!(f.app.world().get::<PendingFeathersActivation>(b).is_some());
    for action in [
        MaterialGraphToolbarAction::EffectContext,
        MaterialGraphToolbarAction::LocateSource(f.original.id),
        MaterialGraphToolbarAction::AddNode(f.original.id, f.scope),
        MaterialGraphToolbarAction::Arrange(f.original.id, f.scope, arrange::ArrangeScope::Graph),
        MaterialGraphToolbarAction::ToggleAllPreviews(f.original.id, f.scope),
    ] {
        let b = f.button(action);
        f.app.world_mut().trigger(Activate { entity: b });
        f.app.world_mut().flush();
        assert!(f.app.world().get::<PendingFeathersActivation>(b).is_some());
    }
    f.unchanged();
}

#[test]
fn placement_context_uses_matching_mounted_view_and_preserves_existing_nodes() {
    use crate::feathers::node_graph::{
        geometry::{GraphGeometryNode, GraphGeometryView},
        placement::Context,
    };
    use bevy::ecs::system::RunSystemOnce;
    let mut f = Fixture::new(false);
    let key = GraphViewKey {
        document: GraphDocumentKey {
            project: f.root.path().into(),
            asset: crate::document::DocumentKey::MaterialProgram(f.original.id),
        },
        view: f.scope,
    };
    f.app.world_mut().entity_mut(f.viewport).insert((
        GraphGeometryView {
            key: key.clone(),
            nodes: BTreeSet::new(),
        },
        ComputedNode {
            size: Vec2::new(800.0, 600.0),
            ..default()
        },
    ));
    let node = *f.selected.first().unwrap();
    let graph_key = material_graph_view_key(f.original.id);
    f.app
        .world_mut()
        .resource_mut::<GraphViewportMemory>()
        .remove_node(&graph_key, &material_graph_expression_node_key(node));
    f.app.world_mut().spawn((
        ChildOf(f.viewport),
        GraphGeometryNode::new(GraphNodeKey::Expression(node), &"bootstrap", false),
        FeathersGraphNode::clipboard_fixture(
            graph_key.clone(),
            material_graph_expression_node_key(node),
            Vec2::new(90.0, 70.0),
        ),
        ComputedNode::default(),
    ));
    let captured_key = key.clone();
    f.app
        .world_mut()
        .run_system_once(
            move |placement: Context, mut memory: ResMut<GraphViewportMemory>| {
                assert_eq!(
                    placement.document_view(&captured_key.document),
                    Some(captured_key.clone())
                );
                assert_eq!(
                    placement.center(&captured_key),
                    Some(Vec2::new(180.0, 140.0))
                );
                placement.preserve_existing(&captured_key, &mut memory);
            },
        )
        .unwrap();
    assert_eq!(
        f.app
            .world()
            .resource::<GraphViewportMemory>()
            .node_position(&graph_key, &material_graph_expression_node_key(node)),
        Some(Vec2::new(90.0, 70.0))
    );
    f.open_palette();
    let b = f.button(f.action());
    f.activate(b);
    assert_eq!(
        f.app
            .world()
            .resource::<GraphViewportMemory>()
            .node_position(&graph_key, &material_graph_expression_node_key(node)),
        Some(Vec2::new(90.0, 70.0))
    );
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .status
            .contains("Local spacing unavailable")
    );
}
