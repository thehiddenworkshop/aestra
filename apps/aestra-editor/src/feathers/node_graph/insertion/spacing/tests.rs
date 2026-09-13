use super::*;
use aestra_core::{MaterialExpressionId, MaterialProgramId};

fn key(id: u128) -> GraphNodeKey {
    GraphNodeKey::Expression(MaterialExpressionId::from_u128(id))
}

fn scene() -> resize::Snapshot<GraphNodeKey, GraphViewKey> {
    resize::Snapshot {
        identity: GraphViewKey {
            document: GraphDocumentKey {
                project: "project".into(),
                asset: crate::document::DocumentKey::MaterialProgram(MaterialProgramId::from_u128(
                    1,
                )),
            },
            view: None,
        },
        revision: default(),
        nodes: [
            (0, 0.0, 0.0, true),   // upstream source
            (1, 150.0, 0.0, true), // inserted node stays at the drop
            (2, 200.0, 0.0, false),
            (3, 330.0, 0.0, false),
            (4, 800.0, 400.0, false), // unrelated intentional overlap
            (5, 800.0, 400.0, false),
        ]
        .into_iter()
        .map(|(id, x, y, frozen)| {
            (
                key(id),
                resize::Node {
                    position: Vec2::new(x, y),
                    size: Vec2::splat(100.0),
                    frozen,
                },
            )
        })
        .collect(),
    }
}

#[test]
fn insertion_spacing_moves_only_the_conflict_chain_deterministically() {
    let before = scene();
    let after = solve(&before, key(1)).unwrap();
    assert_eq!(after.nodes[&key(2)].position, Vec2::new(274.0, 0.0));
    assert_eq!(after.nodes[&key(3)].position, Vec2::new(398.0, 0.0));
    for id in [0, 1, 4, 5] {
        assert_eq!(before.nodes[&key(id)], after.nodes[&key(id)]);
    }
    assert_eq!(before, scene());
    let mut reverse = before.clone();
    reverse.nodes = before.nodes.iter().rev().map(|(k, n)| (*k, *n)).collect();
    assert_eq!(solve(&reverse, key(1)).unwrap(), after);
    assert_eq!(solve(&after, key(1)).unwrap(), after);
}

#[test]
fn insertion_spacing_frozen_conflicts_and_budgets_do_not_produce_partial_moves() {
    let mut snapshot = scene();
    snapshot.nodes.get_mut(&key(2)).unwrap().frozen = true;
    let before = snapshot.clone();
    assert!(
        solve(&snapshot, key(1))
            .unwrap_err()
            .contains("protected node")
    );
    assert_eq!(snapshot, before);
    // Seventeen affected nodes exceeds the insertion-specific sixteen-node budget.
    let mut snapshot = scene();
    snapshot.nodes.retain(|k, _| *k == key(0) || *k == key(1));
    for id in 2..=18 {
        snapshot.nodes.insert(
            key(id),
            resize::Node {
                position: Vec2::new(200.0 + (id - 2) as f32 * 124.0, 0.0),
                size: Vec2::splat(100.0),
                frozen: false,
            },
        );
    }
    let before = snapshot.clone();
    assert!(
        solve(&snapshot, key(1))
            .unwrap_err()
            .contains("limit reached")
    );
    assert_eq!(snapshot, before);
    snapshot.nodes.get_mut(&key(1)).unwrap().size.x = f32::NAN;
    assert!(solve(&snapshot, key(1)).is_err());
}

#[test]
fn insertion_spacing_rejects_stale_candidates_and_preserves_bootstrap_history() {
    let mut snapshot = scene();
    let candidate = resize::solve_insertion(&snapshot, &key(1), limits()).unwrap();
    snapshot.nodes.get_mut(&key(2)).unwrap().position.y += 1.0;
    let changed = snapshot.clone();
    assert_eq!(candidate.apply(&mut snapshot), Err(resize::Conflict::Stale));
    assert_eq!(snapshot, changed);

    let spacing = Spacing::test_move(
        "neighbor",
        None,
        Vec2::new(200.0, 0.0),
        Vec2::new(274.0, 0.0),
    );
    let mut memory = GraphViewportMemory::default();
    spacing.validate("graph", &memory).unwrap();
    spacing.preserve("graph", &mut memory);
    assert_eq!(
        memory.node("graph", "neighbor"),
        Some((Vec2::new(200.0, 0.0), false))
    );
    spacing.rollback("graph", &mut memory);
    assert_eq!(memory.node("graph", "neighbor"), None);
    // Even an untouched bootstrap neighbor must not jump to a new depth column after rewiring.
    let untouched = Spacing {
        moves: vec![],
        seeds: BTreeMap::from([("source".into(), (Vec2::new(10.0, 30.0), true))]),
    };
    assert_eq!(untouched.count(), 0);
    untouched.validate("graph", &memory).unwrap();
    untouched.preserve("graph", &mut memory);
    untouched.apply("graph", &mut memory);
    assert_eq!(
        memory.node("graph", "source"),
        Some((Vec2::new(10.0, 30.0), true))
    );
    untouched.rollback("graph", &mut memory);
    assert_eq!(memory.node("graph", "source"), None);
    memory.set_node("graph", "neighbor", Vec2::new(201.0, 0.0), false);
    assert!(spacing.validate("graph", &memory).is_err());

    let spacing = Spacing::test_move(
        "neighbor",
        memory.node("graph", "neighbor"),
        Vec2::new(201.0, 0.0),
        Vec2::new(274.0, 0.0),
    );
    spacing.validate("graph", &memory).unwrap();
    memory.set_temporary_offset("graph", "neighbor", Vec2::Y);
    assert!(spacing.validate("graph", &memory).is_err());
}

impl Spacing {
    pub(crate) fn test_move(
        key: &str,
        stored: Option<(Vec2, bool)>,
        before: Vec2,
        after: Vec2,
    ) -> Self {
        Self {
            seeds: if stored.is_none() {
                BTreeMap::from([(key.into(), (before, false))])
            } else {
                BTreeMap::new()
            },
            moves: vec![Move {
                key: key.into(),
                stored,
                before: (before, false),
                after,
            }],
        }
    }
}
