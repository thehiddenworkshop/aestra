//! Immutable material preparation shared by independently spawned players.
use crate::{CompiledEffect, PresentedEffect};
use bevy::prelude::*;
use std::{collections::BTreeMap, sync::Arc};

/// Reuses compiled semantic material programs for a loaded effect artifact.
/// Each instance gets independent parameters, bindings, resources and history.
///
/// The plugin prepares on first use. For latency-sensitive spawning, use this
/// resource during loading to instantiate the presentations supplied to
/// [`crate::EffectPipelinePreparation::new`]. That also warms the same programs
/// used by future [`crate::EffectPlayer`] entities, before gameplay starts.
///
/// Keys use artifact identity, so a newly compiled artifact with the same effect
/// ID cannot reuse obsolete materials. Entries retire after the artifact has no
/// other owners; keeping a compiled catalog loaded intentionally keeps it warm.
/// No player count limit or mutable simulation pool is introduced.
#[derive(Resource, Default)]
pub struct EffectPresentationCache {
    templates: BTreeMap<usize, PresentedEffect>,
}

impl EffectPresentationCache {
    /// Creates a fresh presentation, compiling its immutable programs only on
    /// the first use of this exact compiled artifact.
    pub fn instantiate(&mut self, effect: Arc<CompiledEffect>) -> PresentedEffect {
        let key = Arc::as_ptr(&effect) as usize;
        self.templates
            .entry(key)
            .or_insert_with(|| PresentedEffect::new(effect))
            .fresh_instance()
    }

    pub(crate) fn prune_unused(&mut self) {
        self.templates
            .retain(|_, template| Arc::strong_count(template.effect()) > 1);
    }
}

#[cfg(test)]
#[path = "tests/presentation_cache.rs"]
mod tests;
