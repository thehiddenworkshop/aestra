//! Presentation deltas share the existing document chronology, but never shader state.
mod semantic;
mod transaction;
pub(crate) use transaction::{Snapshot, Transaction};
mod state;
pub(crate) use state::State;
mod context;
pub(super) use context::visible;
mod batch_input;
#[cfg(test)]
pub(crate) use batch_input::batch_edit;
mod preview;
pub(super) use preview::preview_edit;
mod edits;
pub(crate) use edits::arrange;
pub(super) use edits::register;

#[cfg(test)]
mod tests;
