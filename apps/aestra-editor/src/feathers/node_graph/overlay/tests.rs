use super::*;
use crate::{docking::EditorViewId, document::DocumentKey};
use aestra_core::{MaterialExpressionId, MaterialFunctionId, MaterialProgramId};

struct Surface {
    viewport: Entity,
    nodes: Vec<Entity>,
    bodies: Vec<Entity>,
    previews: Vec<Entity>,
}

fn surface(
    app: &mut App,
    root: Entity,
    document: &GraphDocumentKey,
    view: u64,
    preview: bool,
) -> Surface {
    let mut result = Surface {
        viewport: Entity::PLACEHOLDER,
        nodes: Vec::new(),
        bodies: Vec::new(),
        previews: Vec::new(),
    };
    let keys = [
        GraphNodeKey::Expression(MaterialExpressionId::from_u128(1)),
        if matches!(document.asset, DocumentKey::MaterialFunction(_)) {
            GraphNodeKey::FunctionOutputs
        } else {
            GraphNodeKey::MaterialOutputs
        },
    ];
    app.world_mut()
        .commands()
        .entity(root)
        .with_children(|parent| {
            result.viewport = spawn_graph_viewport(
                parent,
                GraphViewportProps {
                    key: format!("test#view:{view}"),
                    content_size: Vec2::new(720.0, 420.0),
                    selection_bounds: None,
                    initial_view: Some((Vec2::new(20.0, 30.0), if view == 1 { 0.5 } else { 1.75 })),
                },
                (),
                |_| {},
                |canvas| {
                    for (index, key) in keys.iter().enumerate() {
                        let node = spawn_graph_node(
                            canvas,
                            GraphNodeProps {
                                graph_key: "test".into(),
                                node_key: index.to_string(),
                                title: "Node".into(),
                                position: Vec2::new(0.0, index as f32 * 120.0),
                                selected: false,
                                pinned: false,
                                muted: false,
                                collapse_icon: default(),
                                expand_icon: default(),
                                pin_icon: default(),
                                collapse_label: "Collapse".into(),
                                expand_label: "Expand".into(),
                            },
                            GraphGeometryNode::new(*key, &"body", preview && index == 0),
                            |_, body| {
                                result.bodies.push(body.target_entity());
                                spawn_graph_port(
                                    body,
                                    GraphPortProps {
                                        label: Some("Value".into()),
                                        tooltip_title: "Value".into(),
                                        tooltip_description: "".into(),
                                        side: GraphSocketSide::Input,
                                        color: Color::WHITE,
                                    },
                                    (),
                                );
                                if preview && index == 0 {
                                    result.previews.push(spawn_graph_node_preview(body, ()));
                                }
                            },
                        );
                        result.nodes.push(node);
                    }
                },
            );
            parent
                .commands()
                .entity(result.viewport)
                .insert(GraphGeometryView {
                    key: GraphViewKey {
                        document: document.clone(),
                        view: Some(EditorViewId(view)),
                    },
                    nodes: keys.into_iter().collect(),
                });
        });
    app.world_mut().flush();
    result
}

fn toggle(app: &mut App, surface: &mut Surface, visible: bool) {
    if visible {
        app.world_mut()
            .commands()
            .entity(surface.bodies[0])
            .with_children(|body| surface.previews.push(spawn_graph_node_preview(body, ())));
    } else {
        for entity in surface.previews.drain(..) {
            app.world_mut().despawn(entity);
        }
    }
    app.world_mut()
        .entity_mut(surface.nodes[0])
        .insert(GraphGeometryNode::new(
            GraphNodeKey::Expression(MaterialExpressionId::from_u128(1)),
            &"body",
            visible,
        ));
    app.world_mut().flush();
}

fn advance(app: &mut App) {
    for _ in 0..8 {
        app.update();
    }
}

fn app(scale: f32) -> (App, Entity) {
    let (mut app, root) = geometry::tests::layout_app(scale);
    app.init_resource::<GraphOverlays>().add_systems(
        Update,
        reconcile
            .after(restore_graph_nodes)
            .before(sync_graph_nodes_from_memory),
    );
    (app, root)
}

#[test]
fn actual_shared_widget_restores_previews_across_views_zoom_and_dpi() {
    for scale in [1.0, 1.25, 2.0] {
        for asset in [
            DocumentKey::MaterialProgram(MaterialProgramId::from_u128(1)),
            DocumentKey::MaterialFunction(MaterialFunctionId::from_u128(2)),
        ] {
            let (mut app, root) = app(scale);
            let document = GraphDocumentKey {
                project: "project".into(),
                asset,
            };
            let mut first = surface(&mut app, root, &document, 1, false);
            let mut second = surface(&mut app, root, &document, 2, false);
            advance(&mut app);
            let bases = app
                .world()
                .resource::<GraphViewportMemory>()
                .base_nodes("test");
            toggle(&mut app, &mut first, true);
            toggle(&mut app, &mut second, true);
            advance(&mut app);
            let memory = app.world().resource::<GraphViewportMemory>();
            let shifted = memory.node_position("test", "1").unwrap();
            assert!(shifted.y > 120.0, "{scale}: {shifted:?}");
            assert_eq!(memory.base_nodes("test"), bases);
            let snapshot = app
                .world()
                .resource::<GraphGeometryRegistry>()
                .snapshot(&document)
                .unwrap();
            let source =
                &snapshot.nodes[&GraphNodeKey::Expression(MaterialExpressionId::from_u128(1))];
            assert!(shifted.y + 0.01 >= source.size.y + 22.0);
            assert!((source.size.y - source.compact_size.y - 216.0).abs() <= 0.5);
            for surface in [&first, &second] {
                assert_eq!(
                    app.world()
                        .get::<FeathersGraphNode>(surface.nodes[1])
                        .unwrap()
                        .position,
                    shifted
                );
            }
            let cameras = [first.viewport, second.viewport].map(|id| {
                let v = app.world().get::<FeathersGraphViewport>(id).unwrap();
                (v.pan, v.zoom)
            });
            assert_ne!(cameras[0], cameras[1]);
            // Owner closure/remeasurement must not stack another copy of the offsets.
            app.world_mut()
                .get_mut::<Node>(first.viewport)
                .unwrap()
                .display = Display::None;
            advance(&mut app);
            assert_eq!(
                app.world()
                    .resource::<GraphViewportMemory>()
                    .node_position("test", "1"),
                Some(shifted)
            );
            app.world_mut()
                .get_mut::<Node>(first.viewport)
                .unwrap()
                .display = Display::Flex;
            toggle(&mut app, &mut first, false);
            toggle(&mut app, &mut second, false);
            advance(&mut app);
            assert_eq!(
                app.world()
                    .resource::<GraphViewportMemory>()
                    .node_position("test", "1"),
                Some(Vec2::new(0.0, 120.0))
            );
            assert_eq!(
                app.world()
                    .resource::<GraphViewportMemory>()
                    .base_nodes("test"),
                bases
            );
        }
    }
}

#[test]
fn rebuilt_preview_reconstructs_once_and_manual_placement_survives_close() {
    let (mut app, root) = app(1.0);
    let document = GraphDocumentKey {
        project: "project".into(),
        asset: DocumentKey::MaterialProgram(MaterialProgramId::from_u128(1)),
    };
    let mut first = surface(&mut app, root, &document, 1, true);
    advance(&mut app);
    let shifted = app
        .world()
        .resource::<GraphViewportMemory>()
        .node_position("test", "1")
        .unwrap();
    assert!(shifted.y > 120.0);
    app.world_mut().despawn(first.viewport);
    first = surface(&mut app, root, &document, 1, true);
    advance(&mut app);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node_position("test", "1"),
        Some(shifted)
    );
    // Reproduce history's offset-invalidation seam while the preview remains open.
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .clear_offsets("test");
    advance(&mut app);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node_position("test", "1"),
        Some(shifted)
    );
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node("test", "1", Vec2::new(350.0, 350.0), false);
    advance(&mut app);
    toggle(&mut app, &mut first, false);
    advance(&mut app);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node_position("test", "1"),
        Some(Vec2::new(350.0, 350.0))
    );
}

#[test]
fn project_memory_reset_discards_overlay_journals() {
    let (mut app, root) = app(1.0);
    let document = GraphDocumentKey {
        project: "project".into(),
        asset: DocumentKey::MaterialProgram(MaterialProgramId::from_u128(1)),
    };
    let first = surface(&mut app, root, &document, 1, true);
    advance(&mut app);
    assert_eq!(app.world().resource::<GraphOverlays>().documents.len(), 1);
    app.world_mut().despawn(first.viewport);
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .retain_graphs(|_| false);
    advance(&mut app);
    assert!(app.world().resource::<GraphOverlays>().documents.is_empty());
    let other = GraphDocumentKey {
        project: "other-project".into(),
        asset: document.asset,
    };
    let second = surface(&mut app, root, &other, 1, false);
    advance(&mut app);
    assert!(
        !app.world()
            .resource::<GraphOverlays>()
            .documents
            .contains_key(&document)
    );
    assert!(
        app.world()
            .resource::<GraphViewportMemory>()
            .offsets
            .is_empty()
    );
    app.world_mut().despawn(second.viewport);
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .retain_graphs(|_| false);
    let _returned = surface(&mut app, root, &document, 1, true);
    advance(&mut app);
    assert!(
        !app.world()
            .resource::<GraphOverlays>()
            .documents
            .contains_key(&other)
    );
    assert!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node_position("test", "1")
            .unwrap()
            .y
            > 120.0
    );
}

#[test]
fn visible_preview_disk_roundtrip_keeps_bases_and_reconstructs_offsets() {
    use aestra_project::{
        MaterialGraphLayoutMetadata, MaterialGraphNodeLayout, ProjectEditorLayout,
    };
    let root_dir = tempfile::tempdir().unwrap();
    let id = MaterialProgramId::from_u128(1);
    let expression = MaterialExpressionId::from_u128(1);
    let document = GraphDocumentKey {
        project: root_dir.path().to_owned(),
        asset: DocumentKey::MaterialProgram(id),
    };
    let (mut first, root) = app(1.0);
    let _surface = surface(&mut first, root, &document, 1, true);
    advance(&mut first);
    let memory = first.world().resource::<GraphViewportMemory>();
    let expected = memory.node_position("test", "1").unwrap();
    let layout_node = |key| {
        let (position, collapsed) = memory.node("test", key).unwrap();
        MaterialGraphNodeLayout {
            position: position.to_array(),
            collapsed,
            pinned: memory.is_pinned("test", key),
        }
    };
    let mut saved = ProjectEditorLayout::default();
    saved.material_graphs.insert(
        id,
        MaterialGraphLayoutMetadata {
            nodes: BTreeMap::from([(expression, layout_node("0"))]),
            output: Some(layout_node("1")),
            visible_previews: BTreeSet::from([expression]),
            ..default()
        },
    );
    saved.save(root_dir.path()).unwrap();
    let restored = ProjectEditorLayout::load(root_dir.path()).unwrap();
    let layout = &restored.material_graphs[&id];
    assert_eq!(layout.output.unwrap().position, [0.0, 120.0]);
    let (mut reopened, root) = app(2.0);
    for (key, node) in [
        ("0", layout.nodes[&expression]),
        ("1", layout.output.unwrap()),
    ] {
        reopened
            .world_mut()
            .resource_mut::<GraphViewportMemory>()
            .set_node("test", key, Vec2::from_array(node.position), node.collapsed);
    }
    let _surface = surface(
        &mut reopened,
        root,
        &document,
        1,
        layout.visible_previews.contains(&expression),
    );
    advance(&mut reopened);
    assert!(
        (reopened
            .world()
            .resource::<GraphViewportMemory>()
            .node_position("test", "1")
            .unwrap()
            - expected)
            .length()
            <= 0.5
    );
    assert_eq!(
        reopened
            .world()
            .resource::<GraphViewportMemory>()
            .node("test", "1")
            .unwrap()
            .0,
        Vec2::new(0.0, 120.0)
    );
}

#[test]
fn function_body_expansion_uses_real_collapsed_bounds_without_tidying_initial_layout() {
    let (mut app, root) = app(1.25);
    let document = GraphDocumentKey {
        project: "project".into(),
        asset: DocumentKey::MaterialFunction(MaterialFunctionId::from_u128(3)),
    };
    let _surface = surface(&mut app, root, &document, 1, false);
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_node("test", "1", Vec2::new(0.0, 74.0), false);
    advance(&mut app);
    assert!(
        app.world()
            .resource::<GraphViewportMemory>()
            .offsets
            .is_empty()
    );
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_collapsed("test", "0", Vec2::ZERO, true);
    advance(&mut app);
    assert!(
        app.world()
            .resource::<GraphViewportMemory>()
            .offsets
            .is_empty()
    );
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_collapsed("test", "0", Vec2::ZERO, false);
    advance(&mut app);
    assert!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node_position("test", "1")
            .unwrap()
            .y
            > 74.0
    );
    app.world_mut()
        .resource_mut::<GraphViewportMemory>()
        .set_collapsed("test", "0", Vec2::ZERO, true);
    advance(&mut app);
    assert_eq!(
        app.world()
            .resource::<GraphViewportMemory>()
            .node_position("test", "1"),
        Some(Vec2::new(0.0, 74.0))
    );
}
