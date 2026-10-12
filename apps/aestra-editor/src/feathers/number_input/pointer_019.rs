//! Bevy 0.19 numeric-interaction boundary; no behavior translation.
pub(super) use bevy::feathers::cursor::{EntityCursor, OverrideCursor};
use bevy::picking::events::{Click, Drag, DragEnd, DragStart, Pointer};
pub(super) type PointerClick = Pointer<Click>;
pub(super) type PointerDragStart = Pointer<DragStart>;
pub(super) type PointerDrag = Pointer<Drag>;
pub(super) type PointerDragEnd = Pointer<DragEnd>;

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

    pub(crate) fn start(
        entity: Entity,
        window: Entity,
        button: bevy::picking::pointer::PointerButton,
    ) -> PointerDragStart {
        Pointer::new(
            PointerId::Mouse,
            location(window),
            DragStart {
                button,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            entity,
        )
    }
    pub(crate) fn click(entity: Entity, window: Entity) -> PointerClick {
        Pointer::new(
            PointerId::Mouse,
            location(window),
            Click {
                button: bevy::picking::pointer::PointerButton::Primary,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                duration: std::time::Duration::ZERO,
                count: 1,
            },
            entity,
        )
    }
    pub(crate) fn drag(
        entity: Entity,
        window: Entity,
        button: bevy::picking::pointer::PointerButton,
        delta: Vec2,
    ) -> PointerDrag {
        Pointer::new(
            PointerId::Mouse,
            location(window),
            Drag {
                button,
                distance: delta,
                delta,
            },
            entity,
        )
    }
    pub(crate) fn end(
        entity: Entity,
        window: Entity,
        button: bevy::picking::pointer::PointerButton,
    ) -> PointerDragEnd {
        Pointer::new(
            PointerId::Mouse,
            location(window),
            DragEnd {
                button,
                distance: Vec2::ZERO,
            },
            entity,
        )
    }
}
