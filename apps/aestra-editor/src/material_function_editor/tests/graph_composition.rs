//! Real function canvas construction; native input dispatch is deliberately not simulated here.
use super::*;
use crate::{
    docking::EditorViewId,
    feathers::{icon::SvgPlugin, text_input::tests::app_with},
    material_document::MaterialEditingTarget,
    material_graph::{MaterialGraphSelectionState, asset_drop::GraphDropTarget},
};
use aestra_core::{
    MaterialFunctionOutputId,
    material::{MaterialFunctionOutput, MaterialValueType},
};
use bevy::ui_widgets::ValueChange;

fn function() -> MaterialFunction {
    MaterialFunction::from_ron(include_str!(
        "../../../../../assets/test/materials/dissolve_edge.aestra.material-function.ron"
    ))
    .unwrap()
}
struct Fixture {
    root: tempfile::TempDir,
    function: MaterialFunction,
    catalog: ProjectEffectCatalog,
    session: EditorSession,
    memory: GraphViewportMemory,
    selection: MaterialGraphSelectionState,
    menus: FunctionGraphMenuState,
    app: App,
}
impl Fixture {
    fn new(function: MaterialFunction) -> Self {
        let root = tempfile::tempdir().unwrap();
        function
            .save_ron(root.path().join("function.aestra.material-function.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        Self {
            root,
            function,
            catalog,
            session: crate::test_support::session_with_timing_slack(),
            memory: default(),
            selection: default(),
            menus: default(),
            app: app_with(|app| {
                app.add_plugins(SvgPlugin);
            }),
        }
    }
    fn target(&self) -> MaterialEditingTarget {
        MaterialEditingTarget::Function {
            root: self.root.path().to_owned(),
            id: self.function.id,
        }
    }
    fn mount(&mut self, target: &MaterialEditingTarget, view: Option<EditorViewId>) -> Entity {
        let server = self.app.world().resource::<AssetServer>().clone();
        let mut root = Entity::PLACEHOLDER;
        self.app
            .world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|parent| {
                root = parent.target_entity();
                spawn(
                    parent,
                    &self.session,
                    &self.catalog,
                    &server,
                    &self.memory,
                    &self.selection,
                    &self.menus,
                    target,
                    view,
                );
            });
        self.app.world_mut().flush();
        self.app.update();
        root
    }
    fn count<T: Component>(&mut self, root: Entity) -> usize {
        let world = self.app.world_mut();
        world
            .query_filtered::<Entity, With<T>>()
            .iter(world)
            .filter(|e| within(world, *e, root))
            .count()
    }
    fn viewport(&mut self, root: Entity) -> Entity {
        let world = self.app.world_mut();
        world
            .query_filtered::<Entity, With<View>>()
            .iter(world)
            .find(|e| within(world, *e, root))
            .unwrap()
    }
}
fn within(world: &World, mut entity: Entity, root: Entity) -> bool {
    loop {
        if entity == root {
            return true;
        }
        let Some(parent) = world.get::<ChildOf>(entity) else {
            return false;
        };
        entity = parent.parent();
    }
}

#[test]
fn explicit_unfocused_function_mounts_real_nodes_ports_wires_and_drop_identity_without_editing() {
    let mut f = Fixture::new(function());
    let target = f.target();
    let effect = f.session.effect.clone();
    let root = f.mount(&target, Some(EditorViewId(3)));
    let viewport = f.viewport(root);
    let world = f.app.world_mut();
    assert_eq!(world.get::<View>(viewport).unwrap().0, f.function.id);
    assert_eq!(
        world.get::<ViewScope>(viewport).unwrap().0,
        Some(EditorViewId(3))
    );
    assert_eq!(
        world
            .get::<GraphDropTarget>(viewport)
            .unwrap()
            .editing_target(),
        &target
    );
    let geometry = world.get::<GraphGeometryView>(viewport).unwrap();
    assert_eq!(geometry.key.document.project, f.root.path());
    assert_eq!(
        geometry.key.document.asset,
        crate::document::DocumentKey::MaterialFunction(f.function.id)
    );
    assert_eq!(geometry.nodes.len(), f.function.expressions.len() + 1);
    let sources = world
        .query::<&Socket>()
        .iter(world)
        .filter_map(|socket| {
            assert_eq!(socket.owner, f.function.id);
            assert!(world.get::<FeathersGraphNode>(socket.node).is_some());
            match socket.kind {
                SocketKind::Source(id) => Some(id),
                _ => None,
            }
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        sources,
        f.function.expressions.iter().map(|e| e.id).collect()
    );
    let outputs = world
        .query::<(&Socket, &GraphGeometryPort)>()
        .iter(world)
        .filter_map(|(socket, port)| {
            if let SocketKind::Target(Target::Output(id)) = socket.kind {
                assert_eq!(*port, GraphGeometryPort::FunctionOutput(id));
                Some(id)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        outputs,
        f.function.outputs.iter().map(|p| p.id).collect::<Vec<_>>()
    );
    for (wire, identity) in world
        .query::<(&Wire, &crate::feathers::node_graph::insertion::Wire)>()
        .iter(world)
    {
        assert_eq!(wire.owner, f.function.id);
        assert_eq!(identity.source, GraphNodeKey::Expression(wire.source));
        if let Target::Output(id) = wire.target {
            assert_eq!(identity.target, GraphNodeKey::FunctionOutputs);
            assert_eq!(identity.port, GraphGeometryPort::FunctionOutput(id));
        }
    }
    assert_eq!(f.count::<Wire>(root), 5);
    assert_eq!(f.count::<PreviewWire>(root), 1);
    assert_eq!(
        f.session.material_target,
        MaterialEditingTarget::EffectInstance
    );
    assert_eq!(f.session.effect, effect);
    assert_eq!(f.session.operation_order.edit_serial(), 0);
    assert_eq!(
        f.session
            .graph_function_for(&f.target(), &f.catalog)
            .unwrap(),
        f.function.normalized()
    );
}

#[test]
fn split_views_keep_document_placement_but_independent_camera_and_frame_targets() {
    let mut f = Fixture::new(function());
    let target = f.target();
    let key = crate::material_graph::function_graph_memory_key(f.root.path(), f.function.id);
    let expression = f.function.expressions[0].id;
    f.memory.set_node(
        &key,
        expression.to_string(),
        Vec2::new(-100.0, 500.0),
        false,
    );
    f.memory.set_pinned(&key, &expression.to_string(), true);
    for (view, pan, zoom) in [
        (Some(EditorViewId(1)), Vec2::new(20.0, 30.0), 0.5),
        (Some(EditorViewId(2)), Vec2::new(70.0, 80.0), 1.5),
    ] {
        let camera_key = format!("{key}#view:{}", view.unwrap().0);
        f.memory.set_view(&camera_key, pan, zoom);
        f.selection
            .select_function_expression(view, f.function.id, expression, false, false);
        let root = f.mount(&target, view);
        let viewport = f.viewport(root);
        let world = f.app.world_mut();
        let camera = world.get::<FeathersGraphViewport>(viewport).unwrap();
        assert_eq!(camera.project_graph_point(Vec2::ZERO), pan);
        assert_eq!(
            camera.project_graph_point(Vec2::ONE),
            pan + Vec2::splat(zoom)
        );
        let nodes = world
            .query::<(Entity, &FeathersGraphNode)>()
            .iter(world)
            .filter(|(e, _)| within(world, *e, root))
            .map(|(_, n)| n)
            .collect::<Vec<_>>();
        assert!(
            nodes.iter().any(|n| n.node_key() == expression.to_string()
                && n.position() == Vec2::new(-100.0, 500.0))
        );
        for (entity, action) in world.query::<(Entity, &GraphFrameAction)>().iter(world) {
            if within(world, entity, root) {
                assert_eq!(action.key_target().0, camera_key);
            }
        }
        for (entity, action) in world
            .query::<(Entity, &FunctionGraphNodeAction)>()
            .iter(world)
        {
            if within(world, entity, root) {
                assert_eq!(action.scope, view);
            }
        }
        assert_eq!(f.count::<GraphFrameAction>(root), 2);
    }
}

#[test]
fn unavailable_wrong_project_and_custom_wesl_targets_do_not_mount_a_surrogate_graph() {
    let mut f = Fixture::new(function());
    for target in [
        MaterialEditingTarget::Function {
            root: f.root.path().join("other"),
            id: f.function.id,
        },
        MaterialEditingTarget::Function {
            root: f.root.path().to_owned(),
            id: MaterialFunctionId::new(),
        },
        MaterialEditingTarget::EffectInstance,
    ] {
        let root = f.mount(&target, None);
        assert_eq!(f.count::<View>(root), 0);
        assert_eq!(f.count::<GraphDropTarget>(root), 0);
    }
    let mut custom = f.function.clone();
    custom.custom_wesl = Some(aestra_core::material::MaterialCustomWeslImplementation {
        evaluation_domain: aestra_core::material::MaterialExpressionDomain::ShaderStatic,
        entry_points: BTreeMap::from([(custom.outputs[0].id, "custom".into())]),
        source: "fn custom() -> f32 { return 0.0; }".into(),
    });
    f.catalog
        .replace_material_function(&f.function, &custom)
        .unwrap();
    let root = f.mount(&f.target(), None);
    assert_eq!(f.count::<View>(root), 0);
}

#[test]
fn context_menus_match_function_and_view_and_preserve_connection_policy_labels() {
    let mut f = Fixture::new(function());
    let target = f.target();
    let view = Some(EditorViewId(4));
    for kind in [
        FunctionGraphMenuKind::Node(f.function.expressions[0].id),
        FunctionGraphMenuKind::Connections(vec![(
            f.function.expressions[0].id,
            Target::Output(f.function.outputs[0].id),
        )]),
        FunctionGraphMenuKind::Connections(vec![(
            f.function.expressions[0].id,
            Target::Input(
                f.function.expressions[0].id,
                MaterialExpressionInput::FunctionArgument(f.function.inputs[0].id),
            ),
        )]),
    ] {
        let expected_label = match &kind {
            FunctionGraphMenuKind::Node(_) => "Duplicate node(s)",
            FunctionGraphMenuKind::Connections(connections) => {
                if matches!(connections[0].1, Target::Output(_)) {
                    "Required connection"
                } else {
                    "Break connection"
                }
            }
        };
        f.menus.open = Some(FunctionGraphMenuOpen {
            owner: f.function.id,
            scope: view,
            position: Vec2::ONE,
            kind,
        });
        let root = f.mount(&target, view);
        assert_eq!(f.count::<FunctionGraphContextMenu>(root), 1);
        assert!(f.count::<FunctionGraphContextAction>(root) > 0);
        let world = f.app.world_mut();
        assert!(
            world
                .query::<(Entity, &Text)>()
                .iter(world)
                .any(|(entity, text)| within(world, entity, root) && text.0 == expected_label)
        );
        for wrong_view in [Some(EditorViewId(5)), None] {
            let root = f.mount(&target, wrong_view);
            assert_eq!(f.count::<FunctionGraphContextMenu>(root), 0);
        }
        f.menus.open.as_mut().unwrap().owner = MaterialFunctionId::new();
        let root = f.mount(&target, view);
        assert_eq!(f.count::<FunctionGraphContextMenu>(root), 0);
    }
}

#[test]
fn scalar_vector_color_and_boolean_constants_keep_typed_controls_and_component_indices() {
    let mut function = function();
    function.expressions.clear();
    function.inputs.clear();
    function.outputs.clear();
    for (ty, value) in [
        (MaterialValueType::Float, MaterialValue::Float(2.5)),
        (MaterialValueType::Vec2, MaterialValue::Vec2([1.0, 2.0])),
        (
            MaterialValueType::Vec3,
            MaterialValue::Vec3([1.0, 2.0, 3.0]),
        ),
        (
            MaterialValueType::Vec4,
            MaterialValue::Vec4([1.0, 2.0, 3.0, 4.0]),
        ),
        (
            MaterialValueType::Color,
            MaterialValue::ColorSrgb([0.2, 0.3, 0.4, 0.5]),
        ),
        (MaterialValueType::Bool, MaterialValue::Bool(true)),
    ] {
        let id = MaterialExpressionId::new();
        function.expressions.push(MaterialExpression {
            id,
            kind: MaterialExpressionKind::Constant(value),
        });
        function.outputs.push(MaterialFunctionOutput {
            id: MaterialFunctionOutputId::new(),
            name: format!("{ty:?}"),
            expression: id,
            value_type: ty,
        });
    }
    let mut f = Fixture::new(function);
    let root = f.mount(&f.target(), None);
    assert_eq!(f.count::<Constant>(root), 14);
    let world = f.app.world_mut();
    for (control, scrub) in world
        .query::<(&Constant, &crate::feathers::number_input::ScrubbableNumber)>()
        .iter(world)
    {
        assert_eq!(control.owner, f.function.id);
        let literal = f
            .function
            .expressions
            .iter()
            .find(|e| e.id == control.expression)
            .unwrap();
        let MaterialExpressionKind::Constant(value) = &literal.kind else {
            panic!()
        };
        assert!(control.component < components(value).len());
        assert_eq!(scrub.value, components(value)[control.component].1);
        assert_eq!(
            (scrub.min, scrub.max, scrub.step),
            (-f32::MAX, f32::MAX, 0.01)
        );
    }
    assert_eq!(
        world
            .query::<&BodyAction>()
            .iter(world)
            .filter(|a| matches!(a.kind, BodyActionKind::Boolean(_, _)))
            .count(),
        2
    );
}

#[test]
fn add_menu_search_filters_real_catalog_rows_without_mutating_function_or_effect() {
    let mut f = Fixture::new(function());
    let root = f.mount(&f.target(), Some(EditorViewId(9)));
    let world = f.app.world_mut();
    let input = world
        .query_filtered::<(Entity, &AccessibleLabel), With<bevy::feathers::controls::FeathersTextInputContainer>>()
        .iter(world)
        .find(|(e, l)| within(world, *e, root) && l.0 == "Search nodes")
        .map(|(e, _)| e)
        .unwrap();
    world.trigger(ValueChange {
        source: input,
        value: "no_such_function_node_938".to_owned(),
        is_final: false,
    });
    world.flush();
    let hidden = world
        .query::<(Entity, &BodyAction)>()
        .iter(world)
        .filter(|(e, a)| {
            within(world, *e, root)
                && matches!(a.kind, BodyActionKind::Create(_) | BodyActionKind::Input(_))
        })
        .map(|(e, _)| world.get::<ChildOf>(e).unwrap().parent())
        .collect::<Vec<_>>();
    assert!(!hidden.is_empty());
    assert!(
        hidden
            .iter()
            .all(|e| world.get::<Node>(*e).unwrap().display == Display::None)
    );
    world.trigger(ValueChange {
        source: input,
        value: String::new(),
        is_final: false,
    });
    world.flush();
    assert!(
        hidden
            .iter()
            .all(|e| world.get::<Node>(*e).unwrap().display == Display::Flex)
    );
    assert_eq!(f.session.operation_order.edit_serial(), 0);
    assert_eq!(
        f.session
            .graph_function_for(&f.target(), &f.catalog)
            .unwrap(),
        f.function.normalized()
    );
}

#[test]
fn invalid_unsaved_function_keeps_a_graph_without_replacing_its_draft_or_effect() {
    let mut f = Fixture::new(function());
    let mut invalid = f.function.clone();
    invalid.outputs[0].expression = MaterialExpressionId::new();
    f.catalog
        .replace_material_function(&f.function, &invalid)
        .unwrap();
    let before = f
        .session
        .graph_function_for(&f.target(), &f.catalog)
        .unwrap();
    let root = f.mount(&f.target(), None);
    assert_eq!(f.count::<View>(root), 1);
    assert_eq!(
        f.session
            .graph_function_for(&f.target(), &f.catalog)
            .unwrap(),
        before
    );
    assert_eq!(
        f.session.material_target,
        MaterialEditingTarget::EffectInstance
    );
    assert_eq!(f.session.operation_order.edit_serial(), 0);
}

#[test]
fn bootstrap_and_target_mapping_preserve_function_output_and_argument_identity() {
    let f = function();
    let library = aestra_compiler::MaterialFunctionLibrary::new(vec![f.clone()]);
    let projection = MaterialCompiler.project_function_graph(&f, &library);
    let MaterialFunctionBodyProjection::Graph { nodes, edges } = projection.body else {
        panic!()
    };
    let (positions, output, extent) = bootstrap_layout(&nodes, &edges);
    assert_eq!(
        (positions.clone(), output, extent),
        bootstrap_layout(&nodes, &edges)
    );
    assert!(
        positions
            .values()
            .all(|position| position.is_finite() && position.x < output.x)
    );
    assert!(extent.x > output.x && extent.y >= 420.0);
    assert_eq!(
        target(&MaterialFunctionGraphTarget::Output(f.outputs[0].id)),
        Some(Target::Output(f.outputs[0].id))
    );
    let expression = f.expressions[0].id;
    assert_eq!(
        target(&MaterialFunctionGraphTarget::Input {
            expression,
            port: "left".into()
        }),
        Some(Target::Input(expression, MaterialExpressionInput::Left))
    );
    assert_eq!(
        target(&MaterialFunctionGraphTarget::Input {
            expression,
            port: "unknown".into()
        }),
        None
    );
    assert_eq!(
        target(&MaterialFunctionGraphTarget::Argument {
            expression,
            input: f.inputs[0].id
        }),
        Some(Target::Input(
            expression,
            MaterialExpressionInput::FunctionArgument(f.inputs[0].id)
        ))
    );
}
