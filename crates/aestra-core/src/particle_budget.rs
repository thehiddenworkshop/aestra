//! Explicit, opt-in compile-time density profiles. No automatic fan-out scaling.
use crate::{
    Diagnostic, DiagnosticCode, EffectAsset, EmitterId, EventId, RendererId, RendererProperties,
    ValidationReport,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Exact budgets for a named non-high quality tier. Omitted targets retain their
/// authored values. This changes pools and event demand, not lifetime, velocity,
/// emission modules, host input bursts, material values or playback semantics.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParticleBudgetProfile {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub emitter_capacity: BTreeMap<EmitterId, u32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub event_count: BTreeMap<EventId, u32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub trail_capacity: BTreeMap<RendererId, u32>,
}

impl EffectAsset {
    pub fn particle_budget_validation_report(&self) -> ValidationReport {
        let mut report = ValidationReport::default();
        validate_profiles(self, &mut report);
        report
    }
    /// Applies a matching explicit profile to a copy, preserving the authored
    /// document. Callers still validate/compile the result; this is not admission
    /// control or a guarantee that the authored demand fits the selected pools.
    pub fn with_particle_budget_profile(&self, tier: &str) -> Self {
        let mut result = self.clone();
        if let Some(profile) = self.particle_budgets.get(tier) {
            for emitter in &mut result.emitters {
                if let Some(capacity) = profile.emitter_capacity.get(&emitter.id) {
                    emitter.max_particles = *capacity;
                }
                for renderer in &mut emitter.renderers {
                    if let RendererProperties::Trail { max_trails, .. } = &mut renderer.properties
                        && let Some(capacity) = profile.trail_capacity.get(&renderer.id)
                    {
                        *max_trails = *capacity;
                    }
                }
            }
            for event in &mut result.events {
                if let Some(count) = profile.event_count.get(&event.id) {
                    event.count = *count;
                }
            }
            // The effective copy is no longer an authored high-tier document.
            result.particle_budgets.clear();
        }
        result
    }

    /// Removes dangling profile targets after an authoring deletion. Undo must
    /// restore the old profiles alongside the removed objects.
    pub fn prune_particle_budget_targets(&mut self, previous: &Self) {
        for profile in self.particle_budgets.values_mut() {
            profile.emitter_capacity.retain(|id, _| {
                !previous.emitters.iter().any(|e| e.id == *id)
                    || self.emitters.iter().any(|e| e.id == *id)
            });
            profile.event_count.retain(|id, _| {
                !previous.events.iter().any(|e| e.id == *id)
                    || self.events.iter().any(|e| e.id == *id)
            });
            profile.trail_capacity.retain(|id, _| {
                !previous.emitters.iter().any(|e| {
                    e.renderers.iter().any(|r| {
                        r.id == *id && matches!(r.properties, RendererProperties::Trail { .. })
                    })
                }) || self.emitters.iter().any(|e| {
                    e.renderers.iter().any(|r| {
                        r.id == *id && matches!(r.properties, RendererProperties::Trail { .. })
                    })
                })
            });
        }
    }
}

pub(crate) fn validate_profiles(effect: &EffectAsset, report: &mut ValidationReport) {
    for (name, profile) in &effect.particle_budgets {
        let path = format!("effect.particle_budgets[{name:?}]");
        if name.trim().is_empty() || name == "high" || name.trim() != name {
            report.push(Diagnostic::error(DiagnosticCode::InvalidValue, &path,
                "particle budget profiles need a non-blank, unpadded tier name other than high; high remains authored"));
        }
        for (&id, &value) in &profile.emitter_capacity {
            check(
                report,
                format!("{path}.emitter_capacity[{id}]"),
                value,
                effect
                    .emitters
                    .iter()
                    .find(|e| e.id == id)
                    .map(|e| e.max_particles),
            );
        }
        for (&id, &value) in &profile.event_count {
            check(
                report,
                format!("{path}.event_count[{id}]"),
                value,
                effect.events.iter().find(|e| e.id == id).map(|e| e.count),
            );
        }
        for (&id, &value) in &profile.trail_capacity {
            let authored = effect.emitters.iter().find_map(|e| {
                e.renderers.iter().find_map(|r| {
                    if r.id != id {
                        return None;
                    }
                    if let RendererProperties::Trail { max_trails, .. } = r.properties {
                        Some(if max_trails == 0 {
                            e.max_particles
                        } else {
                            max_trails
                        })
                    } else {
                        None
                    }
                })
            });
            check(
                report,
                format!("{path}.trail_capacity[{id}]"),
                value,
                authored,
            );
        }
    }
}

fn check(report: &mut ValidationReport, path: String, value: u32, authored: Option<u32>) {
    match authored {
        None => report.push(Diagnostic::error(
            DiagnosticCode::InvalidReference,
            path,
            "particle budget target is missing or is not a trail renderer",
        )),
        Some(maximum) if value == 0 || value > maximum => report.push(Diagnostic::error(
            DiagnosticCode::InvalidValue,
            path,
            format!("particle budget must be in 1..={maximum}; got {value}"),
        )),
        Some(_) => {}
    }
}
