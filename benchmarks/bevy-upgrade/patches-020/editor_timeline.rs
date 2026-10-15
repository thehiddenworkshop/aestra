//! Actual timeline state and keyboard consumer; full native action/UI composition stays separate.
use crate::material_graph::MaterialGraphViewport;
use crate::{EditorSession, ModulePaletteState};
use aestra_authoring::SemanticTarget;
use aestra_core::{
    ChoreographyTrackId, EffectAsset, EffectClip, EffectClipId, Emitter, EmitterId, EmitterRegionId,
};
use bevy::{input_focus::InputFocus, prelude::*, text::EditableText, ui::RelativeCursorPosition};
use std::collections::{BTreeMap, BTreeSet};

#[path = "../../../apps/aestra-editor/src/timeline/interaction_state.rs"]
mod interaction_state;
use interaction_state::*;
#[path = "../../../apps/aestra-editor/src/timeline/state.rs"]
mod state;
use state::TimelineState;
#[path = "../../../apps/aestra-editor/src/timeline/asset_drop/source.rs"]
mod asset_drop;
#[path = "../../../apps/aestra-editor/src/timeline/automation.rs"]
mod automation;
#[path = "../../../apps/aestra-editor/src/timeline/automation_gestures.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "Production system dependencies match shipping editor lint policy"
)]
mod automation_gestures;
#[path = "../../../apps/aestra-editor/src/timeline/clicks.rs"]
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "Production system dependencies match shipping editor lint policy"
)]
mod clicks;
#[path = "../../../apps/aestra-editor/src/timeline/clip_gestures.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "Production system dependencies match shipping editor lint policy"
)]
mod clip_gestures;
#[path = "../../../apps/aestra-editor/src/timeline/drop_execution.rs"]
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "Production system dependencies match shipping editor lint policy"
)]
mod drop_execution;
#[path = "../../../apps/aestra-editor/src/timeline/gesture_math.rs"]
mod gesture_math;
#[path = "../../../apps/aestra-editor/src/timeline/gestures.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "Production system dependencies match shipping editor lint policy"
)]
mod gestures;
#[path = "../../../apps/aestra-editor/src/timeline/lane_composition.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "Production composition dependencies match shipping editor lint policy"
)]
mod lane_composition;
#[path = "../../../apps/aestra-editor/src/timeline/motion_execution.rs"]
mod motion_execution;
#[path = "../../../apps/aestra-editor/src/timeline/motion_gestures.rs"]
mod motion_gestures;
#[path = "../../../apps/aestra-editor/src/timeline/motion_state.rs"]
mod motion_state;
#[path = "../../../apps/aestra-editor/src/timeline/navigation.rs"]
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "Production system dependencies match shipping editor lint policy"
)]
mod navigation;
#[path = "../../../apps/aestra-editor/src/timeline/cursor_020.rs"]
mod navigation_cursor;
#[path = "../../../apps/aestra-editor/src/timeline/region_gestures.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "Production system dependencies match shipping editor lint policy"
)]
mod region_gestures;
#[path = "../../../apps/aestra-editor/src/timeline/reorder.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "Production system dependencies match shipping editor lint policy"
)]
mod reorder;
#[path = "../../../apps/aestra-editor/src/timeline/pointer_020.rs"]
mod timeline_pointer;
mod host_motion {
    pub(super) use super::motion_state::keyboard_input;
}
#[path = "../../../apps/aestra-editor/src/timeline/actions.rs"]
mod actions;
use actions::{ChoreographyAction, TimelineAction};
#[path = "../../../apps/aestra-editor/src/timeline/action_controls.rs"]
#[allow(
    clippy::type_complexity,
    reason = "Production system queries match shipping editor lint policy"
)]
mod action_controls;
#[path = "../../../apps/aestra-editor/src/timeline/control_input_020.rs"]
#[allow(
    clippy::type_complexity,
    reason = "Version-specific production control query"
)]
mod control_input;
#[path = "../../../apps/aestra-editor/src/timeline/document_actions.rs"]
mod document_actions;
use document_actions::TimelineSnapMode;
#[path = "../../../apps/aestra-editor/src/timeline/automation_data.rs"]
mod automation_data;
#[path = "../../../apps/aestra-editor/src/timeline/automation_execution.rs"]
mod automation_execution;
use automation_data::add_automation_key_at_pointer_action;
#[cfg(test)]
#[path = "../../../apps/aestra-editor/src/timeline/action_execution_tests.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "Observer adapter matches the native automation handler's explicit dependencies"
)]
mod action_execution_tests;
#[path = "../../../apps/aestra-editor/src/timeline/execution.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "Production observer dependencies match shipping editor lint policy"
)]
mod execution;
#[cfg(test)]
#[path = "../../../apps/aestra-editor/src/timeline/execution_tests.rs"]
mod execution_tests;
#[path = "../../../apps/aestra-editor/src/timeline/keyboard.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "Explicit keyboard system dependencies match shipping editor lint policy"
)]
mod keyboard;
#[path = "../../../apps/aestra-editor/src/timeline/referenced_execution.rs"]
mod referenced_execution;
#[path = "../../../apps/aestra-editor/src/timeline/region_execution.rs"]
mod region_execution;

#[path = "../../../apps/aestra-editor/src/timeline/cue_composition.rs"]
mod cue_composition;
#[path = "../../../apps/aestra-editor/src/timeline/effect_clip_composition.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "Production composition dependencies match shipping editor lint policy"
)]
mod effect_clip_composition;
#[path = "../../../apps/aestra-editor/src/timeline/emitter_composition.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "Production composition dependencies match shipping lint policy"
)]
mod emitter_composition;
#[path = "../../../apps/aestra-editor/src/timeline/presentation.rs"]
#[allow(
    clippy::type_complexity,
    clippy::too_many_arguments,
    reason = "Production system dependencies match shipping editor lint policy"
)]
mod presentation;
#[path = "../../../apps/aestra-editor/src/timeline/projections.rs"]
mod projections;
#[path = "../../../apps/aestra-editor/src/timeline/referenced_effect.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "Production composition dependencies match shipping editor lint policy"
)]
mod referenced_effect;
#[path = "../../../apps/aestra-editor/src/timeline/toolbar_composition.rs"]
mod toolbar_composition;
#[path = "../../../apps/aestra-editor/src/timeline/track_style.rs"]
mod track_style;
