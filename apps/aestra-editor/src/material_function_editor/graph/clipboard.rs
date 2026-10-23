//! Function-native adapter for the shared material graph clipboard.
#[cfg(test)]
use super::*;
#[cfg(test)]
use crate::material_graph::clipboard::{GraphClipboard, Shortcut};

#[cfg(test)]
#[path = "../tests/graph_clipboard.rs"]
mod tests;

mod execution;
#[cfg(test)]
use execution::keyboard;
pub(super) use execution::{execute, insert, register};
