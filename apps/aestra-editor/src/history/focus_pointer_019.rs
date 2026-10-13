//! Shipping pointer event name; behavior remains in the shared history focus observer.
pub(super) type PointerPress = bevy::picking::events::Pointer<bevy::picking::events::Press>;

#[cfg(test)]
pub(super) fn press(entity: bevy::prelude::Entity) -> PointerPress {
    use bevy::{
        camera::NormalizedRenderTarget,
        picking::{
            backend::HitData,
            pointer::{Location, PointerButton, PointerId},
        },
        prelude::*,
    };
    Pointer::new(
        PointerId::Mouse,
        Location {
            target: NormalizedRenderTarget::None {
                width: 800,
                height: 600,
            },
            position: Vec2::ZERO,
        },
        Press {
            button: PointerButton::Primary,
            count: 1,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        },
        entity,
    )
}
