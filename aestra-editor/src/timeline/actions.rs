use super::*;

#[derive(Component, Event, Debug, Clone, Copy, PartialEq)]
pub(crate) enum TimelineAction {
    AdjustEffectDuration(f32),
    SetSnap(TimelineSnapMode),
    FrameAll,
    AddMarker,
    SelectMarker(MarkerId),
    DeleteMarker(MarkerId),
    AddChoreographyEvent,
    SelectChoreographyEvent(ChoreographyEventId),
    DeleteChoreographyEvent(ChoreographyEventId),
    SplitEmitterRegion,
    JoinEmitterRegion,
}

#[derive(Component, Event, Debug, Clone, PartialEq, Eq)]
pub(crate) enum ChoreographyAction {
    SelectEmitter(EmitterId),
    SelectEmitterRegion {
        emitter: EmitterId,
        region: EmitterRegionId,
    },
    SelectEffectClip(EffectClipId),
    SelectEffectClipEmitter {
        path: EffectClipPath,
        emitter: EmitterId,
    },
    SelectReferencedEffectClip(EffectClipPath),
    ToggleEffectClipExpanded(EffectClipPath),
    ToggleEffectClipMuted(EffectClipId),
    ToggleEffectClipSolo(EffectClipId),
    EditEffectClipSource(EffectClipId),
    EditEffectClipEmitterSource {
        path: EffectClipPath,
        emitter: EmitterId,
    },
    DeleteEffectClip(EffectClipId),
    AddEmitter,
    DuplicateEmitter(Option<EmitterId>),
    DuplicateSelectedEmitterRegions,
    DeleteEmitter(Option<EmitterId>),
    DeleteSelectedEmitterRegions,
    SetEmitterEnabled {
        emitter: EmitterId,
        enabled: bool,
    },
    ToggleEmitterSolo(EmitterId),
    ToggleEmitterColorPicker(EmitterId),
    ToggleEmitterAutomation(EmitterId),
    SetEmitterAutomationVisibility {
        emitter: EmitterId,
        lanes: Vec<AutomationLaneId>,
        visible: bool,
    },
    SetAutomationLaneVisibility {
        lane: AutomationLaneId,
        visible: bool,
    },
    SelectAutomationKey(TimelineAutomationKeySelection),
    AddAutomationKey(AutomationLaneId),
    AddAutomationKeyAt {
        lane: AutomationLaneId,
        normalized_time_bits: u32,
        value_bits: Option<u32>,
    },
    DeleteAutomationKey(TimelineAutomationKeySelection),
}
