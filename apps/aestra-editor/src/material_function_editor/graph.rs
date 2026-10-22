//! Function-native canvas: no surrogate material program or effect is created.
#[cfg(test)]
use super::*;
#[cfg(test)]
use crate::feathers::node_graph::geometry::{GraphDocumentKey, GraphNodeKey};
#[cfg(test)]
use crate::feathers::node_graph::*;
#[cfg(test)]
use aestra_compiler::{MaterialCompiler, MaterialFunctionBodyProjection};
#[cfg(test)]
use bevy::ui::RelativeCursorPosition;
#[cfg(test)]
use std::collections::{BTreeMap, BTreeSet};

mod clipboard;
mod socket_palette;
#[cfg(test)]
pub(crate) use socket_palette::tests::setup_with_registration as setup_with_registration_for_test;

mod clipboard_context;
#[cfg(test)]
use clipboard_context::View;
#[cfg(test)]
use clipboard_context::ViewScope;
mod context_input;
mod modified_input;
#[cfg(test)]
use context_input::handle_function_graph_context_action;
#[cfg(test)]
use modified_input::function_branch_nodes;
mod connection;
#[cfg(test)]
use connection::ConnectionPreview;
#[cfg(test)]
use connection::{end_connection, move_connection, start_connection};
mod menu_state;
mod selection_input;
pub(crate) use menu_state::FunctionGraphMenuState;
#[cfg(test)]
use menu_state::{FunctionGraphMenuKind, FunctionGraphMenuOpen, Target};

mod model;
#[cfg(test)]
use model::*;
mod body_input;
mod composition;
#[cfg(test)]
use body_input::set_component;
#[cfg(test)]
use composition::components;
#[cfg(test)]
use composition::target;
pub(crate) use composition::{bootstrap_layout, estimated_size, spawn};

mod registration;
pub(super) use registration::register;
mod wires;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn function_selection_is_scoped_and_produces_partial_arrange_seeds() {
        let function = MaterialFunctionId::new();
        let other_function = MaterialFunctionId::new();
        let first = MaterialExpressionId::new();
        let second = MaterialExpressionId::new();
        let left = Some(crate::docking::EditorViewId(1));
        let right = Some(crate::docking::EditorViewId(2));
        let mut selection = crate::material_graph::MaterialGraphSelectionState::default();

        selection.select_function_expression(left, function, first, false, false);
        selection.select_function_expression(left, function, second, false, true);
        selection.select_function_expression(right, other_function, first, false, false);

        assert!(selection.is_function_expression_selected(left, function, first));
        assert!(selection.is_function_expression_selected(left, function, second));
        assert!(!selection.is_function_expression_selected(right, function, first));
        assert_eq!(
            selection.function_arrange_seeds(left, function),
            BTreeSet::from([
                GraphNodeKey::Expression(first),
                GraphNodeKey::Expression(second),
            ])
        );
        assert!(selection.function_arrange_seeds(right, function).is_empty());
        assert!(selection.clear_function_selection(left, function));
        assert!(selection.function_arrange_seeds(left, function).is_empty());
        assert!(!selection.clear_function_selection(left, function));
    }

    #[test]
    fn modifier_drag_branch_closures_follow_function_edge_direction() {
        let function = MaterialFunction::from_ron(include_str!(
            "../../../../assets/test/materials/dissolve_edge.aestra.material-function.ron"
        ))
        .unwrap();
        let library = aestra_compiler::MaterialFunctionLibrary::new(vec![function.clone()]);
        let projection = MaterialCompiler.project_function_graph(&function, &library);
        let MaterialFunctionBodyProjection::Graph { edges, .. } = &projection.body else {
            panic!("fixture should have a graph body");
        };
        let edge = edges
            .iter()
            .find_map(|edge| target(&edge.target).map(|target| (edge.source, target)))
            .expect("fixture graph should contain a directed edge");
        let source = GraphNodeKey::Expression(edge.0);
        let target = match edge.1 {
            Target::Input(expression, _) => GraphNodeKey::Expression(expression),
            Target::Output(_) => GraphNodeKey::FunctionOutputs,
        };

        assert!(function_branch_nodes(&projection.body, target, true).contains(&source));
        assert!(function_branch_nodes(&projection.body, source, false).contains(&target));
    }

    #[test]
    fn function_graph_rebuild_uses_expression_ids_for_manual_placement_and_output_ids_for_sockets()
    {
        use bevy::ecs::system::RunSystemOnce;
        let root = tempfile::tempdir().unwrap();
        let function = MaterialFunction::from_ron(include_str!(
            "../../../../assets/test/materials/dissolve_edge.aestra.material-function.ron"
        ))
        .unwrap();
        function
            .save_ron(root.path().join("body.aestra.material-function.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
        session
            .open_material_function(&catalog, function.id)
            .unwrap();
        let effect_before = session.effect.clone();
        let function_before = session.graph_function(&catalog).unwrap();
        let editing_target = session.material_target.clone();
        let graph_key = format!("function:{}:{}", catalog.root().display(), function.id);
        let mut memory = GraphViewportMemory::default();
        let positions = function
            .expressions
            .iter()
            .enumerate()
            .map(|(index, expression)| {
                let position = Vec2::new(-400.0 + index as f32 * 17.0, 700.0 + index as f32 * 23.0);
                memory.set_node(&graph_key, expression.id.to_string(), position, false);
                (expression.id, position)
            })
            .collect::<BTreeMap<_, _>>();
        let (mut app, ui_root) = geometry::tests::layout_app(1.25);
        geometry::tests::enable_overlays(&mut app);
        app.insert_resource(session)
            .insert_resource(catalog)
            .insert_resource(memory);

        for index in 0..2 {
            // The explicitly rendered function need not be the session's current target.
            app.world_mut()
                .resource_mut::<EditorSession>()
                .material_target = crate::material_document::MaterialEditingTarget::EffectInstance;
            let target = editing_target.clone();
            let host = app
                .world_mut()
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                    ChildOf(ui_root),
                ))
                .id();
            app.world_mut()
                .run_system_once(
                    move |mut commands: Commands,
                          session: Res<EditorSession>,
                          catalog: Res<ProjectEffectCatalog>,
                          assets: Res<AssetServer>,
                          memory: Res<GraphViewportMemory>| {
                        commands.entity(host).with_children(|parent| {
                            spawn(
                                parent,
                                &session,
                                &catalog,
                                &assets,
                                &memory,
                                &crate::material_graph::MaterialGraphSelectionState::default(),
                                &FunctionGraphMenuState::default(),
                                &target,
                                Some(crate::docking::EditorViewId(index)),
                            )
                        });
                    },
                )
                .unwrap();
            for _ in 0..4 {
                app.update();
            }
            geometry::tests::assert_frame_all(app.world_mut());
            let world = app.world_mut();
            let document = GraphDocumentKey {
                project: world.resource::<ProjectEffectCatalog>().root().to_owned(),
                asset: crate::document::DocumentKey::MaterialFunction(function.id),
            };
            let measured = world
                .resource::<geometry::GraphGeometryRegistry>()
                .snapshot(&document)
                .unwrap_or_else(|| {
                    panic!(
                        "Missing function geometry on rebuild {index}: {:#?}",
                        world.resource::<geometry::GraphGeometryRegistry>()
                    )
                });
            assert_eq!(
                measured.measured_in.view,
                Some(crate::docking::EditorViewId(index))
            );
            assert_eq!(measured.nodes.len(), positions.len() + 1);
            assert_eq!(
                measured.nodes[&GraphNodeKey::FunctionOutputs].ports.len(),
                function.outputs.len()
            );
            let mut sources = BTreeMap::new();
            let mut outputs = BTreeSet::new();
            for socket in world.query::<&Socket>().iter(world) {
                assert_eq!(socket.owner, function.id);
                match socket.kind {
                    SocketKind::Source(expression) => {
                        let node = world.get::<FeathersGraphNode>(socket.node).unwrap();
                        sources.insert(expression, node.position());
                    }
                    SocketKind::Target(Target::Output(output)) => {
                        outputs.insert(output);
                    }
                    _ => {}
                }
            }
            assert_eq!(sources, positions);
            assert_eq!(
                outputs,
                function.outputs.iter().map(|output| output.id).collect()
            );
            assert_eq!(
                world.query::<&FeathersGraphNode>().iter(world).count(),
                positions.len() + 1
            );
            assert_eq!(world.resource::<EditorSession>().effect, effect_before);
            assert_eq!(
                world
                    .resource::<EditorSession>()
                    .graph_function_for(&editing_target, world.resource::<ProjectEffectCatalog>())
                    .unwrap(),
                function_before
            );
            world.despawn(host);
        }
    }

    #[test]
    fn connection_preview_starts_at_pointer_and_clears_on_release() {
        use bevy::picking::{
            backend::HitData,
            pointer::{Location, PointerButton, PointerId},
        };
        let mut app = App::new();
        app.init_resource::<ConnectionPreview>()
            .add_observer(start_connection)
            .add_observer(move_connection)
            .add_observer(end_connection);
        let socket = app
            .world_mut()
            .spawn(Socket {
                owner: MaterialFunctionId::new(),
                kind: SocketKind::Source(MaterialExpressionId::new()),
                node: Entity::PLACEHOLDER,
            })
            .id();
        let location = Location {
            target: bevy::camera::NormalizedRenderTarget::None {
                width: 800,
                height: 600,
            },
            position: Vec2::new(100.0, 120.0),
        };
        app.world_mut()
            .trigger(bevy::picking::events::PointerDragStart {
                entity: socket,
                pointer: bevy::picking::events::Pointer::new(PointerId::Mouse, location.clone()),

                button: PointerButton::Primary,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            });
        assert_eq!(
            app.world().resource::<ConnectionPreview>().0,
            Some((socket, location.position))
        );
        app.world_mut()
            .trigger(bevy::picking::events::PointerDragEnd {
                entity: socket,
                pointer: bevy::picking::events::Pointer::new(PointerId::Mouse, location),

                button: PointerButton::Primary,
                distance: Vec2::ZERO,
            });
        assert!(app.world().resource::<ConnectionPreview>().0.is_none());
    }

    #[test]
    fn component_edits_preserve_literal_type_and_other_channels() {
        for mut value in [
            MaterialValue::Vec2([1.0, 2.0]),
            MaterialValue::Vec3([1.0, 2.0, 3.0]),
            MaterialValue::Vec4([1.0, 2.0, 3.0, 4.0]),
            MaterialValue::ColorSrgb([1.0, 2.0, 3.0, 4.0]),
        ] {
            let before = value.clone();
            set_component(&mut value, 1, 0.75).unwrap();
            assert_eq!(
                std::mem::discriminant(&value),
                std::mem::discriminant(&before)
            );
            let channels = components(&value);
            assert_eq!(channels[0].1, 1.0);
            assert_eq!(channels[1].1, 0.75);
            assert_eq!(&channels[2..], &components(&before)[2..]);
            let saved = value.clone();
            assert!(set_component(&mut value, 0, f32::NAN).is_err());
            assert!(set_component(&mut value, 8, 1.0).is_err());
            assert_eq!(value, saved);
        }
    }

    #[test]
    fn feather_add_and_socket_drop_edit_the_function_not_the_effect() {
        use bevy::{
            camera::NormalizedRenderTarget,
            picking::{
                backend::HitData,
                pointer::{Location, PointerButton, PointerId},
            },
        };
        let root = tempfile::tempdir().unwrap();
        let first = MaterialExpressionId::new();
        let second = MaterialExpressionId::new();
        let output = MaterialFunctionOutputId::new();
        let function = MaterialFunction {
            id: MaterialFunctionId::new(),
            name: "Body".into(),
            schema_version: aestra_core::material::MaterialSchemaVersion::CURRENT,
            inputs: vec![],
            outputs: vec![MaterialFunctionOutput {
                id: output,
                name: "Value".into(),
                value_type: MaterialValueType::Float,
                expression: first,
            }],
            expressions: vec![
                MaterialExpression {
                    id: first,
                    kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
                },
                MaterialExpression {
                    id: second,
                    kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
                },
            ],
            custom_wesl: None,
        };
        function
            .save_ron(root.path().join("body.aestra.material-function.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = crate::test_support::session_with_timing_slack();
        let effect = session.effect.clone();
        session
            .open_material_function(&catalog, function.id)
            .unwrap();
        let original_function = session.graph_function(&catalog).unwrap();
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(catalog)
            .init_resource::<GraphViewportMemory>()
            .init_resource::<crate::history::MaterialProgramEditHistory>()
            .init_resource::<crate::history::EditorHistoryLedger>()
            .add_observer(crate::history::execute_history_action);
        super::super::register(&mut app);
        let button = app
            .world_mut()
            .spawn(BodyAction {
                owner: function.id,
                scope: None,
                kind: BodyActionKind::Create(aestra_compiler::MaterialGraphCreateKind::Function(
                    aestra_compiler::MaterialGraphFunction::Multiply,
                )),
            })
            .id();
        app.world_mut().trigger(Activate { entity: button });
        let graph_key = crate::material_graph::function_graph_memory_key(root.path(), function.id);
        let placed = app
            .world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph_key);
        assert!(!placed.is_empty());
        let positions = placed
            .values()
            .map(|(position, _)| *position)
            .collect::<Vec<_>>();
        for (index, position) in positions.iter().enumerate() {
            assert!(!positions[index + 1..].contains(position));
        }
        app.world_mut().trigger(crate::history::HistoryAction::Undo);
        app.world_mut().flush();
        assert!(
            app.world()
                .resource::<GraphViewportMemory>()
                .base_nodes(&graph_key)
                .is_empty()
        );
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .graph_function(app.world().resource::<ProjectEffectCatalog>())
                .unwrap(),
            original_function
        );
        app.world_mut().trigger(crate::history::HistoryAction::Redo);
        app.world_mut().flush();
        assert_eq!(
            app.world()
                .resource::<GraphViewportMemory>()
                .base_nodes(&graph_key),
            placed
        );
        let source = app
            .world_mut()
            .spawn(Socket {
                owner: function.id,
                kind: SocketKind::Source(second),
                node: Entity::PLACEHOLDER,
            })
            .id();
        let destination = app
            .world_mut()
            .spawn(Socket {
                owner: function.id,
                kind: SocketKind::Target(Target::Output(output)),
                node: Entity::PLACEHOLDER,
            })
            .id();
        app.world_mut()
            .trigger(bevy::picking::events::PointerDragStart {
                entity: source,
                pointer: bevy::picking::events::Pointer::new(
                    PointerId::Mouse,
                    Location {
                        target: NormalizedRenderTarget::None {
                            width: 800,
                            height: 600,
                        },
                        position: Vec2::ZERO,
                    },
                ),

                button: PointerButton::Primary,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            });
        assert!(
            app.world()
                .resource::<ConnectionPreview>()
                .1
                .contains(&destination)
        );
        assert!(
            !app.world()
                .resource::<ConnectionPreview>()
                .1
                .contains(&source)
        );
        assert_eq!(app.world().resource::<EditorSession>().effect, effect);
        app.world_mut()
            .trigger(bevy::picking::events::PointerDragDrop {
                entity: destination,
                pointer: bevy::picking::events::Pointer::new(
                    PointerId::Mouse,
                    Location {
                        target: NormalizedRenderTarget::None {
                            width: 800,
                            height: 600,
                        },
                        position: Vec2::ZERO,
                    },
                ),

                button: PointerButton::Primary,
                dropped: source,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            });
        let session = app.world().resource::<EditorSession>();
        let current = session
            .graph_function(app.world().resource::<ProjectEffectCatalog>())
            .unwrap();
        assert_eq!(current.outputs[0].expression, second);
        assert_eq!(current.expressions.len(), 5);
        assert_eq!(session.effect, effect);
        assert!(
            app.world()
                .resource::<FunctionEditor>()
                .available(session, true)
        );

        // Real picking can start on socket descendants. A near-socket release must
        // resolve the same endpoint, even without a DragDrop or a preview frame.
        app.world_mut()
            .resource_scope(|world, mut editor: Mut<FunctionEditor>| {
                world.resource_scope(|world, mut session: Mut<EditorSession>| {
                    editor
                        .step(
                            &mut session,
                            &mut world.resource_mut::<ProjectEffectCatalog>(),
                            true,
                        )
                        .unwrap();
                });
            });
        let source_child = app.world_mut().spawn(ChildOf(source)).id();
        app.world_mut().entity_mut(destination).insert((
            ComputedNode {
                inverse_scale_factor: 0.5,
                ..default()
            },
            UiGlobalTransform::from(bevy::math::Affine2::from_translation(Vec2::splat(200.0))),
        ));
        let location = Location {
            target: NormalizedRenderTarget::None {
                width: 800,
                height: 600,
            },
            position: Vec2::new(110.0, 100.0),
        };
        app.world_mut()
            .trigger(bevy::picking::events::PointerDragStart {
                entity: source_child,
                pointer: bevy::picking::events::Pointer::new(PointerId::Mouse, location.clone()),

                button: PointerButton::Primary,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            });
        assert_eq!(
            app.world().resource::<ConnectionPreview>().0.unwrap().0,
            source
        );
        app.world_mut()
            .trigger(bevy::picking::events::PointerDragEnd {
                entity: source_child,
                pointer: bevy::picking::events::Pointer::new(PointerId::Mouse, location),

                button: PointerButton::Primary,
                distance: Vec2::ZERO,
            });
        assert!(app.world().resource::<ConnectionPreview>().0.is_none());
        let session = app.world().resource::<EditorSession>();
        assert_eq!(
            session
                .graph_function(app.world().resource::<ProjectEffectCatalog>())
                .unwrap()
                .outputs[0]
                .expression,
            second
        );
        assert_eq!(session.effect, effect);
    }
}
