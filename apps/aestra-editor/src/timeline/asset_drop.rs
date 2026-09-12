//! Both browser and transitional Library sources feed the same placement command.
use super::*;
use crate::asset_drop::AssetPayload;
#[cfg(test)]
mod tests;

pub(super) use crate::asset_drop::AuthoringDropGuard as DropGuard;

pub(super) fn clear_cancelled_preview(
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

pub(super) type EffectDragRows<'w, 's> = Query<
    'w,
    's,
    (
        Option<&'static ProjectEffectRow>,
        Option<&'static AssetPayload>,
    ),
    Or<(With<ProjectEffectRow>, With<AssetPayload>)>,
>;

#[derive(Clone)]
pub(super) enum EffectDragSource {
    Library(ProjectEffectEntryId),
    Asset(AssetPayload),
}

impl EffectDragSource {
    pub(super) fn resolve(
        &self,
        catalog: &ProjectEffectCatalog,
    ) -> Result<(EffectAssetRef, String), String> {
        match self {
            Self::Asset(payload) => payload.timeline_effect(catalog),
            Self::Library(row) => {
                let entry = catalog
                    .entry(*row)
                    .ok_or("The Library entry no longer exists")?;
                Ok((
                    entry
                        .reference
                        .ok_or("The Library entry is not a valid effect asset")?,
                    entry.display_name.clone(),
                ))
            }
        }
    }

    pub(super) fn preview(
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
