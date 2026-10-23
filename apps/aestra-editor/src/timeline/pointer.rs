//! Bevy native pointer/control names only; no behavior translation.
pub(crate) use bevy::picking::events::{
    PointerClick, PointerDrag, PointerDragDrop, PointerDragEnd, PointerDragEnter, PointerDragLeave,
    PointerDragStart, PointerPress,
};
// Equivalent upstream marker for controls intentionally removed from button handling.
pub(super) use bevy::ui_widgets::Button as TimelineButton;

/// Actual upstream pointer location used for floating timeline window routing.
pub(super) trait TimelinePointerLocation {
    fn timeline_pointer_location(&self) -> bevy::picking::pointer::Location;
}
impl TimelinePointerLocation for PointerDrag {
    fn timeline_pointer_location(&self) -> bevy::picking::pointer::Location {
        self.pointer.location()
    }
}
impl TimelinePointerLocation for PointerClick {
    fn timeline_pointer_location(&self) -> bevy::picking::pointer::Location {
        self.pointer.location()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) fn drop_on(
        entity: Entity,
        source: Entity,
        window: Entity,
        button: PointerButton,
    ) -> PointerDragDrop {
        PointerDragDrop {
            entity,
            pointer: bevy::picking::events::Pointer::new(
                PointerId::Mouse,
                location(window, Vec2::ZERO),
            ),
            button,
            dropped: source,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        }
    }
    pub(crate) fn enter(
        entity: Entity,
        source: Entity,
        window: Entity,
        button: PointerButton,
    ) -> PointerDragEnter {
        PointerDragEnter {
            entity,
            pointer: bevy::picking::events::Pointer::new(
                PointerId::Mouse,
                location(window, Vec2::ZERO),
            ),
            button,
            dragged: source,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        }
    }
    pub(crate) fn leave(
        entity: Entity,
        source: Entity,
        window: Entity,
        button: PointerButton,
    ) -> PointerDragLeave {
        PointerDragLeave {
            entity,
            pointer: bevy::picking::events::Pointer::new(
                PointerId::Mouse,
                location(window, Vec2::ZERO),
            ),
            button,
            dragged: source,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        }
    }

    pub(crate) fn click_at(
        entity: Entity,
        window: Entity,
        button: PointerButton,
        count: u8,
        position: Vec2,
    ) -> PointerClick {
        let mut event = click(entity, window, button, count);
        event.pointer =
            bevy::picking::events::Pointer::new(PointerId::Mouse, location(window, position));
        event
    }

    pub(crate) fn click(
        entity: Entity,
        window: Entity,
        button: PointerButton,
        count: u8,
    ) -> PointerClick {
        PointerClick {
            entity,
            pointer: bevy::picking::events::Pointer::new(
                PointerId::Mouse,
                location(window, Vec2::ZERO),
            ),
            button,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            duration: std::time::Duration::ZERO,
            count,
        }
    }
    pub(crate) fn motion_button(
        entity: Entity,
        window: Entity,
        position: Vec2,
        id: PointerId,
        button: PointerButton,
    ) -> PointerDrag {
        PointerDrag {
            entity,
            pointer: bevy::picking::events::Pointer::new(id, location(window, position)),
            button,
            distance: Vec2::ZERO,
            delta: Vec2::ZERO,
        }
    }
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
    fn location(window: Entity, position: Vec2) -> Location {
        Location {
            target: RenderTarget::Window(WindowRef::Entity(window))
                .normalize(None)
                .unwrap(),
            position,
        }
    }
    pub(crate) fn start(
        entity: Entity,
        window: Entity,
        position: Vec2,
        id: PointerId,
        button: PointerButton,
    ) -> PointerDragStart {
        PointerDragStart {
            entity,
            pointer: bevy::picking::events::Pointer::new(id, location(window, position)),
            button,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        }
    }
    pub(crate) fn motion_delta(entity: Entity, window: Entity, delta: Vec2) -> PointerDrag {
        PointerDrag {
            entity,
            pointer: bevy::picking::events::Pointer::new(PointerId::Mouse, location(window, delta)),
            button: PointerButton::Primary,
            distance: delta,
            delta,
        }
    }
    pub(crate) fn end(
        entity: Entity,
        window: Entity,
        position: Vec2,
        id: PointerId,
        button: PointerButton,
    ) -> PointerDragEnd {
        PointerDragEnd {
            entity,
            pointer: bevy::picking::events::Pointer::new(id, location(window, position)),
            button,
            distance: Vec2::ZERO,
        }
    }
    pub(crate) fn end_distance(
        entity: Entity,
        window: Entity,
        distance: Vec2,
        button: PointerButton,
    ) -> PointerDragEnd {
        PointerDragEnd {
            entity,
            pointer: bevy::picking::events::Pointer::new(
                PointerId::Mouse,
                location(window, Vec2::ZERO),
            ),
            button,
            distance,
        }
    }
    pub(crate) fn press(
        entity: Entity,
        window: Entity,
        position: Vec2,
        id: PointerId,
        button: PointerButton,
    ) -> PointerPress {
        PointerPress {
            entity,
            pointer: bevy::picking::events::Pointer::new(id, location(window, position)),
            button,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            count: 1,
        }
    }
}
