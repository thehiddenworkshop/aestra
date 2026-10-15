//! Production emitter-region semantic edits; pointer controllers remain separate.
use super::TimelineState;
use super::execution::preview_selected_emitter_deletion;
use crate::{CurvesState, EditorSession, localization::Localizer};
use aestra_authoring::{EffectCommand, EffectTransaction};
use aestra_core::{Emitter, EmitterId, EmitterRegion, EmitterRegionId};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct EmitterRegionMerge {
    pub(super) merged_region: EmitterRegionId,
    pub(super) regions: Vec<EmitterRegion>,
    pub(super) emitter_start_time: f32,
    pub(super) emitter_duration: f32,
}

pub(super) fn merge_selected_emitter_regions(
    effect_duration: f32,
    emitter: &Emitter,
    selected: &BTreeSet<EmitterRegionId>,
) -> Option<EmitterRegionMerge> {
    if selected.len() < 2 {
        return None;
    }
    let mut regions = emitter.timeline_regions();
    regions.sort_by(|left, right| left.start_time.total_cmp(&right.start_time));
    let selected_regions = regions
        .iter()
        .copied()
        .filter(|region| selected.contains(&region.id))
        .collect::<Vec<_>>();
    if selected_regions.len() != selected.len() {
        return None;
    }

    let first = *selected_regions.first()?;
    let merged_start = selected_regions
        .iter()
        .map(|region| region.start_time)
        .reduce(f32::min)?;
    let merged_end = selected_regions
        .iter()
        .map(|region| region.end_time())
        .reduce(f32::max)?;
    let merged_duration = merged_end - merged_start;
    let emitter_duration = emitter
        .duration
        .max(first.source_offset + merged_duration)
        .min(effect_duration);
    let emitter_start_time = emitter
        .start_time
        .min((effect_duration - emitter_duration).max(0.0));
    let source_offset = first
        .source_offset
        .min((emitter_duration - merged_duration).max(0.0));

    regions.retain(|region| !selected.contains(&region.id));
    regions.push(EmitterRegion {
        id: first.id,
        start_time: merged_start,
        source_offset,
        duration: merged_duration,
    });
    regions.sort_by(|left, right| left.start_time.total_cmp(&right.start_time));

    let mut prospective = emitter.clone();
    prospective.start_time = emitter_start_time;
    prospective.duration = emitter_duration;
    let regions = prospective.normalize_timeline_regions(regions);
    Some(EmitterRegionMerge {
        merged_region: first.id,
        regions,
        emitter_start_time,
        emitter_duration,
    })
}

pub(super) fn split_selected_region_at_playhead(
    session: &mut EditorSession,
    state: &mut TimelineState,
) -> bool {
    let selected_region = state.selected_emitter_region;
    let Some(emitter_id) = selected_region
        .map(|(emitter, _)| emitter)
        .or_else(|| session.selection.emitter(&session.effect))
    else {
        session.status = "Select an emitter before splitting".into();
        return false;
    };
    let playhead = session.time();
    let Some(emitter) = session
        .effect
        .emitters
        .iter()
        .find(|emitter| emitter.id == emitter_id)
    else {
        return false;
    };
    let preferred = selected_region
        .filter(|(selected_emitter, _)| *selected_emitter == emitter_id)
        .and_then(|(_, region)| emitter.timeline_region(region))
        .filter(|region| playhead >= region.start_time && playhead <= region.end_time());
    let region = preferred.or_else(|| {
        emitter
            .timeline_regions()
            .into_iter()
            .find(|region| playhead >= region.start_time && playhead <= region.end_time())
    });
    let Some(region) = region else {
        session.status = "Move the playhead inside the selected emitter region".into();
        return false;
    };
    let minimum_duration = (1.0 / session.tick_rate().max(1) as f32).max(0.001);
    if playhead < region.start_time + minimum_duration
        || playhead > region.end_time() - minimum_duration
    {
        session.status = "Move the playhead farther from the region boundary".into();
        return false;
    }
    let Some(regions) = emitter.split_timeline_region(region.id, playhead, EmitterRegionId::new())
    else {
        return false;
    };
    if !session.execute(
        "Split emitter region",
        EffectCommand::SetEmitterRegions {
            id: emitter_id,
            regions,
        },
        true,
    ) {
        return false;
    }
    state.select_only_emitter_region(emitter_id, region.id);
    session.select_emitter_region(emitter_id, region.id);
    true
}

pub(super) fn merge_selected_regions(
    session: &mut EditorSession,
    state: &mut TimelineState,
) -> bool {
    let selected = state
        .selected_emitter_regions
        .iter()
        .copied()
        .collect::<Vec<_>>();
    let Some((emitter_id, _)) = selected.first().copied() else {
        session.status = "Select two or more regions from the same emitter to merge them".into();
        return false;
    };
    if selected.len() < 2 || selected.iter().any(|(emitter, _)| *emitter != emitter_id) {
        session.status = "Select two or more regions from the same emitter to merge them".into();
        return false;
    }
    let Some(emitter) = session
        .effect
        .emitters
        .iter()
        .find(|emitter| emitter.id == emitter_id)
    else {
        return false;
    };
    let selected_ids = selected
        .iter()
        .map(|(_, region)| *region)
        .collect::<BTreeSet<_>>();
    let Some(merged) =
        merge_selected_emitter_regions(session.effect.duration, emitter, &selected_ids)
    else {
        session.status = "The selected emitter regions no longer exist".into();
        return false;
    };
    let selected = if merged.regions.is_empty() {
        emitter.implicit_region_id()
    } else {
        merged.merged_region
    };
    let mut commands = Vec::with_capacity(2);
    if (merged.emitter_start_time - emitter.start_time).abs() > 0.000_1
        || (merged.emitter_duration - emitter.duration).abs() > 0.000_1
    {
        commands.push(EffectCommand::SetEmitterTiming {
            id: emitter_id,
            start_time: merged.emitter_start_time,
            duration: merged.emitter_duration,
        });
    }
    commands.push(EffectCommand::SetEmitterRegions {
        id: emitter_id,
        regions: merged.regions,
    });
    if !session.execute_transaction(
        EffectTransaction::new("Merged emitter regions", commands),
        true,
    ) {
        return false;
    }
    state.select_only_emitter_region(emitter_id, selected);
    session.select_emitter_region(emitter_id, selected);
    true
}

fn selected_regions_for_one_emitter(
    session: &EditorSession,
    state: &TimelineState,
) -> Option<(EmitterId, Vec<EmitterRegion>)> {
    let emitter_id = state.selected_emitter_regions.first()?.0;
    if state
        .selected_emitter_regions
        .iter()
        .any(|(selected, _)| *selected != emitter_id)
    {
        return None;
    }
    let emitter = session
        .effect
        .emitters
        .iter()
        .find(|emitter| emitter.id == emitter_id)?;
    let mut regions = emitter
        .timeline_regions()
        .into_iter()
        .filter(|region| {
            state
                .selected_emitter_regions
                .contains(&(emitter_id, region.id))
        })
        .collect::<Vec<_>>();
    regions.sort_by(|left, right| left.start_time.total_cmp(&right.start_time));
    (!regions.is_empty()).then_some((emitter_id, regions))
}

pub(super) fn duplicate_selected_emitter_regions(
    session: &mut EditorSession,
    state: &mut TimelineState,
    curves: &mut CurvesState,
) -> bool {
    let Some((emitter_id, selected)) = selected_regions_for_one_emitter(session, state) else {
        session.status = "Select one or more emitter regions to duplicate".into();
        return false;
    };
    let Some(emitter) = session
        .effect
        .emitters
        .iter()
        .find(|emitter| emitter.id == emitter_id)
        .cloned()
    else {
        return false;
    };
    let selected_start = selected
        .iter()
        .map(|region| region.start_time)
        .fold(f32::INFINITY, f32::min);
    let selected_end = selected
        .iter()
        .map(|region| region.end_time())
        .fold(f32::NEG_INFINITY, f32::max);
    let span = (selected_end - selected_start).max(0.0);
    let effect_duration = session.effect.duration.max(span);
    let target_start = if selected_end + span <= effect_duration + 0.000_1 {
        selected_end
    } else if selected_start >= span {
        selected_start - span
    } else {
        session.time().clamp(0.0, (effect_duration - span).max(0.0))
    };
    let offset = target_start - selected_start;
    let mut duplicated = selected
        .iter()
        .map(|source| EmitterRegion {
            id: EmitterRegionId::new(),
            start_time: source.start_time + offset,
            source_offset: source.source_offset,
            duration: source.duration,
        })
        .collect::<Vec<_>>();
    let duplicated_ids = duplicated
        .iter()
        .map(|region| region.id)
        .collect::<Vec<_>>();
    let mut regions = emitter.timeline_regions();
    regions.append(&mut duplicated);
    regions.sort_by(|left, right| left.start_time.total_cmp(&right.start_time));
    if !session.execute(
        "Duplicated emitter regions",
        EffectCommand::SetEmitterRegions {
            id: emitter_id,
            regions: emitter.normalize_timeline_regions(regions),
        },
        true,
    ) {
        return false;
    }
    state.select_only_emitter_regions(emitter_id, &duplicated_ids);
    if let Some(primary) = duplicated_ids.first().copied() {
        session.select_emitter_region(emitter_id, primary);
    }
    curves.clear();
    true
}

pub(super) fn delete_selected_emitter_regions(
    session: &mut EditorSession,
    state: &mut TimelineState,
    curves: &mut CurvesState,
    localizer: &Localizer,
) -> bool {
    let Some((emitter_id, selected)) = selected_regions_for_one_emitter(session, state) else {
        session.status = "Select one or more emitter regions to delete".into();
        return false;
    };
    let Some(emitter) = session
        .effect
        .emitters
        .iter()
        .find(|emitter| emitter.id == emitter_id)
        .cloned()
    else {
        return false;
    };
    let regions = emitter.timeline_regions();
    let selected_ids = selected
        .iter()
        .map(|region| region.id)
        .collect::<BTreeSet<_>>();
    if selected_ids.len() == regions.len() {
        session.select_emitter(emitter_id);
        if preview_selected_emitter_deletion(session, localizer) {
            state.select_only_emitter(emitter_id);
            curves.clear();
            return true;
        }
        return false;
    }

    let primary_start = state
        .selected_emitter_region
        .and_then(|(_, region)| emitter.timeline_region(region))
        .map_or(selected[0].start_time, |region| region.start_time);
    let remaining = regions
        .into_iter()
        .filter(|region| !selected_ids.contains(&region.id))
        .collect::<Vec<_>>();
    let next = remaining
        .iter()
        .filter(|region| region.start_time >= primary_start)
        .min_by(|left, right| left.start_time.total_cmp(&right.start_time))
        .or_else(|| {
            remaining
                .iter()
                .max_by(|left, right| left.start_time.total_cmp(&right.start_time))
        })
        .map(|region| region.id);
    if !session.execute(
        "Deleted emitter regions",
        EffectCommand::SetEmitterRegions {
            id: emitter_id,
            regions: emitter.normalize_timeline_regions(remaining),
        },
        true,
    ) {
        return false;
    }
    if let Some(next) = next {
        state.select_only_emitter_region(emitter_id, next);
        session.select_emitter_region(emitter_id, next);
    } else {
        state.select_only_emitter(emitter_id);
        session.select_emitter(emitter_id);
    }
    curves.clear();
    true
}
