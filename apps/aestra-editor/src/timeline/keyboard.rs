//! Production timeline keyboard consumer; semantic handlers remain separately composed.
use super::*;

pub(super) fn choreography_keyboard_input(
    mut commands: Commands,
    input: crate::input::ShortcutKeys,
    palette: Res<ModulePaletteState>,
    session: Res<EditorSession>,
    state: Res<TimelineState>,
    timelines: Query<(), With<TimelineCanvas>>,
    automation_graphs: Query<(&TimelineAutomationLaneGraph, &RelativeCursorPosition)>,
    material_graphs: Query<&RelativeCursorPosition, With<MaterialGraphViewport>>,
    focus: Option<Res<InputFocus>>,
    editable_text: Query<(), With<EditableText>>,
    shortcuts: crate::input::ShortcutContext,
) {
    let editing_text = focus
        .as_ref()
        .and_then(|focus| focus.get())
        .is_some_and(|entity| editable_text.contains(entity));
    if palette.open
        || shortcuts.blocked()
        || shortcuts.tab_strip_focused()
        || shortcuts.asset_browser_focused()
        || timelines.is_empty()
        || editing_text
        || material_graphs
            .iter()
            .any(RelativeCursorPosition::cursor_over)
    {
        return;
    }
    for keys in input.iter() {
        let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        if host_motion::keyboard_input(&session, &state, keys, &mut commands) {
            continue;
        }
        if control && keys.just_pressed(KeyCode::Enter) {
            commands.trigger(ChoreographyAction::AddEmitter);
        }
        if control && keys.just_pressed(KeyCode::KeyD) {
            commands.trigger(if state.selected_emitter_regions.is_empty() {
                ChoreographyAction::DuplicateEmitter(None)
            } else {
                ChoreographyAction::DuplicateSelectedEmitterRegions
            });
        }
        if keys.just_pressed(KeyCode::Insert) {
            let action = automation_graphs.iter().find_map(|(graph, cursor)| {
                cursor
                    .normalized
                    .filter(|_| cursor.cursor_over())
                    .and_then(|position| {
                        add_automation_key_at_pointer_action(&session.effect, &graph.0, position)
                    })
            });
            if let Some(action) = action {
                commands.trigger(action);
                continue;
            }
        }
        if keys.just_pressed(KeyCode::Delete) {
            if let Some(selection) = state.selected_automation_key.clone() {
                commands.trigger(ChoreographyAction::DeleteAutomationKey(selection));
                continue;
            }
            if !state.selected_emitter_regions.is_empty() {
                commands.trigger(ChoreographyAction::DeleteSelectedEmitterRegions);
                continue;
            }
            match session.selection.primary {
                SemanticTarget::Marker(marker) => {
                    commands.trigger(TimelineAction::DeleteMarker(marker));
                }
                SemanticTarget::ChoreographyEvent(event) => {
                    commands.trigger(TimelineAction::DeleteChoreographyEvent(event));
                }
                SemanticTarget::EffectClip(clip) if state.inspected_child.is_none() => {
                    commands.trigger(ChoreographyAction::DeleteEffectClip(clip));
                }
                _ if state.inspected_child.is_none() => {
                    commands.trigger(ChoreographyAction::DeleteEmitter(None));
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/keyboard.rs"]
mod tests;
