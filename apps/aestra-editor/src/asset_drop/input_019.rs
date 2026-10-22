//! Native asset-drop event representation only; target and edit policy stays shared.
use bevy::prelude::*;
pub(crate) type PointerDragEnter = bevy::picking::events::Pointer<bevy::picking::events::DragEnter>;
pub(crate) type PointerDragLeave = bevy::picking::events::Pointer<bevy::picking::events::DragLeave>;
pub(crate) type PointerDragDrop = bevy::picking::events::Pointer<bevy::picking::events::DragDrop>;
pub(crate) trait DropLocation {
    fn drop_position(&self) -> Vec2;
}
impl DropLocation for PointerDragDrop {
    fn drop_position(&self) -> Vec2 {
        self.pointer_location.position
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
        Pointer::new(
            PointerId::Mouse,
            location,
            bevy::picking::events::DragEnter {
                button,
                dragged,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            entity,
        )
    }
    pub(crate) fn leave(
        entity: Entity,
        dragged: Entity,
        location: Location,
        button: PointerButton,
    ) -> PointerDragLeave {
        Pointer::new(
            PointerId::Mouse,
            location,
            bevy::picking::events::DragLeave {
                button,
                dragged,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            entity,
        )
    }
    pub(crate) fn drop(
        entity: Entity,
        dropped: Entity,
        location: Location,
        button: PointerButton,
    ) -> PointerDragDrop {
        Pointer::new(
            PointerId::Mouse,
            location,
            bevy::picking::events::DragDrop {
                button,
                dropped,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            entity,
        )
    }
}
