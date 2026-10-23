//! Shared menu/text pointer names in the editor. No behavior translation.
#[allow(
    unused_imports,
    reason = "Drag-start is used by the separate tooltip fixture"
)]
pub(super) use bevy::picking::events::PointerDragStart;
pub(super) use bevy::picking::{
    cursor::EntityCursor,
    events::{PointerClick, PointerOver},
};

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
        PointerOver {
            entity,
            pointer: Pointer::new(PointerId::Mouse, location(window)),
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        }
    }
    pub(crate) fn click(entity: Entity, window: Entity) -> PointerClick {
        PointerClick {
            entity,
            pointer: Pointer::new(PointerId::Mouse, location(window)),
            button: PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            duration: std::time::Duration::ZERO,
            count: 1,
        }
    }

    #[allow(
        dead_code,
        reason = "Used by the tooltip fixture, not the menu-only fixture"
    )]
    pub(crate) fn drag_start(entity: Entity, window: Entity) -> PointerDragStart {
        PointerDragStart {
            entity,
            pointer: Pointer::new(PointerId::Mouse, location(window)),
            button: PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        }
    }
}
