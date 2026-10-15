//! Production asset-operation action identity shared by menu and host consumers.
use aestra_core::EffectClipId;
use bevy::prelude::*;

#[derive(Component, Event, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AssetAction {
    RefreshProject,
    CreateReusableEffectFromSelection,
    ExplodeEffectClip(EffectClipId),
}
