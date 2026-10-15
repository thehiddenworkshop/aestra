//! Asset payloads feed the shared effect-placement command.
use crate::asset_drop::AssetPayload;
use crate::timeline::{TimelineState, interaction_state::EffectDropPreview};
use crate::{EditorSession, ProjectEffectCatalog};
use aestra_core::EffectAssetRef;
use bevy::prelude::*;

pub(crate) use crate::asset_drop::AuthoringDropGuard as DropGuard;

pub(crate) fn clear_cancelled_preview(
    payloads: Query<&AssetPayload>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    catalog: Res<ProjectEffectCatalog>,
    mut state: ResMut<TimelineState>,
) {
    if state.browser_drop.as_ref().is_some_and(|payload| {
        !crate::asset_drop::active(payload, &payloads, Some(&catalog), keys.as_deref())
    }) {
        state.browser_drop = None;
        state.effect_drop_preview = None;
        state.effect_drop_insertion = None;
    }
}

pub(crate) type EffectDragRows<'w, 's> = Query<'w, 's, &'static AssetPayload>;

#[derive(Clone)]
pub(crate) struct EffectDragSource(pub(crate) AssetPayload);

impl EffectDragSource {
    pub(crate) fn resolve(
        &self,
        catalog: &ProjectEffectCatalog,
    ) -> Result<(EffectAssetRef, String), String> {
        self.0.timeline_effect(catalog)
    }

    pub(crate) fn preview(
        &self,
        catalog: &ProjectEffectCatalog,
        session: &EditorSession,
    ) -> Result<EffectDropPreview, String> {
        if session.pending_change.is_some() {
            return Err("Resolve the pending change before adding an effect".into());
        }
        let (reference, display_name) = self.resolve(catalog)?;
        let source = catalog.effect_for_placement(&session.effect, reference)?;
        Ok(EffectDropPreview {
            source_duration: source.duration,
            display_name,
        })
    }
}
