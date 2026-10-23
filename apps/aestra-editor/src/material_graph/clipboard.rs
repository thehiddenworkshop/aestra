//! Shared graph clipboard: immutable source snapshots, fresh identities and internal-wire remapping.
//! Text editors own their own clipboard shortcuts; this payload is only used by graph canvases.
#[cfg(test)]
use super::*;

#[cfg(test)]
#[path = "tests/clipboard.rs"]
mod tests;

mod model;
pub(crate) use model::{Fragment, GraphClipboard, Shortcut, shortcut};

mod execution;
pub(super) use execution::{capture_material, insert_material};
