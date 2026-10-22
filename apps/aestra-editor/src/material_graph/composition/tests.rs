//! Mount the production canvas rather than a qualification-only UI replica.
use super::*;
use crate::{
    docking::EditorViewId,
    feathers::{
        icon::SvgPlugin,
        node_graph::{
            FeathersGraphNode, FeathersGraphNodePreview, FeathersGraphSocket,
            FeathersGraphViewport, FeathersGraphWireLayer, GraphFrameTarget,
        },
        text_input::tests::app_with,
    },
    material_document::MaterialEditingTarget,
    material_graph::{
        header::material_graph_viewport_key,
        preview_state::{Input, Key, MaterialGraphPreviewCache},
    },
};
use aestra_compiler::MaterialCompiler;
use bevy::ui::Checked;

fn program() -> MaterialProgram {
    MaterialProgram::from_ron(crate::MATERIAL_GRAPH_LAB_PROGRAM_SOURCE).unwrap()
}
fn projection(program: &MaterialProgram) -> MaterialGraphProjection {
    MaterialCompiler.project_graph(program, MaterialCompiler.compile(program).ok().as_ref())
}
fn mount(
    program: &MaterialProgram,
    graph: &MaterialGraphProjection,
    selection: &MaterialGraphSelectionState,
    scope: MaterialSelectionScope,
    previews: &MaterialGraphPreviewState,
    memory: &GraphViewportMemory,
    instance: Option<MaterialId>,
) -> (App, Entity) {
    let mut app = app_with(|app| {
        app.add_plugins(SvgPlugin);
    });
    let server = app.world().resource::<AssetServer>().clone();
    let target = MaterialEditingTarget::Program {
        root: "composition-project".into(),
        id: program.id,
    };
    let mut viewport = Entity::PLACEHOLDER;
    app.world_mut()
        .commands()
        .spawn(Node::default())
        .with_children(|parent| {
            viewport = spawn_material_graph_canvas(
                parent,
                std::path::Path::new("composition-project"),
                graph,
                program,
                &target,
                scope,
                &material_graph_viewport_key(program.id, scope),
                instance,
                selection,
                previews,
                memory,
                &Localizer::new("en-US").unwrap(),
                &server,
            );
        });
    app.world_mut().flush();
    app.update();
    (app, viewport)
}
fn basic(program: &MaterialProgram, graph: &MaterialGraphProjection) -> (App, Entity) {
    mount(
        program,
        graph,
        &default(),
        Some(EditorViewId(7)),
        &default(),
        &default(),
        None,
    )
}
fn nodes(app: &mut App) -> Vec<Entity> {
    app.world_mut()
        .query_filtered::<Entity, With<FeathersGraphNode>>()
        .iter(app.world())
        .collect()
}
fn count<T: Component>(app: &mut App) -> usize {
    app.world_mut()
        .query_filtered::<Entity, With<T>>()
        .iter(app.world())
        .count()
}

#[test]
fn actual_canvas_keeps_clipped_viewport_and_ordered_wire_and_node_layers() {
    let p = program();
    let g = projection(&p);
    let (app, viewport) = basic(&p, &g);
    let world = app.world();
    let n = world.get::<Node>(viewport).unwrap();
    assert_eq!(n.overflow, Overflow::clip());
    assert_eq!(n.width, Val::Percent(100.0));
    let children = world.get::<Children>(viewport).unwrap();
    assert_eq!(children.len(), 3);
    assert_eq!(
        *world.get::<Pickable>(children[0]).unwrap(),
        Pickable::IGNORE
    );
    assert!(world.get::<FeathersGraphWireLayer>(children[1]).is_some());
    assert!(world.get::<MaterialGraphCanvas>(children[2]).is_some());
    assert_eq!(
        *world.get::<Pickable>(children[1]).unwrap(),
        Pickable::IGNORE
    );
    for layer in [children[1], children[2]] {
        assert_eq!(
            *world.get::<UiTransform>(layer).unwrap(),
            UiTransform::IDENTITY
        );
    }
}
#[test]
fn actual_canvas_publishes_project_document_view_and_complete_geometry_inventory() {
    let p = program();
    let g = projection(&p);
    let (app, viewport) = basic(&p, &g);
    let identity = app.world().get::<GraphGeometryView>(viewport).unwrap();
    assert_eq!(
        identity.key.document.project,
        std::path::Path::new("composition-project")
    );
    assert_eq!(
        identity.key.document.asset,
        crate::document::DocumentKey::MaterialProgram(p.id)
    );
    assert_eq!(identity.key.view, Some(EditorViewId(7)));
    assert_eq!(identity.nodes.len(), g.nodes.len() + 1);
    assert!(identity.nodes.contains(&GraphNodeKey::MaterialOutputs));
    let material = app.world().get::<MaterialGraphViewport>(viewport).unwrap();
    assert_eq!(material.program, p.id);
    assert_eq!(material.scope, Some(EditorViewId(7)));
    assert!(
        matches!(&material.editing_target, MaterialEditingTarget::Program { root, id }
        if root == std::path::Path::new("composition-project") && *id == p.id)
    );
}
#[test]
fn actual_canvas_auto_frames_only_without_a_seeded_camera() {
    let p = program();
    let g = projection(&p);
    let (app, v) = basic(&p, &g);
    assert_eq!(
        app.world()
            .get::<FeathersGraphViewport>(v)
            .unwrap()
            .frame_request_for_test(),
        Some(GraphFrameTarget::All)
    );
    for per_view in [false, true] {
        let mut memory = GraphViewportMemory::default();
        memory.set_view(material_graph_view_key(p.id), Vec2::new(12.0, 14.0), 0.5);
        if per_view {
            memory.set_view(
                material_graph_viewport_key(p.id, Some(EditorViewId(7))),
                Vec2::new(20.0, 30.0),
                0.8,
            );
        }
        let (app, v) = mount(
            &p,
            &g,
            &default(),
            Some(EditorViewId(7)),
            &default(),
            &memory,
            None,
        );
        let camera = app.world().get::<FeathersGraphViewport>(v).unwrap();
        assert_eq!(camera.frame_request_for_test(), None);
        assert_eq!(
            camera.project_graph_point(Vec2::ZERO),
            if per_view {
                Vec2::new(20.0, 30.0)
            } else {
                Vec2::new(12.0, 14.0)
            }
        );
    }
}
#[test]
fn actual_nodes_keep_bootstrap_positions_and_synthetic_output_is_not_editable() {
    let p = program();
    let g = projection(&p);
    let expected = layout_graph(&g, &default());
    let (mut app, _) = basic(&p, &g);
    let nodes = nodes(&mut app);
    assert_eq!(nodes.len(), g.nodes.len() + 1);
    for e in nodes {
        let w = app.world();
        let state = w.get::<FeathersGraphNode>(e).unwrap();
        assert_eq!(state.graph_key(), material_graph_view_key(p.id));
        assert!(w.get::<GraphGeometryNode>(e).is_some());
        if let Some(action) = w.get::<MaterialGraphAction>(e) {
            assert_eq!(state.position(), expected.nodes[&action.expression]);
            assert_eq!(action.program, p.id);
            assert!(w.get::<NodeButton>(e).is_some());
            assert!(w.get::<GraphModifiedDragTarget>(e).is_some());
            assert_eq!(
                w.get::<MaterialGraphNodeScope>(e).unwrap().0,
                Some(EditorViewId(7))
            );
        } else {
            assert_eq!(state.node_key(), MATERIAL_GRAPH_OUTPUT_NODE_KEY);
            assert_eq!(state.position(), expected.output);
            assert!(w.get::<GraphModifiedDragTarget>(e).is_none());
            assert!(w.get::<MaterialGraphNodeScope>(e).is_none());
        }
    }
}
#[test]
fn actual_selection_is_per_view_and_pin_state_is_document_shared() {
    let p = program();
    let g = projection(&p);
    let id = g.nodes[0].expression;
    let mut selection = MaterialGraphSelectionState::default();
    selection.select_single(Some(EditorViewId(7)), p.id, id);
    let mut memory = GraphViewportMemory::default();
    memory.set_pinned(
        &material_graph_view_key(p.id),
        &material_graph_expression_node_key(id),
        true,
    );
    for scope in [Some(EditorViewId(7)), Some(EditorViewId(8)), None] {
        let (mut app, _) = mount(&p, &g, &selection, scope, &default(), &memory, None);
        let entity = app
            .world_mut()
            .query::<(Entity, &MaterialGraphAction)>()
            .iter(app.world())
            .find(|(_, a)| a.expression == id)
            .unwrap()
            .0;
        let w = app.world();
        assert_eq!(
            w.get::<BackgroundColor>(entity).unwrap().0,
            if scope == Some(EditorViewId(7)) {
                theme::SELECTION
            } else {
                theme::PANEL
            }
        );
        let labels = app
            .world_mut()
            .query::<&AccessibleLabel>()
            .iter(app.world())
            .map(|l| l.0.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            labels.iter().filter(|l| l.as_str() == "Unpin node").count(),
            1
        );
    }
}
#[test]
fn actual_wire_entities_keep_semantic_endpoints_colors_and_passive_hit_policy() {
    let p = program();
    let g = projection(&p);
    let (mut app, viewport) = basic(&p, &g);
    let layer = app.world().get::<Children>(viewport).unwrap()[1];
    let wires = app
        .world_mut()
        .query::<(Entity, &MaterialGraphWire)>()
        .iter(app.world())
        .map(|(e, w)| (e, *w))
        .collect::<Vec<_>>();
    let edges = g
        .edges
        .iter()
        .filter_map(|e| edge_target(&e.target).map(|t| (e, t)))
        .collect::<Vec<_>>();
    assert_eq!(wires.len(), edges.len());
    for (entity, wire) in wires {
        assert!(edges.iter().any(|(edge, target)| edge.source == wire.source
            && *target == wire.target
            && wire.color == wire_color(edge.value_type)));
        assert_eq!(wire.program, p.id);
        assert_eq!(app.world().get::<ChildOf>(entity).unwrap().parent(), layer);
        assert_eq!(
            *app.world().get::<Pickable>(entity).unwrap(),
            Pickable::IGNORE
        );
        assert!(
            app.world()
                .get::<crate::feathers::node_graph::insertion::Wire>(entity)
                .is_some()
        );
    }
}
#[test]
fn actual_socket_hit_targets_keep_typed_metadata_and_anchor_to_their_real_node() {
    let p = program();
    let g = projection(&p);
    let (mut app, _) = basic(&p, &g);
    let sockets = app
        .world_mut()
        .query::<(Entity, &MaterialGraphSocket, &MaterialGraphSocketAnchor)>()
        .iter(app.world())
        .map(|(e, s, a)| (e, *s, *a))
        .collect::<Vec<_>>();
    assert!(!sockets.is_empty());
    for (e, s, a) in sockets {
        let w = app.world();
        assert_eq!(s.program, p.id);
        assert!(w.get::<FeathersGraphNode>(a.node).is_some());
        assert_eq!(a.offset, None);
        assert!(w.get::<GraphGeometryPort>(e).is_some());
        assert!(w.get::<FeathersGraphSocket>(e).is_some());
        assert!(w.get::<NodeButton>(e).is_some());
        let n = w.get::<Node>(e).unwrap();
        assert_eq!((n.width, n.height), (Val::Px(20.0), Val::Px(20.0)));
        let label = &w.get::<AccessibleLabel>(e).unwrap().0;
        assert!(label.contains("Evaluation:"));
        assert!(label.contains('['));
        let dot = w.get::<Children>(e).unwrap()[0];
        let dot_node = w.get::<Node>(dot).unwrap();
        assert_eq!(
            (dot_node.width, dot_node.height),
            (Val::Px(10.0), Val::Px(10.0))
        );
        assert_eq!(*w.get::<Pickable>(dot).unwrap(), Pickable::IGNORE);
    }
}
#[test]
fn actual_preview_visibility_mounts_requests_only_for_visible_targets() {
    let p = program();
    let g = projection(&p);
    let (mut app, _) = basic(&p, &g);
    assert_eq!(count::<MaterialGraphPreviewRaster>(&mut app), 0);
    let target = MaterialGraphPreviewTarget::Expression(g.nodes[0].expression);
    let mut previews = MaterialGraphPreviewState::default();
    previews.set_visible(p.id, [target, MaterialGraphPreviewTarget::Output], true);
    let (mut app, _) = mount(&p, &g, &default(), None, &previews, &default(), None);
    assert_eq!(count::<MaterialGraphPreviewRaster>(&mut app), 2);
    assert_eq!(count::<FeathersGraphNodePreview>(&mut app), 2);
    let requests = app
        .world_mut()
        .query::<&MaterialGraphPreviewRaster>()
        .iter(app.world())
        .collect::<Vec<_>>();
    assert!(
        requests
            .iter()
            .all(|r| r.program == p.id && r.instance.is_none())
    );
    assert!(
        requests
            .iter()
            .any(|r| r.target == MaterialGraphPreviewTarget::Output
                && r.value_type == Some(MaterialValueType::Color))
    );
}
#[test]
fn actual_canvas_mounts_completed_images_in_the_same_command_batch() {
    let p = program();
    let g = projection(&p);
    let target = MaterialGraphPreviewTarget::Expression(g.nodes[0].expression);
    let image = Handle::<Image>::default();
    let mut previews = MaterialGraphPreviewState::default();
    previews.set_visible(p.id, [target], true);
    previews.cache.insert(
        Key {
            program: p.id,
            target,
            root: "composition-project".into(),
            instance: None,
        },
        MaterialGraphPreviewCache {
            image: image.clone(),
            input: Input {
                program: p.clone(),
                instance: None,
                functions: vec![],
                value_type: g.nodes[0].value_type,
            },
        },
    );
    let (mut app, _) = mount(&p, &g, &default(), None, &previews, &default(), None);
    let images = app
        .world_mut()
        .query::<(&MaterialGraphPreviewRaster, &ImageNode)>()
        .iter(app.world())
        .collect::<Vec<_>>();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].1.image, image);
    assert_eq!(images[0].1.image_mode, NodeImageMode::Stretch);
}
#[test]
fn actual_preview_surface_rejects_other_project_program_instance_and_target_caches() {
    let p = program();
    let g = projection(&p);
    let target = MaterialGraphPreviewTarget::Expression(g.nodes[0].expression);
    for mismatch in 0..4 {
        let mut previews = MaterialGraphPreviewState::default();
        previews.set_visible(p.id, [target], true);
        previews.cache.insert(
            Key {
                program: if mismatch == 0 {
                    MaterialProgramId::new()
                } else {
                    p.id
                },
                target: if mismatch == 1 {
                    MaterialGraphPreviewTarget::Output
                } else {
                    target
                },
                root: if mismatch == 2 {
                    "other-project".into()
                } else {
                    "composition-project".into()
                },
                instance: (mismatch == 3).then(MaterialId::new),
            },
            MaterialGraphPreviewCache {
                image: default(),
                input: Input {
                    program: p.clone(),
                    instance: None,
                    functions: vec![],
                    value_type: None,
                },
            },
        );
        let (mut app, _) = mount(&p, &g, &default(), None, &previews, &default(), None);
        assert_eq!(
            app.world_mut()
                .query_filtered::<&ImageNode, With<MaterialGraphPreviewRaster>>()
                .iter(app.world())
                .count(),
            0
        );
        assert_eq!(count::<MaterialGraphPreviewRaster>(&mut app), 1);
    }
}
#[test]
fn actual_preview_toggles_are_real_sized_controls_even_with_previews_hidden() {
    let p = program();
    let g = projection(&p);
    let (mut app, _) = basic(&p, &g);
    let toggles = app
        .world_mut()
        .query::<(Entity, &MaterialGraphPreviewToggle)>()
        .iter(app.world())
        .map(|(e, t)| (e, t.clone()))
        .collect::<Vec<_>>();
    assert_eq!(toggles.len(), g.nodes.len() + 1);
    for (e, t) in toggles {
        let w = app.world();
        assert_eq!(t.program, p.id);
        assert!(w.get::<NodeButton>(e).is_some());
        let n = w.get::<Node>(e).unwrap();
        assert_eq!((n.width, n.height), (Val::Px(22.0), Val::Px(22.0)));
        assert_eq!((n.top, n.right), (Val::Px(4.0), Val::Px(34.0)));
        assert_eq!(
            w.get::<AccessibleLabel>(e).unwrap().0,
            Localizer::new("en-US")
                .unwrap()
                .text("material-graph-show-preview")
        );
    }
}
#[test]
fn actual_constant_editors_keep_component_snapshot_limits_and_toggle_state() {
    let cases = [
        (MaterialValue::Float(2.5), 1),
        (MaterialValue::Vec2([1.0, 2.0]), 2),
        (MaterialValue::Vec3([1.0, 2.0, 3.0]), 3),
        (MaterialValue::Vec4([1.0, 2.0, 3.0, 4.0]), 4),
        (MaterialValue::ColorSrgb([0.1, 0.2, 0.3, 0.4]), 4),
        (MaterialValue::Bool(true), 0),
        (MaterialValue::Bool(false), 0),
    ];
    for (value, expected) in cases {
        let mut app = app_with(|_| {});
        let program = MaterialProgramId::new();
        let expression = MaterialExpressionId::new();
        app.world_mut()
            .commands()
            .spawn(Node::default())
            .with_children(|parent| {
                spawn_material_graph_constant_editor(
                    parent,
                    program,
                    &MaterialEditingTarget::EffectInstance,
                    expression,
                    &value,
                );
            });
        app.world_mut().flush();
        app.update();
        let controls = app
            .world_mut()
            .query::<(&MaterialGraphDefaultNumberControl, &ScrubbableNumber)>()
            .iter(app.world())
            .collect::<Vec<_>>();
        assert_eq!(controls.len(), expected);
        for (control, scrub) in controls {
            assert_eq!((control.program, control.expression), (program, expression));
            assert_eq!(control.value, value);
            assert_eq!(
                scrub.value,
                material_graph_value_component(&value, control.component).unwrap()
            );
            assert_eq!(scrub.step, 0.01);
            assert_eq!(
                (scrub.min, scrub.max),
                if matches!(value, MaterialValue::ColorSrgb(_)) {
                    (0.0, 1.0)
                } else {
                    (-f32::MAX, f32::MAX)
                }
            );
        }
        if let MaterialValue::Bool(enabled) = value {
            let (entity, control) = app
                .world_mut()
                .query::<(Entity, &MaterialGraphDefaultToggleControl)>()
                .single(app.world())
                .unwrap();
            assert_eq!(control.value, enabled);
            assert_eq!(app.world().get::<Checked>(entity).is_some(), enabled);
        }
    }
}

#[test]
fn actual_inline_defaults_stay_adjacent_to_the_port_label() {
    let p = program();
    let g = projection(&p);
    let (mut app, _) = basic(&p, &g);
    let inline = app
        .world_mut()
        .query::<(Entity, &MaterialGraphDefaultNumberControl)>()
        .iter(app.world())
        .filter(|(_, c)| p.inline_constants().contains(&c.expression))
        .map(|(e, _)| e)
        .collect::<Vec<_>>();
    assert!(!inline.is_empty());
    for e in inline {
        let w = app.world();
        let container = w.get::<ChildOf>(e).unwrap().parent();
        assert_eq!(w.get::<Node>(container).unwrap().width, Val::Px(66.0));
        let row = w.get::<ChildOf>(container).unwrap().parent();
        let children = w.get::<Children>(row).unwrap();
        assert_eq!(children.len(), 3);
        assert!(w.get::<MaterialGraphSocket>(children[0]).is_some());
        assert!(w.get::<Text>(children[1]).is_some());
        assert_eq!(children[2], container);
    }
}

#[test]
fn actual_disabled_or_invalid_nodes_keep_their_warning_and_muted_title() {
    let p = program();
    let mut g = projection(&p);
    g.nodes[0].disabled = true;
    g.nodes[0].validation_message = Some("Composition validation warning".into());
    let id = g.nodes[0].expression;
    let (mut app, _) = basic(&p, &g);
    let e = app
        .world_mut()
        .query::<(Entity, &MaterialGraphAction)>()
        .iter(app.world())
        .find(|(_, a)| a.expression == id)
        .unwrap()
        .0;
    let mut descendants = vec![];
    let mut pending = vec![e];
    while let Some(entity) = pending.pop() {
        descendants.push(entity);
        if let Some(children) = app.world().get::<Children>(entity) {
            pending.extend(children.iter());
        }
    }
    let texts = descendants
        .iter()
        .filter_map(|e| app.world().get::<Text>(*e).map(|t| (*e, t)))
        .collect::<Vec<_>>();
    let warning = texts
        .iter()
        .find(|(_, t)| t.0 == "Composition validation warning")
        .unwrap()
        .0;
    assert_eq!(
        app.world().get::<TextColor>(warning).unwrap().0,
        Color::srgb(1.0, 0.58, 0.30)
    );
    assert!(texts.iter().any(|(e, _)| {
        app.world()
            .get::<TextColor>(*e)
            .is_some_and(|c| c.0 == theme::TEXT_FAINT)
    }));
}

#[test]
fn actual_empty_projection_still_mounts_the_synthetic_outputs() {
    let p = program();
    let mut g = projection(&p);
    g.nodes.clear();
    g.edges.clear();
    let (mut app, viewport) = basic(&p, &g);
    assert_eq!(nodes(&mut app).len(), 1);
    assert_eq!(count::<MaterialGraphWire>(&mut app), 0);
    assert_eq!(count::<MaterialGraphAction>(&mut app), 0);
    assert_eq!(
        app.world()
            .get::<GraphGeometryView>(viewport)
            .unwrap()
            .nodes,
        BTreeSet::from([GraphNodeKey::MaterialOutputs])
    );
}

#[test]
fn actual_preview_surfaces_keep_bounded_passive_geometry() {
    let p = program();
    let g = projection(&p);
    let mut previews = MaterialGraphPreviewState::default();
    previews.set_visible(p.id, [MaterialGraphPreviewTarget::Output], true);
    let (mut app, _) = mount(&p, &g, &default(), None, &previews, &default(), None);
    let e = app
        .world_mut()
        .query_filtered::<Entity, With<MaterialGraphPreviewRaster>>()
        .single(app.world())
        .unwrap();
    let n = app.world().get::<Node>(e).unwrap();
    let size = Val::Px(crate::feathers::node_graph::NODE_PREVIEW_SIZE);
    assert_eq!(
        (n.width, n.height, n.min_width, n.min_height),
        (size, size, size, size)
    );
    assert_eq!(n.overflow, Overflow::clip());
    assert_eq!(*app.world().get::<Pickable>(e).unwrap(), Pickable::IGNORE);
}
