//! Real signature controls, observer registration, publication, placement and history routing.
use super::*;
use crate::{
    feathers::{node_graph::GraphViewportMemory, text_input::tests as text},
    history::{
        EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory, execute_history_action,
    },
    material_graph::keys::function_graph_memory_key,
};
use bevy::input::keyboard::Key;

fn function() -> MaterialFunction {
    let mut f = MaterialFunction::from_ron(include_str!(
        "../../../../../assets/test/materials/pulse_wave.aestra.material-function.ron"
    ))
    .unwrap();
    f.custom_wesl = None;
    f.name = "Signature test".into();
    f.expressions.push(MaterialExpression {
        id: f.outputs[0].expression,
        kind: MaterialExpressionKind::FunctionInput(f.inputs[0].id),
    });
    f.inputs[1].default = Some(MaterialValue::Float(0.25));
    f.normalized()
}

struct Fixture {
    dir: tempfile::TempDir,
    original: MaterialFunction,
    bytes: Vec<u8>,
    app: App,
    mounted: Option<Entity>,
}
impl Fixture {
    fn new(f: MaterialFunction, memory: bool) -> Self {
        Self::with_caller(f, memory, false)
    }
    fn with_caller(f: MaterialFunction, memory: bool, caller: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("function.aestra.material-function.ron");
        f.save_ron(&path).unwrap();
        if caller {
            use aestra_core::material::{MaterialFunctionRef, MaterialProgram};
            let mut program = MaterialProgram::additive_sprite("Signature caller");
            let call = MaterialExpressionId::new();
            program.expressions.push(MaterialExpression {
                id: call,
                kind: MaterialExpressionKind::FunctionCall {
                    function: MaterialFunctionRef::Project(f.id),
                    arguments: std::collections::BTreeMap::from([(
                        f.inputs[0].id,
                        program.outputs.alpha,
                    )]),
                    output: f.outputs[0].id,
                },
            });
            program.outputs.alpha = call;
            program
                .save_ron(dir.path().join("caller.aestra.material.ron"))
                .unwrap();
        }
        let bytes = std::fs::read(&path).unwrap();
        let catalog = ProjectEffectCatalog::scan(dir.path());
        let mut session = crate::test_support::session_with_timing_slack();
        session.open_material_function(&catalog, f.id).unwrap();
        let mut app = text::app();
        app.insert_resource(catalog)
            .insert_resource(session)
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<MaterialProgramEditHistory>()
            .add_observer(execute_history_action);
        register(&mut app);
        if memory {
            app.init_resource::<GraphViewportMemory>();
        }
        let mut fixture = Self {
            dir,
            original: f,
            bytes,
            app,
            mounted: None,
        };
        fixture.mount();
        fixture
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
    fn mount(&mut self) {
        if let Some(root) = self.mounted.take() {
            self.app.world_mut().despawn(root);
        }
        let f = self.current();
        let root = self.app.world_mut().spawn(Node::default()).id();
        self.app
            .world_mut()
            .commands()
            .entity(root)
            .with_children(|p| {
                crate::material_function_editor::spawn(p, &f);
            });
        self.app.world_mut().flush();
        self.app.update();
        self.mounted = Some(root);
    }
    fn field(&mut self, matches: impl Fn(FieldKind) -> bool) -> Entity {
        let world = self.app.world_mut();
        world
            .query::<(Entity, &Field)>()
            .iter(world)
            .find(|(_, f)| matches(f.kind))
            .unwrap()
            .0
    }
    fn action(&mut self, matches: impl Fn(ActionKind) -> bool) -> Entity {
        let world = self.app.world_mut();
        world
            .query::<(Entity, &Action)>()
            .iter(world)
            .find(|(_, a)| matches(a.kind))
            .unwrap()
            .0
    }
    fn string(&mut self, source: Entity, value: &str, is_final: bool) {
        self.app.world_mut().trigger(ValueChange {
            source,
            value: value.to_owned(),
            is_final,
        });
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
    fn activate(&mut self, entity: Entity) {
        self.app.world_mut().trigger(Activate { entity });
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
            std::fs::read(
                self.dir
                    .path()
                    .join("function.aestra.material-function.ron")
            )
            .unwrap(),
            self.bytes
        );
    }
}

#[test]
fn composed_name_submits_only_final_trimmed_changes_once_and_round_trips_real_global_history() {
    let mut f = Fixture::new(function(), true);
    let effect = f.app.world().resource::<EditorSession>().effect.clone();
    let selection = f.app.world().resource::<EditorSession>().selection;
    let field = f.field(|k| matches!(k, FieldKind::Name));
    f.string(field, "Live edit", false);
    assert_eq!(f.current(), f.original);
    assert_eq!(f.serial(), 0);
    f.string(field, "  Renamed function  ", true);
    assert_eq!(f.current().name, "Renamed function");
    assert_eq!(f.serial(), 1);
    f.string(field, "Renamed function", true);
    assert_eq!(f.serial(), 1);
    f.step(true);
    assert_eq!(f.current(), f.original);
    f.step(false);
    assert_eq!(f.current().name, "Renamed function");
    assert_eq!(f.app.world().resource::<EditorSession>().effect, effect);
    assert_eq!(
        f.app.world().resource::<EditorSession>().selection,
        selection
    );
    f.disk_unchanged();
}

#[test]
fn input_and_output_renames_keep_ids_and_reject_empty_names_without_an_extra_undo() {
    let mut f = Fixture::new(function(), true);
    let input_id = f.original.inputs[0].id;
    let output_id = f.original.outputs[0].id;
    let input = f.field(|k| matches!(k, FieldKind::PortName(Port::Input(id)) if id == input_id));
    let output = f.field(|k| matches!(k, FieldKind::PortName(Port::Output(id)) if id == output_id));
    f.string(input, " New input ", true);
    f.string(output, " New output ", true);
    let edited = f.current();
    assert!(
        edited
            .inputs
            .iter()
            .any(|p| p.id == input_id && p.name == "New input")
    );
    assert!(
        edited
            .outputs
            .iter()
            .any(|p| p.id == output_id && p.name == "New output")
    );
    f.string(input, "  ", true);
    assert_eq!(f.current(), edited);
    assert_eq!(f.serial(), 2);
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .status
            .contains("A name is required")
    );
    f.step(true);
    assert_eq!(f.current().outputs, f.original.outputs);
    f.step(true);
    assert_eq!(f.current(), f.original);
    f.disk_unchanged();
}

#[test]
fn numeric_default_commit_is_final_finite_typed_and_single_with_noop_and_clear_support() {
    let mut f = Fixture::new(function(), true);
    let id = f.original.inputs[1].id;
    let field = f.field(|k| matches!(k, FieldKind::Default(actual) if actual == id));
    f.number(field, 0.75, false);
    assert_eq!(f.serial(), 0);
    f.number(field, 0.75, true);
    assert_eq!(f.serial(), 1);
    assert!(
        f.current()
            .inputs
            .iter()
            .any(|p| p.id == id && p.default == Some(MaterialValue::Float(0.75)))
    );
    f.number(field, 0.75, true);
    assert_eq!(f.serial(), 1);
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let before = f.current();
        f.number(field, value, true);
        assert_eq!(f.current(), before);
        assert_eq!(f.serial(), 1);
    }
    f.string(field, " ", true);
    assert_eq!(f.serial(), 2);
    assert!(
        f.current()
            .inputs
            .iter()
            .any(|p| p.id == id && p.default.is_none())
    );
    f.step(true);
    assert!(
        f.current()
            .inputs
            .iter()
            .any(|p| p.id == id && p.default == Some(MaterialValue::Float(0.75)))
    );
    f.step(true);
    assert_eq!(f.current(), f.original);
    f.disk_unchanged();
}

#[test]
fn typed_default_text_accepts_exact_components_and_rejects_bad_or_nonfinite_values_atomically() {
    for (ty, valid, expected, invalid) in [
        (
            MaterialValueType::Bool,
            "true",
            MaterialValue::Bool(true),
            "yes",
        ),
        (
            MaterialValueType::Vec2,
            "1, 2",
            MaterialValue::Vec2([1.0, 2.0]),
            "1",
        ),
        (
            MaterialValueType::Vec3,
            "1, 2, 3",
            MaterialValue::Vec3([1.0, 2.0, 3.0]),
            "1, NaN, 3",
        ),
        (
            MaterialValueType::Vec4,
            "1,2,3,4",
            MaterialValue::Vec4([1.0, 2.0, 3.0, 4.0]),
            "1,2,3",
        ),
        (
            MaterialValueType::Color,
            "0.2,0.3,0.4,0.5",
            MaterialValue::ColorSrgb([0.2, 0.3, 0.4, 0.5]),
            "0,0,inf,1",
        ),
    ] {
        let mut function = function();
        function.inputs[1].value_type = ty;
        function.inputs[1].default = None;
        let mut f = Fixture::new(function, true);
        let id = f.original.inputs[1].id;
        let field = f.field(|k| matches!(k, FieldKind::Default(actual) if actual == id));
        f.string(field, invalid, true);
        assert_eq!(f.current(), f.original);
        assert_eq!(f.serial(), 0);
        f.string(field, valid, false);
        assert_eq!(f.serial(), 0);
        f.string(field, valid, true);
        assert_eq!(f.serial(), 1);
        assert!(
            f.current()
                .inputs
                .iter()
                .any(|p| p.id == id && p.default == Some(expected.clone()))
        );
        f.step(true);
        assert_eq!(f.current(), f.original);
        f.disk_unchanged();
    }
}

#[test]
fn real_add_actions_use_semantic_placement_and_one_compound_history_step_per_activation() {
    let mut f = Fixture::new(function(), true);
    let key = function_graph_memory_key(f.dir.path(), f.original.id);
    let existing = f.original.expressions[0].id.to_string();
    let position = Vec2::new(-400.0, 700.0);
    f.app
        .world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node(&key, &existing, position, false);
    f.app
        .world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_pinned(&key, &existing, true);
    let input = f.action(|k| matches!(k, ActionKind::AddInput));
    let output = f.action(|k| matches!(k, ActionKind::AddOutput));
    f.activate(input);
    let after_input = f.current();
    assert_eq!(after_input.inputs.len(), f.original.inputs.len() + 1);
    assert_eq!(f.serial(), 1);
    f.activate(output);
    let after_output = f.current();
    assert_eq!(after_output.outputs.len(), f.original.outputs.len() + 1);
    assert_eq!(f.serial(), 2);
    let created = after_output
        .expressions
        .iter()
        .find(|e| !after_input.expressions.iter().any(|old| old.id == e.id))
        .unwrap()
        .id;
    let memory = f.app.world().resource::<GraphViewportMemory>();
    assert_eq!(memory.node(&key, &existing), Some((position, false)));
    assert!(memory.is_pinned(&key, &existing));
    assert!(memory.node(&key, &created.to_string()).is_some());
    f.step(true);
    assert_eq!(f.current(), after_input);
    assert_eq!(
        f.app
            .world()
            .resource::<GraphViewportMemory>()
            .node(&key, &created.to_string()),
        None
    );
    f.step(false);
    assert_eq!(f.current(), after_output);
    f.disk_unchanged();
}

#[test]
fn invalid_type_and_required_removal_actions_preserve_the_function_and_history() {
    // Output/body type consistency is checked through real indexed call sites.
    let mut f = Fixture::with_caller(function(), true, true);
    let input_id = f.original.inputs[0].id;
    let output_id = f.original.outputs[0].id;
    let width_id = f.original.inputs[1].id;
    let actions = [
        f.action(|k| matches!(k, ActionKind::Remove(Port::Input(id)) if id == input_id)),
        f.action(|k| matches!(k, ActionKind::Remove(Port::Output(id)) if id == output_id)),
        f.action(|k| matches!(k, ActionKind::Type(Port::Input(id), MaterialValueType::Vec3) if id == width_id)),
        f.action(|k| matches!(k, ActionKind::Type(Port::Output(id), MaterialValueType::Vec3) if id == output_id)),
    ];
    for action in actions {
        f.activate(action);
        assert_eq!(f.current(), f.original);
        assert_eq!(f.serial(), 0);
    }
    f.disk_unchanged();
}

#[test]
fn stale_control_owners_and_untyped_event_sources_do_not_edit_a_new_active_target() {
    let mut f = Fixture::new(function(), true);
    let field = f.field(|k| matches!(k, FieldKind::Name));
    let action = f.action(|k| matches!(k, ActionKind::AddOutput));
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .material_target = crate::material_target::MaterialEditingTarget::EffectInstance;
    f.string(field, "Stale", true);
    f.activate(action);
    assert_eq!(f.serial(), 0);
    let target = crate::material_target::MaterialEditingTarget::Function {
        root: f.dir.path().to_owned(),
        id: f.original.id,
    };
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .material_target = target;
    let untyped = f.app.world_mut().spawn_empty().id();
    f.string(untyped, "Invalid", true);
    f.number(untyped, 1.0, true);
    f.activate(untyped);
    assert_eq!(f.current(), f.original);
    assert_eq!(f.serial(), 0);
    f.disk_unchanged();
}

#[test]
fn clearing_default_allows_unused_port_type_change_and_removal_with_one_step_each() {
    let mut f = Fixture::new(function(), true);
    let id = f.original.inputs[1].id;
    let default = f.field(|k| matches!(k, FieldKind::Default(actual) if actual == id));
    f.string(default, "", true);
    let change = f.action(|k| matches!(k, ActionKind::Type(Port::Input(actual), MaterialValueType::Vec3) if actual == id));
    f.activate(change);
    assert_eq!(f.serial(), 2);
    let changed = f.current();
    assert!(
        changed
            .inputs
            .iter()
            .any(|p| p.id == id && p.value_type == MaterialValueType::Vec3 && p.default.is_none())
    );
    f.mount();
    let remove = f.action(|k| matches!(k, ActionKind::Remove(Port::Input(actual)) if actual == id));
    f.activate(remove);
    assert_eq!(f.serial(), 3);
    assert!(!f.current().inputs.iter().any(|p| p.id == id));
    f.step(true);
    assert_eq!(f.current(), changed);
    f.step(true);
    assert!(
        f.current()
            .inputs
            .iter()
            .any(|p| p.id == id && p.value_type == MaterialValueType::Float && p.default.is_none())
    );
    f.step(true);
    assert_eq!(f.current(), f.original);
    f.disk_unchanged();
}

#[test]
fn real_keyboard_text_bridge_commits_on_enter_but_not_during_live_typing() {
    let mut f = Fixture::new(function(), true);
    let field = f.field(|k| matches!(k, FieldKind::Name));
    let input = f.app.world().get::<Children>(field).unwrap()[0];
    text::focus(&mut f.app, input);
    text::tap(&mut f.app, KeyCode::End, Key::End, None);
    text::tap(
        &mut f.app,
        KeyCode::KeyA,
        Key::Character("é".into()),
        Some("é"),
    );
    assert_eq!(f.serial(), 0);
    text::tap(&mut f.app, KeyCode::Enter, Key::Enter, None);
    assert_eq!(f.current().name, format!("{}é", f.original.name));
    assert_eq!(f.serial(), 1);
    f.step(true);
    assert_eq!(f.current(), f.original);
    f.disk_unchanged();
}

#[test]
fn structural_action_without_ui_memory_keeps_validation_and_undo_semantics() {
    let mut f = Fixture::new(function(), false);
    let action = f.action(|k| matches!(k, ActionKind::AddOutput));
    f.activate(action);
    assert_eq!(f.current().outputs.len(), f.original.outputs.len() + 1);
    assert_eq!(f.serial(), 1);
    assert!(!f.app.world().contains_resource::<GraphViewportMemory>());
    f.step(true);
    assert_eq!(f.current(), f.original);
    f.disk_unchanged();
}
