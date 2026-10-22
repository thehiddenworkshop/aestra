//! Pointer click API boundary only: native event identity, location and propagation.
use bevy::prelude::*;
pub(crate) type PointerClick = bevy::picking::events::Pointer<bevy::picking::events::Click>;
pub(crate) type PointerPress = bevy::picking::events::Pointer<bevy::picking::events::Press>;
pub(crate) trait GraphClick {
    fn graph_click_position(&self) -> Vec2;
}
impl GraphClick for PointerClick {
    fn graph_click_position(&self) -> Vec2 {
        self.pointer_location.position
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
    Pointer::new(
        PointerId::Mouse,
        location,
        bevy::picking::events::Click {
            button,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            duration: std::time::Duration::ZERO,
            count: 1,
        },
        entity,
    )
}

#[cfg(test)]
pub(crate) fn with_click_count(mut click: PointerClick, count: u8) -> PointerClick {
    click.event.count = count;
    click
}

#[cfg(test)]
pub(crate) fn press(
    entity: Entity,
    window: Entity,
    button: bevy::picking::pointer::PointerButton,
) -> PointerPress {
    let click = click(entity, window, Vec2::ZERO, button);
    bevy::picking::events::Pointer::new(
        click.pointer_id,
        click.pointer_location,
        bevy::picking::events::Press {
            button,
            hit: click.event.hit,
            count: 1,
        },
        entity,
    )
}
