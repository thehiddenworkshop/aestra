//! Explicit resource removal: upstream resource extraction only copies presence.
use super::extraction::Extract;
use bevy::prelude::*;

pub(super) fn remove_missing_resource<R: Resource>(
    source: Extract<Option<Res<R>>>,
    mut commands: Commands,
) {
    if source.is_none() {
        commands.remove_resource::<R>();
    }
}
