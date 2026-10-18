//! Existing panel reveal and persisted workspace behavior.
use crate::{
    EditorSession,
    docking::{ToolPanel, WorkspaceLayout},
};
use bevy::prelude::*;

pub(crate) fn reveal_dock_panel(
    layout: &mut WorkspaceLayout,
    session: &mut EditorSession,
    panel: ToolPanel,
) {
    if !layout.show(panel) {
        return;
    }
    session.ui_revision += 1;
    if let Err(error) = layout.save() {
        warn!("failed to save editor workspace layout: {error}");
    }
}
