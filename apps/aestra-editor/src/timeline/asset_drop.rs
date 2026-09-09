//! Both browser and transitional Library sources feed the same placement command.
use super::*;
use crate::asset_browser::payload::AssetPayload;
#[cfg(test)]
mod tests;

#[derive(bevy::ecs::system::SystemParam)]
pub(super) struct DropGuard<'w> {
    protection: Option<Res<'w, crate::DocumentProtectionState>>,
    tasks: Option<Res<'w, crate::project_content::io::ProjectIoTasks>>,
}

impl DropGuard<'_> {
    pub(super) fn check(&self) -> Result<(), String> {
        if self
            .protection
            .as_ref()
            .is_some_and(|state| state.is_open())
            || !crate::project_content::io::idle(self.tasks.as_ref().map(Res::clone))
        {
            return Err("Finish the current document operation before adding an effect".into());
        }
        Ok(())
    }
}

pub(super) fn clear_cancelled_preview(
    payloads: Query<&AssetPayload>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    catalog: Res<ProjectEffectCatalog>,
    mut state: ResMut<TimelineState>,
) {
    if state.browser_drop.as_ref().is_some_and(|payload| {
        keys.as_ref()
            .is_some_and(|keys| keys.just_pressed(KeyCode::Escape))
            || payload.resolve(&catalog).is_err()
            || !payloads.iter().any(|active| active == payload)
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
