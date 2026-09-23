//! M0 contracts for the shared widget, before introducing measured layout or movement.
use super::*;
use bevy::{
    ecs::system::RunSystemOnce,
    picking::pointer::{Location, PointerId},
};

#[test]
fn effective_offsets_survive_ui_rebuild_without_becoming_base_positions() {
    let key = "function:project:f";
    let base = Vec2::new(70.0, 30.0);
    let offset = Vec2::new(90.0, 140.0);
    let mut app = App::new();
    let mut memory = GraphViewportMemory::default();
    memory.set_node(key, "n", base, false);
    assert!(memory.set_temporary_offset(key, "n", offset));
    // Replacing the same derived offset must not compound it.
    assert!(memory.set_temporary_offset(key, "n", offset));
    app.insert_resource(memory);
    for _ in 0..2 {
        let entity = app
            .world_mut()
            .spawn((node(key, "n"), Node::default()))
            .id();
        app.world_mut()
            .run_system_once(restore_graph_nodes)
            .unwrap();
        assert_eq!(
            app.world()
                .get::<FeathersGraphNode>(entity)
                .unwrap()
                .position,
            base + offset
        );
        let mut memory = app.world_mut().resource_mut::<GraphViewportMemory>();
        memory.set_collapsed(key, "n", base + offset, true);
        assert_eq!(memory.node(key, "n"), Some((base, true)));
        assert_eq!(memory.node_position(key, "n"), Some(base + offset));
        app.world_mut().despawn(entity);
    }
    let mut memory = app.world_mut().resource_mut::<GraphViewportMemory>();
    assert!(!memory.set_temporary_offset(key, "missing", offset));
    assert!(!memory.set_temporary_offset(key, "n", Vec2::splat(f32::NAN)));
    // A manual placement becomes the new base and supersedes old temporary placement.
    memory.set_node(key, "n", Vec2::new(250.0, 60.0), false);
    assert_eq!(memory.node_position(key, "n"), Some(Vec2::new(250.0, 60.0)));
    memory.set_temporary_offset(key, "n", offset);
    memory.remove_node(key, "n");
    memory.set_node(key, "n", base, false);
    assert_eq!(memory.node_position(key, "n"), Some(base));
}

#[test]
fn explicit_pin_survives_manual_moves_and_is_removed_with_the_node() {
    let graph = "material:p";
    let node = "expression:n";
    let mut memory = GraphViewportMemory::default();
    memory.set_node(graph, node, Vec2::ZERO, false);

    assert!(!memory.is_pinned(graph, node));
    memory.set_pinned(graph, node, true);
    memory.set_node(graph, node, Vec2::new(240.0, 80.0), false);

    assert!(memory.is_pinned(graph, node));
    assert_eq!(
        memory.node(graph, node),
        Some((Vec2::new(240.0, 80.0), false))
    );

    memory.remove_node(graph, node);
    assert!(!memory.is_pinned(graph, node));
}

fn node(graph_key: &str, node_key: &str) -> FeathersGraphNode {
    FeathersGraphNode {
        graph_key: graph_key.into(),
        node_key: node_key.into(),
        position: Vec2::ZERO,
        selected: false,
        pinned: false,
        collapsed: false,
        dragging: false,
        drag_before: None,
        drag_modifier: None,
        suppress_release_click: false,
    }
}

fn viewport(key: &str, zoom: f32) -> FeathersGraphViewport {
    FeathersGraphViewport {
        key: key.into(),
        pan: Vec2::ZERO,
        zoom,
        content_size: Vec2::new(720.0, 420.0),
        selection_bounds: None,
        frame_request: Some(GraphFrameTarget::All),
        measured_frame: None,
        suppress_context_click: false,
    }
}

#[test]
fn rebuilt_material_and_function_nodes_restore_manual_position_and_collapsed_body() {
    for (graph_key, node_key) in [
        ("material:p", "expression:n"),
        ("material:p", "output"),
        ("function:project:p", "n"),
        ("function:project:p", "outputs"),
    ] {
        let mut world = World::new();
        let mut memory = GraphViewportMemory::default();
        let position = Vec2::new(-120.0, 340.0);
        memory.set_node(graph_key, node_key, position, true);
        world.insert_resource(memory);
        for _ in 0..2 {
            let entity = world
                .spawn((node(graph_key, node_key), Node::default()))
                .id();
            let body = world
                .spawn((FeathersGraphNodeBody { node: entity }, Node::default()))
                .id();
            world.run_system_once(restore_graph_nodes).unwrap();

            let restored = world.get::<FeathersGraphNode>(entity).unwrap();
            assert_eq!(restored.position, position);
            assert!(restored.collapsed);
            let style = world.get::<Node>(entity).unwrap();
            assert_eq!(style.left, Val::Px(position.x));
            assert_eq!(style.top, Val::Px(position.y));
            assert_eq!(world.get::<Node>(body).unwrap().display, Display::None);
            world.despawn(body);
            world.despawn(entity);
        }
    }
}

#[test]
fn viewport_restore_keeps_per_view_cameras_separate_from_document_nodes() {
    let mut world = World::new();
    let mut memory = GraphViewportMemory::default();
    memory.set_view("material:p#view:1", Vec2::new(20.0, 40.0), 0.5);
    memory.set_view("material:p#view:2", Vec2::new(-80.0, 10.0), 1.5);
    memory.set_node("material:p", "expression:n", Vec2::new(90.0, 30.0), true);
    world.insert_resource(memory);
    let first = world.spawn(viewport("material:p#view:1", 1.0)).id();
    let second = world.spawn(viewport("material:p#view:2", 1.0)).id();
    world.run_system_once(restore_graph_viewports).unwrap();
    for (entity, pan, zoom) in [
        (first, Vec2::new(20.0, 40.0), 0.5),
        (second, Vec2::new(-80.0, 10.0), 1.5),
    ] {
        let restored = world.get::<FeathersGraphViewport>(entity).unwrap();
        assert_eq!(restored.pan, pan);
        assert_eq!(restored.zoom, zoom);
        assert!(restored.frame_request.is_none());
    }
    assert_eq!(
        world
            .resource::<GraphViewportMemory>()
            .node("material:p", "expression:n"),
        Some((Vec2::new(90.0, 30.0), true))
    );
}

#[test]
fn pointer_drag_uses_own_view_zoom_and_inverse_ui_scale_without_semantic_edits() {
    for inverse_scale in [1.0, 0.8, 0.5] {
        for zoom in [0.5, 1.0, 1.75] {
            let mut app = App::new();
            let session = crate::test_support::session_with_timing_slack();
            let effect = session.effect.clone();
            let revision = session.document_revision();
            let undo_len = session.effect_undo_len();
            app.insert_resource(session)
                .init_resource::<GraphViewportMemory>()
                .init_resource::<OverrideCursor>()
                .init_resource::<drag_assist::State>()
                .init_resource::<GraphNodeDragGesture>()
                .add_observer(drag_graph_node);
            // The other view deliberately has a different zoom. The node key is per document,
            // so matching the key instead of walking the hierarchy would give the wrong scale.
            app.world_mut().spawn(viewport("material:p#view:1", 0.25));
            let owner = app
                .world_mut()
                .spawn(viewport("material:p#view:2", zoom))
                .id();
            let mut graph_node = node("material:p", "expression:n");
            graph_node.position = Vec2::new(90.0, 30.0);
            graph_node.begin_drag();
            let entity = app
                .world_mut()
                .spawn((
                    graph_node,
                    Node::default(),
                    ComputedNode {
                        inverse_scale_factor: inverse_scale,
                        ..default()
                    },
                    ChildOf(owner),
                ))
                .id();
            {
                let mut gesture = app.world_mut().resource_mut::<GraphNodeDragGesture>();
                gesture.active = Some(entity);
                gesture.members.insert(entity);
            }
            let delta = Vec2::new(35.0, -21.0);
            app.world_mut().trigger(Pointer::new(
                PointerId::Mouse,
                Location {
                    target: bevy::camera::NormalizedRenderTarget::None {
                        width: 800,
                        height: 600,
                    },
                    position: Vec2::new(100.0, 120.0),
                },
                Drag {
                    button: PointerButton::Primary,
                    distance: delta,
                    delta,
                },
                entity,
            ));
            let expected = Vec2::new(90.0, 30.0) + delta * inverse_scale / zoom;
            assert!(
                app.world()
                    .get::<FeathersGraphNode>(entity)
                    .unwrap()
                    .position
                    .distance(expected)
                    < 0.001
            );
            let saved = app
                .world()
                .resource::<GraphViewportMemory>()
                .node("material:p", "expression:n")
                .unwrap();
            assert!(saved.0.distance(expected) < 0.001);
            assert!(!saved.1);
            assert_eq!(
                app.world().resource::<crate::EditorSession>().effect,
                effect
            );
            let session = app.world().resource::<crate::EditorSession>();
            assert_eq!(session.document_revision(), revision);
            assert_eq!(session.effect_undo_len(), undo_len);
        }
    }
}

#[derive(Resource, Default)]
struct Edits(Vec<GraphPresentationEdit>);

#[test]
fn drag_emits_one_base_placement_transaction_and_sibling_views_follow_undo() {
    let mut app = App::new();
    app.init_resource::<GraphViewportMemory>()
        .init_resource::<OverrideCursor>()
        .init_resource::<drag_assist::State>()
        .init_resource::<GraphNodeDragGesture>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<Edits>()
        .add_observer(begin_graph_node_drag)
        .add_observer(drag_graph_node)
        .add_observer(end_graph_node_drag)
        .add_observer(
            |event: On<GraphPresentationEdit>, mut edits: ResMut<Edits>| {
                edits.0.push(event.event().clone());
            },
        );
    let base = Vec2::new(100.0, 20.0);
    let graph = "function:test:f";
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node(graph, "outputs", base, false);
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_temporary_offset(graph, "outputs", Vec2::splat(50.0));
    let owner = app
        .world_mut()
        .spawn(viewport("function:test:f#view:2", 2.0))
        .id();
    let entity = app
        .world_mut()
        .spawn((
            node(graph, "outputs"),
            Node::default(),
            ComputedNode::default(),
            ChildOf(owner),
        ))
        .id();
    let sibling = app
        .world_mut()
        .spawn((node(graph, "outputs"), Node::default()))
        .id();
    app.world_mut()
        .run_system_once(restore_graph_nodes)
        .unwrap();
    let location = Location {
        target: bevy::camera::NormalizedRenderTarget::None {
            width: 800,
            height: 600,
        },
        position: Vec2::ZERO,
    };
    app.world_mut().trigger(Pointer::new(
        PointerId::Mouse,
        location.clone(),
        DragStart {
            button: PointerButton::Primary,
            hit: bevy::picking::backend::HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        },
        entity,
    ));
    for _ in 0..20 {
        app.world_mut().trigger(Pointer::new(
            PointerId::Mouse,
            location.clone(),
            Drag {
                button: PointerButton::Primary,
                distance: Vec2::splat(10.0),
                delta: Vec2::splat(10.0),
            },
            entity,
        ));
    }
    assert!(app.world().resource::<Edits>().0.is_empty());
    app.world_mut().trigger(Pointer::new(
        PointerId::Mouse,
        location,
        DragEnd {
            button: PointerButton::Primary,
            distance: Vec2::splat(200.0),
        },
        entity,
    ));
    app.world_mut().flush();
    let edits = &app.world().resource::<Edits>().0;
    assert_eq!(edits.len(), 1);
    assert_eq!(
        edits[0].before,
        (base, false),
        "Undo stores base, not temporary displacement"
    );
    assert_eq!(edits[0].after, (base + Vec2::splat(150.0), false));
    // The same shared memory path used by Undo updates both views without changing cameras.
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node(graph, "outputs", base, false);
    app.world_mut()
        .run_system_once(sync_graph_nodes_from_memory)
        .unwrap();
    for entity in [entity, sibling] {
        assert_eq!(
            app.world()
                .get::<FeathersGraphNode>(entity)
                .unwrap()
                .position(),
            base
        );
    }
}
