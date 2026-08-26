//! Engine-independent semantic source model for Aestra effects.

mod diagnostic;
mod id;
mod model;

pub use diagnostic::{Diagnostic, DiagnosticCode, DiagnosticSeverity, ValidationReport};
pub use id::*;
pub use model::*;

/// The only effect format accepted by this version of Aestra.
pub const CURRENT_FORMAT_VERSION: u32 = 2;
