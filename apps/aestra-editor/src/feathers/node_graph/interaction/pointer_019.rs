//! Pointer API names only; no synthetic event dispatch or behavior translation.
use bevy::{
    picking::pointer::{Location, PointerId},
    prelude::*,
};
pub(super) trait GraphPointerEvent {
    fn graph_pointer(&self) -> (PointerId, Location);
}
use bevy::picking::events::{Cancel, Drag, DragEnd, DragStart, Pointer, Press};
pub(super) type PointerDragStart = Pointer<DragStart>;
pub(super) type PointerDrag = Pointer<Drag>;
pub(super) type PointerDragEnd = Pointer<DragEnd>;
pub(super) type PointerPress = Pointer<Press>;
pub(super) type PointerCancel = Pointer<Cancel>;
impl<E: std::fmt::Debug + Clone + Reflect> GraphPointerEvent for Pointer<E> {
    fn graph_pointer(&self) -> (PointerId, Location) {
        (self.pointer_id, self.pointer_location.clone())
    }
}
#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use bevy::{
        camera::RenderTarget,
        picking::{backend::HitData, pointer::PointerButton},
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
        id: PointerId,
        button: PointerButton,
    ) -> PointerDragStart {
        Pointer::new(
            id,
            location(window, position),
            DragStart {
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
        id: PointerId,
        button: PointerButton,
    ) -> PointerDrag {
        Pointer::new(
            id,
            location(window, position),
            Drag {
                button,
                distance: Vec2::ZERO,
                delta: Vec2::ZERO,
            },
            entity,
        )
    }
    pub(crate) fn motion_delta(entity: Entity, window: Entity, delta: Vec2) -> PointerDrag {
        Pointer::new(
            PointerId::Mouse,
            location(window, delta),
            Drag {
                button: PointerButton::Primary,
                distance: delta,
                delta,
            },
            entity,
        )
    }
    pub(crate) fn end(
        entity: Entity,
        window: Entity,
        position: Vec2,
        id: PointerId,
        button: PointerButton,
    ) -> PointerDragEnd {
        Pointer::new(
            id,
            location(window, position),
            DragEnd {
                button,
                distance: Vec2::ZERO,
            },
            entity,
        )
    }
    pub(crate) fn press(
        entity: Entity,
        window: Entity,
        position: Vec2,
        id: PointerId,
        button: PointerButton,
    ) -> PointerPress {
        Pointer::new(
            id,
            location(window, position),
            Press {
                button,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                count: 1,
            },
            entity,
        )
    }
    pub(crate) fn cancel(entity: Entity, window: Entity, id: PointerId) -> PointerCancel {
        Pointer::new(
            id,
            location(window, Vec2::ZERO),
            Cancel {
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            entity,
        )
    }
}
