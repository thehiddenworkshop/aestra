//! Native pointer event name; no synthetic dispatch or behavior translation.
pub(super) use bevy::picking::events::PointerPress;

#[cfg(test)]
pub(super) fn press(entity: bevy::prelude::Entity) -> PointerPress {
    use bevy::{
        camera::NormalizedRenderTarget,
        picking::{
            backend::HitData,
            events::Pointer,
            pointer::{Location, PointerButton, PointerId},
        },
        prelude::*,
    };
    PointerPress {
        entity,
        pointer: Pointer::new(
            PointerId::Mouse,
            Location {
                target: NormalizedRenderTarget::None {
                    width: 800,
                    height: 600,
                },
                position: Vec2::ZERO,
            },
        ),
        button: PointerButton::Primary,
        count: 1,
        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
    }
}
