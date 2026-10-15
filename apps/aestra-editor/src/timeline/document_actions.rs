//! Actual timeline document action contract and session-backed marker/event edits.
//! The native observer retains timeline selection/view/region cleanup.
use crate::{EditorSession, localization::Localizer};
use aestra_authoring::EffectCommand;
use aestra_core::{
    ChoreographyEvent, ChoreographyEventId, ChoreographyEventPayload, EffectMarker, MarkerId,
};
use bevy::prelude::*;

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TimelineSnapMode {
    None,
    Frames,
    Seconds,
    #[default]
    Smart,
}

impl TimelineSnapMode {
    pub(super) const ALL: [Self; 4] = [Self::None, Self::Frames, Self::Seconds, Self::Smart];

    pub(super) fn message_id(self) -> &'static str {
        match self {
            Self::None => "timeline-snap-off",
            Self::Frames => "timeline-snap-frames",
            Self::Seconds => "timeline-snap-time",
            Self::Smart => "timeline-snap-smart",
        }
    }
}

/// Returns false for actions owned by the native timeline view/region adapter.
/// A handled action remains handled when semantic validation rejects it.
pub(crate) fn apply(
    action: TimelineAction,
    session: &mut EditorSession,
    localizer: Option<&Localizer>,
) -> bool {
    match action {
        TimelineAction::AddMarker => {
            let index = session.effect.markers.len();
            let marker_label = localizer.map_or_else(
                || "Marker".to_owned(),
                |localizer| localizer.text("timeline-marker"),
            );
            let marker = EffectMarker::new(format!("{marker_label} {}", index + 1), session.time());
            let id = marker.id;
            let command_label = localizer.map_or_else(
                || "Added timeline marker".to_owned(),
                |localizer| localizer.text("timeline-add-marker-command"),
            );
            if session.execute(
                &command_label,
                EffectCommand::AddMarker { marker, index },
                true,
            ) {
                session.select_marker(id);
            }
        }
        TimelineAction::SelectMarker(id) => {
            session.select_marker(id);
        }
        TimelineAction::DeleteMarker(id) => {
            let label = localizer.map_or_else(
                || "Deleted timeline marker".to_owned(),
                |localizer| localizer.text("timeline-delete-marker-command"),
            );
            session.execute(label, EffectCommand::RemoveMarker { id }, true);
        }
        TimelineAction::AddChoreographyEvent => {
            let index = session.effect.choreography_events.len();
            let event = ChoreographyEvent::new(
                format!("Event {}", index + 1),
                session.time(),
                ChoreographyEventPayload::GameplayNotify {
                    topic: String::new(),
                },
            );
            let id = event.id;
            if session.execute(
                "Added choreography event",
                EffectCommand::AddChoreographyEvent { event, index },
                true,
            ) {
                session.select_choreography_event(id);
            }
        }
        TimelineAction::SelectChoreographyEvent(id) => {
            session.select_choreography_event(id);
        }
        TimelineAction::DeleteChoreographyEvent(id) => {
            let label = localizer.map_or_else(
                || "Deleted choreography event".to_owned(),
                |localizer| localizer.text("timeline-delete-event-command"),
            );
            session.execute(label, EffectCommand::RemoveChoreographyEvent { id }, true);
        }
        _ => return false,
    }
    true
}

#[cfg(test)]
#[path = "document_actions/tests.rs"]
mod tests;
