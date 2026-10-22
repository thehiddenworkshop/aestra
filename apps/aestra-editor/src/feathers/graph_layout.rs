//! Semantic-neutral contract for explicit graph arrangement engines.
//!
//! This boundary owns only immutable input snapshots and candidate validation. It does not read
//! ECS state, mutate documents, persist positions, or create history entries. M9 can therefore
//! qualify an external engine without coupling it to material graph types.
#![allow(dead_code)] // M8 contract is intentionally consumed by the M9 engine/controller.

#[cfg(test)]
use bevy::prelude::Vec2;
#[cfg(test)]
use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
pub(crate) mod comparison;
#[cfg(test)]
mod elk;
pub(crate) mod native;
pub(crate) mod partial;
#[cfg(test)]
pub(crate) use elk::ElkLayeredLayout;

mod model;
pub(crate) use model::*;
