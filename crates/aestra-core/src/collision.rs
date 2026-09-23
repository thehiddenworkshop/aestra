//! The collision input provider abstraction (hybrid roadmap M11).
//!
//! This is the portable half of the semantic/backend boundary for collision. A stateful effect that
//! collides needs *collision inputs* — the geometry particles collide against — from somewhere. The
//! portable plan describes **what** collision capability is required and **how its inputs are available
//! over time**; an engine adapter (e.g. the Bevy backend) declares which sources it can actually
//! supply. Nothing here depends on any engine.
//!
//! Two rules fall out of this boundary:
//!
//! - **Backend capability failure is explicit.** [`CollisionInputs::resolve_against`] returns an error
//!   naming the sources a backend cannot supply, rather than silently producing wrong results.
//! - **Exact backward seeking is only advertised when historical collision inputs can be
//!   reconstructed.** [`CollisionInputAvailability`] classifies each provider, and
//!   [`CollisionInputs::supports_exact_backward_seek`] is false as soon as any provider's past inputs
//!   cannot be recovered (see the historical-input rule below).

use serde::{Deserialize, Serialize};

/// Where a stateful island's collision inputs come from. Names the capability the backend must
/// satisfy, without depending on any engine. Only [`AuthoredColliders`](Self::AuthoredColliders) is
/// produced today (hybrid roadmap M10); the rest reserve the boundary for engine scene collision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CollisionInputSource {
    /// Explicit colliders authored on the effect (hybrid roadmap M10). Static data carried in the
    /// compiled effect — needs no engine scene, and is reproducible at any tick.
    AuthoredColliders,
    /// A signed distance field supplied by the backend.
    SignedDistanceField,
    /// The engine's depth buffer.
    DepthBuffer,
    /// Live engine physics queries.
    EnginePhysicsQuery,
    /// The engine's mesh / acceleration structure.
    MeshAccelerationStructure,
}

/// How a collision provider's inputs can be recovered for a *past* tick — the historical-input rule
/// that decides whether exact backward seeking can be advertised (hybrid roadmap M11). Ordered from
/// least to most restrictive, so the aggregate over several providers is their `max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CollisionInputAvailability {
    /// Reproducible at any tick from static data (authored colliders) — exact backward seek is free.
    TimeAddressable,
    /// Not static, but the backend records the inputs per tick, so any past tick replays exactly.
    Recordable,
    /// Reconstructible only at checkpoints; between them, replay forward from the nearest checkpoint
    /// (the M7 seek model already does this for particle state).
    Checkpointed,
    /// Available only going forward in real time; past inputs cannot be reconstructed, so an exact
    /// backward seek is impossible — only a preview (M12) or a forward re-run can be offered.
    ForwardOnly,
}

impl CollisionInputAvailability {
    /// Whether a provider with this availability can have its inputs reconstructed for a past tick, so
    /// exact backward seeking stays possible. Everything but [`ForwardOnly`](Self::ForwardOnly) can —
    /// directly, from a recording, or by replaying from a checkpoint.
    pub fn supports_exact_backward_seek(self) -> bool {
        !matches!(self, Self::ForwardOnly)
    }
}

/// A declared collision provider: a source and how its inputs are historically available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CollisionProvider {
    pub source: CollisionInputSource,
    pub availability: CollisionInputAvailability,
}

impl CollisionProvider {
    /// The provider for authored Aestra colliders (hybrid roadmap M10): static data, so its inputs are
    /// time-addressable and exact backward seeking is always available.
    pub const AUTHORED: Self = Self {
        source: CollisionInputSource::AuthoredColliders,
        availability: CollisionInputAvailability::TimeAddressable,
    };
}

/// The collision inputs a compiled effect requires and their aggregate historical availability
/// (hybrid roadmap M11). Drives whether exact backward seeking is advertised for the effect, and what
/// the backend must be able to supply.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CollisionInputs {
    /// The distinct sources required, deduplicated and sorted for a stable order.
    sources: Vec<CollisionInputSource>,
    /// The most restrictive availability across every provider; `None` when no collision is used.
    availability: Option<CollisionInputAvailability>,
}

impl CollisionInputs {
    /// Aggregates the providers of an effect: the distinct required sources, and the most restrictive
    /// availability (so a single forward-only provider makes the whole effect forward-only).
    pub fn from_providers(providers: impl IntoIterator<Item = CollisionProvider>) -> Self {
        let mut sources = Vec::new();
        let mut availability: Option<CollisionInputAvailability> = None;
        for provider in providers {
            if !sources.contains(&provider.source) {
                sources.push(provider.source);
            }
            availability = Some(match availability {
                Some(current) => current.max(provider.availability),
                None => provider.availability,
            });
        }
        sources.sort();
        Self {
            sources,
            availability,
        }
    }

    /// The distinct collision input sources this effect requires, in a stable order.
    pub fn sources(&self) -> &[CollisionInputSource] {
        &self.sources
    }

    /// The most restrictive availability across the effect's collision providers, or `None` when the
    /// effect uses no collision.
    pub fn availability(&self) -> Option<CollisionInputAvailability> {
        self.availability
    }

    /// Whether the effect uses no collision inputs at all.
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// Whether exact backward seeking can be advertised: true when the effect uses no collision, or
    /// when every collision provider's past inputs can be reconstructed (hybrid roadmap M11).
    pub fn supports_exact_backward_seek(&self) -> bool {
        self.availability
            .is_none_or(CollisionInputAvailability::supports_exact_backward_seek)
    }

    /// Checks a backend can supply every required source, returning an explicit error naming the ones
    /// it cannot (hybrid roadmap M11: backend capability failure is explicit, never silent).
    pub fn resolve_against(
        &self,
        backend: &CollisionBackendCapabilities,
    ) -> Result<(), CollisionCapabilityError> {
        let missing: Vec<CollisionInputSource> = self
            .sources
            .iter()
            .copied()
            .filter(|source| !backend.supports(*source))
            .collect();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(CollisionCapabilityError { missing })
        }
    }
}

/// The collision input sources a backend can supply (hybrid roadmap M11). An engine adapter declares
/// this; portable crates never depend on the adapter, only on this description of it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CollisionBackendCapabilities {
    supported: Vec<CollisionInputSource>,
}

impl CollisionBackendCapabilities {
    pub fn new(sources: impl IntoIterator<Item = CollisionInputSource>) -> Self {
        let mut supported: Vec<CollisionInputSource> = sources.into_iter().collect();
        supported.sort();
        supported.dedup();
        Self { supported }
    }

    /// The capability of the current GPU stateful backend: authored colliders only. It resolves them
    /// itself on the GPU, needing no engine scene data (hybrid roadmap M10).
    pub fn authored_only() -> Self {
        Self::new([CollisionInputSource::AuthoredColliders])
    }

    pub fn supports(&self, source: CollisionInputSource) -> bool {
        self.supported.contains(&source)
    }

    pub fn supported(&self) -> &[CollisionInputSource] {
        &self.supported
    }
}

/// A backend cannot supply one or more of an effect's required collision input sources (hybrid roadmap
/// M11). Explicit rather than a silent no-op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollisionCapabilityError {
    /// The required sources the backend does not support.
    pub missing: Vec<CollisionInputSource>,
}

impl std::fmt::Display for CollisionCapabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "backend cannot supply required collision input source(s): {:?}",
            self.missing
        )
    }
}

impl std::error::Error for CollisionCapabilityError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability_orders_least_to_most_restrictive_and_gates_exact_seek() {
        assert!(
            CollisionInputAvailability::TimeAddressable < CollisionInputAvailability::ForwardOnly
        );
        assert!(CollisionInputAvailability::Recordable < CollisionInputAvailability::Checkpointed);
        for availability in [
            CollisionInputAvailability::TimeAddressable,
            CollisionInputAvailability::Recordable,
            CollisionInputAvailability::Checkpointed,
        ] {
            assert!(
                availability.supports_exact_backward_seek(),
                "{availability:?} can reconstruct past inputs"
            );
        }
        assert!(
            !CollisionInputAvailability::ForwardOnly.supports_exact_backward_seek(),
            "forward-only inputs cannot be reconstructed for a past tick"
        );
    }

    #[test]
    fn no_collision_supports_exact_backward_seek() {
        let inputs = CollisionInputs::default();
        assert!(inputs.is_empty());
        assert_eq!(inputs.availability(), None);
        assert!(
            inputs.supports_exact_backward_seek(),
            "an effect with no collision can always be seeked backward exactly"
        );
    }

    #[test]
    fn authored_colliders_are_time_addressable_and_exactly_seekable() {
        let inputs = CollisionInputs::from_providers([CollisionProvider::AUTHORED]);
        assert_eq!(inputs.sources(), &[CollisionInputSource::AuthoredColliders]);
        assert_eq!(
            inputs.availability(),
            Some(CollisionInputAvailability::TimeAddressable)
        );
        assert!(inputs.supports_exact_backward_seek());
    }

    #[test]
    fn a_forward_only_provider_makes_the_whole_effect_forward_only() {
        // The aggregate availability is the most restrictive provider's, so mixing authored colliders
        // (time-addressable) with a live physics query (forward-only) forbids exact backward seek.
        let inputs = CollisionInputs::from_providers([
            CollisionProvider::AUTHORED,
            CollisionProvider {
                source: CollisionInputSource::EnginePhysicsQuery,
                availability: CollisionInputAvailability::ForwardOnly,
            },
        ]);
        assert_eq!(
            inputs.availability(),
            Some(CollisionInputAvailability::ForwardOnly)
        );
        assert!(
            !inputs.supports_exact_backward_seek(),
            "a forward-only collision input forbids advertising exact backward seek"
        );
        assert_eq!(inputs.sources().len(), 2, "both sources are recorded");
    }

    #[test]
    fn checkpointed_inputs_still_support_exact_backward_seek() {
        let inputs = CollisionInputs::from_providers([CollisionProvider {
            source: CollisionInputSource::SignedDistanceField,
            availability: CollisionInputAvailability::Checkpointed,
        }]);
        assert!(
            inputs.supports_exact_backward_seek(),
            "checkpointed inputs replay exactly from the nearest checkpoint"
        );
    }

    #[test]
    fn backend_capability_failure_is_explicit() {
        // The current GPU backend supplies only authored colliders. An effect needing them resolves;
        // one needing an SDF fails explicitly, naming the missing source.
        let backend = CollisionBackendCapabilities::authored_only();

        let authored = CollisionInputs::from_providers([CollisionProvider::AUTHORED]);
        assert!(authored.resolve_against(&backend).is_ok());

        let needs_sdf = CollisionInputs::from_providers([CollisionProvider {
            source: CollisionInputSource::SignedDistanceField,
            availability: CollisionInputAvailability::Checkpointed,
        }]);
        let error = needs_sdf
            .resolve_against(&backend)
            .expect_err("SDF is unsupported by the authored-only backend");
        assert_eq!(
            error.missing,
            vec![CollisionInputSource::SignedDistanceField]
        );
    }
}
