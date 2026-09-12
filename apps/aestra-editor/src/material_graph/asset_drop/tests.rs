use super::*;
use aestra_core::{
    MaterialFunctionOutputId,
    material::{MaterialFunctionOutput, MaterialSchemaVersion},
};
use bevy::{
    camera::NormalizedRenderTarget,
    picking::{
        backend::HitData,
        pointer::{Location, PointerId},
    },
};

fn function(name: &str) -> MaterialFunction {
    let expression = MaterialExpressionId::new();
    MaterialFunction {
        id: MaterialFunctionId::new(),
        name: name.into(),
        schema_version: MaterialSchemaVersion::CURRENT,
        inputs: vec![],
        outputs: vec![MaterialFunctionOutput {
            id: MaterialFunctionOutputId::new(),
            name: "Value".into(),
            value_type: MaterialValueType::Float,
            expression,
        }],
        expressions: vec![MaterialExpression {
            id: expression,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
        }],
        custom_wesl: None,
    }
}

struct Fixture {
    root: tempfile::TempDir,
    app: App,
    source: Entity,
    target: Entity,
    caller: MaterialFunction,
    program: MaterialProgram,
}
impl Fixture {
    fn new(function_target: bool, multi: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut callee = function("Dropped function");
        if multi {
            let mut output = callee.outputs[0].clone();
            output.id = MaterialFunctionOutputId::new();
            output.name = "Other".into();
            callee.outputs.push(output);
        }
        callee
            .save_ron(root.path().join("callee.aestra.material-function.ron"))
            .unwrap();
        let caller = function("Caller");
        caller
            .save_ron(root.path().join("caller.aestra.material-function.ron"))
            .unwrap();
        let program = MaterialProgram::additive_sprite("Material").normalized();
        program
            .save_ron(root.path().join("material.aestra.material.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let payload = AssetPayload::capture(
            &catalog,
            catalog
                .content()
                .source_tree()
                .at_relative_path("callee.aestra.material-function.ron")
                .unwrap()
                .id,
        );
        let mut session = crate::test_support::session_with_timing_slack();
        let target = if function_target {
            session.open_material_function(&catalog, caller.id).unwrap();
            GraphDropTarget::function(&session, caller.id)
        } else {
            session.open_material_program(&catalog, program.id).unwrap();
            GraphDropTarget::program(&session, program.id)
        };
        let key = target.key(&catalog);
        let mut app = App::new();
        register(&mut app);
        app.insert_resource(catalog)
            .insert_resource(session)
            .init_resource::<MaterialProgramEditHistory>()
            .init_resource::<EditorHistoryLedger>()
            .init_resource::<FunctionEditor>()
            .init_resource::<GraphViewportMemory>()
            .add_observer(crate::history::execute_history_action);
        let source = app.world_mut().spawn(payload).id();
        let parent = app.world_mut().spawn_empty().id();
        let mut viewport = Entity::PLACEHOLDER;
        app.world_mut()
            .commands()
            .entity(parent)
            .with_children(|parent| {
                viewport = spawn_graph_viewport(
                    parent,
                    GraphViewportProps {
                        key,
                        content_size: Vec2::new(800.0, 600.0),
                        selection_bounds: None,
                        initial_view: None,
                    },
                    (),
                    |_| {},
                    |_| {},
                );
            });
        app.world_mut().flush();
        app.world_mut().entity_mut(viewport).insert(target);
        Self {
            root,
            app,
            source,
            target: viewport,
            caller,
            program,
        }
    }
    fn payload(&self) -> AssetPayload {
        self.app
            .world()
            .get::<AssetPayload>(self.source)
            .unwrap()
            .clone()
    }
    fn plan(&self) -> Result<DropPlan, String> {
        plan(
            &self.payload(),
            self.app
                .world()
                .get::<GraphDropTarget>(self.target)
                .unwrap(),
            self.app.world().resource::<EditorSession>(),
            self.app.world().resource::<ProjectEffectCatalog>(),
        )
    }
    fn drop(&mut self) {
        let source = self.app.world_mut().spawn(ChildOf(self.source)).id();
        let target = self.app.world_mut().spawn(ChildOf(self.target)).id();
        self.app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            location(),
            DragDrop {
                button: PointerButton::Primary,
                dropped: source,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            target,
        ));
        self.app.world_mut().flush();
    }
    fn history(&mut self, undo: bool) {
        self.app.world_mut().trigger(if undo {
            crate::history::HistoryAction::Undo
        } else {
            crate::history::HistoryAction::Redo
        });
        self.app.world_mut().flush();
    }
    fn expressions(&self, function: bool) -> Vec<MaterialExpression> {
        let catalog = self.app.world().resource::<ProjectEffectCatalog>();
        if function {
            self.app
                .world()
                .resource::<EditorSession>()
                .graph_function(catalog)
                .unwrap()
                .expressions
        } else {
            catalog
                .material_program(self.program.id)
                .unwrap()
                .expressions
        }
    }
}
fn location() -> Location {
    Location {
        target: NormalizedRenderTarget::None {
            width: 800,
            height: 600,
        },
        position: Vec2::new(310.0, 220.0),
    }
}

#[test]
fn custom_wesl_callees_and_typed_defaults_work_in_both_graphs() {
    use aestra_core::{
        MaterialFunctionInputId,
        material::{MaterialCustomWeslImplementation, MaterialFunctionInput},
    };
    for function_target in [false, true] {
        let mut fixture = Fixture::new(function_target, false);
        let path = fixture
            .root
            .path()
            .join("callee.aestra.material-function.ron");
        let mut callee = MaterialFunction::load_ron(&path).unwrap();
        let input = MaterialFunctionInputId::new();
        callee.inputs.push(MaterialFunctionInput {
            id: input,
            name: "Value".into(),
            value_type: MaterialValueType::Float,
            default: Some(MaterialValue::Float(0.25)),
        });
        callee.expressions.clear();
        callee.custom_wesl = Some(MaterialCustomWeslImplementation {
            evaluation_domain: MaterialExpressionDomain::Fragment,
            source: "fn custom_value(value: f32) -> f32 { return value; }".into(),
            entry_points: [(callee.outputs[0].id, "custom_value".into())].into(),
        });
        callee.save_ron(&path).unwrap();
        let catalog = ProjectEffectCatalog::scan(fixture.root.path());
        let payload = AssetPayload::capture(
            &catalog,
            catalog
                .content()
                .source_tree()
                .at_relative_path("callee.aestra.material-function.ron")
                .unwrap()
                .id,
        );
        fixture.app.insert_resource(catalog);
        fixture
            .app
            .world_mut()
            .entity_mut(fixture.source)
            .insert(payload);
        fixture.drop();
        let expressions = fixture.expressions(function_target);
        let arguments = expressions
            .iter()
            .find_map(|expression| match &expression.kind {
                MaterialExpressionKind::FunctionCall { arguments, .. } => Some(arguments),
                _ => None,
            })
            .unwrap_or_else(|| {
                panic!("{}", fixture.app.world().resource::<EditorSession>().status)
            });
        let constant = expressions
            .iter()
            .find(|expression| expression.id == arguments[&input])
            .unwrap();
        assert_eq!(
            constant.kind,
            MaterialExpressionKind::Constant(MaterialValue::Float(0.25))
        );
        assert_eq!(
            MaterialFunction::load_ron(path).unwrap(),
            callee.normalized()
        );
    }
}

#[test]
fn function_drop_on_effect_material_uses_material_history_without_changing_effect() {
    use aestra_core::material::MaterialProgramRef;
    let mut fixture = Fixture::new(false, false);
    let target = {
        let mut session = fixture.app.world_mut().resource_mut::<EditorSession>();
        session.return_to_effect_material();
        let instance = MaterialInstance {
            id: MaterialId::new(),
            program: MaterialProgramRef::Project(fixture.program.id),
            values: default(),
            render_state: fixture.program.render_state_policy.default,
        };
        session.effect.emitters[0].renderers[0].material = instance.id;
        session.effect.material_instances.push(instance);
        session.selection.primary =
            SemanticTarget::Renderer(session.effect.emitters[0].renderers[0].id);
        GraphDropTarget::program(&session, fixture.program.id)
    };
    fixture
        .app
        .world_mut()
        .entity_mut(fixture.target)
        .insert(target);
    let before = fixture.expressions(false);
    let effect = fixture
        .app
        .world()
        .resource::<EditorSession>()
        .effect
        .clone();
    fixture.drop();
    let after = fixture.expressions(false);
    assert_ne!(
        before,
        after,
        "{}",
        fixture.app.world().resource::<EditorSession>().status
    );
    fixture.history(true);
    assert_eq!(fixture.expressions(false), before);
    fixture.history(false);
    assert_eq!(fixture.expressions(false), after);
    assert_eq!(
        fixture.app.world().resource::<EditorSession>().effect,
        effect
    );
}

#[test]
fn both_graphs_accept_child_target_drops_with_one_undo_redo_and_no_file_changes() {
    for function in [false, true] {
        for multi in [false, true] {
            let mut fixture = Fixture::new(function, multi);
            let before = fixture.expressions(function);
            let effect = fixture
                .app
                .world()
                .resource::<EditorSession>()
                .effect
                .clone();
            let callee_path = fixture
                .root
                .path()
                .join("callee.aestra.material-function.ron");
            let bytes = std::fs::read(&callee_path).unwrap();
            fixture.drop();
            let after = fixture.expressions(function);
            assert!(
                after.len() > before.len(),
                "{}",
                fixture.app.world().resource::<EditorSession>().status
            );
            let calls = after
                .iter()
                .filter(|expression| {
                    matches!(expression.kind, MaterialExpressionKind::FunctionCall { .. })
                })
                .collect::<Vec<_>>();
            assert_eq!(calls.len(), if multi { 2 } else { 1 });
            let target = fixture
                .app
                .world()
                .get::<GraphDropTarget>(fixture.target)
                .unwrap();
            let key = target.key(fixture.app.world().resource::<ProjectEffectCatalog>());
            let memory = fixture.app.world().resource::<GraphViewportMemory>();
            assert!(calls.iter().any(|call| {
                let node_key = if function {
                    call.id.to_string()
                } else {
                    material_graph_expression_node_key(call.id)
                };
                memory.node_position(&key, &node_key)
                    == Some(
                        location().position - Vec2::new(NODE_WIDTH * 0.5, NODE_HEADER_HEIGHT * 0.5),
                    )
            }));
            fixture.history(true);
            assert_eq!(fixture.expressions(function), before);
            fixture.history(false);
            assert_eq!(fixture.expressions(function), after);
            assert_eq!(
                fixture.app.world().resource::<EditorSession>().effect,
                effect
            );
            assert_eq!(std::fs::read(callee_path).unwrap(), bytes);
            assert_eq!(
                MaterialProgram::load_ron(fixture.root.path().join("material.aestra.material.ron"))
                    .unwrap(),
                fixture.program
            );
            assert_eq!(
                MaterialFunction::load_ron(
                    fixture
                        .root
                        .path()
                        .join("caller.aestra.material-function.ron")
                )
                .unwrap(),
                fixture.caller
            );
        }
    }
}

#[test]
fn stale_wrong_type_switched_target_and_pending_drops_do_not_edit() {
    for case in 0..6 {
        let mut fixture = Fixture::new(false, false);
        match case {
            0 => {
                fixture
                    .app
                    .world_mut()
                    .resource_mut::<ProjectEffectCatalog>()
                    .refresh();
            }
            1 => {
                let catalog = fixture.app.world().resource::<ProjectEffectCatalog>();
                let payload = AssetPayload::capture(
                    catalog,
                    catalog
                        .content()
                        .source_tree()
                        .at_relative_path("material.aestra.material.ron")
                        .unwrap()
                        .id,
                );
                fixture
                    .app
                    .world_mut()
                    .entity_mut(fixture.source)
                    .insert(payload);
            }
            2 => {
                fixture
                    .app
                    .world_mut()
                    .resource_mut::<EditorSession>()
                    .return_to_effect_material();
            }
            3 => {
                fixture
                    .app
                    .world_mut()
                    .resource_mut::<EditorSession>()
                    .preview_transaction(EffectTransaction::single(
                        "Pending",
                        EffectCommand::SetEffectName {
                            name: "Keep".into(),
                        },
                    ));
            }
            4 => {
                let mut protection = DocumentProtectionState::default();
                protection.asset_create_open = true;
                fixture.app.insert_resource(protection);
            }
            _ => {
                let mut keys = ButtonInput::<KeyCode>::default();
                keys.press(KeyCode::Escape);
                fixture.app.insert_resource(keys);
            }
        }
        fixture.drop();
        assert_eq!(fixture.expressions(false), fixture.program.expressions);
        assert!(
            fixture
                .app
                .world()
                .resource::<EditorSession>()
                .status
                .starts_with("Function drop rejected:")
        );
    }
}

#[test]
fn direct_and_indirect_recursion_are_rejected() {
    for indirect in [false, true] {
        let mut fixture = Fixture::new(true, false);
        if indirect {
            let mut callee = MaterialFunction::load_ron(
                fixture
                    .root
                    .path()
                    .join("callee.aestra.material-function.ron"),
            )
            .unwrap();
            callee.expressions[0].kind = MaterialExpressionKind::FunctionCall {
                function: MaterialFunctionRef::Project(fixture.caller.id),
                output: fixture.caller.outputs[0].id,
                arguments: default(),
            };
            callee
                .save_ron(
                    fixture
                        .root
                        .path()
                        .join("callee.aestra.material-function.ron"),
                )
                .unwrap();
        }
        let catalog = ProjectEffectCatalog::scan(fixture.root.path());
        let name = if indirect {
            "callee.aestra.material-function.ron"
        } else {
            "caller.aestra.material-function.ron"
        };
        let payload = AssetPayload::capture(
            &catalog,
            catalog
                .content()
                .source_tree()
                .at_relative_path(name)
                .unwrap()
                .id,
        );
        fixture.app.insert_resource(catalog);
        fixture
            .app
            .world_mut()
            .entity_mut(fixture.source)
            .insert(payload);
        assert!(fixture.plan().is_err());
        fixture.drop();
        assert_eq!(fixture.expressions(true), fixture.caller.expressions);
    }
}

#[test]
fn hover_feedback_is_non_mutating_and_clears_on_leave_and_escape() {
    let mut fixture = Fixture::new(false, false);
    for escape in [false, true] {
        fixture.app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            location(),
            DragEnter {
                button: PointerButton::Primary,
                dragged: fixture.source,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            fixture.target,
        ));
        fixture.app.world_mut().flush();
        assert_eq!(
            fixture
                .app
                .world_mut()
                .query::<&Feedback>()
                .iter(fixture.app.world())
                .count(),
            1
        );
        assert_eq!(fixture.expressions(false), fixture.program.expressions);
        if escape {
            let mut keys = ButtonInput::<KeyCode>::default();
            keys.press(KeyCode::Escape);
            fixture.app.insert_resource(keys);
            fixture.app.update();
        } else {
            fixture.app.world_mut().trigger(Pointer::new(
                PointerId::Mouse,
                location(),
                DragLeave {
                    button: PointerButton::Primary,
                    dragged: fixture.source,
                    hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                },
                fixture.target,
            ));
            fixture.app.world_mut().flush();
        }
        assert_eq!(
            fixture
                .app
                .world_mut()
                .query::<&Feedback>()
                .iter(fixture.app.world())
                .count(),
            0
        );
    }
}
