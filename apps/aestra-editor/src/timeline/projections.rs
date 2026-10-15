//! Actual nested effect-track projection and visible automation layout metadata.
use super::TimelineState;
use super::interaction_state::*;
use super::state::normalized_choreography_order;
use crate::ProjectEffectCatalog;
use crate::feathers::automation_curve;
use aestra_core::{ChoreographyTrackId, EffectAsset, EffectAssetRef, EffectClip, Emitter, Value};

#[derive(Clone, Copy, Debug)]
pub(super) struct ReferencedTimingContext {
    pub(super) root_source_start: f32,
    pub(super) local_source_offset: f32,
    pub(super) duration: f32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ReferencedTrackTiming {
    pub(super) root_source_start: f32,
    pub(super) duration: f32,
}

#[derive(Clone, Debug)]
pub(super) enum ReferencedTrackKind {
    EffectClip {
        clip: EffectClip,
        source_name: String,
        child_count: usize,
    },
    Emitter {
        emitter: Emitter,
    },
}

#[derive(Clone, Debug)]
pub(super) struct ReferencedTrackProjection {
    pub(super) path: EffectClipPath,
    pub(super) depth: usize,
    pub(super) timing: Option<ReferencedTrackTiming>,
    pub(super) kind: ReferencedTrackKind,
}

pub(super) fn effect_clip_source_name(
    catalog: &ProjectEffectCatalog,
    source: EffectAssetRef,
) -> String {
    catalog
        .entries()
        .iter()
        .find(|entry| entry.reference == Some(source))
        .map_or_else(
            || format!("Missing effect {}", source.id),
            |entry| entry.display_name.clone(),
        )
}

pub(super) fn map_referenced_interval(
    context: ReferencedTimingContext,
    item_start: f32,
    item_duration: f32,
) -> Option<(ReferencedTrackTiming, f32)> {
    let source_start = item_start.max(context.local_source_offset);
    let source_end =
        (item_start + item_duration).min(context.local_source_offset + context.duration);
    (source_end > source_start).then_some((
        ReferencedTrackTiming {
            root_source_start: context.root_source_start + source_start
                - context.local_source_offset,
            duration: source_end - source_start,
        },
        source_start - item_start,
    ))
}

fn append_referenced_track_projections(
    rows: &mut Vec<ReferencedTrackProjection>,
    catalog: &ProjectEffectCatalog,
    state: &TimelineState,
    source: &EffectAsset,
    parent_path: &EffectClipPath,
    context: Option<ReferencedTimingContext>,
    depth: usize,
) {
    for track in normalized_choreography_order(source) {
        match track {
            ChoreographyTrackId::EffectClip(id) => {
                let Some(clip) = source.effect_clips.iter().find(|clip| clip.id == id) else {
                    continue;
                };
                let path = parent_path.child(clip.id);
                let mapped = context.and_then(|context| {
                    map_referenced_interval(context, clip.start_time, clip.duration)
                });
                let child_source = catalog.cached_effect(clip.source).ok();
                let child_count = child_source.as_ref().map_or(0, |effect| {
                    effect.effect_clips.len() + effect.emitters.len()
                });
                rows.push(ReferencedTrackProjection {
                    path: path.clone(),
                    depth,
                    timing: mapped.map(|(timing, _)| timing),
                    kind: ReferencedTrackKind::EffectClip {
                        clip: clip.clone(),
                        source_name: effect_clip_source_name(catalog, clip.source),
                        child_count,
                    },
                });
                if state.expanded_effect_clips.contains(&path)
                    && let Some(child_source) = child_source
                {
                    let child_context =
                        mapped.map(|(timing, clipped_offset)| ReferencedTimingContext {
                            root_source_start: timing.root_source_start,
                            local_source_offset: clip.source_offset + clipped_offset,
                            duration: timing.duration,
                        });
                    append_referenced_track_projections(
                        rows,
                        catalog,
                        state,
                        &child_source,
                        &path,
                        child_context,
                        depth + 1,
                    );
                }
            }
            ChoreographyTrackId::Emitter(id) => {
                let Some(emitter) = source.emitters.iter().find(|emitter| emitter.id == id) else {
                    continue;
                };
                rows.push(ReferencedTrackProjection {
                    path: parent_path.clone(),
                    depth,
                    timing: context
                        .and_then(|context| {
                            map_referenced_interval(context, emitter.start_time, emitter.duration)
                        })
                        .map(|(timing, _)| timing),
                    kind: ReferencedTrackKind::Emitter {
                        emitter: emitter.clone(),
                    },
                });
            }
        }
    }
}

pub(super) fn referenced_track_projections(
    catalog: &ProjectEffectCatalog,
    state: &TimelineState,
    clip: &EffectClip,
) -> Vec<ReferencedTrackProjection> {
    let path = EffectClipPath::root_path(clip.id);
    if !state.expanded_effect_clips.contains(&path) {
        return Vec::new();
    }
    let Ok(source) = catalog.cached_effect(clip.source) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    append_referenced_track_projections(
        &mut rows,
        catalog,
        state,
        &source,
        &path,
        Some(ReferencedTimingContext {
            root_source_start: clip.source_offset,
            local_source_offset: clip.source_offset,
            duration: clip.duration,
        }),
        1,
    );
    rows
}

pub(super) fn visible_automation_lane_count(state: &TimelineState, emitter: &Emitter) -> usize {
    state
        .visible_automation_lanes
        .iter()
        .filter(|lane| emitter_has_automation_lane(emitter, lane))
        .count()
}

pub(super) fn emitter_has_automation_lane(emitter: &Emitter, lane: &AutomationLaneId) -> bool {
    if lane.emitter != emitter.id {
        return false;
    }
    let Some(module) = emitter
        .modules
        .iter()
        .find(|module| module.id == lane.module)
    else {
        return false;
    };
    matches!(
        (
            module.property_source(&lane.parameter),
            module.active_parameter_value(&lane.parameter),
            lane.channel,
        ),
        (
            Some(aestra_core::PropertySource::Curve(_)),
            Some(Value::Vec3Curve(_)),
            Some(0..=2)
        ) | (
            Some(aestra_core::PropertySource::Curve(_)),
            Some(Value::Curve(_)),
            None
        ) | (
            Some(aestra_core::PropertySource::Gradient(_)),
            Some(Value::Gradient(_)),
            None
        )
    )
}

pub(super) fn automation_lanes_height(state: &TimelineState, emitter: &Emitter) -> f32 {
    let default_height = automation_curve::DEFAULT_HEIGHT;
    visible_automation_lane_count(state, emitter) as f32 * default_height
        + state
            .automation_lane_heights
            .iter()
            .filter(|(lane, _)| {
                emitter_has_automation_lane(emitter, lane)
                    && state.visible_automation_lanes.contains(*lane)
            })
            .map(|(_, height)| *height - default_height)
            .sum::<f32>()
}
