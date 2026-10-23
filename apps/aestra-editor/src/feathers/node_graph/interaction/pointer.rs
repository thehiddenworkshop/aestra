//! Pointer API names only; no synthetic event dispatch or behavior translation.
use bevy::picking::pointer::{Location, PointerId};
pub(super) trait GraphPointerEvent {
    fn graph_pointer(&self) -> (PointerId, Location);
}
pub(super) use bevy::picking::events::{
    PointerCancel, PointerDrag, PointerDragEnd, PointerDragStart, PointerPress,
};
macro_rules! graph_pointer_event {
    ($($event:ty),*) => {$(
        impl GraphPointerEvent for $event {
            fn graph_pointer(&self) -> (PointerId, Location) {
                (self.pointer.id, self.pointer.location())
            }
        }
    )*};
}
graph_pointer_event!(
    PointerCancel,
    PointerDrag,
    PointerDragEnd,
    PointerDragStart,
    PointerPress
);
#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use bevy::{
        camera::RenderTarget,
        picking::{backend::HitData, pointer::PointerButton},
        prelude::{Entity, Vec2},
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
        PointerDragStart {
            entity,
            pointer: bevy::picking::events::Pointer::new(id, location(window, position)),
            button,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        }
    }
    pub(crate) fn motion(
        entity: Entity,
        window: Entity,
        position: Vec2,
        id: PointerId,
        button: PointerButton,
    ) -> PointerDrag {
        PointerDrag {
            entity,
            pointer: bevy::picking::events::Pointer::new(id, location(window, position)),
            button,
            distance: Vec2::ZERO,
            delta: Vec2::ZERO,
        }
    }
    pub(crate) fn motion_delta(entity: Entity, window: Entity, delta: Vec2) -> PointerDrag {
        PointerDrag {
            entity,
            pointer: bevy::picking::events::Pointer::new(PointerId::Mouse, location(window, delta)),
            button: PointerButton::Primary,
            distance: delta,
            delta,
        }
    }
    pub(crate) fn end(
        entity: Entity,
        window: Entity,
        position: Vec2,
        id: PointerId,
        button: PointerButton,
    ) -> PointerDragEnd {
        PointerDragEnd {
            entity,
            pointer: bevy::picking::events::Pointer::new(id, location(window, position)),
            button,
            distance: Vec2::ZERO,
        }
    }
    pub(crate) fn press(
        entity: Entity,
        window: Entity,
        position: Vec2,
        id: PointerId,
        button: PointerButton,
    ) -> PointerPress {
        PointerPress {
            entity,
            pointer: bevy::picking::events::Pointer::new(id, location(window, position)),
            button,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            count: 1,
        }
    }
    pub(crate) fn end_distance(entity: Entity, window: Entity, distance: Vec2) -> PointerDragEnd {
        let mut event = end(
            entity,
            window,
            Vec2::ZERO,
            PointerId::Mouse,
            PointerButton::Primary,
        );
        event.distance = distance;
        event
    }
    pub(crate) fn cancel(entity: Entity, window: Entity, id: PointerId) -> PointerCancel {
        PointerCancel {
            entity,
            pointer: bevy::picking::events::Pointer::new(id, location(window, Vec2::ZERO)),
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        }
    }
}
