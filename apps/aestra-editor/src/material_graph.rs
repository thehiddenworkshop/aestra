//! Projectional material graph workspace backed by semantic material commands.

#[cfg(test)]
use crate::feathers::context_menu::PointerContextSubmenuSurface;
#[cfg(test)]
use crate::feathers::node_graph::{FeathersGraphNode, FeathersGraphWireLayer, GraphSelectionMode};
#[cfg(test)]
use bevy::input_focus::{FocusCause, InputFocus};
#[cfg(test)]
use palette_state::MaterialGraphPaletteConnection;

#[cfg(test)]
use crate::feathers::node_graph::geometry::GraphDocumentKey;
#[cfg(test)]
use crate::feathers::node_graph::geometry::GraphNodeKey;
#[cfg(test)]
use crate::feathers::node_graph::{
    GraphViewportProps, geometry::GraphGeometryNode, spawn_graph_viewport,
};
#[cfg(test)]
use aestra_authoring::MaterialOutputSocket;
#[cfg(test)]
use aestra_core::material::MaterialExpressionDomain;
#[cfg(test)]
use aestra_core::{MaterialFunctionId, MaterialId, MaterialProgramId};

#[cfg(test)]
use crate::feathers::node_graph::{FeathersGraphViewport, GraphViewportMemory};
use crate::*;
#[cfg(test)]
use aestra_authoring::MaterialConnectionTarget;
#[cfg(test)]
use aestra_authoring::{MaterialAuthoringDocument, MaterialToolCommand};
#[cfg(test)]
use aestra_compiler::MaterialGraphNodeDescriptor;
#[cfg(test)]
use aestra_compiler::MaterialGraphProjection;
#[cfg(test)]
use aestra_compiler::{MaterialCompiler, MaterialGraphCreateKind};
#[cfg(test)]
use aestra_core::material::{MaterialDomain, MaterialExpression, MaterialValue, MaterialValueType};
#[cfg(test)]
use aestra_core::{
    MaterialExpressionId,
    material::{MaterialExpressionKind, MaterialProgram},
};
#[cfg(test)]
use aestra_project::ProjectEditorLayout;
#[cfg(test)]
use std::collections::{BTreeMap, BTreeSet};

mod composition;
mod workspace;
#[cfg(test)]
use composition::*;
mod ports;
pub(crate) use ports::input_port_presentation;
#[cfg(test)]
use ports::{material_slot_description, material_slot_title};
mod preview_surface;
#[cfg(test)]
use preview_surface::MaterialGraphPreviewRaster;
mod header;
#[cfg(test)]
use header::material_graph_viewport_key;
mod workspace_layout;
pub(crate) use workspace_layout::expression_depth;
#[cfg(test)]
use workspace_layout::layout_graph;
mod appearance;
#[path = "material_graph/appearance_020.rs"]
mod appearance_input;
pub(crate) mod arrange;
mod connection_state;
mod controls;
mod menu_actions;
mod node_actions;
mod node_controls;
#[cfg(test)]
use controls::*;
#[cfg(test)]
use node_actions::*;
mod connection_drag;
#[cfg(test)]
use connection_drag::{connection_endpoints, wire_viewport};
#[path = "material_graph/drag_020.rs"]
pub(crate) mod drag_input;
pub(crate) use connection_drag::update_wire_material;
#[path = "material_graph/click_020.rs"]
pub(crate) mod click_input;
mod menu_opening;
mod sockets;
#[cfg(test)]
use menu_actions::*;
#[cfg(test)]
use sockets::*;
pub(crate) use sockets::{distance_to_graph_wire, viewport_local_position};
#[path = "material_graph/control_input_020.rs"]
mod control_input;
mod node_metrics;
#[cfg(test)]
use node_metrics::*;
#[cfg(test)]
mod arrange_context_tests;
pub(crate) mod asset_drop;
mod asset_preview;
pub(crate) mod clipboard;
mod command_preparation;
mod function_layout;
#[cfg(test)]
#[path = "material_graph/function_layout/native_tests.rs"]
mod function_layout_native_tests;
pub(crate) mod insertion;
pub(crate) mod keys;
mod layout_adapter;
mod layout_lifecycle;
mod node_previews;
pub(crate) mod presentation;
pub(crate) mod semantic_placement;
pub(crate) use asset_preview::render_material_asset_preview;
pub(crate) use keys::function_graph_memory_key;
#[cfg(test)]
use keys::material_graph_expression_node_key;
#[cfg(test)]
use keys::{MATERIAL_GRAPH_OUTPUT_NODE_KEY, material_graph_view_key};
mod layout_persistence;
#[cfg(test)]
use layout_persistence::{restore_material_graph_layouts, update_material_graph_layout_document};

mod registration;
pub(crate) use registration::EditorMaterialGraphPlugin;

mod host_integration;
pub(crate) use host_integration::clear_document_transients;

mod edit_context;
use edit_context::MaterialGraphAction;

pub(crate) use edit_context::MaterialGraphViewport;

mod palette;
#[cfg(test)]
use palette::*;
mod connections;
use connections::edge_target;
pub(crate) use connections::input_target;
mod palette_state;
#[cfg(test)]
use palette_state::MaterialGraphNodeMenuOpen;
pub(crate) use palette_state::{MaterialGraphPaletteState, MaterialSelectionScope};

mod selection;
pub(crate) use selection::MaterialGraphSelectionState;

mod preview_target;
pub(crate) use preview_target::MaterialGraphPreviewTarget;

mod preview_state;
pub(crate) use preview_state::MaterialGraphPreviewState;
mod graph_inventory;

mod preset_previews;
#[cfg(test)]
pub(crate) use preset_previews::MaterialPresetPreviewRaster;
pub(crate) use preset_previews::{
    material_preset_base, resolve_preset_program, spawn_material_preset_preview,
};

mod execution;
#[cfg(test)]
use execution::{activate_material_graph_target, apply_material_tool_command};
mod keyboard;
#[cfg(test)]
use keyboard::material_graph_keyboard_input;

#[cfg(test)]
use node_previews::update as rasterize_material_graph_previews;

mod preview_pixels;
#[cfg(test)]
use preview_pixels::*;

mod workspace_host;
pub(crate) use workspace_host::spawn_material_graph_workspace;

/// Whether the singleton Material Graph tool tab is showing a shared program with unsaved edits,
/// used to flag its dock tab. The tool tab always renders the effect's material (M12), so this
/// reflects the effect material's selected program — independent of any focused editor tab, which
/// carries its own dirty marker. Per-document editor tabs use [`crate::editor_view`] dirty checks.
pub(crate) fn material_graph_unsaved(
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
) -> bool {
    selected_projection_for(
        &crate::material_document::MaterialEditingTarget::EffectInstance,
        session,
        catalog,
    )
    .ok()
    .is_some_and(|(_, graph, _, _)| {
        catalog
            .material_drafts
            .programs
            .contains_key(&graph.program)
    })
}

#[cfg(test)]
fn selected_projection(
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
) -> Result<
    (
        String,
        MaterialGraphProjection,
        Option<MaterialId>,
        MaterialProgram,
    ),
    String,
> {
    selected_projection_for(&session.material_target, session, catalog)
}

mod projection;
use projection::selected_projection_for;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;

    #[test]
    fn material_graph_canvas_bundle_spawns_without_duplicate_components() {
        let mut world = World::new();
        let viewport = world.spawn_empty().id();
        let entity = world
            .spawn(crate::feathers::node_graph::graph_canvas_bundle(
                viewport,
                Vec2::new(720.0, 420.0),
                MaterialGraphCanvas,
            ))
            .id();

        assert!(world.get::<Node>(entity).is_some());
        assert!(world.get::<MaterialGraphCanvas>(entity).is_some());
    }

    #[test]
    fn feathers_activation_queues_material_graph_menu_actions() {
        let mut app = App::new();
        app.add_observer(queue_material_graph_menu_action_activation);
        let context_action = app
            .world_mut()
            .spawn((
                MaterialGraphContextAction::Delete(MaterialProgramId::new()),
                FeathersActionButton,
                Interaction::None,
            ))
            .id();
        let palette_action = app
            .world_mut()
            .spawn((
                MaterialGraphPaletteAction {
                    program: MaterialProgramId::new(),
                    editing_target: crate::material_document::MaterialEditingTarget::EffectInstance,
                    scope: None,
                    kind: MaterialGraphCreateKind::Function(
                        aestra_compiler::MaterialGraphFunction::Remap,
                    ),
                    source: None,
                    target: Some(MaterialConnectionTarget::ProgramOutput(
                        MaterialOutputSocket::Color,
                    )),
                    label: "Remap".into(),
                    graph_position: Vec2::ZERO,
                    graph_key: "test".into(),
                    searchable: "remap".into(),
                },
                FeathersActionButton,
                Interaction::None,
            ))
            .id();
        let preview_action = app
            .world_mut()
            .spawn((
                MaterialGraphPreviewToggle {
                    program: MaterialProgramId::new(),
                    editing_target: crate::material_document::MaterialEditingTarget::EffectInstance,
                    target: MaterialGraphPreviewTarget::Output,
                },
                FeathersActionButton,
                Interaction::None,
            ))
            .id();
        let toolbar_action = app
            .world_mut()
            .spawn((
                MaterialGraphToolbarAction::AddNode(MaterialProgramId::new(), None),
                FeathersActionButton,
                Interaction::None,
            ))
            .id();

        app.world_mut().trigger(Activate {
            entity: context_action,
        });
        app.world_mut().trigger(Activate {
            entity: palette_action,
        });
        app.world_mut().trigger(Activate {
            entity: preview_action,
        });
        app.world_mut().trigger(Activate {
            entity: toolbar_action,
        });
        app.update();

        for entity in [
            context_action,
            palette_action,
            preview_action,
            toolbar_action,
        ] {
            let action = app.world().entity(entity);
            assert!(action.contains::<PendingFeathersActivation>());
            assert!(action.contains::<bevy::ui::Pressed>());
        }
    }

    #[test]
    fn arrange_activation_keeps_the_mounted_graph_alive_until_completion() {
        use bevy::ecs::system::RunSystemOnce;

        let root = tempfile::tempdir().unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let session = test_support::session_with_timing_slack();
        let revision = session.ui_revision;
        let mut app = App::new();
        app.insert_resource(catalog)
            .insert_resource(session)
            .init_resource::<MaterialGraphPaletteState>()
            .init_resource::<MaterialGraphPreviewState>()
            .init_resource::<MaterialGraphSelectionState>();
        let action = app
            .world_mut()
            .spawn((
                MaterialGraphToolbarAction::Arrange(
                    MaterialProgramId::new(),
                    None,
                    arrange::ArrangeScope::Graph,
                ),
                FeathersActionButton,
                Interaction::Pressed,
                PendingFeathersActivation,
            ))
            .id();

        app.world_mut()
            .run_system_once(handle_material_graph_toolbar_actions)
            .unwrap();

        assert_eq!(
            app.world().resource::<EditorSession>().ui_revision,
            revision
        );
        assert!(
            !app.world()
                .entity(action)
                .contains::<PendingFeathersActivation>()
        );
    }

    #[test]
    fn material_graph_preview_visibility_is_remembered_per_node() {
        let program = MaterialProgramId::new();
        let first = MaterialGraphPreviewTarget::Expression(MaterialExpressionId::new());
        let second = MaterialGraphPreviewTarget::Expression(MaterialExpressionId::new());
        let mut previews = MaterialGraphPreviewState::default();

        assert!(!previews.is_visible(program, first));
        assert!(previews.toggle(program, first));
        assert!(previews.is_visible(program, first));
        assert!(!previews.is_visible(program, second));
        assert!(!previews.toggle(program, first));
        assert!(!previews.is_visible(program, first));
    }

    #[test]
    fn material_graph_preview_visibility_can_be_changed_for_the_whole_graph() {
        let program = MaterialProgramId::new();
        let first = MaterialGraphPreviewTarget::Expression(MaterialExpressionId::new());
        let second = MaterialGraphPreviewTarget::Expression(MaterialExpressionId::new());
        let output = MaterialGraphPreviewTarget::Output;
        let targets = [first, second, output];
        let mut previews = MaterialGraphPreviewState::default();

        previews.set_visible(program, targets, true);
        assert!(
            targets
                .into_iter()
                .all(|target| previews.is_visible(program, target))
        );

        previews.set_visible(program, targets, false);
        assert!(
            targets
                .into_iter()
                .all(|target| !previews.is_visible(program, target))
        );
    }

    #[test]
    fn viewport_key_is_per_view_while_node_key_is_shared() {
        use crate::docking::EditorViewId;
        let program = MaterialProgramId::new();
        let node_key = material_graph_view_key(program);
        // Two editor views of one program get distinct pan/zoom keys.
        let left = material_graph_viewport_key(program, Some(EditorViewId(1)));
        let right = material_graph_viewport_key(program, Some(EditorViewId(2)));
        assert_ne!(left, right);
        assert_ne!(left, node_key);
        // The tool panel must not fight the elected view for the persisted document camera.
        assert_ne!(material_graph_viewport_key(program, None), node_key);
        assert_ne!(material_graph_viewport_key(program, None), left);
    }

    #[test]
    fn material_graph_layout_round_trip_does_not_mutate_semantic_programs() {
        let program = MaterialProgram::additive_sprite("Layout contract");
        let expression = program.expressions[0].id;
        let semantic_source = program.to_pretty_ron().unwrap();
        let graph_key = material_graph_view_key(program.id);
        let mut memory = GraphViewportMemory::default();
        memory.set_view(graph_key.clone(), Vec2::new(24.0, -12.0), 1.25);
        memory.set_node(
            graph_key.clone(),
            material_graph_expression_node_key(expression),
            Vec2::new(180.0, 96.0),
            true,
        );
        memory.set_node(
            graph_key.clone(),
            MATERIAL_GRAPH_OUTPUT_NODE_KEY,
            Vec2::new(520.0, 110.0),
            false,
        );
        let mut previews = MaterialGraphPreviewState::default();
        previews.toggle(
            program.id,
            MaterialGraphPreviewTarget::Expression(expression),
        );
        previews.toggle(program.id, MaterialGraphPreviewTarget::Output);
        let mut document = ProjectEditorLayout::default();

        update_material_graph_layout_document(
            &mut document,
            std::slice::from_ref(&program),
            &memory,
            &previews,
        );

        assert_eq!(program.to_pretty_ron().unwrap(), semantic_source);
        let root = tempfile::tempdir().unwrap();
        document.save(root.path()).unwrap();
        let document = ProjectEditorLayout::load(root.path()).unwrap();
        let mut restored_memory = GraphViewportMemory::default();
        let mut restored_previews = MaterialGraphPreviewState::default();
        restore_material_graph_layouts(&document, &mut restored_memory, &mut restored_previews);
        assert_eq!(
            restored_memory.view(&graph_key),
            Some((Vec2::new(24.0, -12.0), 1.25))
        );
        assert_eq!(
            restored_memory.node(&graph_key, &material_graph_expression_node_key(expression)),
            Some((Vec2::new(180.0, 96.0), true))
        );
        assert_eq!(
            restored_memory.node(&graph_key, MATERIAL_GRAPH_OUTPUT_NODE_KEY),
            Some((Vec2::new(520.0, 110.0), false))
        );
        assert!(restored_previews.is_visible(
            program.id,
            MaterialGraphPreviewTarget::Expression(expression)
        ));
        assert!(restored_previews.is_visible(program.id, MaterialGraphPreviewTarget::Output));
    }

    #[test]
    fn graph_function_calls_are_inlined_in_previews() {
        use aestra_core::material::{
            MaterialFunction, MaterialFunctionInput, MaterialFunctionOutput, MaterialFunctionRef,
            MaterialSchemaVersion,
        };
        use aestra_core::{MaterialFunctionId, MaterialFunctionInputId, MaterialFunctionOutputId};

        // A passthrough function: its single output is its single input.
        let input_id = MaterialFunctionInputId::new();
        let output_id = MaterialFunctionOutputId::new();
        let body = MaterialExpressionId::new();
        let function = MaterialFunction {
            id: MaterialFunctionId::new(),
            schema_version: MaterialSchemaVersion::CURRENT,
            name: "Passthrough".into(),
            inputs: vec![MaterialFunctionInput {
                id: input_id,
                name: "In".into(),
                value_type: MaterialValueType::Float,
                default: None,
            }],
            outputs: vec![MaterialFunctionOutput {
                id: output_id,
                name: "Out".into(),
                value_type: MaterialValueType::Float,
                expression: body,
            }],
            expressions: vec![MaterialExpression {
                id: body,
                kind: MaterialExpressionKind::FunctionInput(input_id),
            }],
            custom_wesl: None,
        };
        let library = aestra_compiler::MaterialFunctionLibrary::new([function.clone()]);

        let mut program = material_preset_base("Fn", MaterialDomain::Sprite);
        let argument = MaterialExpressionId::new();
        program.expressions.push(MaterialExpression {
            id: argument,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.42)),
        });
        let call = MaterialExpressionId::new();
        program.expressions.push(MaterialExpression {
            id: call,
            kind: MaterialExpressionKind::FunctionCall {
                function: MaterialFunctionRef::Project(function.id),
                arguments: std::collections::BTreeMap::from([(input_id, argument)]),
                output: output_id,
            },
        });
        let context = MaterialPreviewContext {
            uv: Vec2::ZERO,
            normal: Vec3::Z,
        };

        // With the library, the call inlines to its argument value.
        let evaluator = MaterialPreviewEvaluator {
            program: &program,
            instance: None,
            functions: &library,
        };
        assert!(
            matches!(evaluator.evaluate(call, context), Some(PreviewValue::Numeric(value, 1)) if (value[0] - 0.42).abs() < 1e-6),
            "a graph function call should preview as its inlined result"
        );

        // Without the function in the library (e.g. an unresolved/custom-WESL call), it stays
        // unrenderable rather than guessing.
        let evaluator = MaterialPreviewEvaluator {
            program: &program,
            instance: None,
            functions: &aestra_compiler::MaterialFunctionLibrary::default(),
        };
        assert!(evaluator.evaluate(call, context).is_none());
    }

    #[test]
    fn material_canvas_publishes_measured_preview_changes_across_ui_rebuilds() {
        use crate::feathers::node_graph::geometry::{
            self, GraphGeometryChange, GraphGeometryRegistry, GraphResizeReason,
        };
        use bevy::ecs::system::RunSystemOnce;
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Measured material").normalized();
        program
            .save_ron(root.path().join("measured.aestra.material.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let document = GraphDocumentKey {
            project: catalog.root().to_owned(),
            asset: crate::document::DocumentKey::MaterialProgram(program.id),
        };
        let mut session = test_support::session_with_timing_slack();
        session.open_material_program(&catalog, program.id).unwrap();
        let effect_before = session.effect.clone();
        let (mut app, ui_root) = geometry::tests::layout_app(1.25);
        geometry::tests::enable_overlays(&mut app);
        app.insert_resource(session)
            .insert_resource(catalog)
            .init_resource::<MaterialGraphPreviewState>();
        let mut previous_host = None;
        let mut original_positions = None;
        for step in 0..3 {
            if step > 0 {
                app.world_mut()
                    .resource_mut::<MaterialGraphPreviewState>()
                    .toggle(program.id, MaterialGraphPreviewTarget::Output);
            }
            if let Some(host) = previous_host {
                app.world_mut().despawn(host);
            }
            let host = app
                .world_mut()
                .spawn((
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    ChildOf(ui_root),
                ))
                .id();
            previous_host = Some(host);
            app.world_mut()
                .run_system_once(
                    move |mut commands: Commands,
                          session: Res<EditorSession>,
                          catalog: Res<ProjectEffectCatalog>,
                          previews: Res<MaterialGraphPreviewState>,
                          memory: Res<GraphViewportMemory>,
                          assets: Res<AssetServer>| {
                        commands.entity(host).with_children(|parent| {
                            spawn_material_graph_workspace(
                                parent,
                                None,
                                Some(crate::docking::EditorViewId(1)),
                                &session,
                                &catalog,
                                &MaterialGraphPaletteState::default(),
                                &crate::material_function_editor::graph::FunctionGraphMenuState::default(),
                                &MaterialGraphSelectionState::default(),
                                &previews,
                                &memory,
                                &Localizer::new("en-US").unwrap(),
                                &assets,
                            );
                        });
                    },
                )
                .unwrap();
            let mut changes = Vec::new();
            for _ in 0..4 {
                app.update();
                changes
                    .extend_from_slice(app.world().resource::<GraphGeometryRegistry>().changes());
            }
            geometry::tests::assert_frame_all(app.world_mut());
            let registry = app.world().resource::<GraphGeometryRegistry>();
            let snapshot = registry.snapshot(&document).unwrap();
            let output = &snapshot.nodes[&GraphNodeKey::MaterialOutputs];
            assert!(!output.ports.is_empty());
            assert!(
                snapshot
                    .nodes
                    .values()
                    .all(|node| node.size.min_element() > 0.0)
            );
            let positions = snapshot
                .nodes
                .iter()
                .map(|(key, node)| (*key, node.effective_position))
                .collect::<BTreeMap<_, _>>();
            if step == 0 {
                assert!(changes.is_empty());
                original_positions = Some(positions);
            } else {
                assert_eq!(Some(positions), original_positions);
                let reason = if step == 1 {
                    GraphResizeReason::PreviewOpened
                } else {
                    GraphResizeReason::PreviewClosed
                };
                assert!(
                    matches!(&changes[..], [geometry::GraphGeometryEvent { change: GraphGeometryChange::Resized { node: GraphNodeKey::MaterialOutputs, reason: actual, .. }, .. }] if *actual == reason),
                    "{changes:?}"
                );
            }
            assert_eq!(
                app.world().resource::<EditorSession>().effect,
                effect_before
            );
        }
    }

    #[test]
    fn material_graph_preview_expands_only_its_node_height() {
        assert_eq!(
            node_height(3, false, true) - node_height(3, false, false),
            MATERIAL_PREVIEW_LAYOUT_HEIGHT
        );
        assert_eq!(
            node_height(3, true, false) - node_height(3, false, false),
            18.0
        );
    }

    #[test]
    fn graph_input_names_cover_every_current_semantic_socket() {
        let expression = MaterialExpressionId::new();
        for name in [
            "left",
            "right",
            "start",
            "end",
            "factor",
            "value",
            "min",
            "max",
            "input_min",
            "input_max",
            "output_min",
            "output_max",
            "edge_min",
            "edge_max",
            "normal",
            "view",
            "power",
            "radius",
            "softness",
            "threshold",
            "edge_width",
            "scene_depth",
            "pixel_depth",
            "fade_distance",
            "invert",
            "speed",
            "time",
            "center",
            "angle",
            "scale",
            "texture",
            "uv",
            "source",
            "alpha",
            "sample",
            "strength",
            "flip_y",
            "tangent",
            "bitangent",
        ] {
            assert!(input_target(expression, name).is_some(), "missing {name}");
        }
        assert!(input_target(expression, "unknown").is_none());
    }

    #[test]
    fn graph_ports_use_presentation_labels_instead_of_storage_names() {
        assert_eq!(input_port_presentation("input_min").label, "Source minimum");
        assert_eq!(
            input_port_presentation("output_max").label,
            "Target maximum"
        );
        assert_eq!(input_port_presentation("edge_min").label, "Lower edge");
        assert_eq!(input_port_presentation("view").label, "View direction");
    }

    #[test]
    fn graph_slot_tooltips_report_type_and_evaluation_domain() {
        let title = material_slot_title("Target minimum input", Some(MaterialValueType::Color));
        let description = material_slot_description(
            "Value produced by this node.",
            Some(MaterialExpressionDomain::Fragment),
        );

        assert_eq!(title, "Target minimum input [Color]");
        assert!(!description.contains("Type:"));
        assert!(description.contains("Evaluation: Fragment"));
    }

    #[test]
    fn single_use_constants_inline_while_nodes_and_shared_constants_do_not() {
        let mut program = MaterialProgram::additive_sprite("Inline defaults");
        // A constant bound directly to a program output stays a node.
        let output_root = program.outputs.color;
        let single = MaterialExpressionId::new();
        let shared = MaterialExpressionId::new();
        let promoted = MaterialExpressionId::new();
        let multiply = MaterialExpressionId::new();
        let add = MaterialExpressionId::new();
        program.expressions.extend([
            MaterialExpression {
                id: single,
                kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
            },
            MaterialExpression {
                id: shared,
                kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.25)),
            },
            MaterialExpression {
                id: promoted,
                kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.75)),
            },
            MaterialExpression {
                id: multiply,
                kind: MaterialExpressionKind::Multiply(single, shared),
            },
            MaterialExpression {
                id: add,
                kind: MaterialExpressionKind::Add(shared, promoted),
            },
        ]);
        // `promoted` is single-use but explicitly kept as a node.
        program.node_constants.push(promoted);

        let inline = program.inline_constants();
        // single-use, not an output, not promoted → inline on its consuming socket.
        assert!(inline_material_graph_default(&program, &inline, single).is_some());
        // consumed by two inputs → stays a node.
        assert!(inline_material_graph_default(&program, &inline, shared).is_none());
        // explicitly promoted to a node.
        assert!(inline_material_graph_default(&program, &inline, promoted).is_none());
        // bound to a program output.
        assert!(inline_material_graph_default(&program, &inline, output_root).is_none());
    }

    #[test]
    fn graph_depth_places_consumers_after_their_sources() {
        let source = MaterialExpressionId::new();
        let middle = MaterialExpressionId::new();
        let output = MaterialExpressionId::new();
        let inputs = BTreeMap::from([
            (source, vec![]),
            (middle, vec![source]),
            (output, vec![middle]),
        ]);
        let mut memo = BTreeMap::new();
        assert_eq!(
            expression_depth(output, &inputs, &mut memo, &mut BTreeSet::new()),
            2
        );
        assert_eq!(memo[&source], 0);
        assert_eq!(memo[&middle], 1);
    }

    #[test]
    fn bootstrap_layout_is_deterministic_and_places_outputs_after_consumers() {
        let program = MaterialProgram::from_ron(crate::MATERIAL_GRAPH_LAB_PROGRAM_SOURCE).unwrap();
        let ir = MaterialCompiler.compile(&program).unwrap();
        let graph = MaterialCompiler.project_graph(&program, Some(&ir));
        let previews = MaterialGraphPreviewState::default();
        let first = layout_graph(&graph, &previews);
        let second = layout_graph(&graph, &previews);

        assert!(!first.nodes.is_empty());
        assert_eq!(first.nodes, second.nodes);
        assert_eq!(first.output, second.output);
        assert_eq!(first.size, second.size);
        let inline = program.inline_constants();
        assert!(
            inline
                .iter()
                .all(|expression| !first.nodes.contains_key(expression))
        );
        let mut visible_edges = 0;
        for node in &graph.nodes {
            for input in &node.inputs {
                if let Some(source) = first.nodes.get(&input.source) {
                    assert!(source.x < first.nodes[&node.expression].x);
                    visible_edges += 1;
                } else {
                    // Inline constants remain semantic inputs, not independently placed nodes.
                    assert!(inline.contains(&input.source));
                }
            }
            assert!(first.nodes[&node.expression].x < first.output.x);
        }
        assert!(visible_edges > 0);
    }

    #[test]
    fn add_node_palette_uses_the_typed_compiler_catalog() {
        let effect = aestra_core::EffectAsset::from_ron(crate::MATERIAL_GRAPH_LAB_EFFECT_SOURCE)
            .expect("material graph lab effect should parse");
        let program = aestra_core::material::MaterialProgram::from_ron(
            crate::MATERIAL_GRAPH_LAB_PROGRAM_SOURCE,
        )
        .expect("material graph lab program should parse");
        let document = MaterialAuthoringDocument::new(effect, vec![program.clone()]);
        let compiler = MaterialCompiler;
        let ir = compiler.compile(&program).unwrap();
        let projection = compiler.project_graph(&program, Some(&ir));
        let descriptors = compiler.graph_node_catalog(&program);

        let options =
            select_palette_operations(&document, program.id, &projection, &descriptors, None);

        assert!(!options.is_empty());
        assert!(options.iter().all(|option| option.source.is_none()));
        assert!(options.iter().all(|option| option.target.is_none()));
        assert!(options.iter().any(|option| matches!(
            option.descriptor.kind,
            MaterialGraphCreateKind::Constant(MaterialValueType::Float)
        )));
        assert!(options.iter().any(|option| matches!(
            option.descriptor.kind,
            MaterialGraphCreateKind::Function(aestra_compiler::MaterialGraphFunction::Add)
        )));

        let source = program.outputs.color;
        let from_output = select_palette_operations(
            &document,
            program.id,
            &projection,
            &descriptors,
            Some(MaterialGraphPaletteConnection::FromOutput(source)),
        );
        assert!(!from_output.is_empty());
        assert!(
            from_output
                .iter()
                .all(|option| option.source == Some(source))
        );
        assert!(from_output.iter().all(|option| option.target.is_none()));
        assert!(
            from_output
                .iter()
                .all(|option| option.descriptor.kind.consumes_source())
        );

        let target = MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Color);
        let from_input = select_palette_operations(
            &document,
            program.id,
            &projection,
            &descriptors,
            Some(MaterialGraphPaletteConnection::FromInput(target)),
        );
        assert!(!from_input.is_empty());
        assert!(
            from_input
                .iter()
                .all(|option| option.target == Some(target))
        );
        assert!(
            from_input
                .iter()
                .any(|option| !option.descriptor.kind.consumes_source() && option.source.is_none())
        );
        assert!(from_input.iter().any(
            |option| option.descriptor.kind.consumes_source() && option.source == Some(source)
        ));
    }

    #[test]
    fn add_node_palette_groups_options_by_category_in_catalog_order() {
        let option = |label: &str, category: &str| MaterialGraphPaletteOption {
            descriptor: MaterialGraphNodeDescriptor {
                kind: MaterialGraphCreateKind::Constant(MaterialValueType::Float),
                label: label.to_owned(),
                category: category.to_owned(),
            },
            source: None,
            target: None,
        };
        let options = vec![
            option("Float", "Constants"),
            option("UV", "Inputs"),
            option("Color", "Constants"),
            option("Add", "Math"),
        ];

        let categories = material_graph_palette_categories(&options);

        assert_eq!(
            categories
                .iter()
                .map(|(category, _)| *category)
                .collect::<Vec<_>>(),
            vec!["Constants", "Inputs", "Math"]
        );
        assert_eq!(
            categories[0]
                .1
                .iter()
                .map(|option| option.descriptor.label.as_str())
                .collect::<Vec<_>>(),
            vec!["Float", "Color"]
        );
    }

    #[test]
    fn connection_endpoints_accept_either_drag_direction() {
        let source = MaterialExpressionId::new();
        let target = MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Color);
        let output = MaterialGraphSocketKind::ExpressionOutput(source);
        let input = MaterialGraphSocketKind::ConnectionInput(target);

        assert_eq!(connection_endpoints(output, input), Some((source, target)));
        assert_eq!(connection_endpoints(input, output), Some((source, target)));
        assert_eq!(connection_endpoints(output, output), None);
        assert_eq!(connection_endpoints(input, input), None);
    }

    #[test]
    fn graph_selection_supports_replace_add_and_toggle() {
        use crate::docking::EditorViewId;
        let program = MaterialProgramId::new();
        let first = MaterialExpressionId::new();
        let second = MaterialExpressionId::new();
        let mut selection = MaterialGraphSelectionState::default();
        let scope: MaterialSelectionScope = Some(EditorViewId(1));
        let expressions = |selection: &MaterialGraphSelectionState| {
            selection
                .get(scope)
                .map(|selection| selection.expressions.clone())
                .unwrap_or_default()
        };

        assert_eq!(
            selection.select_expression(scope, program, first, false, false),
            Some(first)
        );
        assert_eq!(
            selection.select_expression(scope, program, second, false, true),
            Some(second)
        );
        assert_eq!(expressions(&selection), BTreeSet::from([first, second]));
        assert_eq!(
            selection.select_expression(scope, program, first, true, false),
            Some(second)
        );
        assert_eq!(expressions(&selection), BTreeSet::from([second]));
        selection.select_expression(scope, program, second, true, false);
        assert!(expressions(&selection).is_empty());
    }

    #[test]
    fn modifier_drag_branch_closures_follow_material_edge_direction() {
        let program = MaterialProgram::from_ron(crate::MATERIAL_GRAPH_LAB_PROGRAM_SOURCE).unwrap();
        let projection = MaterialCompiler
            .project_graph(&program, MaterialCompiler.compile(&program).ok().as_ref());
        let edge = projection
            .edges
            .iter()
            .find_map(|edge| edge_target(&edge.target).map(|target| (edge.source, target)))
            .expect("fixture graph should contain a directed edge");
        let target = match edge.1 {
            MaterialConnectionTarget::ExpressionInput { expression, .. } => {
                GraphNodeKey::Expression(expression)
            }
            MaterialConnectionTarget::ProgramOutput(_) => GraphNodeKey::MaterialOutputs,
        };
        let source = GraphNodeKey::Expression(edge.0);

        assert!(graph_branch_nodes(&projection, target, true).contains(&source));
        assert!(graph_branch_nodes(&projection, source, false).contains(&target));
    }

    #[test]
    fn arrange_seeds_use_only_the_target_program_and_view() {
        use crate::docking::EditorViewId;
        let program = MaterialProgramId::new();
        let other_program = MaterialProgramId::new();
        let first = MaterialExpressionId::new();
        let second = MaterialExpressionId::new();
        let left = Some(EditorViewId(1));
        let right = Some(EditorViewId(2));
        let mut selection = MaterialGraphSelectionState::default();
        selection.select_expression(left, program, first, false, false);
        selection.select_expression(left, program, second, false, true);
        selection.select_expression(right, program, MaterialExpressionId::new(), false, false);

        assert_eq!(
            arrange_seeds(&selection, left, program),
            BTreeSet::from([
                GraphNodeKey::Expression(first),
                GraphNodeKey::Expression(second),
            ])
        );
        assert!(arrange_seeds(&selection, left, other_program).is_empty());
        assert_ne!(
            arrange_seeds(&selection, left, program),
            arrange_seeds(&selection, right, program)
        );
    }

    #[test]
    fn a_wire_resolves_the_viewport_that_owns_it() {
        use bevy::ecs::system::RunSystemOnce;
        // Two views of one program each have their own wire layer; a wire resolves to the viewport
        // its layer names, so it projects through that view's pan/zoom rather than the first match.
        let mut app = App::new();
        let left_viewport = app.world_mut().spawn_empty().id();
        let right_viewport = app.world_mut().spawn_empty().id();
        let left_layer = app
            .world_mut()
            .spawn(FeathersGraphWireLayer {
                viewport: left_viewport,
            })
            .id();
        let right_layer = app
            .world_mut()
            .spawn(FeathersGraphWireLayer {
                viewport: right_viewport,
            })
            .id();
        let left_wire = app.world_mut().spawn(ChildOf(left_layer)).id();
        let right_wire = app.world_mut().spawn(ChildOf(right_layer)).id();

        let resolved = app
            .world_mut()
            .run_system_once(
                move |parents: Query<&ChildOf>, layers: Query<&FeathersGraphWireLayer>| {
                    (
                        wire_viewport(left_wire, &parents, &layers),
                        wire_viewport(right_wire, &parents, &layers),
                    )
                },
            )
            .unwrap();
        assert_eq!(resolved, (Some(left_viewport), Some(right_viewport)));
    }

    #[test]
    fn selection_is_independent_per_view() {
        use crate::docking::EditorViewId;
        let program = MaterialProgramId::new();
        let node = MaterialExpressionId::new();
        let left: MaterialSelectionScope = Some(EditorViewId(1));
        let right: MaterialSelectionScope = Some(EditorViewId(2));
        let mut selection = MaterialGraphSelectionState::default();

        // Selecting a node in one view leaves the other view's selection empty.
        selection.select_expression(left, program, node, false, false);
        assert!(selection.is_expression_selected(left, program, node));
        assert!(!selection.is_expression_selected(right, program, node));
        assert_eq!(selection.program(right), None);
    }

    #[test]
    fn graph_wire_hit_distance_follows_the_rendered_bezier() {
        let start = Vec2::new(20.0, 40.0);
        let end = Vec2::new(360.0, 180.0);
        assert!(distance_to_graph_wire(start, start, end) < 0.001);
        assert!(distance_to_graph_wire(end, start, end) < 0.001);
        assert!(distance_to_graph_wire(Vec2::new(180.0, 430.0), start, end) > 100.0);
    }

    #[test]
    fn graph_add_and_delete_recompile_the_project_preview() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("graph.aestra.material.ron");
        let effect =
            aestra_core::EffectAsset::from_ron(crate::MATERIAL_GRAPH_LAB_EFFECT_SOURCE).unwrap();
        let program = aestra_core::material::MaterialProgram::from_ron(
            crate::MATERIAL_GRAPH_LAB_PROGRAM_SOURCE,
        )
        .unwrap();
        program.save_ron(&path).unwrap();
        let mut catalog = ProjectEffectCatalog::scan(temporary.path());
        let compiled = catalog.compile_project(&effect).unwrap().root;
        let mut session = test_support::session_with_timing_slack();
        session.open_compiled_effect("material_graph_lab.aestra.ron", effect, compiled);
        let mut history = MaterialProgramEditHistory::default();

        let added = apply_material_tool_command(
            &mut session,
            &mut catalog,
            &mut history,
            program.id,
            "Add graph node",
            MaterialToolCommand::CreateMaterialGraphNode {
                program: program.id,
                kind: MaterialGraphCreateKind::Function(
                    aestra_compiler::MaterialGraphFunction::Remap,
                ),
                source: Some(program.outputs.color),
                target: Some(MaterialConnectionTarget::ProgramOutput(
                    MaterialOutputSocket::Color,
                )),
            },
        )
        .unwrap();
        let wrapper = *added.created_expressions.last().unwrap();
        assert!(
            session
                .preview()
                .unwrap()
                .effect()
                .material_program(program.id)
                .unwrap()
                .expressions
                .iter()
                .any(|expression| expression.id == wrapper)
        );

        apply_material_tool_command(
            &mut session,
            &mut catalog,
            &mut history,
            program.id,
            "Delete graph node",
            MaterialToolCommand::DeleteMaterialExpressions {
                program: program.id,
                expressions: vec![wrapper],
            },
        )
        .unwrap();
        assert!(
            !session
                .preview()
                .unwrap()
                .effect()
                .material_program(program.id)
                .unwrap()
                .expressions
                .iter()
                .any(|expression| expression.id == wrapper)
        );
        assert!(session.diagnostics.is_valid());
    }

    #[test]
    fn standalone_graph_actions_and_projection_do_not_require_an_effect_renderer() {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Unused graph").normalized();
        program
            .save_ron(root.path().join("unused.aestra.material.ron"))
            .unwrap();
        let mut catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = test_support::session_with_timing_slack();
        let effect = session.effect.clone();
        let selection = session.selection;
        session.open_material_program(&catalog, program.id).unwrap();
        let (_, graph, instance, source) = selected_projection(&session, &catalog).unwrap();
        assert!(instance.is_none());
        assert_eq!(source, program);
        assert!(
            !material_graph_palette_options(
                &session,
                &catalog,
                &session.material_target,
                program.id,
                &graph,
                None,
            )
            .is_empty()
        );
        let mut history = MaterialProgramEditHistory::default();
        let added = apply_material_tool_command(
            &mut session,
            &mut catalog,
            &mut history,
            program.id,
            "Add standalone constant",
            MaterialToolCommand::CreateMaterialGraphNode {
                program: program.id,
                kind: MaterialGraphCreateKind::Constant(MaterialValueType::Float),
                source: None,
                target: None,
            },
        )
        .unwrap();
        let created = added.created_expressions[0];
        let (_, graph, instance, _) = selected_projection(&session, &catalog).unwrap();
        assert!(instance.is_none());
        assert!(graph.nodes.iter().any(|node| node.expression == created));
        apply_material_tool_command(
            &mut session,
            &mut catalog,
            &mut history,
            program.id,
            "Delete standalone constant",
            MaterialToolCommand::DeleteMaterialExpressions {
                program: program.id,
                expressions: vec![created],
            },
        )
        .unwrap();
        assert_eq!(catalog.material_program(program.id).unwrap(), program);
        assert_eq!(session.effect, effect);
        assert_eq!(session.selection, selection);
        assert_eq!(session.effect_undo_len(), 0);
    }

    #[test]
    fn restored_material_tab_builds_palette_from_its_own_target_before_focus() {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Restored graph").normalized();
        program
            .save_ron(root.path().join("restored.aestra.material.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = test_support::session_with_timing_slack();
        let target = crate::material_document::MaterialEditingTarget::Program {
            root: root.path().to_owned(),
            id: program.id,
        };
        assert_eq!(
            session.material_target,
            crate::material_document::MaterialEditingTarget::EffectInstance
        );
        let (_, projection, _, _) = selected_projection_for(&target, &session, &catalog).unwrap();

        let options = material_graph_palette_options(
            &session,
            &catalog,
            &target,
            program.id,
            &projection,
            None,
        );

        assert!(!options.is_empty());
        activate_material_graph_target(&mut session, &catalog, &target, program.id).unwrap();
        assert_eq!(session.standalone_material(), Some(program.id));
    }

    #[test]
    fn singleton_material_graph_dirty_marker_ignores_the_focused_editor_tab() {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Focused tab").normalized();
        program
            .save_ron(root.path().join("focused.aestra.material.ron"))
            .unwrap();
        let mut catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = test_support::session_with_timing_slack();
        // Focus the standalone material (roving target) and give it an unsaved draft.
        session.open_material_program(&catalog, program.id).unwrap();
        let mut edited = program.clone();
        edited.name = "Focused edited".into();
        catalog.replace_material_program(&program, &edited).unwrap();
        session.set_material_drafts(catalog.material_drafts.clone());
        assert!(catalog.material_drafts.programs.contains_key(&program.id));

        // The singleton Material Graph tab renders the effect material (M12), which is clean, so its
        // dirty marker stays off even though the focused editor tab's document is dirty.
        assert!(!material_graph_unsaved(&session, &catalog));
    }

    #[test]
    fn restored_material_tab_rasterizes_previews_from_its_own_target_before_focus() {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Restored preview").normalized();
        program
            .save_ron(root.path().join("restored.aestra.material.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let session = test_support::session_with_timing_slack();
        assert_eq!(
            session.material_target,
            crate::material_document::MaterialEditingTarget::EffectInstance
        );
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(catalog)
            .init_resource::<MaterialGraphPreviewState>()
            .init_resource::<Assets<Image>>()
            .add_systems(Update, rasterize_material_graph_previews);
        let request = app
            .world_mut()
            .spawn(MaterialGraphPreviewRaster {
                program: program.id,
                editing_target: crate::material_document::MaterialEditingTarget::Program {
                    root: root.path().to_owned(),
                    id: program.id,
                },
                instance: None,
                target: MaterialGraphPreviewTarget::Output,
                value_type: Some(MaterialValueType::Color),
            })
            .id();

        app.update();
        assert!(
            app.world().get::<ImageNode>(request).is_none(),
            "the UI frame queues pixels instead of rendering them synchronously"
        );
        node_previews::wait_for_image(&mut app, request, None);
    }

    #[test]
    fn standalone_preview_cache_tracks_source_edits_without_an_effect_revision() {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Preview").normalized();
        program
            .save_ron(root.path().join("preview.aestra.material.ron"))
            .unwrap();
        let mut catalog = ProjectEffectCatalog::scan(root.path());
        let mut session = test_support::session_with_timing_slack();
        session.open_material_program(&catalog, program.id).unwrap();
        let revision = session.document_revision();
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(catalog.clone())
            .init_resource::<MaterialGraphPreviewState>()
            .init_resource::<Assets<Image>>()
            .add_systems(Update, rasterize_material_graph_previews);
        let request = MaterialGraphPreviewRaster {
            program: program.id,
            editing_target: crate::material_document::MaterialEditingTarget::Program {
                root: root.path().to_owned(),
                id: program.id,
            },
            instance: None,
            target: MaterialGraphPreviewTarget::Output,
            value_type: None,
        };
        let first = app.world_mut().spawn(request.clone()).id();
        let first_image = node_previews::wait_for_image(&mut app, first, None);
        let mut session = app.world_mut().remove_resource::<EditorSession>().unwrap();
        let mut history = MaterialProgramEditHistory::default();
        apply_material_tool_command(
            &mut session,
            &mut catalog,
            &mut history,
            program.id,
            "Change alpha",
            MaterialToolCommand::ReplaceMaterialExpression {
                program: program.id,
                expression: program.outputs.alpha,
                replacement: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
            },
        )
        .unwrap();
        assert_eq!(session.document_revision(), revision);
        app.insert_resource(session).insert_resource(catalog);
        let second = app.world_mut().spawn(request).id();
        let second_image = node_previews::wait_for_image(&mut app, second, Some(&first_image));
        assert_ne!(first_image, second_image);
        let images = app.world().resource::<Assets<Image>>();
        assert_ne!(
            images.get(&first_image).unwrap().data,
            images.get(&second_image).unwrap().data
        );
    }

    #[test]
    fn graph_extraction_stages_a_function_and_recompiles_the_call() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("extract.aestra.material.ron");
        let effect =
            aestra_core::EffectAsset::from_ron(crate::MATERIAL_GRAPH_LAB_EFFECT_SOURCE).unwrap();
        let program = aestra_core::material::MaterialProgram::from_ron(
            crate::MATERIAL_GRAPH_LAB_PROGRAM_SOURCE,
        )
        .unwrap();
        program.save_ron(&path).unwrap();
        let mut catalog = ProjectEffectCatalog::scan(temporary.path());
        let compiled = catalog.compile_project(&effect).unwrap().root;
        let mut session = test_support::session_with_timing_slack();
        session.open_compiled_effect("material_graph_lab.aestra.ron", effect, compiled);
        let mut history = MaterialProgramEditHistory::default();
        let function = MaterialFunctionId::from_u128(0xE871);

        let plan = apply_material_tool_command(
            &mut session,
            &mut catalog,
            &mut history,
            program.id,
            "Extract material function",
            MaterialToolCommand::ExtractMaterialFunction {
                program: program.id,
                function,
                name: "Extracted Alpha".into(),
                expressions: vec![program.outputs.alpha],
            },
        )
        .unwrap();

        assert_eq!(plan.created_function().unwrap().id, function);
        assert!(
            catalog
                .material_functions()
                .unwrap()
                .iter()
                .any(|candidate| candidate.id == function)
        );
        assert_eq!(MaterialProgram::load_ron(&path).unwrap(), program);
        let replacement = session
            .graph_material_programs(&catalog)
            .unwrap()
            .into_iter()
            .find(|candidate| candidate.id == program.id)
            .unwrap();
        assert!(matches!(
            replacement
                .expressions
                .iter()
                .find(|expression| expression.id == replacement.outputs.alpha)
                .map(|expression| &expression.kind),
            Some(MaterialExpressionKind::FunctionCall {
                function: aestra_core::material::MaterialFunctionRef::Project(id),
                ..
            }) if *id == function
        ));
        assert!(session.diagnostics.is_valid());
    }
}
