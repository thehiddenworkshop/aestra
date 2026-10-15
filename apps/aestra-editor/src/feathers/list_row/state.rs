//! Actual shared list identities; list composition remains in its widget module.
use bevy::prelude::*;

/// Marks a headless list box that uses Aestra's shared keyboard-focus treatment.
#[derive(Component)]
pub(crate) struct KeyboardNavigableList;

/// Marks a row whose active-descendant focus should be drawn by the editor widget layer.
#[derive(Component)]
pub(crate) struct KeyboardNavigableListRow;
