//! Engine-independent semantic source model for Aestra effects.

mod collision;
mod diagnostic;
mod host_transform;
mod id;
pub mod material;
mod migration;
mod model;
mod property_schema;
mod transform_curve;

pub use collision::*;
pub use property_schema::*;
pub use diagnostic::{Diagnostic, DiagnosticCode, DiagnosticSeverity, ValidationReport};
pub use host_transform::*;
pub use id::*;
pub use migration::*;
pub use model::*;
pub use transform_curve::*;

/// The only effect format accepted by this version of Aestra.
pub const CURRENT_FORMAT_VERSION: u32 = 3;
