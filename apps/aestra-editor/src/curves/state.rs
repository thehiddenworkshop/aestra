//! Actual curve-panel selection and transient host-curve drag state.
use aestra_core::{EffectId, HostTransformTrack, ModuleId};
use bevy::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ComplexSelection {
    pub(crate) module: ModuleId,
    pub(crate) input: u8,
    pub(crate) key: usize,
}

#[derive(Resource, Default)]
pub(crate) struct CurvesState {
    pub(super) host: Option<Selection>,
    pub(super) host_drag: Option<DragState>,
    pub(super) host_released: Option<Entity>,
    pub(super) complex: Option<ComplexSelection>,
    pub(super) vector_channel: u8,
}

impl CurvesState {
    pub(crate) fn clear(&mut self) {
        self.host = None;
        self.host_drag = None;
        self.host_released = None;
        self.complex = None;
        self.vector_channel = 0;
    }

    pub(crate) fn select_key_channel(
        &mut self,
        module: ModuleId,
        input: u8,
        key: usize,
        vector_channel: Option<u8>,
    ) {
        self.host = None;
        self.complex = Some(ComplexSelection { module, input, key });
        self.vector_channel = vector_channel.unwrap_or(0).min(2);
    }

    pub(crate) fn selected_key(&self) -> Option<ComplexSelection> {
        self.complex
    }

    pub(crate) fn selected_vector_channel(&self) -> u8 {
        self.vector_channel
    }

    #[cfg(test)]
    pub(crate) fn has_selection(&self) -> bool {
        self.complex.is_some()
    }

    #[cfg(test)]
    pub(crate) fn select_for_test(&mut self, module: ModuleId, input: u8, key: usize) {
        self.complex = Some(ComplexSelection { module, input, key });
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Selection {
    pub(super) effect: EffectId,
    pub(super) channel: u8,
    pub(super) key: usize,
}

pub(super) struct DragState {
    pub(super) selection: Selection,
    pub(super) original: HostTransformTrack,
    pub(super) candidate: HostTransformTrack,
    pub(super) time: f32,
}
