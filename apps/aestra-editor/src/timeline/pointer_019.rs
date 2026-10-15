//! Bevy 0.19 shipping pointer/control names only; no behavior translation.
use bevy::picking::events::{
    Click, Drag, DragDrop, DragEnd, DragEnter, DragLeave, DragStart, Pointer, Press,
};
// Equivalent upstream marker for controls intentionally removed from button handling.
pub(super) use bevy::prelude::Button as TimelineButton;
pub(super) type PointerClick = Pointer<Click>;
pub(super) type PointerPress = Pointer<Press>;
pub(super) type PointerDragStart = Pointer<DragStart>;
pub(super) type PointerDrag = Pointer<Drag>;
pub(super) type PointerDragEnd = Pointer<DragEnd>;
pub(super) type PointerDragDrop = Pointer<DragDrop>;
pub(super) type PointerDragEnter = Pointer<DragEnter>;
pub(super) type PointerDragLeave = Pointer<DragLeave>;

/// Actual upstream pointer location used for floating timeline window routing.
pub(super) trait TimelinePointerLocation {
    fn timeline_pointer_location(&self) -> bevy::picking::pointer::Location;
}
impl TimelinePointerLocation for PointerDrag {
    fn timeline_pointer_location(&self) -> bevy::picking::pointer::Location {
        self.pointer_location.clone()
    }
}
impl TimelinePointerLocation for PointerClick {
    fn timeline_pointer_location(&self) -> bevy::picking::pointer::Location {
        self.pointer_location.clone()
    }
}

#[cfg(test)]
pub(super) mod tests {
    pub(crate) fn drop_on(
        entity: Entity,
        source: Entity,
        window: Entity,
        button: PointerButton,
    ) -> PointerDragDrop {
        Pointer::new(
            PointerId::Mouse,
            location(window, Vec2::ZERO),
            DragDrop {
                button,
                dropped: source,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            entity,
        )
    }
    pub(crate) fn enter(
        entity: Entity,
        source: Entity,
        window: Entity,
        button: PointerButton,
    ) -> PointerDragEnter {
        Pointer::new(
            PointerId::Mouse,
            location(window, Vec2::ZERO),
            DragEnter {
                button,
                dragged: source,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            entity,
        )
    }
    pub(crate) fn leave(
        entity: Entity,
        source: Entity,
        window: Entity,
        button: PointerButton,
    ) -> PointerDragLeave {
        Pointer::new(
            PointerId::Mouse,
            location(window, Vec2::ZERO),
            DragLeave {
                button,
                dragged: source,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            entity,
        )
    }

    pub(crate) fn click_at(
        entity: Entity,
        window: Entity,
        button: PointerButton,
        count: u8,
        position: Vec2,
    ) -> PointerClick {
        let mut event = click(entity, window, button, count);
        event.pointer_location = location(window, position);
        event
    }

    pub(crate) fn click(
        entity: Entity,
        window: Entity,
        button: PointerButton,
        count: u8,
    ) -> PointerClick {
        Pointer::new(
            PointerId::Mouse,
            location(window, Vec2::ZERO),
            Click {
                button,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                duration: std::time::Duration::ZERO,
                count,
            },
            entity,
        )
    }
    pub(crate) fn motion_button(
        entity: Entity,
        window: Entity,
        position: Vec2,
        id: PointerId,
        button: PointerButton,
    ) -> PointerDrag {
        Pointer::new(
            id,
            location(window, position),
            Drag {
                button,
                distance: Vec2::ZERO,
                delta: Vec2::ZERO,
            },
            entity,
        )
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
        Pointer::new(
            id,
            location(window, position),
            DragStart {
                button,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            },
            entity,
        )
    }
    pub(crate) fn motion_delta(entity: Entity, window: Entity, delta: Vec2) -> PointerDrag {
        Pointer::new(
            PointerId::Mouse,
            location(window, delta),
            Drag {
                button: PointerButton::Primary,
                distance: delta,
                delta,
            },
            entity,
        )
    }
    pub(crate) fn end(
        entity: Entity,
        window: Entity,
        position: Vec2,
        id: PointerId,
        button: PointerButton,
    ) -> PointerDragEnd {
        Pointer::new(
            id,
            location(window, position),
            DragEnd {
                button,
                distance: Vec2::ZERO,
            },
            entity,
        )
    }
    pub(crate) fn press(
        entity: Entity,
        window: Entity,
        position: Vec2,
        id: PointerId,
        button: PointerButton,
    ) -> PointerPress {
        Pointer::new(
            id,
            location(window, position),
            Press {
                button,
                hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                count: 1,
            },
            entity,
        )
    }
}
