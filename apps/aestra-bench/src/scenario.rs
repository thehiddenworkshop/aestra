//! Canonical benchmark scenarios. Each is a real `.aestra.ron` asset embedded at
//! build time so the harness is fully self-contained and deterministic regardless
//! of the working directory. This is the first slice of the strategy's B001-B010
//! set; the remaining scenarios are added as the matrix grows (strategy §16).

/// A named, deterministic benchmark input.
pub struct Scenario {
    pub name: &'static str,
    pub purpose: &'static str,
    pub ron: &'static str,
}

pub const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "b001_empty",
        purpose: "floor / harness overhead: one tiny emitter, minimal work",
        ron: include_str!("../scenarios/b001_empty.aestra.ron"),
    },
    Scenario {
        name: "b002_single_small",
        purpose: "baseline runtime overhead: 1 emitter, ~1k capacity, high occupancy",
        ron: include_str!("../scenarios/b002_single_small.aestra.ron"),
    },
    Scenario {
        name: "b003_single_dense",
        purpose: "raw particle throughput: 1 emitter, 100k capacity, ~100% occupancy",
        ron: include_str!("../scenarios/b003_single_dense.aestra.ron"),
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
