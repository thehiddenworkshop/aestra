//! Actual extraction-dialog state shared with shortcut ownership.
use aestra_core::EmitterId;
use bevy::prelude::*;

#[derive(Resource, Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AssetOperationState {
    pub(super) extraction: Option<ReusableEffectExtractionState>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReusableEffectExtractionState {
    pub(super) emitters: Vec<EmitterId>,
    pub(super) draft: String,
    pub(super) replace_selection: bool,
    pub(super) error: Option<String>,
}

impl AssetOperationState {
    pub(crate) fn is_open(&self) -> bool {
        self.extraction.is_some()
    }

    pub(super) fn close_all(&mut self) {
        self.extraction = None;
    }
}

#[cfg(test)]
impl AssetOperationState {
    pub(crate) fn open_for_test() -> Self {
        Self {
            extraction: Some(ReusableEffectExtractionState {
                emitters: vec![EmitterId::new()],
                draft: "Reusable smoke".into(),
                replace_selection: true,
                error: Some("Retryable error".into()),
            }),
        }
    }
}
