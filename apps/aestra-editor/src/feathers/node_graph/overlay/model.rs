//! Session-only composition. Never subtract a remembered delta from authored placement.
use super::super::resize;
use bevy::math::Vec2;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Input {
    pub base: Vec2,
    pub size: Vec2,
    pub compact: Vec2,
    pub collapsed: bool,
    pub revision: u64,
    pub frozen: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Conflict {
    Solve(resize::Conflict<String>),
    Restore(String, String),
}

#[derive(Default)]
pub(super) struct Overlay {
    previous: BTreeMap<String, Input>,
    rest: BTreeMap<String, Vec2>,
    protected: BTreeSet<String>,
    origin: BTreeMap<String, Input>,
    pub offsets: BTreeMap<String, Vec2>,
    epoch: u64,
    active: bool,
}

impl Overlay {
    pub fn update(
        &mut self,
        input: BTreeMap<String, Input>,
        epoch: u64,
        content_resizes: &BTreeSet<String>,
    ) -> Result<(), Conflict> {
        let reset = self.epoch != epoch;
        if reset {
            self.offsets.clear();
            self.protected.clear();
            self.origin.clear();
        }
        self.epoch = epoch;
        self.rest.retain(|key, _| input.contains_key(key));
        self.offsets.retain(|key, _| input.contains_key(key));
        self.protected.retain(|key| input.contains_key(key));
        for (key, node) in &input {
            let previous = self.previous.get(key);
            if !reset && self.active && previous.is_none_or(|old| old.revision != node.revision) {
                self.protected.insert(key.clone());
                // Manual placement already committed the displayed location as base.
                self.offsets.remove(key);
            }
            let rest = self.rest.entry(key.clone()).or_insert(node.compact);
            if node.collapsed {
                *rest = node.compact;
            } else if let Some(old) = previous {
                if old.collapsed {
                    *rest = old.compact; // Body expansion is another reversible cause.
                } else if content_resizes.contains(key) {
                    *rest = rest.min(old.compact).min(node.compact);
                } else if (*rest - old.compact).abs().max_element() < 0.5 {
                    // Owner/DPI/rebuild remeasurement is not a new content expansion.
                    *rest = node.compact;
                }
            }
            *rest = rest.min(node.compact);
        }
        let causes = input
            .iter()
            .filter(|(key, n)| (n.size - self.rest[*key]).max_element() > 0.5)
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        if self.origin.is_empty() && !causes.is_empty() {
            self.origin = input
                .iter()
                .map(|(key, node)| {
                    let mut rest = node.clone();
                    rest.size = self.rest[key];
                    (key.clone(), rest)
                })
                .collect();
        }
        let result = self.compose(&input, &causes);
        self.previous = input;
        self.active = !causes.is_empty() || !self.offsets.is_empty();
        match result {
            Ok(offsets) => {
                self.offsets = offsets;
                self.active = !causes.is_empty() || !self.offsets.is_empty();
                if !self.active {
                    self.protected.clear();
                    self.origin.clear();
                }
                Ok(())
            }
            Err(error) => Err(error), // Keep safe existing offsets; no partial new cascade.
        }
    }

    fn compose(
        &self,
        input: &BTreeMap<String, Input>,
        causes: &[String],
    ) -> Result<BTreeMap<String, Vec2>, Conflict> {
        let limits = resize::Limits::default();
        let mut scene = resize::Snapshot {
            identity: (),
            revision: resize::Revision::default(),
            nodes: input
                .iter()
                .map(|(key, node)| {
                    (
                        key.clone(),
                        resize::Node {
                            position: node.base,
                            size: self.rest[key],
                            frozen: node.frozen || self.protected.contains(key),
                        },
                    )
                })
                .collect(),
        };
        let mut remaining = limits;
        if causes.len() > limits.max_affected {
            return Err(Conflict::Solve(resize::Conflict::Budget(
                resize::Budget::AffectedNodes,
            )));
        }
        // Grow one cause at a time in stable key order. A later root can have been
        // displaced by an earlier one; anchor its already-composed position, not base.
        for key in causes {
            let old = scene.nodes[key].size;
            scene.nodes.get_mut(key).unwrap().size = input[key].size;
            let result = resize::solve(&scene, &BTreeMap::from([(key.clone(), old)]), remaining)
                .map_err(|e| Conflict::Solve(e.conflict))?;
            let stats = result.apply(&mut scene).map_err(Conflict::Solve)?;
            remaining.max_pair_checks -= stats.pair_checks;
            remaining.max_iterations -= stats.iterations;
        }
        // With no growing causes there is still a restore to validate.
        let offsets = scene
            .nodes
            .iter()
            .filter_map(|(key, node)| {
                let delta = node.position - input[key].base;
                (delta != Vec2::ZERO).then(|| (key.clone(), delta))
            })
            .collect::<BTreeMap<_, _>>();
        if offsets.len() > limits.max_affected {
            return Err(Conflict::Solve(resize::Conflict::Budget(
                resize::Budget::AffectedNodes,
            )));
        }
        let total = offsets
            .values()
            .map(|offset| offset.abs().element_sum())
            .sum::<f32>();
        if !total.is_finite() || total > limits.max_total_displacement {
            return Err(Conflict::Solve(resize::Conflict::Budget(
                resize::Budget::TotalDisplacement,
            )));
        }
        if offsets
            .values()
            .any(|offset| offset.abs().element_sum() > limits.max_node_displacement)
        {
            return Err(Conflict::Solve(resize::Conflict::Budget(
                resize::Budget::NodeDisplacement,
            )));
        }
        // Only changed/returning nodes need validation, not unrelated manual overlaps.
        // This also catches an insertion/manual move into a displaced node's old space.
        for key in offsets
            .keys()
            .chain(self.offsets.keys())
            .collect::<BTreeSet<_>>()
        {
            let candidate = resize::Node {
                size: input[key].size,
                ..scene.nodes[key]
            };
            for (other, node) in &scene.nodes {
                if key == other {
                    continue;
                }
                if remaining.max_pair_checks == 0 {
                    return Err(Conflict::Solve(resize::Conflict::Budget(
                        resize::Budget::PairChecks,
                    )));
                }
                remaining.max_pair_checks -= 1;
                let other_node = resize::Node {
                    size: input[other].size,
                    ..*node
                };
                if !resize::overlaps(&candidate, &other_node, limits) {
                    continue;
                }
                let current = resize::Node {
                    position: input[key].base + self.offsets.get(key).copied().unwrap_or_default(),
                    ..candidate
                };
                let current_other = resize::Node {
                    position: input[other].base
                        + self.offsets.get(other).copied().unwrap_or_default(),
                    ..other_node
                };
                let intentional = self
                    .origin
                    .get(key)
                    .zip(self.origin.get(other))
                    .is_some_and(|(a, b)| {
                        a.base == input[key].base
                            && b.base == input[other].base
                            && a.revision == input[key].revision
                            && b.revision == input[other].revision
                            && resize::overlaps(
                                &resize::Node {
                                    position: a.base,
                                    size: a.size,
                                    frozen: false,
                                },
                                &resize::Node {
                                    position: b.base,
                                    size: b.size,
                                    frozen: false,
                                },
                                limits,
                            )
                    });
                if !intentional && !resize::overlaps(&current, &current_other, limits) {
                    return Err(Conflict::Restore(key.clone(), other.clone()));
                }
            }
        }
        Ok(offsets)
    }
}

#[cfg(test)]
mod tests;
