//! Registered presentation consumers and atomic arrangement regression coverage.
use super::*;
use crate::feathers::node_graph::GraphPresentationBatchEdit;
use crate::history::asset_order::Context;
use crate::material_graph::{
    asset_drop::GraphDropTarget,
    keys::{function_graph_memory_key, material_graph_view_key},
    presentation::tests::{fixture, step},
};
use std::collections::BTreeMap;

fn capture(app: &App, graph: &str) -> Snapshot {
    Snapshot::capture(
        graph,
        app.world().resource::<ProjectEffectCatalog>(),
        app.world().resource::<EditorSession>(),
        app.world().resource::<GraphViewportMemory>(),
    )
    .unwrap()
}
fn revision(app: &App, graph: &str) -> u64 {
    app.world()
        .resource::<GraphViewportMemory>()
        .placement_revision(graph)
}
fn serial(app: &App) -> u64 {
    app.world()
        .resource::<EditorSession>()
        .operation_order
        .edit_serial()
}
fn positions(before: &Snapshot) -> BTreeMap<String, Vec2> {
    before
        .keys
        .iter()
        .enumerate()
        .map(|(i, key)| (key.clone(), Vec2::new(i as f32 * 70.0, 90.0)))
        .collect()
}
fn move_node(
    app: &mut App,
    graph: &str,
    node: &str,
    before: (Vec2, bool),
    after: (Vec2, bool),
    origin: Option<Entity>,
) {
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node(graph, node, after.0, after.1);
    app.world_mut().trigger(GraphPresentationEdit {
        graph: graph.into(),
        node: node.into(),
        before,
        after,
        origin,
    });
    app.world_mut().flush();
}
fn assert_node(app: &App, graph: &str, node: &str, position: Vec2, collapsed: bool, pinned: bool) {
    let memory = app.world().resource::<GraphViewportMemory>();
    assert_eq!(memory.node(graph, node), Some((position, collapsed)));
    assert_eq!(memory.is_pinned(graph, node), pinned);
}

#[test]
fn registered_node_pin_and_batch_consumers_share_exact_history_without_semantic_changes() {
    for use_function in [false, true] {
        let (root, mut app, program, function) = fixture();
        let (graph, node) = if use_function {
            let catalog = app.world().resource::<ProjectEffectCatalog>().clone();
            app.world_mut()
                .resource_mut::<EditorSession>()
                .open_material_function(&catalog, function.id)
                .unwrap();
            (
                function_graph_memory_key(root.path(), function.id),
                "outputs",
            )
        } else {
            (material_graph_view_key(program.id), "output")
        };
        let target = app
            .world()
            .resource::<EditorSession>()
            .material_target
            .clone();
        let marker = if use_function {
            GraphDropTarget::function_for(
                app.world().resource::<EditorSession>(),
                function.id,
                &target,
            )
        } else {
            GraphDropTarget::program_for(
                app.world().resource::<EditorSession>(),
                program.id,
                &target,
            )
        };
        let owner = app.world_mut().spawn(marker).id();
        let origin = app.world_mut().spawn(ChildOf(owner)).id();
        app.world_mut()
            .resource_mut::<EditorSession>()
            .return_to_effect_material();
        let effect = app.world().resource::<EditorSession>().effect.clone();
        let content_revision = app
            .world()
            .resource::<ProjectEffectCatalog>()
            .content_revision();
        let document_revision = app.world().resource::<EditorSession>().document_revision();
        let program_bytes = std::fs::read(root.path().join("test.aestra.material.ron")).unwrap();
        let function_bytes =
            std::fs::read(root.path().join("test.aestra.material-function.ron")).unwrap();
        let start = Vec2::new(20.0, 40.0);
        let moved = Vec2::new(90.0, 70.0);
        let end = Vec2::new(190.0, 170.0);
        let view = format!("{graph}#view:3");
        app.world_mut()
            .resource_mut::<GraphViewportMemory>()
            .set_view(&view, Vec2::splat(25.0), 0.75);
        app.world_mut()
            .resource_mut::<GraphViewportMemory>()
            .set_node(&graph, node, start, false);
        move_node(
            &mut app,
            &graph,
            node,
            (start, false),
            (moved, false),
            Some(origin),
        );
        assert_eq!(
            app.world().resource::<EditorSession>().material_target,
            target
        );
        app.world_mut()
            .resource_mut::<GraphViewportMemory>()
            .set_pinned(&graph, node, true);
        app.world_mut().trigger(GraphPinEdit {
            graph: graph.clone(),
            node: node.into(),
            before: false,
            after: true,
            origin: Some(origin),
        });
        app.world_mut().flush();
        app.world_mut()
            .resource_mut::<GraphViewportMemory>()
            .set_node(&graph, node, end, true);
        app.world_mut().trigger(GraphPresentationBatchEdit {
            graph: graph.clone(),
            before: BTreeMap::from([(node.into(), (moved, false))]),
            origin: Some(origin),
        });
        app.world_mut().flush();
        assert_node(&app, &graph, node, end, true, true);
        for (position, collapsed, pinned) in [
            (moved, false, true),
            (moved, false, false),
            (start, false, false),
        ] {
            step(&mut app, true);
            assert_node(&app, &graph, node, position, collapsed, pinned);
        }
        for _ in 0..3 {
            step(&mut app, false);
        }
        assert_node(&app, &graph, node, end, true, true);
        assert_eq!(
            app.world().resource::<GraphViewportMemory>().view(&view),
            Some((Vec2::splat(25.0), 0.75))
        );
        assert_eq!(app.world().resource::<EditorSession>().effect, effect);
        assert_eq!(
            app.world().resource::<EditorSession>().document_revision(),
            document_revision
        );
        assert_eq!(
            app.world()
                .resource::<ProjectEffectCatalog>()
                .content_revision(),
            content_revision
        );
        assert_eq!(
            app.world()
                .resource::<ProjectEffectCatalog>()
                .material_program(program.id)
                .unwrap(),
            program
        );
        assert_eq!(
            std::fs::read(root.path().join("test.aestra.material.ron")).unwrap(),
            program_bytes
        );
        assert_eq!(
            std::fs::read(root.path().join("test.aestra.material-function.ron")).unwrap(),
            function_bytes
        );
    }
}

#[test]
fn unknown_and_noop_presentation_events_do_not_replace_redo() {
    let (_root, mut app, program, _) = fixture();
    let graph = material_graph_view_key(program.id);
    move_node(
        &mut app,
        &graph,
        "output",
        (Vec2::ZERO, false),
        (Vec2::ONE, false),
        None,
    );
    step(&mut app, true);
    let before_serial = serial(&app);
    for (key, node) in [
        (&graph, "output"),
        (&graph, "unknown"),
        (&"missing graph".to_string(), "output"),
    ] {
        app.world_mut().trigger(GraphPresentationEdit {
            graph: key.clone(),
            node: node.into(),
            before: (Vec2::ZERO, false),
            after: (Vec2::ZERO, false),
            origin: None,
        });
        app.world_mut().trigger(GraphPinEdit {
            graph: key.clone(),
            node: node.into(),
            before: false,
            after: false,
            origin: None,
        });
    }
    app.world_mut().trigger(GraphPresentationEdit {
        graph: graph.clone(),
        node: "unknown".into(),
        before: (Vec2::ZERO, false),
        after: (Vec2::ONE, true),
        origin: None,
    });
    app.world_mut().trigger(GraphPinEdit {
        graph: graph.clone(),
        node: "unknown".into(),
        before: false,
        after: true,
        origin: None,
    });
    app.world_mut().trigger(GraphPresentationBatchEdit {
        graph: graph.clone(),
        before: BTreeMap::from([("unknown".into(), (Vec2::ONE, true))]),
        origin: None,
    });
    app.world_mut().flush();
    assert_eq!(serial(&app), before_serial);
    step(&mut app, false);
    assert_node(&app, &graph, "output", Vec2::ONE, false, false);
}

#[test]
fn malformed_arrangement_keys_and_nonfinite_positions_are_atomic() {
    let (_root, mut app, program, _) = fixture();
    let graph = material_graph_view_key(program.id);
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node(&graph, "output", Vec2::ONE, true);
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_temporary_offset(&graph, "output", Vec2::splat(20.0));
    let before = capture(&app, &graph);
    let expected = positions(&before);
    let target = app
        .world()
        .resource::<EditorSession>()
        .material_target
        .clone();
    let token = revision(&app, &graph);
    let order = serial(&app);
    for invalid in 0..4 {
        let mut positions = expected.clone();
        let key = before.keys.iter().next().unwrap().clone();
        match invalid {
            0 => {
                positions.remove(&key);
            }
            1 => {
                positions.insert("unexpected".into(), Vec2::ZERO);
            }
            2 => {
                positions.insert(key, Vec2::new(f32::NAN, 0.0));
            }
            _ => {
                positions.insert(key, Vec2::new(0.0, f32::INFINITY));
            }
        }
        assert!(
            arrange(
                app.world_mut(),
                before.clone(),
                token,
                positions,
                target.clone()
            )
            .is_err()
        );
        assert_eq!(
            app.world()
                .resource::<GraphViewportMemory>()
                .base_nodes(&graph),
            before.nodes
        );
        assert_eq!(
            app.world()
                .resource::<GraphViewportMemory>()
                .node_position(&graph, "output"),
            Some(Vec2::splat(21.0))
        );
        assert_eq!(revision(&app, &graph), token);
        assert_eq!(serial(&app), order);
    }
}

#[test]
fn arrangement_rejects_semantic_changes_even_when_manual_placement_did_not_change() {
    let (_root, mut app, program, _) = fixture();
    let graph = material_graph_view_key(program.id);
    let before = capture(&app, &graph);
    let token = revision(&app, &graph);
    let target = app
        .world()
        .resource::<EditorSession>()
        .material_target
        .clone();
    let mut changed = program.clone();
    changed.name.push_str(" external draft");
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .replace_material_program(&program, &changed)
        .unwrap();
    let order = serial(&app);
    assert!(
        arrange(
            app.world_mut(),
            before.clone(),
            token,
            positions(&before),
            target
        )
        .unwrap_err()
        .contains("stale")
    );
    assert_eq!(revision(&app, &graph), token);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph),
        before.nodes
    );
    assert_eq!(
        app.world()
            .resource::<ProjectEffectCatalog>()
            .material_program(program.id)
            .unwrap(),
        changed
    );
    assert_eq!(serial(&app), order);
}

#[test]
fn pin_changes_since_dispatch_reject_the_arrangement_without_losing_the_pin() {
    let (_root, mut app, program, _) = fixture();
    let graph = material_graph_view_key(program.id);
    let before = capture(&app, &graph);
    let token = revision(&app, &graph);
    let target = app
        .world()
        .resource::<EditorSession>()
        .material_target
        .clone();
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_pinned(&graph, "output", true);
    let newer = revision(&app, &graph);
    assert!(newer > token);
    assert!(
        arrange(
            app.world_mut(),
            before.clone(),
            token,
            positions(&before),
            target
        )
        .unwrap_err()
        .contains("placement changed")
    );
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph),
        before.nodes
    );
    assert!(
        app.world()
            .resource::<GraphViewportMemory>()
            .is_pinned(&graph, "output")
    );
    assert_eq!(revision(&app, &graph), newer);
}

#[test]
fn unfocused_function_arrangement_preserves_pins_collapse_camera_and_other_graphs() {
    let (root, mut app, program, function) = fixture();
    let graph = function_graph_memory_key(root.path(), function.id);
    let material = material_graph_view_key(program.id);
    let target = MaterialEditingTarget::Function {
        root: root.path().to_owned(),
        id: function.id,
    };
    let anchor = Vec2::new(70.0, 90.0);
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node(&graph, "outputs", anchor, true);
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_pinned(&graph, "outputs", true);
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_temporary_offset(&graph, "outputs", Vec2::splat(12.0));
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node(&material, "output", Vec2::splat(800.0), false);
    let view = format!("{graph}#view:5");
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_view(&view, Vec2::splat(11.0), 0.8);
    let before = capture(&app, &graph);
    let mut positions = positions(&before);
    positions.insert("outputs".into(), anchor);
    let token = revision(&app, &graph);
    arrange(
        app.world_mut(),
        before.clone(),
        token,
        positions.clone(),
        target.clone(),
    )
    .unwrap();
    assert_eq!(
        app.world().resource::<EditorSession>().material_target,
        target
    );
    assert_node(&app, &graph, "outputs", anchor, true, true);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node_position(&graph, "outputs"),
        Some(anchor)
    );
    assert!(
        app.world()
            .resource::<EditorSession>()
            .operation_order
            .layout(&Context::Material(target), true)
            .is_some()
    );
    step(&mut app, true);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph),
        before.nodes
    );
    step(&mut app, false);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph)
            .into_iter()
            .map(|(k, (p, _))| (k, p))
            .collect::<BTreeMap<_, _>>(),
        positions
    );
    assert_node(&app, &material, "output", Vec2::splat(800.0), false, false);
    assert_eq!(
        app.world().resource::<GraphViewportMemory>().view(&view),
        Some((Vec2::splat(11.0), 0.8))
    );
    assert_eq!(
        app.world()
            .resource::<EditorSession>()
            .graph_function(app.world().resource::<ProjectEffectCatalog>())
            .unwrap(),
        function
    );
}

#[test]
fn exact_noop_arrangement_keeps_the_existing_redo_branch() {
    let (_root, mut app, program, _) = fixture();
    let graph = material_graph_view_key(program.id);
    let initial = capture(&app, &graph);
    for (node, position) in positions(&initial) {
        app.world_mut()
            .resource_mut::<GraphViewportMemory>()
            .set_node(&graph, node, position, false);
    }
    let base = app
        .world()
        .resource::<GraphViewportMemory>()
        .node(&graph, "output")
        .unwrap();
    move_node(
        &mut app,
        &graph,
        "output",
        base,
        (Vec2::splat(600.0), false),
        None,
    );
    step(&mut app, true);
    let before = capture(&app, &graph);
    let positions = before
        .nodes
        .iter()
        .map(|(node, (p, _))| (node.clone(), *p))
        .collect();
    let target = app
        .world()
        .resource::<EditorSession>()
        .material_target
        .clone();
    let token = revision(&app, &graph);
    let order = serial(&app);
    arrange(app.world_mut(), before, token, positions, target).unwrap();
    assert_eq!(serial(&app), order);
    step(&mut app, false);
    assert_node(&app, &graph, "output", Vec2::splat(600.0), false, false);
}

#[test]
fn a_project_switch_rejects_an_old_arrangement_even_with_the_same_program_id() {
    let (_root, mut app, program, _) = fixture();
    let graph = material_graph_view_key(program.id);
    let before = capture(&app, &graph);
    let token = revision(&app, &graph);
    let target = app
        .world()
        .resource::<EditorSession>()
        .material_target
        .clone();
    let other = tempfile::tempdir().unwrap();
    program
        .save_ron(other.path().join("same.aestra.material.ron"))
        .unwrap();
    app.insert_resource(ProjectEffectCatalog::scan(other.path()));
    let order = serial(&app);
    assert!(
        arrange(
            app.world_mut(),
            before.clone(),
            token,
            positions(&before),
            target
        )
        .unwrap_err()
        .contains("stale")
    );
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .base_nodes(&graph),
        before.nodes
    );
    assert_eq!(revision(&app, &graph), token);
    assert_eq!(serial(&app), order);
}
