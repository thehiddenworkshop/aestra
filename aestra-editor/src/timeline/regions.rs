use super::*;

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
    let minimum_duration = (1.0 / session.clock.tick_rate().max(1) as f32).max(0.001);
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

impl TimelineState {
    pub(super) fn clear_emitter_region_selection(&mut self) {
        self.selected_emitter_region = None;
        self.selected_emitter_regions.clear();
        self.emitter_region_selection_anchor = None;
    }

    pub(super) fn select_only_emitter_region(
        &mut self,
        emitter: EmitterId,
        region: EmitterRegionId,
    ) {
        self.select_only_emitter(emitter);
        self.selected_emitter_region = Some((emitter, region));
        self.selected_emitter_regions.insert((emitter, region));
        self.emitter_region_selection_anchor = Some((emitter, region));
    }

    pub(super) fn select_only_emitter_regions(
        &mut self,
        emitter: EmitterId,
        regions: &[EmitterRegionId],
    ) {
        self.select_only_emitter(emitter);
        self.selected_emitter_regions
            .extend(regions.iter().map(|region| (emitter, *region)));
        self.selected_emitter_region = regions.first().map(|region| (emitter, *region));
        self.emitter_region_selection_anchor = self.selected_emitter_region;
    }

    pub(super) fn select_emitter_region(
        &mut self,
        emitter: &Emitter,
        region: EmitterRegionId,
        control: bool,
        shift: bool,
    ) -> Option<EmitterRegionId> {
        let emitter_id = emitter.id;
        let mut order = emitter.timeline_regions();
        order.sort_by(|left, right| left.start_time.total_cmp(&right.start_time));
        if !order.iter().any(|candidate| candidate.id == region) {
            return self
                .selected_emitter_region
                .filter(|(selected, _)| *selected == emitter_id)
                .map(|(_, region)| region);
        }

        self.selected_emitters.clear();
        self.selected_emitters.insert(emitter_id);
        self.emitter_selection_anchor = Some(emitter_id);
        self.selected_emitter_regions
            .retain(|(selected, _)| *selected == emitter_id);

        if shift {
            let anchor = self
                .emitter_region_selection_anchor
                .filter(|(selected, anchor)| {
                    *selected == emitter_id && order.iter().any(|candidate| candidate.id == *anchor)
                })
                .map_or(region, |(_, anchor)| anchor);
            let anchor_index = order
                .iter()
                .position(|candidate| candidate.id == anchor)
                .unwrap_or_default();
            let region_index = order
                .iter()
                .position(|candidate| candidate.id == region)
                .unwrap_or(anchor_index);
            let (start, end) = if anchor_index <= region_index {
                (anchor_index, region_index)
            } else {
                (region_index, anchor_index)
            };
            self.selected_emitter_regions.clear();
            self.selected_emitter_regions.extend(
                order[start..=end]
                    .iter()
                    .map(|candidate| (emitter_id, candidate.id)),
            );
            self.selected_emitter_region = Some((emitter_id, region));
            self.emitter_region_selection_anchor = Some((emitter_id, anchor));
            return Some(region);
        }

        if control {
            let selection = (emitter_id, region);
            if !self.selected_emitter_regions.remove(&selection) {
                self.selected_emitter_regions.insert(selection);
                self.selected_emitter_region = Some(selection);
            } else if self.selected_emitter_region == Some(selection) {
                self.selected_emitter_region = order
                    .iter()
                    .rev()
                    .find(|candidate| {
                        self.selected_emitter_regions
                            .contains(&(emitter_id, candidate.id))
                    })
                    .map(|candidate| (emitter_id, candidate.id));
            }
            self.emitter_region_selection_anchor = Some(selection);
            return self.selected_emitter_region.map(|(_, region)| region);
        }

        self.select_only_emitter_region(emitter_id, region);
        Some(region)
    }
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
    layout: &mut WorkspaceLayout,
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
            reveal_dock_panel(layout, session, DockPanel::Changes);
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

pub(super) fn timeline_region_preview_timing(
    state: &TimelineState,
    emitter: EmitterId,
    region: EmitterRegion,
) -> (f32, f32) {
    let Some(drag) = state.drag.filter(|drag| drag.emitter == emitter) else {
        return (region.start_time, region.duration);
    };
    if drag.region == region.id {
        return (drag.current_start, drag.current_duration);
    }
    if drag.kind == TimelineDragKind::Move
        && let Some(member) = state
            .drag_regions
            .iter()
            .find(|member| member.region == region.id)
    {
        return (
            member.original_start + drag.current_start - drag.original_start,
            member.duration,
        );
    }
    (region.start_time, region.duration)
}

fn snap_timeline_boundary(
    candidate: f32,
    emitter: EmitterId,
    region: EmitterRegionId,
    ignored_regions: &[TimelineRegionMove],
    session: &EditorSession,
    mode: TimelineSnapMode,
    view: TimelineView,
    canvas_width: f32,
) -> (f32, Option<f32>) {
    match mode {
        TimelineSnapMode::None => (candidate, None),
        TimelineSnapMode::Frames => {
            let frame = 1.0 / session.clock.tick_rate().max(1) as f32;
            let snapped = (candidate / frame).round() * frame;
            (snapped, Some(snapped))
        }
        TimelineSnapMode::Seconds => {
            let interval = nice_timeline_step(view.span(), canvas_width) / 5.0;
            let snapped = (candidate / interval).round() * interval;
            (snapped, Some(snapped))
        }
        TimelineSnapMode::Smart => {
            let threshold = view.span() / canvas_width.max(1.0) * 9.0;
            let frame = 1.0 / session.clock.tick_rate().max(1) as f32;
            let mut targets = vec![
                0.0,
                session.playback_duration(),
                session.time(),
                (candidate / frame).round() * frame,
            ];
            for other in &session.effect.emitters {
                for other_region in other.timeline_regions() {
                    if other.id != emitter
                        || (other_region.id != region
                            && !ignored_regions
                                .iter()
                                .any(|ignored| ignored.region == other_region.id))
                    {
                        targets.push(other_region.start_time);
                        targets.push(other_region.end_time());
                    }
                }
            }
            let nearest = targets.into_iter().min_by(|left, right| {
                (candidate - *left)
                    .abs()
                    .total_cmp(&(candidate - *right).abs())
            });
            nearest
                .filter(|target| (candidate - *target).abs() <= threshold)
                .map_or((candidate, None), |target| (target, Some(target)))
        }
    }
}

fn snap_moved_timing(
    start: f32,
    duration: f32,
    emitter: EmitterId,
    region: EmitterRegionId,
    ignored_regions: &[TimelineRegionMove],
    session: &EditorSession,
    mode: TimelineSnapMode,
    view: TimelineView,
    canvas_width: f32,
) -> (f32, Option<f32>) {
    let start_snap = snap_timeline_boundary(
        start,
        emitter,
        region,
        ignored_regions,
        session,
        mode,
        view,
        canvas_width,
    );
    if mode != TimelineSnapMode::Smart {
        return start_snap;
    }
    let end = start + duration;
    let end_snap = snap_timeline_boundary(
        end,
        emitter,
        region,
        ignored_regions,
        session,
        mode,
        view,
        canvas_width,
    );
    let start_delta = (start_snap.0 - start).abs();
    let end_delta = (end_snap.0 - end).abs();
    match (start_snap.1, end_snap.1) {
        (None, Some(guide)) => (start + end_snap.0 - end, Some(guide)),
        (Some(_), Some(guide)) if end_delta < start_delta => {
            (start + end_snap.0 - end, Some(guide))
        }
        _ => start_snap,
    }
}

pub(super) fn update_timeline_drag(
    drag: &mut TimelineDrag,
    drag_regions: &[TimelineRegionMove],
    pointer_time: f32,
    session: &EditorSession,
    snap: TimelineSnapMode,
    view: TimelineView,
    canvas_width: f32,
    snap_guide: &mut Option<f32>,
) {
    let effect_duration = session.playback_duration();
    let minimum_duration = (1.0 / session.clock.tick_rate().max(1) as f32).max(0.001);
    let pointer_delta = pointer_time - drag.pointer_start;
    *snap_guide = None;
    match drag.kind {
        TimelineDragKind::Move => {
            let group_start = drag_regions
                .iter()
                .map(|region| region.original_start)
                .fold(drag.original_start, f32::min);
            let group_end = drag_regions
                .iter()
                .map(|region| region.original_start + region.duration)
                .fold(drag.original_start + drag.original_duration, f32::max);
            let minimum_start = drag.original_start - group_start;
            let maximum_start = drag.original_start + effect_duration - group_end;
            let unsnapped = (drag.original_start + pointer_delta)
                .clamp(minimum_start, maximum_start.max(minimum_start));
            let (start, guide) = snap_moved_timing(
                unsnapped,
                drag.original_duration,
                drag.emitter,
                drag.region,
                drag_regions,
                session,
                snap,
                view,
                canvas_width,
            );
            drag.current_start = start.clamp(minimum_start, maximum_start.max(minimum_start));
            drag.current_duration = drag.original_duration;
            drag.current_source_offset = drag.original_source_offset;
            *snap_guide = guide;
        }
        TimelineDragKind::TrimStart => {
            let end = drag.original_start + drag.original_duration;
            let minimum_start = 0.0;
            let unsnapped = (drag.original_start + pointer_delta)
                .clamp(minimum_start, (end - minimum_duration).max(minimum_start));
            let (start, guide) = snap_timeline_boundary(
                unsnapped,
                drag.emitter,
                drag.region,
                &[],
                session,
                snap,
                view,
                canvas_width,
            );
            drag.current_start = start.clamp(minimum_start, end - minimum_duration);
            drag.current_source_offset =
                (drag.original_source_offset + drag.current_start - drag.original_start).max(0.0);
            drag.current_duration = end - drag.current_start;
            *snap_guide = guide;
        }
        TimelineDragKind::TrimEnd => {
            let maximum_end = effect_duration;
            let unsnapped = (drag.original_start + drag.original_duration + pointer_delta)
                .clamp(drag.original_start + minimum_duration, maximum_end);
            let (end, guide) = snap_timeline_boundary(
                unsnapped,
                drag.emitter,
                drag.region,
                &[],
                session,
                snap,
                view,
                canvas_width,
            );
            let end = end.clamp(drag.original_start + minimum_duration, maximum_end);
            drag.current_start = drag.original_start;
            drag.current_source_offset = drag.original_source_offset;
            drag.current_duration = end - drag.original_start;
            *snap_guide = guide;
        }
    }
}

pub(super) fn commit_timeline_drag(
    session: &mut EditorSession,
    drag: TimelineDrag,
    drag_regions: &[TimelineRegionMove],
) {
    let changed = (drag.current_start - drag.original_start).abs() > 0.000_1
        || (drag.current_source_offset - drag.original_source_offset).abs() > 0.000_1
        || (drag.current_duration - drag.original_duration).abs() > 0.000_1;
    if changed {
        let label = match drag.kind {
            TimelineDragKind::Move => "Moved emitter on timeline",
            TimelineDragKind::TrimStart | TimelineDragKind::TrimEnd => {
                "Trimmed emitter on timeline"
            }
        };
        let Some(emitter) = session
            .effect
            .emitters
            .iter()
            .find(|emitter| emitter.id == drag.emitter)
            .cloned()
        else {
            return;
        };
        if drag.kind == TimelineDragKind::Move && drag_regions.len() > 1 {
            let delta = drag.current_start - drag.original_start;
            let moved = drag_regions
                .iter()
                .map(|member| member.region)
                .collect::<BTreeSet<_>>();
            let mut regions = emitter.timeline_regions();
            for region in &mut regions {
                if moved.contains(&region.id) {
                    region.start_time += delta;
                }
            }
            session.execute(
                label,
                EffectCommand::SetEmitterRegions {
                    id: drag.emitter,
                    regions: emitter.normalize_timeline_regions(regions),
                },
                false,
            );
            return;
        }
        let Some(transaction) = session.emitter_region_timing_transaction(
            drag.emitter,
            drag.region,
            drag.current_start,
            drag.current_source_offset,
            drag.current_duration,
            label,
        ) else {
            return;
        };
        session.execute_transaction(transaction, false);
    }
}

pub(super) fn begin_timeline_clip_drag(
    drag: On<Pointer<DragStart>>,
    targets: Query<&TimelineClipInteraction>,
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(target) = targets.get(drag.event_target()) else {
        return;
    };
    let Some(emitter) = session
        .effect
        .emitters
        .iter()
        .find(|emitter| emitter.id == target.emitter)
    else {
        return;
    };
    let Some(region) = emitter.timeline_region(target.region) else {
        return;
    };
    state.drag_regions = if target.kind == TimelineDragKind::Move
        && state
            .selected_emitter_regions
            .contains(&(target.emitter, target.region))
    {
        emitter
            .timeline_regions()
            .into_iter()
            .filter(|region| {
                state
                    .selected_emitter_regions
                    .contains(&(target.emitter, region.id))
            })
            .map(|region| TimelineRegionMove {
                region: region.id,
                original_start: region.start_time,
                duration: region.duration,
            })
            .collect()
    } else if target.kind == TimelineDragKind::Move {
        vec![TimelineRegionMove {
            region: region.id,
            original_start: region.start_time,
            duration: region.duration,
        }]
    } else {
        Vec::new()
    };
    state.drag = Some(TimelineDrag {
        emitter: target.emitter,
        region: target.region,
        kind: target.kind,
        pointer_start: 0.0,
        original_start: region.start_time,
        original_duration: region.duration,
        original_source_offset: region.source_offset,
        current_start: region.start_time,
        current_duration: region.duration,
        current_source_offset: region.source_offset,
        source_duration: emitter.duration,
    });
    override_cursor.0 = Some(EntityCursor::System(timeline_system_cursor(
        target.kind,
        true,
    )));
    **cursor = timeline_drag_cursor(target.kind, true);
}

pub(super) fn move_timeline_clip_drag(
    mut drag_event: On<Pointer<Drag>>,
    targets: Query<&TimelineClipInteraction>,
    canvases: Query<&ComputedNode, With<TimelineCanvas>>,
    window: Single<&Window, With<PrimaryWindow>>,
    session: Res<EditorSession>,
    mut state: ResMut<TimelineState>,
) {
    let Ok(target) = targets.get(drag_event.event_target()) else {
        return;
    };
    drag_event.propagate(false);
    let Some(mut drag) = state.drag else {
        return;
    };
    if drag.emitter != target.emitter || drag.region != target.region || drag.kind != target.kind {
        return;
    }
    let width = canvases
        .iter()
        .map(|canvas| canvas.size().x)
        .fold(0.0, f32::max)
        .max(1.0);
    let logical_distance = screen_distance_to_logical(drag_event.distance.x, window.scale_factor());
    let pointer_time = logical_distance / width * state.view.span();
    let mut snap_guide = state.snap_guide;
    let drag_regions = state.drag_regions.clone();
    update_timeline_drag(
        &mut drag,
        &drag_regions,
        pointer_time,
        &session,
        state.snap,
        state.view,
        width,
        &mut snap_guide,
    );
    state.drag = Some(drag);
    state.snap_guide = snap_guide;
}

pub(super) fn finish_timeline_clip_drag(
    drag_event: On<Pointer<DragEnd>>,
    targets: Query<&TimelineClipInteraction>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<TimelineState>,
    mut commands: Commands,
    mut override_cursor: ResMut<OverrideCursor>,
    mut cursor: Single<&mut CursorIcon, With<PrimaryWindow>>,
) {
    let Ok(target) = targets.get(drag_event.event_target()) else {
        return;
    };
    let Some(drag) = state.drag.take() else {
        return;
    };
    if drag.emitter != target.emitter || drag.region != target.region || drag.kind != target.kind {
        return;
    }
    state.snap_guide = None;
    let drag_regions = std::mem::take(&mut state.drag_regions);
    override_cursor.0 = None;
    **cursor = timeline_drag_cursor(target.kind, false);
    let preserve_multi_selection = drag.kind == TimelineDragKind::Move && drag_regions.len() > 1;
    commit_timeline_drag(&mut session, drag, &drag_regions);
    if preserve_multi_selection {
        session.select_emitter_region(target.emitter, target.region);
    } else {
        commands.trigger(ChoreographyAction::SelectEmitterRegion {
            emitter: target.emitter,
            region: target.region,
        });
    }
}

pub(super) fn dismiss_emitter_region_selection(
    buttons: Res<ButtonInput<MouseButton>>,
    regions: Query<&RelativeCursorPosition, With<TimelineClip>>,
    tools: Query<&RelativeCursorPosition, (With<TimelineRegionToolButton>, Without<TimelineClip>)>,
    mut state: ResMut<TimelineState>,
    mut session: ResMut<EditorSession>,
) {
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let over_region = regions.iter().any(RelativeCursorPosition::cursor_over);
    let over_tool = tools.iter().any(RelativeCursorPosition::cursor_over);
    if over_region || over_tool {
        return;
    }

    if !state.selected_emitter_regions.is_empty() {
        let emitter = state.selected_emitter_region.map(|(emitter, _)| emitter);
        state.clear_emitter_region_selection();
        if let Some(emitter) = emitter
            && session
                .effect
                .emitters
                .iter()
                .any(|candidate| candidate.id == emitter)
        {
            session.select_emitter(emitter);
        }
        session.ui_revision += 1;
    }
}

pub(super) fn open_timeline_region_context_menu(
    mut click: On<Pointer<Click>>,
    targets: Query<&TimelineClipInteraction>,
    headers: Query<(&EmitterTrackHeader, &ComputedNode, &UiGlobalTransform)>,
    mut session: ResMut<EditorSession>,
    mut curves: ResMut<CurvesState>,
    mut state: ResMut<TimelineState>,
) {
    if click.button != PointerButton::Secondary {
        return;
    }
    let Ok(target) = targets.get(click.event_target()) else {
        return;
    };
    let selection = (target.emitter, target.region);
    if !state.selected_emitter_regions.contains(&selection) {
        state.select_only_emitter_region(target.emitter, target.region);
    } else {
        state.selected_emitter_region = Some(selection);
        state.emitter_region_selection_anchor = Some(selection);
    }
    session.select_emitter_region(target.emitter, target.region);
    curves.clear();

    let position = headers
        .iter()
        .find(|(header, _, _)| header.emitter == target.emitter)
        .map_or(Vec2::ZERO, |(_, node, transform)| {
            pointer_position_in_node(click.pointer_location.position, node, transform)
        });
    state.color_picker_emitter = None;
    state.automation_menu_emitter = None;
    state.context_effect_clip = None;
    state.context_emitter = Some(target.emitter);
    state.context_menu_position = position;
    session.ui_revision += 1;
    click.propagate(false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_supports_single_toggle_and_range_selection() {
        let mut emitter = Emitter::basic_sprite("Emitter", 3.0);
        let first = emitter.implicit_region_id();
        let second = EmitterRegionId::from_u128(0x71);
        emitter.regions = emitter.split_timeline_region(first, 1.0, second).unwrap();
        let third = EmitterRegionId::from_u128(0x72);
        emitter.regions = emitter.split_timeline_region(second, 2.0, third).unwrap();
        let mut state = TimelineState::framed(3.0);

        assert_eq!(
            state.select_emitter_region(&emitter, first, false, false),
            Some(first)
        );
        assert_eq!(state.selected_emitter_regions.len(), 1);

        assert_eq!(
            state.select_emitter_region(&emitter, third, false, true),
            Some(third)
        );
        assert_eq!(state.selected_emitter_regions.len(), 3);

        state.select_emitter_region(&emitter, second, true, false);
        assert_eq!(state.selected_emitter_regions.len(), 2);
        assert!(
            !state
                .selected_emitter_regions
                .contains(&(emitter.id, second))
        );

        state.clear_emitter_region_selection();
        assert!(state.selected_emitter_regions.is_empty());
        assert_eq!(state.selected_emitters, BTreeSet::from([emitter.id]));
    }

    #[test]
    fn merge_consolidates_separate_and_overlapping_regions() {
        let mut emitter = Emitter::basic_sprite("Emitter", 3.0);
        let first = emitter.implicit_region_id();
        let second = EmitterRegionId::from_u128(0x73);
        emitter.regions = emitter.split_timeline_region(first, 1.0, second).unwrap();
        let third = EmitterRegionId::from_u128(0x74);
        emitter.regions = emitter.split_timeline_region(second, 2.0, third).unwrap();

        emitter.regions[2].start_time = 4.0;
        let separated =
            merge_selected_emitter_regions(6.0, &emitter, &BTreeSet::from([first, third])).unwrap();
        assert_eq!(separated.merged_region, first);
        assert_eq!(separated.emitter_duration, 5.0);
        assert_eq!(separated.regions.len(), 2);
        assert_eq!(separated.regions[0].start_time, 0.0);
        assert_eq!(separated.regions[0].duration, 5.0);
        assert_eq!(separated.regions[1].id, second);

        emitter.regions[2].start_time = 0.5;
        let overlapping =
            merge_selected_emitter_regions(3.0, &emitter, &BTreeSet::from([first, third])).unwrap();
        assert_eq!(overlapping.merged_region, first);
        assert_eq!(overlapping.emitter_duration, 3.0);
        assert_eq!(overlapping.regions.len(), 2);
        assert_eq!(overlapping.regions[0].start_time, 0.0);
        assert_eq!(overlapping.regions[0].duration, 1.5);
        assert_eq!(overlapping.regions[1].id, second);
    }
}
