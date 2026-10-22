//! Real composed menus and modified-drag consumers with publication and global history.
use super::*;
use crate::{
    docking::EditorViewId,
    material_document::MaterialEditingTarget,
    material_function_editor::graph::{modified_input, socket_palette::tests::setup_with_function},
    material_graph::{
        MaterialGraphSelectionState,
        arrange::{ArrangeGraph, ArrangeScope},
    },
};
use aestra_core::{
    MaterialExpressionId,
    material::{MaterialFunction, MaterialValue},
};
use bevy::ecs::system::RunSystemOnce;

fn function() -> MaterialFunction {
    let mut f = MaterialFunction::from_ron(include_str!(
        "../../../../../../assets/test/materials/pulse_wave.aestra.material-function.ron"
    ))
    .unwrap();
    f.custom_wesl = None;
    let a = MaterialExpressionId::new();
    let b = MaterialExpressionId::new();
    f.expressions = vec![
        MaterialExpression {
            id: a,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        },
        MaterialExpression {
            id: b,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(2.0)),
        },
        MaterialExpression {
            id: f.outputs[0].expression,
            kind: MaterialExpressionKind::Multiply(a, b),
        },
        MaterialExpression {
            id: MaterialExpressionId::new(),
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(3.0)),
        },
    ];
    f.normalized()
}
struct Fixture {
    app: App,
    dir: tempfile::TempDir,
    original: MaterialFunction,
    host: Entity,
    scope: Option<EditorViewId>,
    bytes: Vec<u8>,
}
impl Fixture {
    fn new() -> Self {
        let original = function();
        let (mut app, dir, viewport, _, _) = setup_with_function(1.25, 0.75, original.clone());
        let host = app.world().get::<ChildOf>(viewport).unwrap().parent();
        app.init_resource::<FunctionGraphMenuState>()
            .init_resource::<crate::material_graph::clipboard::GraphClipboard>()
            .add_observer(crate::material_graph::presentation::batch_edit);
        register(&mut app);
        register_dismissal(&mut app);
        modified_input::register(&mut app);
        let bytes = std::fs::read(dir.path().join("body.aestra.material-function.ron")).unwrap();
        Self {
            app,
            dir,
            original,
            host,
            scope: Some(EditorViewId(9)),
            bytes,
        }
    }
    fn current(&self) -> MaterialFunction {
        self.app
            .world()
            .resource::<EditorSession>()
            .graph_function(self.app.world().resource::<ProjectEffectCatalog>())
            .unwrap()
    }
    fn serial(&self) -> u64 {
        self.app
            .world()
            .resource::<EditorSession>()
            .operation_order
            .edit_serial()
    }
    fn key(&self) -> String {
        crate::material_graph::function_graph_memory_key(self.dir.path(), self.original.id)
    }
    fn output(&self) -> MaterialExpressionId {
        self.original.outputs[0].expression
    }
    fn unused(&self) -> MaterialExpressionId {
        self.original
            .expressions
            .iter()
            .find(|e| e.kind == MaterialExpressionKind::Constant(MaterialValue::Float(3.0)))
            .unwrap()
            .id
    }
    fn a(&self) -> MaterialExpressionId {
        match self
            .original
            .expressions
            .iter()
            .find(|e| e.id == self.output())
            .unwrap()
            .kind
        {
            MaterialExpressionKind::Multiply(a, _) => a,
            _ => unreachable!(),
        }
    }
    fn node(&mut self, id: MaterialExpressionId) -> Entity {
        let world = self.app.world_mut();
        world
            .query::<(Entity, &FunctionGraphNodeAction)>()
            .iter(world)
            .find(|(_, a)| a.expression == id && a.scope == self.scope)
            .unwrap()
            .0
    }
    fn right(&mut self, entity: Entity) {
        let position = self
            .app
            .world()
            .get::<UiGlobalTransform>(entity)
            .map_or(Vec2::ZERO, |t| t.translation.trunc() / 1.25);
        self.app
            .world_mut()
            .trigger(crate::material_graph::click_input::click(
                entity,
                Entity::PLACEHOLDER,
                position,
                PointerButton::Secondary,
            ));
        self.app.world_mut().flush();
    }
    fn mount(&mut self) {
        self.app
            .world_mut()
            .entity_mut(self.host)
            .despawn_related::<Children>();
        let host = self.host;
        let target = MaterialEditingTarget::Function {
            root: self.dir.path().to_owned(),
            id: self.original.id,
        };
        let scope = self.scope;
        self.app
            .world_mut()
            .run_system_once(
                move |mut commands: Commands,
                      session: Res<EditorSession>,
                      catalog: Res<ProjectEffectCatalog>,
                      assets: Res<AssetServer>,
                      memory: Res<GraphViewportMemory>,
                      selection: Res<MaterialGraphSelectionState>,
                      menus: Res<FunctionGraphMenuState>| {
                    commands.entity(host).with_children(|p| {
                        crate::material_function_editor::spawn_graph(
                            p, &session, &catalog, &assets, &memory, &selection, &menus, &target,
                            scope,
                        )
                    });
                },
            )
            .unwrap();
        for _ in 0..4 {
            self.app.update();
        }
    }
    fn open_node(&mut self, id: MaterialExpressionId) {
        let node = self.node(id);
        self.right(node);
        assert!(
            matches!(self.app.world().resource::<FunctionGraphMenuState>().open.as_ref().unwrap().kind, FunctionGraphMenuKind::Node(actual) if actual == id)
        );
        self.mount();
    }
    fn activate(&mut self, predicate: impl Fn(&FunctionGraphContextAction) -> bool) {
        let world = self.app.world_mut();
        let entity = world
            .query::<(Entity, &FunctionGraphContextAction)>()
            .iter(world)
            .find(|(_, a)| predicate(a))
            .unwrap()
            .0;
        world.trigger(Activate { entity });
        world.flush();
    }
    fn select(&mut self, ids: &[MaterialExpressionId]) {
        self.app
            .world_mut()
            .resource_mut::<MaterialGraphSelectionState>()
            .select_function_expressions(
                self.scope,
                self.original.id,
                &ids.iter().copied().collect(),
                GraphSelectionMode::Replace,
            );
    }
    fn step(&mut self, undo: bool) {
        self.app.world_mut().trigger(if undo {
            crate::history::HistoryAction::Undo
        } else {
            crate::history::HistoryAction::Redo
        });
        self.app.world_mut().flush();
    }
    fn disk_unchanged(&self) {
        assert_eq!(
            std::fs::read(self.dir.path().join("body.aestra.material-function.ron")).unwrap(),
            self.bytes
        );
    }
    fn positions(&self) -> std::collections::BTreeMap<String, (Vec2, bool)> {
        self.app
            .world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&self.key())
    }
    fn remember_layout(&mut self) {
        // These cases exercise modified dragging of an explicitly positioned graph.
        let key = self.key();
        let world = self.app.world_mut();
        let nodes = world
            .query::<&FeathersGraphNode>()
            .iter(world)
            .filter(|n| n.graph_key() == key)
            .map(|n| (n.node_key().to_owned(), n.position(), n.collapsed()))
            .collect::<Vec<_>>();
        let mut memory = world.resource_mut::<GraphViewportMemory>();
        for (node, position, collapsed) in nodes {
            memory.set_node(&key, node, position, collapsed);
        }
    }
    fn modified(&mut self, id: MaterialExpressionId, modifier: GraphNodeDragModifier, delta: Vec2) {
        let key = self.key();
        let before = self
            .app
            .world()
            .resource::<GraphViewportMemory>()
            .node(&key, &id.to_string())
            .unwrap();
        let after = (before.0 + delta, before.1);
        // The shared pointer-drag core restores a duplicate's origin, but retains a branch drop.
        if modifier != GraphNodeDragModifier::Duplicate {
            self.app
                .world_mut()
                .resource_mut::<GraphViewportMemory>()
                .set_node(&key, id.to_string(), after.0, after.1);
        }
        self.app.world_mut().trigger(GraphModifiedNodeDrag {
            graph: key,
            node: id.to_string(),
            before,
            after,
            modifier,
        });
        self.app.world_mut().flush();
    }
}

#[test]
fn composed_duplicate_menu_activates_its_owner_preserves_group_and_round_trips_once() {
    let mut f = Fixture::new();
    let selected = [f.a(), f.unused()];
    f.select(&selected);
    f.open_node(selected[0]);
    let effect = f.app.world().resource::<EditorSession>().effect.clone();
    let before = f.positions();
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .material_target = MaterialEditingTarget::EffectInstance;
    f.activate(|a| matches!(a, FunctionGraphContextAction::Duplicate(_)));
    let after = f.current();
    assert_eq!(after.expressions.len(), f.original.expressions.len() + 2);
    assert_eq!(f.serial(), 1);
    assert_eq!(f.app.world().resource::<EditorSession>().effect, effect);
    f.step(true);
    assert_eq!(f.current(), f.original);
    assert_eq!(f.positions(), before);
    f.step(false);
    assert_eq!(f.current(), after);
    f.disk_unchanged();
}

#[test]
fn actual_delete_menu_removes_unused_node_and_layout_in_one_undo() {
    let mut f = Fixture::new();
    let unused = f.unused();
    f.open_node(unused);
    let before = f.positions();
    f.activate(|a| matches!(a, FunctionGraphContextAction::Delete(_)));
    assert!(!f.current().expressions.iter().any(|e| e.id == unused));
    assert!(!f.positions().contains_key(&unused.to_string()));
    assert_eq!(f.serial(), 1);
    f.step(true);
    assert_eq!(f.current(), f.original);
    assert_eq!(f.positions(), before);
    f.disk_unchanged();
}

#[test]
fn rejected_required_source_delete_keeps_layout_body_and_history() {
    let mut f = Fixture::new();
    let output = f.output();
    f.open_node(output);
    let before = f.positions();
    f.activate(|a| matches!(a, FunctionGraphContextAction::Delete(_)));
    assert_eq!(f.current(), f.original);
    assert_eq!(f.positions(), before);
    assert_eq!(f.serial(), 0);
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .status
            .starts_with("Could not delete function nodes:")
    );
    f.disk_unchanged();
}

#[test]
fn context_copy_and_paste_use_real_clipboard_and_global_history() {
    let mut f = Fixture::new();
    let id = f.unused();
    f.open_node(id);
    f.activate(|a| {
        matches!(
            a,
            FunctionGraphContextAction::Clipboard(crate::material_graph::clipboard::Shortcut::Copy)
        )
    });
    assert_eq!(f.serial(), 0);
    f.open_node(id);
    f.activate(|a| {
        matches!(
            a,
            FunctionGraphContextAction::Clipboard(
                crate::material_graph::clipboard::Shortcut::Paste
            )
        )
    });
    assert_eq!(
        f.current().expressions.len(),
        f.original.expressions.len() + 1
    );
    assert_eq!(f.serial(), 1);
    f.step(true);
    assert_eq!(f.current(), f.original);
    f.disk_unchanged();
}

#[test]
fn connected_pin_menu_cannot_disconnect_a_required_output() {
    let mut f = Fixture::new();
    let world = f.app.world_mut();
    let socket = world
        .query::<(Entity, &Socket)>()
        .iter(world)
        .find(|(_, s)| matches!(s.kind, SocketKind::Target(Target::Output(_))))
        .unwrap()
        .0;
    f.right(socket);
    let open = f
        .app
        .world()
        .resource::<FunctionGraphMenuState>()
        .open
        .as_ref()
        .unwrap();
    assert_eq!(open.scope, f.scope);
    assert!(matches!(&open.kind, FunctionGraphMenuKind::Connections(edges) if edges.len() == 1));
    f.mount();
    f.activate(|a| matches!(a, FunctionGraphContextAction::Disconnect(Target::Output(_))));
    assert_eq!(f.current(), f.original);
    assert_eq!(f.serial(), 0);
    f.disk_unchanged();
}

#[test]
fn actual_arrange_submenu_keeps_scope_selection_and_unfocused_target() {
    #[derive(Resource, Default)]
    struct Requests(Vec<ArrangeGraph>);
    let mut f = Fixture::new();
    f.app.init_resource::<Requests>().add_observer(
        |event: On<ArrangeGraph>, mut requests: ResMut<Requests>| {
            requests.0.push(event.event().clone())
        },
    );
    let selected = [f.a(), f.unused()];
    f.select(&selected);
    f.open_node(selected[0]);
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .material_target = MaterialEditingTarget::EffectInstance;
    f.activate(|a| {
        matches!(
            a,
            FunctionGraphContextAction::Arrange(ArrangeScope::Upstream)
        )
    });
    let requests = &f.app.world().resource::<Requests>().0;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].view.view, f.scope);
    assert_eq!(requests[0].view.document.project, f.dir.path());
    assert_eq!(
        requests[0].seeds,
        selected.into_iter().map(GraphNodeKey::Expression).collect()
    );
    assert_eq!(
        f.app.world().resource::<EditorSession>().material_target,
        MaterialEditingTarget::EffectInstance
    );
    assert_eq!(f.serial(), 0);
    f.disk_unchanged();
}

#[test]
fn real_menu_hover_retains_state_and_escape_dismisses_without_editing() {
    let mut f = Fixture::new();
    let id = f.unused();
    f.open_node(id);
    let world = f.app.world_mut();
    let surface = world
        .query_filtered::<Entity, With<FunctionGraphContextMenu>>()
        .iter(world)
        .next()
        .unwrap();
    f.app
        .world_mut()
        .entity_mut(surface)
        .insert(RelativeCursorPosition {
            cursor_over: true,
            normalized: None,
        });
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    f.app
        .world_mut()
        .run_system_once(dismiss_function_graph_menu)
        .unwrap();
    assert!(
        f.app
            .world()
            .resource::<FunctionGraphMenuState>()
            .open
            .is_some()
    );
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    f.app
        .world_mut()
        .run_system_once(dismiss_function_graph_menu)
        .unwrap();
    assert!(
        f.app
            .world()
            .resource::<FunctionGraphMenuState>()
            .open
            .is_none()
    );
    assert_eq!(f.current(), f.original);
    assert_eq!(f.serial(), 0);
    f.disk_unchanged();
}

#[test]
fn modified_duplicate_uses_drag_delta_and_is_one_compound_global_edit() {
    let mut f = Fixture::new();
    f.remember_layout();
    let id = f.unused();
    f.select(&[id]);
    let before = f.positions();
    let delta = Vec2::new(50.0, 90.0);
    f.modified(id, GraphNodeDragModifier::Duplicate, delta);
    let after = f.current();
    let created = after
        .expressions
        .iter()
        .find(|e| !f.original.expressions.iter().any(|old| old.id == e.id))
        .unwrap()
        .id;
    assert_eq!(f.serial(), 1);
    assert_eq!(
        f.positions()[&created.to_string()].0,
        before[&id.to_string()].0 + delta
    );
    assert_eq!(f.positions()[&id.to_string()], before[&id.to_string()]);
    f.step(true);
    assert_eq!(f.current(), f.original);
    assert_eq!(f.positions(), before);
    f.step(false);
    assert_eq!(f.current(), after);
    f.disk_unchanged();
}

#[test]
fn upstream_and_downstream_drags_use_actual_presentation_history_without_semantic_edits() {
    for upstream in [true, false] {
        let mut f = Fixture::new();
        f.remember_layout();
        let seed = if upstream { f.output() } else { f.a() };
        let before = f.positions();
        let delta = Vec2::new(60.0, -40.0);
        f.modified(
            seed,
            if upstream {
                GraphNodeDragModifier::Upstream
            } else {
                GraphNodeDragModifier::Downstream
            },
            delta,
        );
        assert_eq!(f.current(), f.original);
        let after = f.positions();
        assert_eq!(
            after[&seed.to_string()].0,
            before[&seed.to_string()].0 + delta
        );
        assert_eq!(
            after[&f.unused().to_string()],
            before[&f.unused().to_string()]
        );
        if !upstream {
            assert_eq!(after["outputs"].0, before["outputs"].0 + delta);
        }
        f.step(true);
        assert_eq!(f.positions(), before);
        assert_eq!(f.current(), f.original);
        f.step(false);
        assert_eq!(f.positions(), after);
        assert_eq!(f.current(), f.original);
        f.disk_unchanged();
    }
}

#[test]
fn unknown_modified_drag_target_does_not_edit_or_move_any_node() {
    let mut f = Fixture::new();
    let before = f.positions();
    let node = f.unused().to_string();
    f.app.world_mut().trigger(GraphModifiedNodeDrag {
        graph: "another graph".into(),
        node,
        before: (Vec2::ZERO, false),
        after: (Vec2::ONE, false),
        modifier: GraphNodeDragModifier::Duplicate,
    });
    f.app.world_mut().flush();
    assert_eq!(f.current(), f.original);
    assert_eq!(f.positions(), before);
    assert_eq!(f.serial(), 0);
    f.disk_unchanged();
}
