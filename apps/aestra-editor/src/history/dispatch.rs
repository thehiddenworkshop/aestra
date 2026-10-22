//! Actual history event bridge shared with semantic editor consumers.
use crate::history::{HistoryAction, routing};
use bevy::prelude::*;

pub(crate) fn execute_history_action(
    action: On<HistoryAction>,
    routing: routing::Routing,
    mut commands: Commands,
) {
    routing.execute(
        *action,
        &mut commands,
        crate::asset_browser::deletion::history_step,
    );
}
