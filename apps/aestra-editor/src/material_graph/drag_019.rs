//! Native drag event types and pointer location only; no lifecycle policy translation.
use bevy::prelude::*;
pub(super) type PointerDragStart = bevy::picking::events::Pointer<bevy::picking::events::DragStart>;
pub(super) type PointerDrag = bevy::picking::events::Pointer<bevy::picking::events::Drag>;
pub(super) type PointerDragEnd = bevy::picking::events::Pointer<bevy::picking::events::DragEnd>;
pub(super) trait GraphDrag {
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
pub(super) mod tests {
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
