//! Shared menu/text pointer names on the shipping graph. No behavior translation.
pub(super) use bevy::feathers::cursor::EntityCursor;
use bevy::picking::events::{Click, DragStart, Over, Pointer};
pub(super) type PointerClick = Pointer<Click>;
pub(super) type PointerOver = Pointer<Over>;
pub(super) type PointerDragStart = Pointer<DragStart>;

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use bevy::{
        camera::RenderTarget,
        picking::{
            backend::HitData,
            pointer::{Location, PointerButton, PointerId},
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
    pub(crate) fn over(entity: Entity, window: Entity) -> PointerOver {
        Pointer::new(
            PointerId::Mouse,
            location(window),
            Over {
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
                button: PointerButton::Primary,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                duration: std::time::Duration::ZERO,
                count: 1,
            },
            entity,
        )
    }

    #[allow(
        dead_code,
        reason = "Used by the tooltip fixture, not the menu-only fixture"
    )]
    pub(crate) fn drag_start(entity: Entity, window: Entity) -> PointerDragStart {
        Pointer::new(
            PointerId::Mouse,
            location(window),
            DragStart {
                button: PointerButton::Primary,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            entity,
        )
    }
}
