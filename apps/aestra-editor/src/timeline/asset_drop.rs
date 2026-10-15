//! Native timeline asset-drop wrapper and integration tests.
#[cfg(test)]
use super::*;
#[cfg(test)]
use crate::asset_drop::AssetPayload;
mod source;
pub(super) use source::*;
#[cfg(test)]
mod tests;
