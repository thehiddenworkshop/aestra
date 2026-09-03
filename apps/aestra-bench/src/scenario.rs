//! Canonical benchmark scenarios. Each is a real `.aestra.ron` asset embedded at
//! build time so the harness is fully self-contained and deterministic regardless
//! of the working directory. This is the first slice of the strategy's B001-B010
//! set; the remaining scenarios are added as the matrix grows (strategy §16).

/// A named, deterministic benchmark input.
pub struct Scenario {
    pub name: &'static str,
    pub purpose: &'static str,
    pub ron: &'static str,
    /// Number of independent effect instances to run concurrently. Values above 1
    /// exercise per-instance overhead at roughly constant total particle count
    /// (strategy §2.7 / B005).
    pub instances: usize,
}

pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "b001_empty",
        purpose: "floor / harness overhead: one tiny emitter, minimal work",
        ron: include_str!("../scenarios/b001_empty.aestra.ron"),
        instances: 1,
    },
    Scenario {
        name: "b002_single_small",
        purpose: "baseline runtime overhead: 1 emitter, ~1k capacity, high occupancy",
        ron: include_str!("../scenarios/b002_single_small.aestra.ron"),
        instances: 1,
    },
    Scenario {
        name: "b003_single_dense",
        purpose: "raw particle throughput: 1 emitter, 100k capacity, ~100% occupancy",
        ron: include_str!("../scenarios/b003_single_dense.aestra.ron"),
        instances: 1,
    },
    Scenario {
        name: "b004_sparse_large",
        purpose: "capacity-bound analytical sim: 500k capacity, ~1% occupancy",
        ron: include_str!("../scenarios/b004_sparse_large.aestra.ron"),
        instances: 1,
    },
    Scenario {
        name: "b005_many_small",
        purpose: "per-instance overhead: 100 small effects, ~10k total particles",
        ron: include_str!("../scenarios/b005_many_small.aestra.ron"),
        instances: 100,
    },
];

pub fn find(name: &str) -> Option<&'static Scenario> {
    SCENARIOS.iter().find(|scenario| scenario.name == name)
}

pub fn names() -> String {
    SCENARIOS
        .iter()
        .map(|scenario| scenario.name)
        .collect::<Vec<_>>()
        .join(", ")
}
