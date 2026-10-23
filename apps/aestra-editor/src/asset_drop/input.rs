//! Native asset-drop event representation only; target and edit policy stays shared.
pub(crate) use bevy::picking::events::{PointerDragDrop, PointerDragEnter, PointerDragLeave};
use bevy::prelude::*;
pub(crate) trait DropLocation {
    fn drop_position(&self) -> Vec2;
}
impl DropLocation for PointerDragDrop {
    fn drop_position(&self) -> Vec2 {
        self.pointer.location().position
    }
}
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use bevy::picking::{
        backend::HitData,
        events::Pointer,
        pointer::{Location, PointerButton, PointerId},
    };
    pub(crate) fn enter(
        entity: Entity,
        dragged: Entity,
        location: Location,
        button: PointerButton,
    ) -> PointerDragEnter {
        PointerDragEnter {
            entity,
            pointer: Pointer::new(PointerId::Mouse, location),
            button,
            dragged,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        }
    }
    pub(crate) fn leave(
        entity: Entity,
        dragged: Entity,
        location: Location,
        button: PointerButton,
    ) -> PointerDragLeave {
        PointerDragLeave {
            entity,
            pointer: Pointer::new(PointerId::Mouse, location),
            button,
            dragged,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        }
    }
    pub(crate) fn drop(
        entity: Entity,
        dropped: Entity,
        location: Location,
        button: PointerButton,
    ) -> PointerDragDrop {
        PointerDragDrop {
            entity,
            pointer: Pointer::new(PointerId::Mouse, location),
            button,
            dropped,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        }
    }
}
