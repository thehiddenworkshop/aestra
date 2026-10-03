//! Engine-independent semantic source model for Aestra effects.

mod authored_v4;
mod binding;
mod collision;
mod diagnostic;
mod event_interface;
mod event_routes;
mod extension_requirement;
mod host_transform;
mod id;
pub mod material;
mod migration;
mod model;
mod particle_budget;
mod property_schema;
mod transform_curve;
mod velocity;

pub use authored_v4::*;
pub use binding::*;
pub use collision::*;
pub use diagnostic::{Diagnostic, DiagnosticCode, DiagnosticSeverity, ValidationReport};
pub use event_interface::*;
pub use event_routes::*;
pub use extension_requirement::*;
pub use host_transform::*;
pub use id::*;
pub use migration::*;
pub use model::*;
pub use particle_budget::*;
pub use property_schema::*;
pub use transform_curve::*;
pub use velocity::*;

/// The only effect format accepted by this version of Aestra.
pub const CURRENT_FORMAT_VERSION: u32 = 4;
