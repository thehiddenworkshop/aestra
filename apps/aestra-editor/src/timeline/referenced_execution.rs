//! Production nested source resolution, selection and source-opening intents.
use super::{ChoreographyAction, EffectClipChildSelection, EffectClipPath, TimelineState};
use crate::{
    CurvesState, EditorSession, ProjectEffectCatalog, localization::Localizer,
    persistence::DocumentAction,
};
use aestra_core::{EffectAsset, EffectClip};
use bevy::prelude::*;

pub(crate) fn resolve_effect_clip_path(
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
    path: &EffectClipPath,
) -> Option<(EffectClip, EffectAsset)> {
    let (root, descendants) = path.ids().split_first()?;
    let mut clip = session
        .effect
        .effect_clips
        .iter()
        .find(|clip| clip.id == *root)?
        .clone();
    let mut source = catalog.cached_effect(clip.source).ok()?;
    for id in descendants {
        clip = source
            .effect_clips
            .iter()
            .find(|clip| clip.id == *id)?
            .clone();
        source = catalog.cached_effect(clip.source).ok()?;
    }
    Some((clip, source))
}

pub(super) fn handle_referenced_effect_action(
    action: &ChoreographyAction,
    commands: &mut Commands,
    session: &mut EditorSession,
    curves: &mut CurvesState,
    state: &mut TimelineState,
    catalog: &ProjectEffectCatalog,
    localizer: &Localizer,
) -> bool {
    match action.clone() {
        ChoreographyAction::SelectEffectClipEmitter { path, emitter } => {
            state.selected_automation_key = None;
            let source =
                resolve_effect_clip_path(session, catalog, &path).map(|(_, source)| source);
            if source
                .as_ref()
                .is_some_and(|effect| effect.emitters.iter().any(|item| item.id == emitter))
            {
                state.clear_emitter_selection();
                session.selection.select_effect_clip(path.root());
                state.inspected_child = Some(EffectClipChildSelection::Emitter { path, emitter });
                session.status = localizer.text("timeline-selected-referenced-emitter");
                session.ui_revision += 1;
                curves.clear();
            }
        }
        ChoreographyAction::SelectReferencedEffectClip(path) => {
            state.selected_automation_key = None;
            if resolve_effect_clip_path(session, catalog, &path).is_some() {
                state.clear_emitter_selection();
                session.selection.select_effect_clip(path.root());
                state.inspected_child = Some(EffectClipChildSelection::EffectClip { path });
                session.status = localizer.text("timeline-selected-referenced-effect");
                session.ui_revision += 1;
                curves.clear();
            }
        }
        ChoreographyAction::ToggleEffectClipExpanded(path) => {
            if !state.expanded_effect_clips.remove(&path) {
                state.expanded_effect_clips.insert(path);
            } else if state
                .inspected_child
                .as_ref()
                .is_some_and(|child| child.is_descendant_of(&path))
            {
                state.inspected_child = None;
            }
            session.ui_revision += 1;
        }
        ChoreographyAction::EditEffectClipEmitterSource { path, emitter } => {
            if let Some((clip, source)) = resolve_effect_clip_path(session, catalog, &path)
                && source
                    .emitters
                    .iter()
                    .any(|candidate| candidate.id == emitter)
            {
                commands.trigger(DocumentAction::OpenSourceEmitter(clip.source, emitter));
            }
        }
        _ => return false,
    }
    true
}
