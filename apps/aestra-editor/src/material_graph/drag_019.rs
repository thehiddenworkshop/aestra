//! Native drag event types and pointer location only; no lifecycle policy translation.
use bevy::prelude::*;
pub(crate) type PointerDragDrop = bevy::picking::events::Pointer<bevy::picking::events::DragDrop>;
pub(crate) type PointerDragStart = bevy::picking::events::Pointer<bevy::picking::events::DragStart>;
pub(crate) type PointerDrag = bevy::picking::events::Pointer<bevy::picking::events::Drag>;
pub(crate) type PointerDragEnd = bevy::picking::events::Pointer<bevy::picking::events::DragEnd>;
pub(crate) trait GraphDrag {
    fn graph_drag_position(&self) -> Vec2;
}
impl GraphDrag for PointerDragStart {
    fn graph_drag_position(&self) -> Vec2 {
        self.pointer_location.position
    }
}
impl GraphDrag for PointerDrag {
    fn graph_drag_position(&self) -> Vec2 {
        self.pointer_location.position
    }
}
impl GraphDrag for PointerDragEnd {
    fn graph_drag_position(&self) -> Vec2 {
        self.pointer_location.position
    }
}
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use bevy::{
        camera::RenderTarget,
        picking::{
            backend::HitData,
            events::Pointer,
            pointer::{Location, PointerButton, PointerId},
        },
        window::WindowRef,
    };
    fn location(window: Entity, position: Vec2) -> Location {
        Location {
            target: RenderTarget::Window(WindowRef::Entity(window))
                .normalize(None)
                .unwrap(),
            position,
        }
    }
    pub(crate) fn drop(
        entity: Entity,
        dropped: Entity,
        window: Entity,
        position: Vec2,
        button: PointerButton,
    ) -> PointerDragDrop {
        Pointer::new(
            PointerId::Mouse,
            location(window, position),
            bevy::picking::events::DragDrop {
                button,
                dropped,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            entity,
        )
    }
    pub(crate) fn start(
        entity: Entity,
        window: Entity,
        position: Vec2,
        button: PointerButton,
    ) -> PointerDragStart {
        Pointer::new(
            PointerId::Mouse,
            location(window, position),
            bevy::picking::events::DragStart {
                button,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            entity,
        )
    }
    pub(crate) fn motion(
        entity: Entity,
        window: Entity,
        position: Vec2,
        button: PointerButton,
    ) -> PointerDrag {
        Pointer::new(
            PointerId::Mouse,
            location(window, position),
            bevy::picking::events::Drag {
                button,
                distance: Vec2::ZERO,
                delta: Vec2::ZERO,
            },
            entity,
        )
    }
    pub(crate) fn end(
        entity: Entity,
        window: Entity,
        position: Vec2,
        button: PointerButton,
    ) -> PointerDragEnd {
        Pointer::new(
            PointerId::Mouse,
            location(window, position),
            bevy::picking::events::DragEnd {
                button,
                distance: Vec2::ZERO,
            },
            entity,
        )
    }
}
