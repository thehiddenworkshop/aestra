//! Pointer click API boundary only: native event identity, location and propagation.
pub(crate) use bevy::picking::events::{PointerClick, PointerPress};
use bevy::prelude::*;
pub(crate) trait GraphClick {
    fn graph_click_position(&self) -> Vec2;
}
impl GraphClick for PointerClick {
    fn graph_click_position(&self) -> Vec2 {
        self.pointer.location().position
    }
}
#[cfg(test)]
pub(crate) fn click(
    entity: Entity,
    window: Entity,
    position: Vec2,
    button: bevy::picking::pointer::PointerButton,
) -> PointerClick {
    use bevy::{
        camera::RenderTarget,
        picking::{
            backend::HitData,
            events::Pointer,
            pointer::{Location, PointerId},
        },
        window::WindowRef,
    };
    let location = Location {
        target: RenderTarget::Window(WindowRef::Entity(window))
            .normalize(None)
            .unwrap(),
        position,
    };
    PointerClick {
        entity,
        pointer: Pointer::new(PointerId::Mouse, location),
        button,
        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        duration: std::time::Duration::ZERO,
        count: 1,
    }
}

#[cfg(test)]
pub(crate) fn with_click_count(mut click: PointerClick, count: u8) -> PointerClick {
    click.count = count;
    click
}

#[cfg(test)]
pub(crate) fn press(
    entity: Entity,
    window: Entity,
    button: bevy::picking::pointer::PointerButton,
) -> PointerPress {
    let click = click(entity, window, Vec2::ZERO, button);
    PointerPress {
        entity,
        pointer: click.pointer,
        button,
        hit: click.hit,
        count: 1,
    }
}
