use super::*;
use crate::history::asset_order::{Context, EditOrder};
use crate::material_target::MaterialEditingTarget;
use aestra_core::MaterialExpressionId;
use std::path::Path;

fn snapshot(graph: &str, position: Vec2) -> Snapshot {
    Snapshot {
        root: "project".into(),
        generation: 3,
        graph: graph.into(),
        stamp: 11,
        keys: BTreeSet::from(["node".into()]),
        nodes: BTreeMap::from([("node".into(), (position, false))]),
        pinned: BTreeSet::new(),
        order_serial: 1,
    }
}

fn transaction(graph: &str) -> Transaction {
    Transaction {
        before: snapshot(graph, Vec2::ZERO),
        after: snapshot(graph, Vec2::new(20.0, 30.0)),
        previews: None,
        invalidated: false,
    }
}

fn memory(snapshot: &Snapshot) -> GraphViewportMemory {
    let mut memory = GraphViewportMemory::default();
    for (key, (position, collapsed)) in &snapshot.nodes {
        memory.set_node(&snapshot.graph, key, *position, *collapsed);
    }
    for key in &snapshot.pinned {
        memory.set_pinned(&snapshot.graph, key, true);
    }
    memory
}

fn validate(
    transaction: &Transaction,
    undo: bool,
    memory: &GraphViewportMemory,
) -> Result<(), String> {
    let expected = if undo {
        &transaction.after
    } else {
        &transaction.before
    };
    transaction.validate_layout(
        undo,
        &expected.root,
        expected.generation,
        |graph| {
            assert_eq!(graph, expected.graph);
            Some(expected.stamp)
        },
        memory,
    )
}

#[test]
fn layout_round_trip_restores_positions_collapse_and_pins_without_touching_other_graphs() {
    let mut transaction = transaction("graph");
    transaction.after.nodes.get_mut("node").unwrap().1 = true;
    transaction.after.pinned.insert("node".into());
    let mut memory = memory(&transaction.after);
    memory.set_node("graph", "unrelated", Vec2::splat(90.0), false);
    memory.set_node("other", "node", Vec2::splat(70.0), true);
    memory.set_view("graph", Vec2::splat(5.0), 1.5);
    memory.set_temporary_offset("graph", "node", Vec2::ONE);
    memory.set_temporary_offset("other", "node", Vec2::ONE);
    let mut previews = Visibility::new();
    validate(&transaction, true, &memory).unwrap();
    transaction.apply_layout(true, &mut memory, &mut previews);
    assert_eq!(memory.node("graph", "node"), Some((Vec2::ZERO, false)));
    assert!(!memory.is_pinned("graph", "node"));
    assert_eq!(memory.node_position("graph", "node"), Some(Vec2::ZERO));
    assert_eq!(
        memory.node("graph", "unrelated"),
        Some((Vec2::splat(90.0), false))
    );
    assert_eq!(
        memory.node_position("other", "node"),
        Some(Vec2::splat(71.0))
    );
    assert_eq!(memory.view("graph"), Some((Vec2::splat(5.0), 1.5)));
    validate(&transaction, false, &memory).unwrap();
    transaction.apply_layout(false, &mut memory, &mut previews);
    assert_eq!(
        memory.node("graph", "node"),
        Some((Vec2::new(20.0, 30.0), true))
    );
    assert!(memory.is_pinned("graph", "node"));
    assert_eq!(transaction.before.keys, BTreeSet::from(["node".into()]));
    assert_eq!(transaction.before.order_serial, 1);
}

#[test]
fn compound_created_node_is_removed_on_undo_and_restored_on_redo() {
    let mut transaction = transaction("graph");
    transaction.before.nodes.clear();
    transaction.before.keys.clear();
    transaction.after.pinned.insert("node".into());
    let mut memory = memory(&transaction.after);
    let mut previews = Visibility::new();
    validate(&transaction, true, &memory).unwrap();
    transaction.apply_layout(true, &mut memory, &mut previews);
    assert_eq!(memory.node("graph", "node"), None);
    assert!(!memory.is_pinned("graph", "node"));
    validate(&transaction, false, &memory).unwrap();
    transaction.apply_layout(false, &mut memory, &mut previews);
    assert_eq!(
        memory.node("graph", "node"),
        transaction.after.nodes.get("node").copied()
    );
    assert!(memory.is_pinned("graph", "node"));
}

#[test]
fn stale_root_generation_semantics_and_invalidation_reject_without_mutation() {
    let mut transaction = transaction("graph");
    let memory = memory(&transaction.after);
    let revision = memory.placement_revision("graph");
    for (root, generation, stamp) in [
        (Path::new("another"), 3, Some(11)),
        (Path::new("project"), 4, Some(11)),
        (Path::new("project"), 3, Some(12)),
        (Path::new("project"), 3, None),
    ] {
        assert!(
            transaction
                .validate_layout(true, root, generation, |_| stamp, &memory)
                .is_err()
        );
        assert_eq!(memory.placement_revision("graph"), revision);
        assert_eq!(
            memory.node("graph", "node"),
            transaction.after.nodes.get("node").copied()
        );
    }
    transaction.invalidate(Some("other"));
    validate(&transaction, true, &memory).unwrap();
    transaction.invalidate(Some("graph"));
    assert!(validate(&transaction, true, &memory).is_err());
    assert_eq!(memory.placement_revision("graph"), revision);
}

#[test]
fn manual_placement_pinning_and_nonfinite_deltas_reject_before_apply() {
    let mut transaction = transaction("graph");
    let mut memory = memory(&transaction.after);
    memory.set_node("graph", "node", Vec2::ONE, false);
    assert!(
        validate(&transaction, true, &memory)
            .unwrap_err()
            .contains("Layout changed")
    );
    memory.set_node("graph", "node", Vec2::new(20.0, 30.0), false);
    memory.set_pinned("graph", "node", true);
    assert!(
        validate(&transaction, true, &memory)
            .unwrap_err()
            .contains("pinning changed")
    );
    memory.set_pinned("graph", "node", false);
    transaction.before.nodes.get_mut("node").unwrap().0.x = f32::NAN;
    assert_eq!(
        validate(&transaction, true, &memory).unwrap_err(),
        "Invalid layout position"
    );
}

#[test]
fn preview_delta_validates_availability_and_round_trips_only_its_program() {
    let mut transaction = transaction("graph");
    let program = MaterialProgramId::from_u128(1);
    let other = MaterialProgramId::from_u128(2);
    let expression = MaterialGraphPreviewTarget::Expression(MaterialExpressionId::from_u128(3));
    let output = MaterialGraphPreviewTarget::Output;
    transaction.previews = Some((
        program,
        BTreeSet::from([expression]),
        BTreeSet::from([output]),
    ));
    let mut previews = Visibility::from([(program, output), (other, expression)]);
    assert!(transaction.validate_previews(true, None).is_err());
    assert!(
        transaction
            .validate_previews(false, Some(&previews))
            .is_err()
    );
    transaction
        .validate_previews(true, Some(&previews))
        .unwrap();
    let mut memory = memory(&transaction.after);
    transaction.apply_layout(true, &mut memory, &mut previews);
    assert_eq!(
        visible_targets(&previews, program),
        BTreeSet::from([expression])
    );
    assert!(previews.contains(&(other, expression)));
    transaction
        .validate_previews(false, Some(&previews))
        .unwrap();
    transaction.apply_layout(false, &mut memory, &mut previews);
    assert_eq!(
        visible_targets(&previews, program),
        BTreeSet::from([output])
    );
    transaction.previews = None;
    transaction.validate_previews(true, None).unwrap();
}

#[test]
fn semantic_layout_attachment_is_one_ordered_item_across_undo_and_redo() {
    let mut order = EditOrder::default();
    let context = Context::Material(MaterialEditingTarget::Program {
        root: "project".into(),
        id: MaterialProgramId::from_u128(1),
    });
    let mut transaction = transaction("graph");
    transaction.before.stamp = 10;
    order.record(context.clone());
    order.attach_layout(context.clone(), transaction.clone());
    assert_eq!(
        order.edit_serial(),
        1,
        "attachment must not create another undo item"
    );
    let mut memory = memory(&transaction.after);
    let mut previews = Visibility::new();
    for undo in [true, false] {
        let (layout, semantic) = order.layout(&context, undo).unwrap();
        assert!(semantic);
        validate(layout, undo, &memory).unwrap();
        let layout = layout.clone();
        // The host performs its semantic history step before applying the attached presentation.
        order.step(context.clone(), undo);
        layout.apply_layout(undo, &mut memory, &mut previews);
        assert!(order.layout(&context, undo).is_none());
        assert!(order.layout(&context, !undo).is_some());
    }
    assert_eq!(
        memory.node("graph", "node"),
        transaction.after.nodes.get("node").copied()
    );
    assert_eq!(order.edit_serial(), 1);
}

#[test]
fn layout_only_items_and_wrong_context_attachment_keep_semantic_order_separate() {
    let mut order = EditOrder::default();
    order.attach_layout(Context::Effect, transaction("graph"));
    assert!(order.layout(&Context::Effect, true).is_none());
    order.record(Context::Effect);
    let other = Context::Material(MaterialEditingTarget::Program {
        root: "project".into(),
        id: MaterialProgramId::from_u128(2),
    });
    order.attach_layout(other.clone(), transaction("other"));
    assert!(order.layout(&Context::Effect, true).is_none());
    order.record_layout(other.clone(), transaction("other"));
    assert!(!order.layout(&other, true).unwrap().1);
    order.attach_layout(other.clone(), transaction("replacement"));
    assert_eq!(order.layout(&other, true).unwrap().0.before.graph, "other");
    assert_eq!(order.edit_serial(), 2);
}

#[test]
fn invalidating_layouts_checks_applied_and_redo_entries_without_cross_graph_damage() {
    let mut order = EditOrder::default();
    let other = Context::Material(MaterialEditingTarget::Program {
        root: "project".into(),
        id: MaterialProgramId::from_u128(2),
    });
    order.record_layout(Context::Effect, transaction("graph"));
    order.record_layout(other.clone(), transaction("other"));
    order.step(Context::Effect, true);
    order.invalidate_layouts(Some("graph"));
    assert!(order.layout(&Context::Effect, false).unwrap().0.invalidated);
    assert!(!order.layout(&other, true).unwrap().0.invalidated);
    order.invalidate_layouts(None);
    assert!(order.layout(&other, true).unwrap().0.invalidated);
}
