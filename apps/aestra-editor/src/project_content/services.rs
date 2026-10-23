//! Project background lifecycle, independent of panels and native rendering.
use super::{io, refresh};
use bevy::prelude::*;

pub(super) struct ProjectContentServicesPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ProjectContentSet {
    Input,
}

impl Plugin for ProjectContentServicesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<io::ProjectIoTasks>()
            .init_resource::<refresh::ProjectEffectWatchState>()
            .add_systems(
                Update,
                (
                    io::poll,
                    refresh::poll_project_effect_catalog.run_if(io::idle),
                )
                    .chain()
                    .in_set(ProjectContentSet::Input),
            );
    }
}

#[cfg(test)]
#[path = "tests/services.rs"]
mod tests;
