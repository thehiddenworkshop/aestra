use super::*;

#[derive(Clone, Copy, Debug)]
pub(super) struct TimelineView {
    pub(super) start: f32,
    pub(super) end: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct TimelineNavigationSnapshot {
    view: TimelineView,
    snap: TimelineSnapMode,
    expanded_effect_clips: BTreeSet<EffectClipPath>,
    expanded_automation_emitters: BTreeSet<EmitterId>,
    visible_automation_lanes: BTreeSet<AutomationLaneId>,
    automation_lane_heights: BTreeMap<AutomationLaneId, f32>,
    inspected_child: Option<EffectClipChildSelection>,
    vertical_scroll: f32,
}

impl TimelineView {
    pub(super) fn span(self) -> f32 {
        (self.end - self.start).max(0.000_1)
    }

    pub(super) fn time_at(self, normalized: f32) -> f32 {
        self.start + normalized.clamp(0.0, 1.0) * self.span()
    }

    pub(super) fn normalized_time(self, time: f32) -> f32 {
        (time - self.start) / self.span()
    }
}

#[derive(Resource, Debug)]
pub(crate) struct TimelineState {
    pub(super) view: TimelineView,
    pub(super) snap: TimelineSnapMode,
    pub(super) drag: Option<TimelineDrag>,
    pub(super) drag_regions: Vec<TimelineRegionMove>,
    pub(super) effect_clip_drag: Option<EffectClipTimelineDrag>,
    pub(super) marker_drag: Option<TimelineMarkerDrag>,
    pub(super) choreography_event_drag: Option<TimelineChoreographyEventDrag>,
    pub(super) automation_key_drag: Option<TimelineAutomationKeyDrag>,
    pub(super) automation_lane_resize: Option<(AutomationLaneId, f32)>,
    pub(super) automation_lane_heights: BTreeMap<AutomationLaneId, f32>,
    pub(super) snap_guide: Option<f32>,
    pub(super) panning: bool,
    pub(super) context_emitter: Option<EmitterId>,
    pub(super) selected_emitters: BTreeSet<EmitterId>,
    pub(super) selected_emitter_region: Option<(EmitterId, EmitterRegionId)>,
    pub(super) selected_emitter_regions: BTreeSet<(EmitterId, EmitterRegionId)>,
    pub(super) emitter_region_selection_anchor: Option<(EmitterId, EmitterRegionId)>,
    pub(super) emitter_selection_anchor: Option<EmitterId>,
    pub(super) color_picker_emitter: Option<EmitterId>,
    pub(super) automation_menu_emitter: Option<EmitterId>,
    pub(super) reorder_drag: Option<EmitterId>,
    pub(super) effect_clip_reorder_drag: Option<EffectClipId>,
    pub(super) expanded_effect_clips: BTreeSet<EffectClipPath>,
    pub(super) expanded_automation_emitters: BTreeSet<EmitterId>,
    pub(super) visible_automation_lanes: BTreeSet<AutomationLaneId>,
    pub(super) selected_automation_key: Option<TimelineAutomationKeySelection>,
    pub(super) muted_effect_clips: BTreeSet<EffectClipId>,
    pub(super) solo_effect_clip: Option<EffectClipId>,
    pub(super) context_effect_clip: Option<EffectClipId>,
    pub(super) context_menu_position: Vec2,
    pub(super) restore_context_emitter_focus: Option<EmitterId>,
    pub(super) restore_context_effect_clip_focus: Option<EffectClipId>,
    pub(crate) inspected_child: Option<EffectClipChildSelection>,
    pub(super) referenced_emitter_click: Option<ReferencedEmitterClick>,
    pub(super) reveal_emitter: Option<EmitterId>,
    pub(super) reveal_wait_frames: u8,
    pub(super) effect_drop_preview: Option<EffectDropPreview>,
    pub(super) effect_drop_insertion: Option<(ChoreographyTrackId, bool)>,
    pub(super) vertical_scroll: f32,
    pub(super) known_duration: f32,
}

impl Default for TimelineState {
    fn default() -> Self {
        Self::framed(1.0)
    }
}

impl TimelineState {
    pub(crate) fn framed(duration: f32) -> Self {
        let duration = duration.max(0.05);
        Self {
            view: TimelineView {
                start: 0.0,
                end: duration,
            },
            snap: TimelineSnapMode::Smart,
            drag: None,
            drag_regions: Vec::new(),
            effect_clip_drag: None,
            marker_drag: None,
            choreography_event_drag: None,
            automation_key_drag: None,
            automation_lane_resize: None,
            automation_lane_heights: BTreeMap::new(),
            snap_guide: None,
            panning: false,
            context_emitter: None,
            selected_emitters: BTreeSet::new(),
            selected_emitter_region: None,
            selected_emitter_regions: BTreeSet::new(),
            emitter_region_selection_anchor: None,
            emitter_selection_anchor: None,
            color_picker_emitter: None,
            automation_menu_emitter: None,
            reorder_drag: None,
            effect_clip_reorder_drag: None,
            expanded_effect_clips: BTreeSet::new(),
            expanded_automation_emitters: BTreeSet::new(),
            visible_automation_lanes: BTreeSet::new(),
            selected_automation_key: None,
            muted_effect_clips: BTreeSet::new(),
            solo_effect_clip: None,
            context_effect_clip: None,
            context_menu_position: Vec2::ZERO,
            restore_context_emitter_focus: None,
            restore_context_effect_clip_focus: None,
            inspected_child: None,
            referenced_emitter_click: None,
            reveal_emitter: None,
            reveal_wait_frames: 0,
            effect_drop_preview: None,
            effect_drop_insertion: None,
            vertical_scroll: 0.0,
            known_duration: duration,
        }
    }

    pub(crate) fn navigation_snapshot(&self) -> TimelineNavigationSnapshot {
        TimelineNavigationSnapshot {
            view: self.view,
            snap: self.snap,
            expanded_effect_clips: self.expanded_effect_clips.clone(),
            expanded_automation_emitters: self.expanded_automation_emitters.clone(),
            visible_automation_lanes: self.visible_automation_lanes.clone(),
            automation_lane_heights: self.automation_lane_heights.clone(),
            inspected_child: self.inspected_child.clone(),
            vertical_scroll: self.vertical_scroll,
        }
    }

    pub(super) fn automation_lane_height(&self, lane: &AutomationLaneId) -> f32 {
        self.automation_lane_heights
            .get(lane)
            .copied()
            .unwrap_or(automation_curve::DEFAULT_HEIGHT)
    }

    pub(crate) fn restore_navigation(
        &mut self,
        snapshot: TimelineNavigationSnapshot,
        duration: f32,
    ) {
        *self = Self::framed(duration);
        self.view = snapshot.view;
        self.snap = snapshot.snap;
        self.expanded_effect_clips = snapshot.expanded_effect_clips;
        self.expanded_automation_emitters = snapshot.expanded_automation_emitters;
        self.visible_automation_lanes = snapshot.visible_automation_lanes;
        self.automation_lane_heights = snapshot.automation_lane_heights;
        self.inspected_child = snapshot.inspected_child;
        self.vertical_scroll = snapshot.vertical_scroll.max(0.0);
        self.clamp_view(duration.max(0.05));
    }

    pub(crate) fn set_snap(&mut self, snap: TimelineSnapMode) -> bool {
        if self.snap == snap {
            return false;
        }
        self.snap = snap;
        self.snap_guide = None;
        true
    }

    pub(crate) fn reveal_emitter(&mut self, emitter: EmitterId) {
        self.reveal_emitter = Some(emitter);
        self.reveal_wait_frames = 1;
    }

    pub(crate) fn selected_local_emitters(&self, effect: &EffectAsset) -> Vec<EmitterId> {
        normalized_choreography_order(effect)
            .into_iter()
            .filter_map(|track| match track {
                ChoreographyTrackId::Emitter(emitter)
                    if self.selected_emitters.contains(&emitter) =>
                {
                    Some(emitter)
                }
                _ => None,
            })
            .collect()
    }

    pub(crate) fn clear_emitter_selection(&mut self) {
        self.selected_emitters.clear();
        self.emitter_selection_anchor = None;
        self.clear_emitter_region_selection();
    }

    pub(super) fn clear_emitter_region_selection(&mut self) {
        self.selected_emitter_region = None;
        self.selected_emitter_regions.clear();
        self.emitter_region_selection_anchor = None;
    }

    pub(super) fn select_only_emitter(&mut self, emitter: EmitterId) {
        self.selected_emitters.clear();
        self.selected_emitters.insert(emitter);
        self.emitter_selection_anchor = Some(emitter);
        self.clear_emitter_region_selection();
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

    pub(super) fn select_emitter(
        &mut self,
        effect: &EffectAsset,
        current: Option<EmitterId>,
        emitter: EmitterId,
        control: bool,
        shift: bool,
    ) -> EmitterId {
        let order = normalized_choreography_order(effect)
            .into_iter()
            .filter_map(|track| match track {
                ChoreographyTrackId::Emitter(emitter) => Some(emitter),
                ChoreographyTrackId::EffectClip(_) => None,
            })
            .collect::<Vec<_>>();
        if shift {
            let anchor = self
                .emitter_selection_anchor
                .filter(|anchor| order.contains(anchor))
                .or_else(|| self.selected_emitters.iter().next().copied())
                .unwrap_or(emitter);
            if !control {
                self.selected_emitters.clear();
            }
            let start = order.iter().position(|candidate| *candidate == anchor);
            let end = order.iter().position(|candidate| *candidate == emitter);
            if let (Some(start), Some(end)) = (start, end) {
                let (start, end) = if start <= end {
                    (start, end)
                } else {
                    (end, start)
                };
                self.selected_emitters
                    .extend(order[start..=end].iter().copied());
            }
            self.emitter_selection_anchor = Some(anchor);
            return emitter;
        }
        if control {
            if self.selected_emitters.is_empty() {
                self.selected_emitters.extend(current);
            }
            if !self.selected_emitters.remove(&emitter) {
                self.selected_emitters.insert(emitter);
            }
            if self.selected_emitters.is_empty() {
                self.selected_emitters.insert(emitter);
            }
            self.emitter_selection_anchor = Some(emitter);
            return if self.selected_emitters.contains(&emitter) {
                emitter
            } else {
                self.selected_emitters
                    .iter()
                    .next()
                    .copied()
                    .unwrap_or(emitter)
            };
        }
        self.select_only_emitter(emitter);
        emitter
    }

    pub(crate) fn effect_clip_preview_timing(&self, clip: EffectClipId) -> Option<(f32, f32, f32)> {
        self.effect_clip_drag
            .filter(|drag| drag.clip == clip)
            .map(|drag| {
                (
                    drag.current_start,
                    drag.current_source_offset,
                    drag.current_duration,
                )
            })
    }

    pub(crate) fn effect_clip_is_audible(&self, clip: EffectClipId) -> bool {
        !self.muted_effect_clips.contains(&clip)
            && self.solo_effect_clip.is_none_or(|solo| solo == clip)
    }

    pub(crate) fn effect_clip_solo_active(&self) -> bool {
        self.solo_effect_clip.is_some()
    }

    pub(crate) fn frame_all(&mut self, duration: f32) {
        let duration = duration.max(0.05);
        self.view = TimelineView {
            start: 0.0,
            end: duration,
        };
        self.known_duration = duration;
        self.snap_guide = None;
    }

    pub(super) fn ensure_duration(&mut self, duration: f32) {
        let duration = duration.max(0.05);
        if (duration - self.known_duration).abs() <= f32::EPSILON {
            return;
        }
        let was_framed =
            self.view.start <= f32::EPSILON && (self.view.end - self.known_duration).abs() < 0.001;
        self.known_duration = duration;
        if was_framed {
            self.frame_all(duration);
        } else {
            self.clamp_view(duration);
        }
    }

    pub(super) fn zoom_at(&mut self, anchor: f32, factor: f32, duration: f32, tick_rate: u32) {
        let duration = duration.max(0.05);
        let minimum_span = (4.0 / tick_rate.max(1) as f32).max(0.01);
        let old_span = self.view.span();
        let new_span = (old_span * factor).clamp(minimum_span.min(duration), duration);
        let anchor_ratio = ((anchor - self.view.start) / old_span).clamp(0.0, 1.0);
        self.view.start = anchor - new_span * anchor_ratio;
        self.view.end = self.view.start + new_span;
        self.clamp_view(duration);
    }

    pub(super) fn pan_by(&mut self, delta: f32, duration: f32) {
        self.view.start += delta;
        self.view.end += delta;
        self.clamp_view(duration.max(0.05));
    }

    fn clamp_view(&mut self, duration: f32) {
        let span = self.view.span().min(duration);
        self.view.start = self.view.start.clamp(0.0, (duration - span).max(0.0));
        self.view.end = self.view.start + span;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support;

    #[test]
    fn emitter_region_selection_supports_single_toggle_and_range_selection() {
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
    fn zoom_keeps_the_time_under_the_pointer() {
        let mut state = TimelineState::default();
        state.frame_all(10.0);
        let anchor = state.view.time_at(0.73);

        state.zoom_at(anchor, 0.5, 10.0, 60);

        assert!((state.view.time_at(0.73) - anchor).abs() < 0.000_1);
        assert!((state.view.span() - 5.0).abs() < 0.000_1);
    }

    #[test]
    fn pan_stays_inside_the_effect() {
        let mut state = TimelineState {
            view: TimelineView {
                start: 2.0,
                end: 4.0,
            },
            ..default()
        };

        state.pan_by(-10.0, 8.0);
        assert_eq!(state.view.start, 0.0);
        assert_eq!(state.view.end, 2.0);

        state.pan_by(20.0, 8.0);
        assert_eq!(state.view.start, 6.0);
        assert_eq!(state.view.end, 8.0);
    }

    #[test]
    fn navigation_snapshot_restores_nested_context_and_view() {
        let mut session = test_support::session_with_timing_slack();
        let clip = EffectClip::new(aestra_bevy::EffectId::from_u128(0xC11D), 0.0, 1.0);
        let path = EffectClipPath::root_path(clip.id);
        session.effect.effect_clips.push(clip);
        let mut state = TimelineState::framed(10.0);
        state.zoom_at(6.0, 0.4, 10.0, 60);
        state.vertical_scroll = 73.0;
        state.expanded_effect_clips.insert(path.clone());
        state.inspected_child = Some(EffectClipChildSelection::EffectClip { path });
        let expected_view = state.view;
        let snapshot = state.navigation_snapshot();

        state = TimelineState::framed(2.0);
        state.restore_navigation(snapshot, 10.0);

        assert_eq!(state.view.start, expected_view.start);
        assert_eq!(state.view.end, expected_view.end);
        assert_eq!(state.vertical_scroll, 73.0);
        assert_eq!(state.expanded_effect_clips.len(), 1);
        assert!(matches!(
            state.inspected_child,
            Some(EffectClipChildSelection::EffectClip { .. })
        ));
    }

    #[test]
    fn control_and_shift_select_emitters_in_timeline_order() {
        let session = test_support::session_with_timing_slack();
        let effect = &session.effect;
        let [first, second, third, ..] = effect.emitters.as_slice() else {
            panic!("fixture needs at least three emitters");
        };
        let mut state = TimelineState::framed(effect.duration);

        state.select_emitter(effect, Some(first.id), first.id, false, false);
        state.select_emitter(effect, Some(first.id), third.id, true, false);
        assert_eq!(
            state.selected_local_emitters(effect),
            vec![first.id, third.id]
        );

        state.select_emitter(effect, Some(third.id), second.id, false, true);
        assert_eq!(
            state.selected_local_emitters(effect),
            vec![second.id, third.id]
        );
    }
}
