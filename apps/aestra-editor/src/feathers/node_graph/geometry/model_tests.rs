use super::*;
use aestra_core::MaterialProgramId;

/// Feed measured observations to the real registry; this is not a UI collector substitute.
pub(crate) fn measured_view(
    registry: &mut GraphGeometryRegistry,
    key: GraphViewKey,
    entity: Entity,
    nodes: Vec<(Entity, GraphNodeKey, GraphNodeGeometry)>,
) {
    let nodes = nodes
        .into_iter()
        .map(|(entity, key, geometry)| {
            (
                key,
                NodeObservation {
                    entity,
                    content: geometry.content,
                    preview: geometry.preview,
                    collapsed: geometry.collapsed,
                    inverse_scale: 1.0,
                    geometry,
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let observation = ViewObservation {
        entity,
        generation: Some(DocumentId(1)),
        expected: nodes.keys().copied().collect(),
        nodes,
        complete: true,
    };
    settle(registry, &[(key, observation)], None);
}

pub(crate) fn key(project: &str, view: u64) -> GraphViewKey {
    GraphViewKey {
        document: GraphDocumentKey {
            project: project.into(),
            asset: DocumentKey::MaterialProgram(MaterialProgramId::from_u128(1)),
        },
        view: Some(EditorViewId(view)),
    }
}

fn raw() -> ViewObservation {
    ViewObservation {
        entity: Entity::from_bits(1),
        generation: Some(DocumentId(1)),
        expected: BTreeSet::from([GraphNodeKey::MaterialOutputs]),
        complete: true,
        nodes: BTreeMap::from([(
            GraphNodeKey::MaterialOutputs,
            NodeObservation {
                entity: Entity::from_bits(2),
                content: 1,
                preview: false,
                collapsed: false,
                inverse_scale: 1.0,
                geometry: GraphNodeGeometry {
                    effective_position: Vec2::new(50.0, 30.0),
                    size: Vec2::new(224.0, 80.0),
                    compact_size: Vec2::new(224.0, 80.0),
                    preview: false,
                    collapsed: false,
                    content: 1,
                    ports: vec![],
                    geometry_revision: 0,
                },
            },
        )]),
    }
}

fn settle(
    registry: &mut GraphGeometryRegistry,
    views: &[(GraphViewKey, ViewObservation)],
    focused: Option<EditorViewId>,
) {
    for _ in 0..STABLE_FRAMES {
        registry.observe(views.iter().cloned().collect(), focused);
    }
}

#[test]
fn stable_preview_resize_emits_once_and_never_moves_a_node() {
    let key = key("project", 1);
    let mut raw = raw();
    let mut registry = GraphGeometryRegistry::default();
    settle(&mut registry, &[(key.clone(), raw.clone())], None);
    assert!(registry.changes().is_empty());
    let before = registry.snapshot(&key.document).unwrap().nodes[&GraphNodeKey::MaterialOutputs]
        .effective_position;
    for (preview, height, reason) in [
        (true, 300.0, GraphResizeReason::PreviewOpened),
        (false, 80.0, GraphResizeReason::PreviewClosed),
    ] {
        let node = raw.nodes.get_mut(&GraphNodeKey::MaterialOutputs).unwrap();
        node.preview = preview;
        node.geometry.size.y = height;
        registry.observe(BTreeMap::from([(key.clone(), raw.clone())]), None);
        assert!(registry.snapshot(&key.document).is_none());
        assert!(registry.changes().is_empty());
        registry.observe(BTreeMap::from([(key.clone(), raw.clone())]), None);
        assert!(
            matches!(registry.changes(), [GraphGeometryEvent { change: GraphGeometryChange::Resized { reason: actual, .. }, .. }] if *actual == reason)
        );
        let snapshot = registry.snapshot(&key.document).unwrap();
        assert_eq!(
            snapshot.nodes[&GraphNodeKey::MaterialOutputs].effective_position,
            before
        );
        assert_eq!(
            snapshot.geometry_revision,
            registry.changes()[0].geometry_revision
        );
        registry.observe(BTreeMap::from([(key.clone(), raw.clone())]), None);
        assert!(registry.changes().is_empty());
    }
}

#[test]
fn move_insert_remove_and_collapse_are_coalesced_after_stability() {
    let key = key("project", 1);
    let mut raw = raw();
    let mut registry = GraphGeometryRegistry::default();
    settle(&mut registry, &[(key.clone(), raw.clone())], None);
    let node = raw.nodes.get_mut(&GraphNodeKey::MaterialOutputs).unwrap();
    node.geometry.effective_position.x += 40.0;
    node.geometry.size.y = 30.0;
    node.collapsed = true;
    settle(&mut registry, &[(key.clone(), raw.clone())], None);
    assert_eq!(registry.changes().len(), 2);
    assert!(matches!(
        registry.changes()[0].change,
        GraphGeometryChange::Moved { .. }
    ));
    assert!(matches!(
        registry.changes()[1].change,
        GraphGeometryChange::Resized {
            reason: GraphResizeReason::Collapsed,
            ..
        }
    ));
    let added = GraphNodeKey::Expression(MaterialExpressionId::from_u128(4));
    raw.expected.insert(added);
    raw.nodes
        .insert(added, raw.nodes[&GraphNodeKey::MaterialOutputs].clone());
    settle(&mut registry, &[(key.clone(), raw.clone())], None);
    assert!(
        matches!(registry.changes()[0].change, GraphGeometryChange::Inserted { node } if node == added)
    );
    raw.nodes.remove(&added); // Not measured yet is NOT semantic removal.
    settle(&mut registry, &[(key.clone(), raw.clone())], None);
    assert!(registry.snapshot(&key.document).is_none());
    assert!(registry.changes().is_empty());
    raw.expected.remove(&added);
    settle(&mut registry, &[(key, raw)], None);
    assert!(
        matches!(registry.changes()[0].change, GraphGeometryChange::Removed { node } if node == added)
    );
}

#[test]
fn owner_handoff_rebuild_dpi_and_document_generation_are_baselines() {
    let first = key("project", 1);
    let second = key("project", 2);
    let mut registry = GraphGeometryRegistry::default();
    let a = raw();
    let mut b = raw();
    b.nodes
        .get_mut(&GraphNodeKey::MaterialOutputs)
        .unwrap()
        .geometry
        .size
        .y = 120.0;
    settle(
        &mut registry,
        &[(first.clone(), a.clone()), (second.clone(), b.clone())],
        Some(EditorViewId(2)),
    );
    assert_eq!(
        registry.snapshot(&first.document).unwrap().measured_in,
        second
    );
    registry.observe(
        BTreeMap::from([(first.clone(), a.clone()), (second.clone(), b)]),
        None,
    );
    assert_eq!(
        registry.snapshot(&first.document).unwrap().measured_in,
        second
    );
    assert!(registry.changes().is_empty());
    // Closing/hiding the owner falls back to the first valid view, without requesting movement.
    registry.observe(BTreeMap::from([(first.clone(), a.clone())]), None);
    assert_eq!(
        registry.snapshot(&first.document).unwrap().measured_in,
        first
    );
    assert!(registry.view_snapshot(&second).is_none());
    assert!(registry.changes().is_empty());
    let mut rebuilt = a;
    rebuilt.entity = Entity::from_bits(3);
    rebuilt
        .nodes
        .get_mut(&GraphNodeKey::MaterialOutputs)
        .unwrap()
        .geometry
        .size
        .y += 10.0;
    settle(&mut registry, &[(first.clone(), rebuilt.clone())], None);
    assert!(registry.changes().is_empty());
    let node = rebuilt
        .nodes
        .get_mut(&GraphNodeKey::MaterialOutputs)
        .unwrap();
    node.inverse_scale = 0.5;
    node.geometry.size.y += 1.0;
    settle(&mut registry, &[(first.clone(), rebuilt.clone())], None);
    assert!(registry.changes().is_empty());
    rebuilt.generation = Some(DocumentId(2));
    rebuilt
        .nodes
        .get_mut(&GraphNodeKey::MaterialOutputs)
        .unwrap()
        .preview = true;
    rebuilt
        .nodes
        .get_mut(&GraphNodeKey::MaterialOutputs)
        .unwrap()
        .geometry
        .size
        .y = 300.0;
    settle(&mut registry, &[(first.clone(), rebuilt)], None);
    assert!(registry.changes().is_empty());
    registry.observe(BTreeMap::new(), None);
    assert!(registry.snapshot(&first.document).is_none());
    assert!(registry.views.is_empty());
    assert!(registry.documents.is_empty());
}

#[test]
fn same_asset_ids_are_scoped_to_project_and_stale_results_have_different_revisions() {
    let first = key("first", 1);
    let second = key("second", 1);
    let mut registry = GraphGeometryRegistry::default();
    settle(&mut registry, &[(first.clone(), raw())], None);
    let revision = registry
        .snapshot(&first.document)
        .unwrap()
        .geometry_revision;
    settle(&mut registry, &[(second.clone(), raw())], None);
    assert!(registry.snapshot(&first.document).is_none());
    assert!(
        registry
            .snapshot(&second.document)
            .unwrap()
            .geometry_revision
            > revision
    );
    assert!(registry.changes().is_empty());
}

#[test]
fn transient_sizes_and_rounding_noise_do_not_emit_resizes() {
    let key = key("project", 1);
    let mut raw = raw();
    let mut registry = GraphGeometryRegistry::default();
    settle(&mut registry, &[(key.clone(), raw.clone())], None);
    let revision = registry.snapshot(&key.document).unwrap().geometry_revision;
    for noise in [0.25, -0.25, 0.4, 0.0] {
        raw.nodes
            .get_mut(&GraphNodeKey::MaterialOutputs)
            .unwrap()
            .geometry
            .size
            .y = 80.0 + noise;
        registry.observe(BTreeMap::from([(key.clone(), raw.clone())]), None);
        assert_eq!(
            registry.snapshot(&key.document).unwrap().geometry_revision,
            revision
        );
        assert!(registry.changes().is_empty());
    }
    raw.nodes
        .get_mut(&GraphNodeKey::MaterialOutputs)
        .unwrap()
        .geometry
        .size
        .y = 150.0;
    registry.observe(BTreeMap::from([(key.clone(), raw.clone())]), None);
    raw.nodes
        .get_mut(&GraphNodeKey::MaterialOutputs)
        .unwrap()
        .geometry
        .size
        .y = 190.0;
    registry.observe(BTreeMap::from([(key.clone(), raw.clone())]), None);
    assert!(registry.changes().is_empty());
    registry.observe(BTreeMap::from([(key, raw)]), None);
    assert!(
        matches!(registry.changes()[0].change, GraphGeometryChange::Resized { old_size, new_size, .. } if old_size.y == 80.0 && new_size.y == 190.0)
    );
}
