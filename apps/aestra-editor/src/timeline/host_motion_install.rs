//! Production host-motion observer and schedule registration, shared by both engine graphs.
use super::*;

pub(super) fn install(app: &mut App) {
    app.add_observer(host_composition::activate)
        .add_observer(motion_execution::execute)
        .add_observer(motion_execution::number_changed)
        .add_systems(
            Update,
            host_composition::buttons.in_set(TimelineSet::Actions),
        )
        .add_systems(
            Update,
            (
                inspector_composition::initialize_numbers,
                motion_execution::cancel_drag.before(update_timeline_visuals),
                motion_gestures::update_keys.after(update_timeline_visuals),
            )
                .in_set(TimelineSet::Visuals),
        );
}
