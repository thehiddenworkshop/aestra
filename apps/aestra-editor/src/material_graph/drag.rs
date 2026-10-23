//! Native drag event types and pointer location only; no lifecycle policy translation.
pub(crate) use bevy::picking::events::PointerDrag;
pub(crate) use bevy::picking::events::PointerDragDrop;
pub(crate) use bevy::picking::events::PointerDragEnd;
pub(crate) use bevy::picking::events::PointerDragStart;
use bevy::prelude::*;
pub(crate) trait GraphDrag {
    fn graph_drag_position(&self) -> Vec2;
}
impl GraphDrag for PointerDragStart {
    fn graph_drag_position(&self) -> Vec2 {
        self.pointer.location().position
    }
}
impl GraphDrag for PointerDrag {
    fn graph_drag_position(&self) -> Vec2 {
        self.pointer.location().position
    }
}
impl GraphDrag for PointerDragEnd {
    fn graph_drag_position(&self) -> Vec2 {
        self.pointer.location().position
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
        PointerDragDrop {
            entity,
            pointer: Pointer::new(PointerId::Mouse, location(window, position)),
            button,
            dropped,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        }
    }
    pub(crate) fn start(
        entity: Entity,
        window: Entity,
        position: Vec2,
        button: PointerButton,
    ) -> PointerDragStart {
        PointerDragStart {
            entity,
            pointer: Pointer::new(PointerId::Mouse, location(window, position)),
            button,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        }
    }
    pub(crate) fn motion(
        entity: Entity,
        window: Entity,
        position: Vec2,
        button: PointerButton,
    ) -> PointerDrag {
        PointerDrag {
            entity,
            pointer: Pointer::new(PointerId::Mouse, location(window, position)),
            button,
            distance: Vec2::ZERO,
            delta: Vec2::ZERO,
        }
    }
    pub(crate) fn end(
        entity: Entity,
        window: Entity,
        position: Vec2,
        button: PointerButton,
    ) -> PointerDragEnd {
        PointerDragEnd {
            entity,
            pointer: Pointer::new(PointerId::Mouse, location(window, position)),
            button,
            distance: Vec2::ZERO,
        }
    }
}
