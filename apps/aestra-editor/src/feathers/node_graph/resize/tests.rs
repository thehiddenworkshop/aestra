use super::*;

fn node(x: f32, y: f32, w: f32, h: f32) -> Node {
    Node {
        position: Vec2::new(x, y),
        size: Vec2::new(w, h),
        frozen: false,
    }
}

fn scene(nodes: impl IntoIterator<Item = (u32, Node)>) -> Snapshot<u32, &'static str> {
    Snapshot {
        identity: "project-A/material/view-1",
        revision: Revision::default(),
        nodes: nodes.into_iter().collect(),
    }
}

fn limits() -> Limits {
    Limits {
        spacing: Vec2::splat(10.0),
        ..default_limits()
    }
}

fn default_limits() -> Limits {
    Limits::default()
}

fn roots() -> BTreeMap<u32, Vec2> {
    BTreeMap::from([(0, Vec2::splat(100.0))])
}

fn chain() -> Snapshot<u32, &'static str> {
    scene([
        (0, node(0.0, 0.0, 150.0, 100.0)),
        (1, node(110.0, 0.0, 100.0, 100.0)),
        (2, node(220.0, 0.0, 100.0, 100.0)),
    ])
}

#[test]
fn width_growth_cascades_minimally_right_and_applies_atomically() {
    let mut input = chain();
    let original = input.clone();
    let result = solve(&input, &roots(), limits()).unwrap();
    assert_eq!(input, original);
    assert_eq!(
        *result.positions(),
        BTreeMap::from([(1, Vec2::new(160.0, 0.0)), (2, Vec2::new(270.0, 0.0))])
    );
    assert_eq!(result.stats.iterations, 2);
    result.apply(&mut input).unwrap();
    assert_eq!(input.nodes[&0], original.nodes[&0]);
    assert_eq!(input.revision.placement, 1);
    for a in 0..3 {
        for b in a + 1..3 {
            assert!(!overlaps(&input.nodes[&a], &input.nodes[&b], limits()));
        }
    }
}

#[test]
fn height_growth_preserves_columns_and_unrelated_manual_overlaps() {
    let mut input = scene([
        (0, node(0.0, 0.0, 100.0, 180.0)),
        (1, node(0.0, 110.0, 100.0, 100.0)),
        (2, node(0.0, 220.0, 100.0, 100.0)),
        (3, node(500.0, 0.0, 100.0, 100.0)),
        (4, node(510.0, 10.0, 100.0, 100.0)),
    ]);
    let before = input.clone();
    solve(&input, &roots(), limits())
        .unwrap()
        .apply(&mut input)
        .unwrap();
    assert_eq!(input.nodes[&1].position, Vec2::new(0.0, 190.0));
    assert_eq!(input.nodes[&2].position, Vec2::new(0.0, 300.0));
    assert_eq!(input.nodes[&3], before.nodes[&3]);
    assert_eq!(input.nodes[&4], before.nodes[&4]);
}

#[test]
fn two_axis_growth_chooses_shorter_separation_and_stable_right_tie() {
    for (position, expected) in [
        (Vec2::new(100.0, 130.0), Vec2::new(100.0, 160.0)),
        (Vec2::splat(100.0), Vec2::new(160.0, 100.0)),
    ] {
        let input = scene([
            (0, node(0.0, 0.0, 150.0, 150.0)),
            (
                1,
                Node {
                    position,
                    ..node(0.0, 0.0, 100.0, 100.0)
                },
            ),
        ]);
        assert_eq!(
            solve(&input, &roots(), limits()).unwrap().positions()[&1],
            expected
        );
    }
}

#[test]
fn shrink_no_cause_and_measurement_noise_do_not_tidy() {
    let input = scene([
        (0, node(0.0, 0.0, 100.0, 100.0)),
        (1, node(20.0, 20.0, 100.0, 100.0)),
    ]);
    for old in [
        BTreeMap::new(),
        roots(),
        BTreeMap::from([(0, Vec2::splat(200.0))]),
        BTreeMap::from([(0, Vec2::splat(99.995))]),
    ] {
        let result = solve(&input, &old, limits()).unwrap();
        assert_eq!(result.stats, Stats::default());
        let mut after = input.clone();
        result.apply(&mut after).unwrap();
        assert_eq!(input, after);
    }
}

#[test]
fn frozen_sources_and_neighbors_remain_hard_anchors() {
    let mut input = chain();
    input.nodes.get_mut(&1).unwrap().frozen = true;
    assert_eq!(
        solve(&input, &roots(), limits()).unwrap_err().conflict,
        Conflict::Anchors(0, 1)
    );
    // A later frozen obstacle does not publish the partial move already planned for 1.
    input.nodes.get_mut(&1).unwrap().frozen = false;
    input.nodes.get_mut(&2).unwrap().frozen = true;
    let before = input.clone();
    let result = solve(
        &input,
        &roots(),
        Limits {
            max_node_displacement: 100.0,
            ..limits()
        },
    );
    let failure = result.unwrap_err();
    assert_eq!(failure.stats.iterations, 1);
    assert_eq!(failure.conflict, Conflict::Budget(Budget::NodeDisplacement));
    assert_eq!(input, before);
    // With enough space/budget the movable node may clear the obstacle along its axis.
    let result = solve(&input, &roots(), limits()).unwrap();
    result.apply(&mut input).unwrap();
    assert_eq!(input.nodes[&2], before.nodes[&2]);
    assert_eq!(input.nodes[&0], before.nodes[&0]);
    assert!(!overlaps(&input.nodes[&1], &input.nodes[&2], limits()));
}

#[test]
fn all_coalesced_resize_roots_are_anchored_including_shrinks() {
    let input = chain();
    let mut old = roots();
    old.insert(1, Vec2::splat(200.0));
    assert_eq!(
        solve(&input, &old, limits()).unwrap_err().conflict,
        Conflict::Anchors(0, 1)
    );
    let input = scene([
        (0, node(0.0, 0.0, 150.0, 100.0)),
        (1, node(110.0, 0.0, 100.0, 100.0)),
        (2, node(0.0, 400.0, 150.0, 100.0)),
        (3, node(110.0, 400.0, 100.0, 100.0)),
    ]);
    let old = BTreeMap::from([(0, Vec2::splat(100.0)), (2, Vec2::splat(100.0))]);
    let result = solve(&input, &old, limits()).unwrap();
    assert_eq!(result.positions().len(), 2);
    assert!(!result.positions().contains_key(&0));
    assert!(!result.positions().contains_key(&2));
}

#[test]
fn every_budget_terminates_without_publishing_partial_positions() {
    let input = chain();
    let before = input.clone();
    for (policy, expected) in [
        (
            Limits {
                max_nodes: 2,
                ..limits()
            },
            Budget::Nodes,
        ),
        (
            Limits {
                max_iterations: 1,
                ..limits()
            },
            Budget::Iterations,
        ),
        (
            Limits {
                max_affected: 1,
                ..limits()
            },
            Budget::AffectedNodes,
        ),
        (
            Limits {
                max_pair_checks: 1,
                ..limits()
            },
            Budget::PairChecks,
        ),
        (
            Limits {
                max_node_displacement: 49.0,
                ..limits()
            },
            Budget::NodeDisplacement,
        ),
        (
            Limits {
                max_total_displacement: 99.0,
                ..limits()
            },
            Budget::TotalDisplacement,
        ),
    ] {
        let error = solve(&input, &roots(), policy).unwrap_err();
        assert_eq!(error.conflict, Conflict::Budget(expected));
        assert!(error.stats.iterations <= policy.max_iterations);
        assert!(error.stats.pair_checks <= policy.max_pair_checks);
        assert!(error.stats.affected_nodes <= policy.max_affected);
        assert_eq!(input, before);
    }
}

#[test]
fn invalid_hidden_unmeasured_and_nonfinite_geometry_is_rejected() {
    for bad in [
        node(f32::NAN, 0.0, 100.0, 100.0),
        node(0.0, f32::INFINITY, 100.0, 100.0),
        node(0.0, 0.0, 0.0, 100.0),
        node(0.0, 0.0, -1.0, 100.0),
        node(f32::MAX, 0.0, f32::MAX, 100.0),
    ] {
        assert_eq!(
            solve(&scene([(0, bad)]), &roots(), limits())
                .unwrap_err()
                .conflict,
            Conflict::InvalidNode(0)
        );
    }
    assert_eq!(
        solve(&scene([]), &roots(), limits()).unwrap_err().conflict,
        Conflict::MissingRoot(0)
    );
    assert_eq!(
        solve(&chain(), &BTreeMap::from([(0, Vec2::ZERO)]), limits())
            .unwrap_err()
            .conflict,
        Conflict::InvalidNode(0)
    );
    for policy in [
        Limits {
            spacing: Vec2::splat(-1.0),
            ..limits()
        },
        Limits {
            epsilon: f32::NAN,
            ..limits()
        },
        Limits {
            max_total_displacement: f32::INFINITY,
            ..limits()
        },
    ] {
        assert_eq!(
            solve(&chain(), &roots(), policy).unwrap_err().conflict,
            Conflict::InvalidInput
        );
    }
}

#[test]
fn exact_spacing_and_fractional_negative_coordinates_are_respected() {
    let input = scene([
        (0, node(-500.25, -400.5, 150.0, 100.0)),
        (1, node(-340.25, -400.5, 100.0, 100.0)),
    ]);
    assert!(
        solve(&input, &roots(), limits())
            .unwrap()
            .positions()
            .is_empty()
    );
    let mut input = input;
    input.nodes.get_mut(&1).unwrap().position.x -= 0.25;
    assert_eq!(
        solve(&input, &roots(), limits()).unwrap().positions()[&1],
        Vec2::new(-340.25, -400.5)
    );
}

#[test]
fn stale_revision_identity_geometry_topology_and_manual_edits_reject_atomically() {
    let original = chain();
    for variant in 0..11 {
        let candidate = solve(&original, &roots(), limits()).unwrap();
        let mut current = original.clone();
        match variant {
            0 => current.identity = "project-B/material/view-1",
            1 => current.identity = "project-A/function/view-1",
            2 => current.identity = "project-A/material/view-2",
            3 => current.revision.generation += 1,
            4 => current.revision.topology += 1,
            5 => current.revision.geometry += 1,
            6 => current.revision.placement += 1,
            7 => current.nodes.get_mut(&1).unwrap().position.x += 1.0,
            8 => current.nodes.get_mut(&1).unwrap().size.y += 1.0,
            9 => current.nodes.get_mut(&1).unwrap().frozen = true,
            _ => {
                current.nodes.remove(&2);
            }
        }
        let before = current.clone();
        assert_eq!(candidate.apply(&mut current), Err(Conflict::Stale));
        assert_eq!(current, before);
    }
    let mut current = original;
    current.revision.placement = u64::MAX;
    let before = current.clone();
    assert_eq!(
        solve(&current, &roots(), limits())
            .unwrap()
            .apply(&mut current),
        Err(Conflict::Stale)
    );
    assert_eq!(current, before);
}

#[test]
fn deterministic_independent_of_container_insertion_and_document_kind() {
    let input = chain();
    let expected = solve(&input, &roots(), limits()).unwrap();
    for identity in ["material", "function"] {
        let mut shuffled = scene(input.nodes.iter().rev().map(|(key, node)| (*key, *node)));
        shuffled.identity = identity;
        let result = solve(&shuffled, &roots(), limits()).unwrap();
        assert_eq!(result.positions(), expected.positions());
        assert_eq!(result.stats, expected.stats);
    }
}

#[test]
fn seeded_dense_cases_always_validate_or_fail_within_budget() {
    // A deterministic stress corpus including intentional overlaps and frozen nodes.
    let mut seed = 42_u32;
    for _ in 0..100 {
        let mut input = chain();
        for id in 3..30 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let mut n = node(
                (seed % 500) as f32,
                ((seed >> 10) % 500) as f32,
                100.0,
                100.0,
            );
            n.frozen = seed.is_multiple_of(7);
            input.nodes.insert(id, n);
        }
        match solve(&input, &roots(), limits()) {
            Ok(result) => {
                let changed = result
                    .positions()
                    .keys()
                    .copied()
                    .chain([0])
                    .collect::<Vec<_>>();
                let before = input.clone();
                result.apply(&mut input).unwrap();
                for key in changed {
                    for (other, n) in &input.nodes {
                        if key != *other {
                            assert!(!overlaps(&input.nodes[&key], n, limits()));
                        }
                    }
                }
                for (key, n) in &before.nodes {
                    if n.frozen || *key == 0 {
                        assert_eq!(*n, input.nodes[key]);
                    }
                }
            }
            Err(error) => {
                assert!(error.stats.iterations <= limits().max_iterations);
                assert!(error.stats.pair_checks <= limits().max_pair_checks);
            }
        }
    }
}
