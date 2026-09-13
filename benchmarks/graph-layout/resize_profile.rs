//! Internal editor solver profiling fixture, kept with the benchmark assets.
use super::*;
use std::{hint::black_box, time::Instant};

fn scenario(count: u32, cascade: bool) -> (Snapshot<u32, &'static str>, BTreeMap<u32, Vec2>) {
    let spacing = if cascade { 122.0 } else { 500.0 };
    let nodes = (0..count)
        .map(|key| {
            (
                key,
                Node {
                    position: Vec2::new(key as f32 * spacing, 0.0),
                    size: Vec2::new(if key == 0 { 150.0 } else { 100.0 }, 100.0),
                    frozen: false,
                },
            )
        })
        .collect();
    (
        Snapshot {
            identity: "profile/material/owner",
            revision: Revision::default(),
            nodes,
        },
        BTreeMap::from([(0, Vec2::splat(100.0))]),
    )
}

#[test]
fn resize_operation_counts_are_bounded_at_representative_sizes() {
    for count in [25, 50, 100, 250, 500] {
        let (input, roots) = scenario(count, false);
        let result = solve(&input, &roots, Limits::default()).unwrap();
        assert_eq!(result.stats.pair_checks, count as usize - 1);
        assert_eq!(result.stats.affected_nodes, 0);
        let (input, roots) = scenario(count, true);
        match solve(&input, &roots, Limits::default()) {
            Ok(result) => {
                assert_eq!(result.stats.affected_nodes, count as usize - 1);
                assert!(result.stats.pair_checks <= Limits::default().max_pair_checks);
            }
            Err(failure) => {
                assert!(count > 64);
                assert!(matches!(failure.conflict, Conflict::Budget(_)));
                assert!(failure.stats.pair_checks <= Limits::default().max_pair_checks);
                assert!(failure.stats.iterations <= Limits::default().max_iterations);
            }
        }
    }
}

#[test]
#[ignore = "manual timing probe; operation-count gates run in the normal suite"]
fn resize_timing_profile() {
    for count in [25, 50, 100, 250, 500] {
        for (name, cascade, frozen) in [
            ("no-op", false, false),
            ("cascade", true, false),
            ("frozen-conflict", true, true),
        ] {
            let (mut input, roots) = scenario(count, cascade);
            if frozen {
                input.nodes.get_mut(&1).unwrap().frozen = true;
            }
            let start = Instant::now();
            let iterations = 100;
            for _ in 0..iterations {
                let _ = black_box(solve(black_box(&input), &roots, Limits::default()));
            }
            let elapsed = start.elapsed() / iterations;
            let outcome = solve(&input, &roots, Limits::default());
            let (stats, status) = match outcome {
                Ok(result) => (result.stats, "ok".to_string()),
                Err(error) => (error.stats, format!("{:?}", error.conflict)),
            };
            println!("{count:3} nodes {name:15} {elapsed:?}/solve {stats:?} {status}");
        }
    }
}
