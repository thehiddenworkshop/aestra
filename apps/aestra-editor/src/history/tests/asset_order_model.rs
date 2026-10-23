use super::*;
use aestra_core::MaterialProgramId;

fn material(root: &str, id: u128) -> Context {
    Context::Material(MaterialEditingTarget::Program {
        root: root.into(),
        id: MaterialProgramId::from_u128(id),
    })
}

#[test]
fn semantic_edits_and_journal_keep_the_existing_256_entry_bound() {
    let mut order = EditOrder::default();
    for _ in 0..LIMIT + 12 {
        order.record(Context::Effect);
    }
    assert_eq!(order.edit_serial(), (LIMIT + 12) as u64);
    assert_eq!(order.applied.len(), LIMIT);
    assert_eq!(order.edits.len(), LIMIT);
    assert_eq!(order.edits.front().unwrap().0, 13);
    assert!(
        order
            .applied
            .iter()
            .all(|entry| entry.semantic && entry.layout.is_none())
    );
}

#[test]
fn undo_redo_selects_latest_matching_context_not_latest_global_entry() {
    let a = material("root", 1);
    let b = material("root", 2);
    let mut order = EditOrder::default();
    for context in [a.clone(), Context::Effect, a.clone(), b.clone()] {
        order.record(context);
    }
    order.step(a.clone(), true);
    assert_eq!(
        order
            .applied
            .iter()
            .map(|entry| entry.context.clone())
            .collect::<Vec<_>>(),
        [a.clone(), Context::Effect, b.clone()]
    );
    assert_eq!(order.redo.last().unwrap().context, a);
    order.step(Context::Effect, true);
    order.step(a.clone(), false);
    assert_eq!(order.applied.back().unwrap().context, a);
    assert_eq!(order.redo.last().unwrap().context, Context::Effect);
    assert_eq!(
        order.edit_serial(),
        4,
        "stepping does not publish a new edit journal event"
    );
}

#[test]
fn new_edits_invalidate_only_their_own_context_redo_branch() {
    let a = material("root-a", 1);
    let b = material("root-b", 1);
    let mut order = EditOrder::default();
    order.record(a.clone());
    order.record(b.clone());
    order.step(a.clone(), true);
    order.step(b.clone(), true);
    order.record(a);
    assert_eq!(order.redo.len(), 1);
    assert_eq!(order.redo[0].context, b);
    order.clear_redo();
    assert!(order.redo.is_empty());
}

#[test]
fn clearing_one_context_preserves_other_assets_and_monotonic_serial() {
    let a = material("root", 1);
    let b = material("root", 2);
    let mut order = EditOrder::default();
    order.record(a.clone());
    order.record(b.clone());
    order.record(a.clone());
    order.step(a.clone(), true);
    order.clear_context(&a);
    assert_eq!(order.applied.len(), 1);
    assert_eq!(order.applied[0].context, b);
    assert!(order.redo.is_empty());
    assert_eq!(order.edits.len(), 1);
    assert_eq!(order.edit_serial(), 3);
    order.record(Context::Effect);
    assert_eq!(order.edit_serial(), 4);
    assert_eq!(order.edits.back().unwrap().0, 4);
}

#[test]
fn empty_steps_and_cloned_order_do_not_mutate_another_history() {
    let mut order = EditOrder::default();
    order.step(Context::Effect, true);
    order.step(Context::Effect, false);
    assert_eq!(order.edit_serial(), 0);
    order.record(Context::Effect);
    let mut clone = order.clone();
    clone.step(Context::Effect, true);
    assert_eq!(order.applied.len(), 1);
    assert!(order.redo.is_empty());
    assert!(clone.applied.is_empty());
    assert_eq!(clone.redo.len(), 1);
}
