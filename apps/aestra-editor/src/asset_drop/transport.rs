//! Actual source ancestry and snapshot/cancellation validity policy for authoring drops.
use crate::{ProjectEffectCatalog, asset_drop::AssetPayload};
use bevy::prelude::*;

/// Resolve the nearest eligible ancestor, so nested input fields win over their cards.
pub(crate) fn nearest<T>(
    entity: Entity,
    parents: &Query<&ChildOf>,
    mut resolve: impl FnMut(Entity) -> Option<T>,
) -> Option<(Entity, T)> {
    std::iter::once(entity)
        .chain(parents.iter_ancestors(entity))
        .find_map(|entity| resolve(entity).map(|target| (entity, target)))
}

pub(crate) fn cancelled(keys: Option<&ButtonInput<KeyCode>>) -> bool {
    keys.is_some_and(|keys| keys.just_pressed(KeyCode::Escape))
}

pub(crate) fn active(
    payload: &AssetPayload,
    sources: &Query<&AssetPayload>,
    catalog: Option<&ProjectEffectCatalog>,
    keys: Option<&ButtonInput<KeyCode>>,
) -> bool {
    !cancelled(keys)
        && catalog.is_some_and(|catalog| payload.resolve(catalog).is_ok())
        && sources.iter().any(|source| source == payload)
}
