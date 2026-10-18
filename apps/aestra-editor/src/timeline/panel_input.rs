//! One engine input boundary shared by timeline and Curves composition.
pub(crate) use super::navigation_cursor::EntityCursor;
#[cfg(test)]
pub(crate) use super::timeline_pointer::tests;
pub(crate) use super::timeline_pointer::{
    PointerClick, PointerDrag, PointerDragEnd, PointerDragStart,
};
