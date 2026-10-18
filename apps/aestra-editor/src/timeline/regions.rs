#[cfg(test)]
use super::region_execution::merge_selected_emitter_regions;
#[cfg(test)]
pub(super) use super::region_gestures::{commit_timeline_drag, update_timeline_drag};
use super::*;

pub(super) fn dismiss_emitter_region_selection(
    buttons: Res<ButtonInput<MouseButton>>,
    regions: Query<&RelativeCursorPosition, With<TimelineClip>>,
    tools: Query<
        &RelativeCursorPosition,
        (
            Or<(
                With<TimelineRegionToolButton>,
                With<host_motion::HostMotionControl>,
            )>,
            Without<TimelineClip>,
        ),
    >,
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
