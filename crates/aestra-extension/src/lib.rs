//! The Aestra extension SDK (extensible plan §23–§25).
//!
//! Everything "extension" lives here, so an extension author depends on this crate alone — not on the
//! compiler:
//!
//! - [`sdk`] — the contract: [`AestraExtension`], the [`ExtensionRegistry`] and its descriptors
//!   (modules, stages, renderers, domains, resources, capabilities), the lowering and payload-migration
//!   traits, recorded requirements, [`link_extension`], and [`EXTENSION_API_VERSION`]. Aestra's
//!   built-in modules register through the same descriptors ([`ModuleRegistry::builtin`]).
//! - [`host`] — installing extensions without rebuilding Aestra: the declarative package format,
//!   discovery, version and dependency checks, registration, disabling, and per-package diagnostics.
//!
//! The compiler depends on this crate and re-exports [`sdk`], so `aestra_compiler::ModuleMetadata`
//! and friends keep working.

mod catalog;
mod descriptors;
pub mod host;
mod linking;

/// The extension contract — re-exported at the crate root and by `aestra-compiler`.
pub mod sdk {
    pub use crate::descriptors::*;
    pub use crate::linking::*;
    pub use aestra_core::{
        PropertyEvaluationDomain as InputEvaluationDomain, PropertySource as InputSourceKind,
        ValueType,
    };
}

pub use sdk::*;
