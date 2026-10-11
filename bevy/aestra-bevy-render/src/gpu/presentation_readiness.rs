//! This-frame presentation readiness shared by simulation and particle light selection.
use bevy::prelude::*;

#[derive(Resource, Default)]
pub(super) struct PresentedOwners {
    pub enabled: bool,
    pub owners: std::collections::BTreeSet<Entity>,
}
