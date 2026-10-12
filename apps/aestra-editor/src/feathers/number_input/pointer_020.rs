//! Bevy 0.20 flattens pointer event types and moves cursor policy into picking.
pub(super) use bevy::picking::{
    cursor::{EntityCursor, OverrideCursor},
    events::{PointerClick, PointerDrag, PointerDragEnd, PointerDragStart},
};

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use bevy::{
        camera::RenderTarget,
        picking::{
            backend::HitData,
            pointer::{Location, PointerId},
        },
        prelude::*,
        window::WindowRef,
    };

    fn location(window: Entity) -> Location {
        Location {
            target: RenderTarget::Window(WindowRef::Entity(window))
                .normalize(None)
                .unwrap(),
            position: Vec2::ZERO,
        }
    }

    use bevy::picking::events::Pointer;

    pub(crate) fn start(
        entity: Entity,
        window: Entity,
        button: bevy::picking::pointer::PointerButton,
    ) -> PointerDragStart {
        PointerDragStart {
            entity,
            pointer: Pointer::new(PointerId::Mouse, location(window)),
            button,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        }
    }
    pub(crate) fn click(entity: Entity, window: Entity) -> PointerClick {
        PointerClick {
            entity,
            pointer: Pointer::new(PointerId::Mouse, location(window)),
            button: bevy::picking::pointer::PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            duration: std::time::Duration::ZERO,
            count: 1,
        }
    }
    pub(crate) fn drag(
        entity: Entity,
        window: Entity,
        button: bevy::picking::pointer::PointerButton,
        delta: Vec2,
    ) -> PointerDrag {
        PointerDrag {
            entity,
            pointer: Pointer::new(PointerId::Mouse, location(window)),
            button,
            distance: delta,
            delta,
        }
    }
    pub(crate) fn end(
        entity: Entity,
        window: Entity,
        button: bevy::picking::pointer::PointerButton,
    ) -> PointerDragEnd {
        PointerDragEnd {
            entity,
            pointer: Pointer::new(PointerId::Mouse, location(window)),
            button,
            distance: Vec2::ZERO,
        }
    }
}
