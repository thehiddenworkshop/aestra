//! Color-plane pointer boundary for shipping Bevy 0.19.
use bevy::picking::events::{Cancel, Click, DragStart, Pointer, Press};
pub(super) type PointerClick = Pointer<Click>;
pub(super) type PointerCancel = Pointer<Cancel>;
pub(super) type PointerDragStart = Pointer<DragStart>;
pub(super) type PointerPress = Pointer<Press>;

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use bevy::{
        camera::RenderTarget,
        picking::{
            backend::HitData,
            events::{Drag, DragEnd, Press},
            pointer::{Location, PointerButton, PointerId},
        },
        prelude::*,
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
    fn hit() -> HitData {
        HitData::new(Entity::PLACEHOLDER, 0.0, None, None)
    }
    pub(crate) fn press(entity: Entity, window: Entity, position: Vec2) -> Pointer<Press> {
        Pointer::new(
            PointerId::Mouse,
            location(window, position),
            Press {
                button: PointerButton::Primary,
                hit: hit(),
                count: 1,
            },
            entity,
        )
    }
    pub(crate) fn click(entity: Entity, window: Entity) -> PointerClick {
        Pointer::new(
            PointerId::Mouse,
            location(window, Vec2::ZERO),
            Click {
                button: PointerButton::Primary,
                hit: hit(),
                duration: std::time::Duration::ZERO,
                count: 1,
            },
            entity,
        )
    }
    pub(crate) fn start(entity: Entity, window: Entity) -> PointerDragStart {
        Pointer::new(
            PointerId::Mouse,
            location(window, Vec2::ZERO),
            DragStart {
                button: PointerButton::Primary,
                hit: hit(),
            },
            entity,
        )
    }
    pub(crate) fn drag(entity: Entity, window: Entity, position: Vec2) -> Pointer<Drag> {
        Pointer::new(
            PointerId::Mouse,
            location(window, position),
            Drag {
                button: PointerButton::Primary,
                delta: Vec2::ZERO,
                distance: Vec2::ZERO,
            },
            entity,
        )
    }
    pub(crate) fn end(entity: Entity, window: Entity, position: Vec2) -> Pointer<DragEnd> {
        Pointer::new(
            PointerId::Mouse,
            location(window, position),
            DragEnd {
                button: PointerButton::Primary,
                distance: Vec2::ZERO,
            },
            entity,
        )
    }
    pub(crate) fn cancel(entity: Entity, window: Entity) -> PointerCancel {
        Pointer::new(
            PointerId::Mouse,
            location(window, Vec2::ZERO),
            Cancel { hit: hit() },
            entity,
        )
    }
    pub(crate) fn channel_y(screen_y: f32) -> f32 {
        screen_y
    }

    pub(crate) fn assert_thumb(world: &World, inner: Entity, value: Vec2) {
        let thumb = world.get::<Children>(inner).unwrap()[0];
        let node = world.get::<Node>(thumb).unwrap();
        assert_eq!(node.left, Val::Percent(value.x * 100.0));
        assert_eq!(node.top, Val::Percent(value.y * 100.0));
    }
}
