//! Shared asset-drop transport and feedback. Target adapters own validation and edits.
//! Filesystem operations deliberately retain their serialized I/O and recovery path.
pub(crate) mod cursor;
mod payload;
mod transport;
pub(crate) use transport::{active, cancelled, nearest};
pub(crate) mod resource;
pub(crate) mod virtual_sources;
pub(crate) use payload::VirtualAsset;
pub(crate) use payload::{AssetPayload, AuthoringDropGuard};

use crate::*;

pub(crate) fn input_row<T: Component>(
    parent: &mut ChildSpawnerCommands,
    target: T,
    help: &'static str,
    build: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent
        .spawn((
            target,
            Node {
                width: Val::Percent(100.0),
                position_type: PositionType::Relative,
                flex_direction: FlexDirection::Column,
                ..default()
            },
            Pickable::default(),
            EditorTooltip::description(help),
        ))
        .with_children(build);
}

pub(crate) mod feedback;
#[path = "asset_drop/input.rs"]
pub(crate) mod input;
pub(crate) use feedback::{Feedback, clear_feedback, register_feedback, resolve, show_feedback};
