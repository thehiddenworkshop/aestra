//! Bevy 0.20 flattens pointer events and fixes color-plane channel-space mapping.
pub(super) use bevy::picking::events::{
    PointerCancel, PointerClick, PointerDragStart, PointerPress,
};

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use bevy::{
        camera::RenderTarget,
        picking::{
            backend::HitData,
            events::{Pointer, PointerDrag, PointerDragEnd, PointerPress},
            pointer::{Location, PointerButton, PointerId},
        },
        prelude::*,
        window::WindowRef,
    };
    fn pointer(window: Entity, position: Vec2) -> Pointer {
        Pointer::new(
            PointerId::Mouse,
            Location {
                target: RenderTarget::Window(WindowRef::Entity(window))
                    .normalize(None)
                    .unwrap(),
                position,
            },
        )
    }
    fn hit() -> HitData {
        HitData::new(Entity::PLACEHOLDER, 0.0, None, None)
    }
    pub(crate) fn press(entity: Entity, window: Entity, position: Vec2) -> PointerPress {
        PointerPress {
            entity,
            pointer: pointer(window, position),
            button: PointerButton::Primary,
            hit: hit(),
            count: 1,
        }
    }
    pub(crate) fn click(entity: Entity, window: Entity) -> PointerClick {
        PointerClick {
            entity,
            pointer: pointer(window, Vec2::ZERO),
            button: PointerButton::Primary,
            hit: hit(),
            duration: std::time::Duration::ZERO,
            count: 1,
        }
    }
    pub(crate) fn start(entity: Entity, window: Entity) -> PointerDragStart {
        PointerDragStart {
            entity,
            pointer: pointer(window, Vec2::ZERO),
            button: PointerButton::Primary,
            hit: hit(),
        }
    }
    pub(crate) fn drag(entity: Entity, window: Entity, position: Vec2) -> PointerDrag {
        PointerDrag {
            entity,
            pointer: pointer(window, position),
            button: PointerButton::Primary,
            delta: Vec2::ZERO,
            distance: Vec2::ZERO,
        }
    }
    pub(crate) fn end(entity: Entity, window: Entity, position: Vec2) -> PointerDragEnd {
        PointerDragEnd {
            entity,
            pointer: pointer(window, position),
            button: PointerButton::Primary,
            distance: Vec2::ZERO,
        }
    }
    pub(crate) fn cancel(entity: Entity, window: Entity) -> PointerCancel {
        PointerCancel {
            entity,
            pointer: pointer(window, Vec2::ZERO),
            hit: hit(),
        }
    }
    pub(crate) fn channel_y(screen_y: f32) -> f32 {
        1.0 - screen_y
    }

    pub(crate) fn assert_thumb(world: &World, inner: Entity, value: Vec2) {
        let thumb = world.get::<Children>(inner).unwrap()[0];
        assert_eq!(
            world.get::<UiTransform>(thumb).unwrap().translation,
            Val2::px(value.x * 100.0 - 5.0, (1.0 - value.y) * 100.0 - 5.0)
        );
    }
}
