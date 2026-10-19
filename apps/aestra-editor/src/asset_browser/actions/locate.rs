//! Actual shared semantic locate intent from graph/property/source controls.
use aestra_project::ProjectAssetId;
use bevy::prelude::*;
/// Shared semantic locate route for graph, property, and source-reference controls.
#[derive(Component, Event, Clone, Copy)]
pub(crate) struct LocateInAssets(pub(crate) ProjectAssetId);
