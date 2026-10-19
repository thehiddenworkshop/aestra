//! Production Profiler resource, observer and schedule registration.
use super::{
    ProfilerState,
    controls::{handle_profiler_actions, queue_profiler_action_activation},
    workspace::{update_profiler_labels, update_project_profile_text},
};
use bevy::prelude::*;

pub(crate) struct EditorProfilerPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ProfilerSet {
    Actions,
    Sync,
}

impl Plugin for EditorProfilerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ProfilerState>()
            .add_observer(queue_profiler_action_activation)
            .add_systems(
                Update,
                (
                    handle_profiler_actions.in_set(ProfilerSet::Actions),
                    (update_profiler_labels, update_project_profile_text).in_set(ProfilerSet::Sync),
                ),
            );
    }
}
