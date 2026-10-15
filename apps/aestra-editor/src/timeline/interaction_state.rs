//! Production timeline identities and transient interaction payloads, independent of UI handlers.
use aestra_core::{
    ChoreographyEventId, ChoreographyTrackId, EffectClipId, EmitterId, EmitterRegionId, MarkerId,
    ModuleId,
};
use bevy::prelude::*;
use std::time::Duration;

#[derive(Component)]
pub(super) struct TimelineInvalidDropFeedback {
    pub(super) timer: Timer,
    pub(super) rejected: bool,
}

#[derive(Event)]
pub(super) struct RejectProjectEffectDrop {
    pub(super) reason: String,
}

#[derive(Component)]
pub(super) struct TimelineDropFeedbackTitle;

#[derive(Component)]
pub(super) struct TimelineDropFeedbackMessage;

#[derive(Component, Clone, Copy)]
pub(super) struct TimelineTrackHeader {
    pub(super) track: ChoreographyTrackId,
}

#[derive(Component)]
pub(super) struct TimelineTrackContextMenuAnchor;

#[derive(Component)]
pub(super) struct TimelineTrackNameLabel;

#[derive(Component)]
pub(super) struct EmitterTrackDiagnostic;

#[derive(Component)]
pub(super) struct EmitterTrackDisabled;

#[derive(Component, Clone, Copy)]
pub(super) struct EffectClipTrackReorderHandle {
    pub(super) clip: EffectClipId,
}

impl Default for TimelineInvalidDropFeedback {
    fn default() -> Self {
        let mut timer = Timer::from_seconds(1.6, TimerMode::Once);
        timer.pause();
        Self {
            timer,
            rejected: false,
        }
    }
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum TimelineSet {
    Input,
    Actions,
    Visuals,
}

#[derive(Component)]
pub(super) struct TimelineCanvas;

#[derive(Component)]
pub(super) struct EmitterTrackColorSwatch {
    pub(super) emitter: EmitterId,
}

#[derive(Component)]
pub(super) struct EmitterTrackColorPicker {
    pub(super) emitter: EmitterId,
}

#[derive(Component, Clone, Copy)]
pub(super) struct TimelineClip {
    pub(super) emitter: EmitterId,
    pub(super) region: EmitterRegionId,
}

#[derive(Component)]
pub(super) struct TimelineTrackActionControl;

#[derive(Component, Clone, Copy)]
pub(super) struct EmitterTrackReorderHandle {
    pub(super) emitter: EmitterId,
}

#[derive(Component)]
pub(super) struct TimelineEffectClipControl;

#[derive(Component, Clone, Copy)]
pub(super) struct ReferencedEmitterTrackHeader;

#[derive(Component)]
pub(super) struct TimelineReferencedEmitterControl;

#[derive(Component)]
pub(super) struct TimelineAutomationLaneResizeHandle;

#[derive(Component)]
pub(super) struct TimelineAutomationLane;

#[derive(Component)]
pub(super) struct TimelineAutomationKey;

#[derive(Component, Clone, Copy)]
pub(super) struct TimelineEffectClipInteraction {
    pub(super) clip: EffectClipId,
    pub(super) kind: TimelineDragKind,
}

#[derive(Component, Clone, Copy)]
pub(super) struct TimelineMarker {
    pub(super) marker: MarkerId,
}

#[derive(Component, Clone, Copy)]
pub(super) struct TimelineChoreographyEvent {
    pub(super) event: ChoreographyEventId,
}

#[derive(Component, Clone, Copy)]
pub(super) struct TimelineClipInteraction {
    pub(super) emitter: EmitterId,
    pub(super) region: EmitterRegionId,
    pub(super) kind: TimelineDragKind,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TimelineVerticalPane {
    Headers,
    Tracks,
}

#[derive(Component, Clone, Copy)]
pub(super) struct EmitterTrackHeader {
    pub(super) emitter: EmitterId,
}

#[derive(Component)]
pub(super) struct EffectClipTrackHeader {
    pub(super) clip: EffectClipId,
}

#[derive(Component)]
pub(super) struct EffectClipTrackContextMenu;

#[derive(Component)]
pub(super) struct EmitterTrackColorChip;

#[derive(Component)]
pub(super) struct EmitterTrackColorPickerPopover;

#[derive(Component)]
pub(super) struct EmitterTrackContextMenu;

#[derive(Component)]
pub(super) struct EmitterAutomationMenuButton;

#[derive(Component)]
pub(super) struct EmitterAutomationVisibilityMenu;

#[derive(Clone, Copy, Debug)]
pub(super) struct TimelineMarkerDrag {
    pub(super) marker: MarkerId,
    pub(super) original_time: f32,
    pub(super) current_time: f32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct TimelineChoreographyEventDrag {
    pub(super) event: ChoreographyEventId,
    pub(super) original_time: f32,
    pub(super) current_time: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TimelineDragKind {
    Move,
    TrimStart,
    TrimEnd,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct EffectClipTimelineDrag {
    pub(super) clip: EffectClipId,
    pub(super) kind: TimelineDragKind,
    pub(super) pointer_start: f32,
    pub(super) original_start: f32,
    pub(super) original_source_offset: f32,
    pub(super) original_duration: f32,
    pub(super) current_start: f32,
    pub(super) current_source_offset: f32,
    pub(super) current_duration: f32,
    pub(super) source_duration: f32,
    pub(super) source_looping: bool,
}

#[derive(Clone, Debug)]
pub(super) struct EffectDropPreview {
    pub(super) source_duration: f32,
    pub(super) display_name: String,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct TimelineDrag {
    pub(super) emitter: EmitterId,
    pub(super) region: EmitterRegionId,
    pub(super) kind: TimelineDragKind,
    pub(super) pointer_start: f32,
    pub(super) original_start: f32,
    pub(super) original_duration: f32,
    pub(super) original_source_offset: f32,
    pub(super) current_start: f32,
    pub(super) current_duration: f32,
    pub(super) current_source_offset: f32,
    pub(super) source_duration: f32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct TimelineRegionMove {
    pub(super) region: EmitterRegionId,
    pub(super) original_start: f32,
    pub(super) duration: f32,
}

#[derive(Component, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct AutomationLaneId {
    pub(super) emitter: EmitterId,
    pub(super) module: ModuleId,
    pub(super) input: u8,
    pub(super) parameter: String,
    pub(super) channel: Option<u8>,
}

#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(crate) struct TimelineAutomationKeySelection {
    pub(super) lane: AutomationLaneId,
    pub(super) key: usize,
}

#[derive(Clone, Debug)]
pub(super) struct TimelineAutomationKeyDrag {
    pub(super) selection: TimelineAutomationKeySelection,
    pub(super) source_start: f32,
    pub(super) source_duration: f32,
    pub(super) original_time: f32,
    pub(super) current_time: f32,
    pub(super) original_value: Option<f32>,
    pub(super) current_value: Option<f32>,
}

#[derive(Component, Clone)]
pub(super) struct TimelineAutomationLaneGraph(pub(super) AutomationLaneId);

#[derive(Clone, Debug)]
pub(super) struct ReferencedEmitterClick {
    pub(super) path: EffectClipPath,
    pub(super) emitter: EmitterId,
    pub(super) at: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct EffectClipPath(Vec<EffectClipId>);

impl EffectClipPath {
    pub(crate) fn root_path(clip: EffectClipId) -> Self {
        Self(vec![clip])
    }

    pub(crate) fn child(&self, clip: EffectClipId) -> Self {
        let mut path = self.0.clone();
        path.push(clip);
        Self(path)
    }

    pub(crate) fn root(&self) -> EffectClipId {
        self.0[0]
    }

    pub(crate) fn ids(&self) -> &[EffectClipId] {
        &self.0
    }

    pub(super) fn starts_with(&self, ancestor: &Self) -> bool {
        self.0.starts_with(&ancestor.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EffectClipChildSelection {
    EffectClip {
        path: EffectClipPath,
    },
    Emitter {
        path: EffectClipPath,
        emitter: EmitterId,
    },
}

impl EffectClipChildSelection {
    pub(super) fn is_descendant_of(&self, ancestor: &EffectClipPath) -> bool {
        match self {
            Self::EffectClip { path } | Self::Emitter { path, .. } => path.starts_with(ancestor),
        }
    }
}

#[derive(Component)]
pub(super) struct TimelineEffectDropPreview;

#[derive(Component)]
pub(super) struct TimelineEffectDropPreviewLabel;

#[derive(Component)]
pub(super) struct TimelineEffectDropSpacer;

#[derive(Component, Clone, Copy)]
pub(super) struct TimelineChoreographyGridRow(pub(super) i16);

#[derive(Component)]
pub(super) struct TimelineHorizontalGutter;

#[derive(Component)]
pub(super) struct TimelineHorizontalScrollbarGutter;

#[derive(Component)]
pub(super) struct TimelineVerticalScrollbarGutter;

#[derive(Component, Clone, Copy)]
pub(super) struct TimelineTrackDropRow {
    pub(super) track: ChoreographyTrackId,
}

#[derive(Component, Clone, Copy)]
pub(super) struct TimelineEffectClip {
    pub(super) clip: EffectClipId,
}

#[derive(Component)]
pub(super) struct TimelineRulerTick(pub(super) usize);

#[derive(Component)]
pub(super) struct TimelineSnapGuide;

#[derive(Component)]
pub(super) struct TimelineHorizontalScrollViewport;

#[derive(Component)]
pub(super) struct TimelineHorizontalScrollContent;

#[derive(Component)]
pub(super) struct Playhead;

#[derive(Component)]
pub(super) struct TimeLabel;

#[derive(Component)]
pub(super) struct TimelineRegionToolButton;

#[derive(Component, Clone, Copy)]
pub(super) struct TimelineReferencedEmitter {
    pub(super) clip: EffectClipId,
    pub(super) source_start: f32,
    pub(super) source_duration: f32,
}
