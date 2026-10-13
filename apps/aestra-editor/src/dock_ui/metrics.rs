//! Shared window/layout floors; no native window implementation.
use bevy::prelude::Vec2;

/// The smallest size the editor window can be resized to, and the floor its UI lays out at. A
/// minimized window reports 0×0; laid out at that size, padded panels hand their stretched children
/// negative sizes, which Bevy's border-radius resolution rejects (a panic). The UI instead keeps this
/// size, clipped, until the window is restored.
pub(crate) const EDITOR_MIN_SIZE: Vec2 = Vec2::new(800.0, 500.0);
/// The same floor for a floating panel window.
pub(crate) const FLOATING_MIN_SIZE: Vec2 = Vec2::new(260.0, 180.0);
