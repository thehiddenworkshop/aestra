//! Composed function controls through actual observers, publication and global history.
use super::*;
use crate::{
    docking::EditorViewId,
    feathers::{icon::SvgPlugin, text_input::tests as text},
    history::{
        EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, execute_history_action,
    },
    material_document::MaterialEditingTarget,
    material_function_editor::graph::menu_state::FunctionGraphMenuState,
    material_graph::{
        MaterialGraphSelectionState,
        arrange::{ArrangeGraph, ArrangeScope},
    },
};
use aestra_compiler::{MaterialGraphCreateKind, MaterialGraphFunction};
use bevy::input::keyboard::Key;
use std::collections::BTreeSet;

fn function(value: MaterialValue) -> MaterialFunction {
    let mut f = MaterialFunction::from_ron(include_str!(
        "../../../../../assets/test/materials/pulse_wave.aestra.material-function.ron"
    ))
    .unwrap();
    f.custom_wesl = None;
    f.expressions = vec![
        MaterialExpression {
            id: f.outputs[0].expression,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        },
        MaterialExpression {
            id: MaterialExpressionId::new(),
            kind: MaterialExpressionKind::Constant(value),
        },
    ];
    f.normalized()
}

#[derive(Resource, Default)]
struct Requests {
    arrange: Vec<ArrangeGraph>,
    locate: Vec<aestra_project::ProjectAssetId>,
}
struct Fixture {
    dir: tempfile::TempDir,
    original: MaterialFunction,
    app: App,
    bytes: Vec<u8>,
    scope: Option<EditorViewId>,
}
impl Fixture {
    fn new(value: MaterialValue) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let original = function(value);
        let path = dir.path().join("body.aestra.material-function.ron");
        original.save_ron(&path).unwrap();
        let bytes = std::fs::read(path).unwrap();
        let catalog = ProjectEffectCatalog::scan(dir.path());
        let mut session = crate::test_support::session_with_timing_slack();
        session
            .open_material_function(&catalog, original.id)
            .unwrap();
        let mut app = text::app_with(|app| {
            app.add_plugins((SvgPlugin, crate::feathers::number_input::NumberScrubPlugin));
        });
        app.insert_resource(catalog)
            .insert_resource(session)
            .init_resource::<FunctionEditor>()
            .init_resource::<GraphViewportMemory>()
            .init_resource::<MaterialGraphSelectionState>()
            .init_resource::<FunctionGraphMenuState>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<Requests>()
            .add_observer(execute_history_action)
            .add_observer(|event: On<ArrangeGraph>, mut requests: ResMut<Requests>| {
                requests.arrange.push(event.event().clone());
            })
            .add_observer(
                |event: On<crate::asset_browser::LocateInAssets>,
                 mut requests: ResMut<Requests>| {
                    requests.locate.push(event.0);
                },
            );
        register_actions(&mut app);
        register_constants(&mut app);
        let scope = Some(EditorViewId(9));
        let target = session_target(dir.path(), original.id);
        app.world_mut()
            .resource_scope(|world, session: Mut<EditorSession>| {
                world.resource_scope(|world, catalog: Mut<ProjectEffectCatalog>| {
                    let assets = world.resource::<AssetServer>().clone();
                    world.resource_scope(|world, memory: Mut<GraphViewportMemory>| {
                        world.resource_scope(
                            |world, selection: Mut<MaterialGraphSelectionState>| {
                                let mut commands = world.commands();
                                commands.spawn(Node::default()).with_children(|p| {
                                    crate::material_function_editor::spawn_graph(
                                        p,
                                        &session,
                                        &catalog,
                                        &assets,
                                        &memory,
                                        &selection,
                                        &FunctionGraphMenuState::default(),
                                        &target,
                                        scope,
                                    );
                                });
                            },
                        );
                    });
                });
            });
        app.world_mut().flush();
        for _ in 0..5 {
            app.update();
        }
        Self {
            dir,
            original,
            app,
            bytes,
            scope,
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
    fn extra(&self) -> MaterialExpressionId {
        self.original
            .expressions
            .iter()
            .find(|e| e.id != self.original.outputs[0].expression)
            .unwrap()
            .id
    }
    fn action(&mut self, predicate: impl Fn(BodyActionKind) -> bool) -> Entity {
        let world = self.app.world_mut();
        world
            .query::<(Entity, &BodyAction)>()
            .iter(world)
            .find(|(_, a)| predicate(a.kind))
            .unwrap()
            .0
    }
    fn constant(&mut self, expression: MaterialExpressionId, component: usize) -> Entity {
        let world = self.app.world_mut();
        world
            .query::<(Entity, &Constant)>()
            .iter(world)
            .find(|(_, c)| c.expression == expression && c.component == component)
            .unwrap()
            .0
    }
    fn activate(&mut self, entity: Entity) {
        self.app.world_mut().trigger(Activate { entity });
        self.app.world_mut().flush();
    }
    fn number(&mut self, source: Entity, value: f32, is_final: bool) {
        self.app.world_mut().trigger(ValueChange {
            source,
            value,
            is_final,
        });
        self.app.world_mut().flush();
    }
    fn string(&mut self, source: Entity, value: &str, is_final: bool) {
        self.app.world_mut().trigger(ValueChange {
            source,
            value: value.to_owned(),
            is_final,
        });
        self.app.world_mut().flush();
    }
    fn step(&mut self, undo: bool) {
        self.app.world_mut().trigger(if undo {
            HistoryAction::Undo
        } else {
            HistoryAction::Redo
        });
        self.app.world_mut().flush();
    }
    fn disk_unchanged(&self) {
        assert_eq!(
            std::fs::read(self.dir.path().join("body.aestra.material-function.ron")).unwrap(),
            self.bytes
        );
    }
}
fn session_target(
    root: &std::path::Path,
    id: aestra_core::MaterialFunctionId,
) -> MaterialEditingTarget {
    MaterialEditingTarget::Function {
        root: root.to_owned(),
        id,
    }
}

#[test]
fn composed_creation_activates_its_owner_and_round_trips_one_compound_edit() {
    let mut f = Fixture::new(MaterialValue::Float(0.25));
    let effect = f.app.world().resource::<EditorSession>().effect.clone();
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .material_target = MaterialEditingTarget::EffectInstance;
    let key = f.key();
    let existing = f.original.outputs[0].expression.to_string();
    let position = Vec2::new(-700.0, 900.0);
    {
        let mut memory = f.app.world_mut().resource_mut::<GraphViewportMemory>();
        memory.set_node(&key, &existing, position, false);
        memory.set_pinned(&key, &existing, true);
    }
    let action = f.action(|k| {
        matches!(
            k,
            BodyActionKind::Create(MaterialGraphCreateKind::Function(
                MaterialGraphFunction::Multiply
            ))
        )
    });
    f.activate(action);
    let created = f.current();
    assert!(created.expressions.len() > f.original.expressions.len());
    assert_eq!(f.serial(), 1);
    assert_eq!(
        f.app.world().resource::<EditorSession>().material_target,
        session_target(f.dir.path(), f.original.id)
    );
    assert_eq!(f.app.world().resource::<EditorSession>().effect, effect);
    let placed = f
        .app
        .world()
        .resource::<GraphViewportMemory>()
        .base_nodes(&key);
    assert_eq!(placed[&existing], (position, false));
    assert!(
        f.app
            .world()
            .resource::<GraphViewportMemory>()
            .is_pinned(&key, &existing)
    );
    let new_ids = created
        .expressions
        .iter()
        .filter(|e| !f.original.expressions.iter().any(|old| old.id == e.id))
        .map(|e| e.id.to_string())
        .collect::<Vec<_>>();
    for id in &new_ids {
        assert!(placed.contains_key(id));
    }
    f.step(true);
    assert_eq!(f.current(), f.original);
    for id in &new_ids {
        assert!(
            f.app
                .world()
                .resource::<GraphViewportMemory>()
                .node(&key, id)
                .is_none()
        );
    }
    f.step(false);
    assert_eq!(f.current(), created);
    assert_eq!(
        f.app
            .world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&key),
        placed
    );
    f.disk_unchanged();
}

#[test]
fn actual_input_creation_uses_the_declared_port_and_is_globally_undoable() {
    let mut f = Fixture::new(MaterialValue::Float(0.25));
    let input = f.original.inputs[0].id;
    let action = f.action(|k| matches!(k, BodyActionKind::Input(id) if id == input));
    f.activate(action);
    assert!(
        f.current()
            .expressions
            .iter()
            .any(|e| matches!(e.kind, MaterialExpressionKind::FunctionInput(id) if id == input))
    );
    assert_eq!(f.serial(), 1);
    f.step(true);
    assert_eq!(f.current(), f.original);
    f.disk_unchanged();
}

#[test]
fn composed_boolean_action_preserves_identity_and_repeated_value_adds_no_history() {
    let mut f = Fixture::new(MaterialValue::Bool(false));
    let id = f.extra();
    let action = f.action(|k| matches!(k, BodyActionKind::Boolean(actual, true) if actual == id));
    f.activate(action);
    assert!(f.current().expressions.iter().any(
        |e| e.id == id && e.kind == MaterialExpressionKind::Constant(MaterialValue::Bool(true))
    ));
    assert_eq!(f.serial(), 1);
    f.activate(action);
    assert_eq!(f.serial(), 1);
    f.step(true);
    assert_eq!(f.current(), f.original);
    f.disk_unchanged();
}

#[test]
fn scalar_text_is_final_only_and_rejects_bad_or_nonfinite_values_without_history() {
    let mut f = Fixture::new(MaterialValue::Float(0.25));
    let id = f.extra();
    let control = f.constant(id, 0);
    f.string(control, "2.5", false);
    assert_eq!(f.current(), f.original);
    f.string(control, " 2.5 ", true);
    assert_eq!(f.serial(), 1);
    let changed = f.current();
    for invalid in ["not a number", "", "NaN", "inf", "-inf"] {
        f.string(control, invalid, true);
        assert_eq!(f.current(), changed);
        assert_eq!(f.serial(), 1);
    }
    f.string(control, "2.5", true);
    assert_eq!(f.serial(), 1);
    f.step(true);
    assert_eq!(f.current(), f.original);
    f.disk_unchanged();
}

#[test]
fn vector_and_color_number_commits_preserve_other_channels_and_literal_type() {
    for value in [
        MaterialValue::Vec2([1.0, 2.0]),
        MaterialValue::Vec3([1.0, 2.0, 3.0]),
        MaterialValue::Vec4([1.0, 2.0, 3.0, 4.0]),
        MaterialValue::ColorSrgb([0.1, 0.2, 0.3, 0.4]),
    ] {
        let mut f = Fixture::new(value.clone());
        let id = f.extra();
        let control = f.constant(id, 1);
        f.number(control, 0.75, false);
        assert_eq!(f.current(), f.original);
        f.number(control, 0.75, true);
        let mut expected = value;
        set_component(&mut expected, 1, 0.75).unwrap();
        assert_eq!(
            f.current()
                .expressions
                .iter()
                .find(|e| e.id == id)
                .unwrap()
                .kind,
            MaterialExpressionKind::Constant(expected)
        );
        assert_eq!(f.serial(), 1);
        let changed = f.current();
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            f.number(control, invalid, true);
            assert_eq!(f.current(), changed);
            assert_eq!(f.serial(), 1);
        }
        f.step(true);
        assert_eq!(f.current(), f.original);
        f.step(false);
        assert_eq!(f.current(), changed);
        f.disk_unchanged();
    }
}

#[test]
fn node_menu_removal_rejects_a_required_output_source_and_can_remove_an_unused_node() {
    let mut f = Fixture::new(MaterialValue::Float(0.25));
    let output = f.original.outputs[0].expression;
    let invalid = f.action(|k| matches!(k, BodyActionKind::Remove(id) if id == output));
    f.activate(invalid);
    assert_eq!(f.current(), f.original);
    assert_eq!(f.serial(), 0);
    let unused = f.extra();
    let action = f.action(|k| matches!(k, BodyActionKind::Remove(id) if id == unused));
    f.activate(action);
    assert!(!f.current().expressions.iter().any(|e| e.id == unused));
    assert_eq!(f.serial(), 1);
    f.step(true);
    assert_eq!(f.current(), f.original);
    f.disk_unchanged();
}

#[test]
fn actual_toolbar_arrange_locate_and_back_route_without_semantic_edits() {
    let mut f = Fixture::new(MaterialValue::Float(0.25));
    let id = f.extra();
    f.app
        .world_mut()
        .resource_mut::<MaterialGraphSelectionState>()
        .select_function_expression(f.scope, f.original.id, id, false, false);
    let arrange = f.action(|k| matches!(k, BodyActionKind::Arrange(ArrangeScope::Graph)));
    let locate = f.action(|k| matches!(k, BodyActionKind::Locate));
    let back = f.action(|k| matches!(k, BodyActionKind::Back));
    f.activate(arrange);
    f.activate(locate);
    let requests = f.app.world().resource::<Requests>();
    assert_eq!(requests.arrange.len(), 1);
    assert_eq!(requests.arrange[0].scope, ArrangeScope::Graph);
    assert_eq!(requests.arrange[0].view.view, f.scope);
    assert_eq!(requests.arrange[0].view.document.project, f.dir.path());
    assert_eq!(
        requests.arrange[0].editing_target,
        session_target(f.dir.path(), f.original.id)
    );
    assert_eq!(
        requests.arrange[0].seeds,
        BTreeSet::from([crate::feathers::node_graph::geometry::GraphNodeKey::Expression(id)])
    );
    assert_eq!(
        requests.locate,
        [aestra_project::ProjectAssetId::MaterialFunction(
            f.original.id
        )]
    );
    assert_eq!(f.serial(), 0);
    f.activate(back);
    assert_eq!(
        f.app.world().resource::<EditorSession>().material_target,
        MaterialEditingTarget::EffectInstance
    );
    assert_eq!(f.serial(), 0);
    f.disk_unchanged();
}

#[test]
fn unavailable_old_controls_cannot_edit_the_new_project_or_target() {
    let mut f = Fixture::new(MaterialValue::Float(0.25));
    let control = f.constant(f.extra(), 0);
    let action = f.action(|k| matches!(k, BodyActionKind::Input(_)));
    let other = tempfile::tempdir().unwrap();
    let mut replacement = function(MaterialValue::Bool(true));
    replacement.id = aestra_core::MaterialFunctionId::new();
    replacement
        .save_ron(other.path().join("other.aestra.material-function.ron"))
        .unwrap();
    let catalog = ProjectEffectCatalog::scan(other.path());
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .open_material_function(&catalog, replacement.id)
        .unwrap();
    f.app.insert_resource(catalog);
    f.string(control, "5.0", true);
    f.number(control, 8.0, true);
    f.activate(action);
    assert_eq!(f.current(), replacement.normalized());
    assert_eq!(f.serial(), 0);
    assert_eq!(
        f.app.world().resource::<EditorSession>().material_target,
        session_target(other.path(), replacement.id)
    );
    f.disk_unchanged();
}

#[test]
fn real_keyboard_constant_edit_commits_on_enter_once() {
    let mut f = Fixture::new(MaterialValue::Float(0.25));
    let expression = f.original.outputs[0].expression;
    let control = f.constant(expression, 0);
    let native = f
        .app
        .world()
        .get::<Children>(control)
        .unwrap()
        .iter()
        .find(|&e| {
            f.app
                .world()
                .get::<bevy::feathers::controls::FeathersNumberInput>(e)
                .is_some()
        })
        .unwrap();
    let input = f
        .app
        .world()
        .get::<Children>(native)
        .unwrap()
        .iter()
        .find(|&e| f.app.world().get::<bevy::text::EditableText>(e).is_some())
        .unwrap();
    text::focus(&mut f.app, input);
    text::tap(&mut f.app, KeyCode::End, Key::End, None);
    text::tap(
        &mut f.app,
        KeyCode::Digit5,
        Key::Character("5".into()),
        Some("5"),
    );
    assert_eq!(f.serial(), 0);
    text::tap(&mut f.app, KeyCode::Enter, Key::Enter, None);
    assert_eq!(
        f.current()
            .expressions
            .iter()
            .find(|e| e.id == expression)
            .unwrap()
            .kind,
        MaterialExpressionKind::Constant(MaterialValue::Float(15.0))
    );
    assert_eq!(f.serial(), 1);
    f.step(true);
    assert_eq!(f.current(), f.original);
    f.disk_unchanged();
}

#[test]
fn creation_without_viewport_memory_keeps_semantic_validation_and_undo() {
    let mut f = Fixture::new(MaterialValue::Float(0.25));
    let action = f.action(|k| matches!(k, BodyActionKind::Input(_)));
    f.app.world_mut().remove_resource::<GraphViewportMemory>();
    f.activate(action);
    assert_eq!(f.serial(), 1);
    assert!(f.current().expressions.len() > f.original.expressions.len());
    f.step(true);
    assert_eq!(f.current(), f.original);
    f.disk_unchanged();
}
