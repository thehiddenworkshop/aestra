//! Transient maximized panel identity, shared with application action consumers.
use super::ToolPanel;
use bevy::prelude::*;

/// The panel currently maximized to fill the whole editor, hiding the rest of the dock tree.
/// Transient: not part of the persisted [`super::WorkspaceLayout`].
#[derive(Resource, Default)]
pub(crate) struct MaximizedPanel(pub(crate) Option<ToolPanel>);
