//! Engine-independent semantic source model for Aestra effects.

mod diagnostic;
mod id;
pub mod material;
mod migration;
mod model;

pub use diagnostic::{Diagnostic, DiagnosticCode, DiagnosticSeverity, ValidationReport};
pub use id::*;
pub use migration::*;
pub use model::*;

/// The only effect format accepted by this version of Aestra.
pub const CURRENT_FORMAT_VERSION: u32 = 3;
