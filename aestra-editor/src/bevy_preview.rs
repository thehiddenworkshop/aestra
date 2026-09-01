//! Temporary boundary around the Bevy game-runtime adapter used by editor preview.
//!
//! Editor code must consume semantic and runtime types from their owning Aestra crates. Keeping
//! these presentation-only imports in one place makes the remaining adapter coupling explicit and
//! gives the renderer extraction a small, auditable surface.

pub(crate) use aestra_bevy::{
    ActiveBackend, AestraPlugin, AestraSet, EffectPlayer, EffectRenderMode, EffectRuntimeStatus,
};
