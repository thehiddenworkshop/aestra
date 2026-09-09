//! Editor state retained inside the filesystem recovery journal, never discarded on failure.
use super::*;
use crate::{material_document::MaterialEditingTarget, material_drafts::MaterialDrafts};
use aestra_project::ProjectAssetId;
use std::path::{Path, PathBuf};

#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ClosedDocuments {
    drafts: MaterialDrafts,
    target: Option<MaterialEditingTarget>,
    effect: Option<(aestra_core::EffectId, PathBuf)>,
}

impl ClosedDocuments {
    pub(super) fn capture(
        catalog: &ProjectEffectCatalog,
        session: &EditorSession,
        source: ProjectSourceId,
    ) -> Result<Self, String> {
        let entry = catalog
            .content()
            .source(source)
            .ok_or("Source disappeared")?;
        let selected = entry.path.canonicalize().map_err(|e| e.to_string())?;
        let mut result = Self::default();
        let inside = |asset| {
            catalog
                .content()
                .unique_source_for_asset(asset)
                .is_ok_and(|source| source.relative_path.starts_with(&entry.relative_path))
        };
        result.drafts.root = catalog.material_drafts.root.clone();
        for (id, draft) in &catalog.material_drafts.programs {
            if inside(ProjectAssetId::MaterialProgram(*id)) {
                result.drafts.programs.insert(*id, draft.clone());
            }
        }
        for (id, draft) in &catalog.material_drafts.functions {
            if inside(ProjectAssetId::MaterialFunction(*id)) {
                result.drafts.functions.insert(*id, draft.clone());
            }
        }
        if session
            .standalone_material()
            .is_some_and(|id| inside(ProjectAssetId::MaterialProgram(id)))
            || session
                .standalone_function()
                .is_some_and(|id| inside(ProjectAssetId::MaterialFunction(id)))
        {
            result.target = Some(session.material_target.clone());
        }
        if let Some(path) = &session.source_path
            && path
                .canonicalize()
                .is_ok_and(|path| path.starts_with(&selected))
        {
            result.effect = Some((
                session.effect.id,
                catalog.root().join(
                    path.canonicalize()
                        .map_err(|e| e.to_string())?
                        .strip_prefix(catalog.root().canonicalize().map_err(|e| e.to_string())?)
                        .map_err(|e| e.to_string())?,
                ),
            ));
        }
        Ok(result)
    }

    pub(super) fn read(item: &DeletedSource) -> Result<Self, String> {
        if item.recovery_data().is_empty() {
            return Ok(Self::default());
        }
        ron::de::from_bytes(item.recovery_data())
            .map_err(|error| format!("Invalid editor recovery state: {error}"))
    }
    pub(super) fn encode(&self) -> Result<Vec<u8>, String> {
        ron::to_string(self)
            .map(String::into_bytes)
            .map_err(|error| error.to_string())
    }
    pub(super) fn without_drafts(&self, catalog: &ProjectEffectCatalog) -> ProjectEffectCatalog {
        let mut result = catalog.clone();
        for id in self.drafts.programs.keys() {
            result.material_drafts.programs.remove(id);
        }
        for id in self.drafts.functions.keys() {
            result.material_drafts.functions.remove(id);
        }
        result
    }
    pub(super) fn validate_restore(
        &self,
        catalog: &ProjectEffectCatalog,
        relative: &Path,
    ) -> Result<(), String> {
        if !self.drafts.is_empty()
            && !self.drafts.root.as_ref().is_some_and(|root| {
                root.canonicalize()
                    .ok()
                    .zip(catalog.root().canonicalize().ok())
                    .is_some_and(|(a, b)| a == b)
            })
        {
            return Err("Recovered drafts belong to another project".into());
        }
        let valid_path = |path: &Path| {
            path.strip_prefix(catalog.root()).is_ok_and(|path| {
                path.starts_with(relative)
                    && !path
                        .components()
                        .any(|part| matches!(part, std::path::Component::ParentDir))
            })
        };
        if self
            .effect
            .as_ref()
            .is_some_and(|(_, path)| !valid_path(path))
            || self.target.as_ref().is_some_and(|target| match target {
                MaterialEditingTarget::Program { root, .. }
                | MaterialEditingTarget::Function { root, .. } => root != catalog.root(),
                MaterialEditingTarget::EffectInstance => true,
            })
        {
            return Err("Invalid recovered editor target".into());
        }
        for (id, draft) in &self.drafts.programs {
            if catalog.material_drafts.programs.contains_key(id)
                || !valid_path(&draft.path)
                || draft.current.as_ref().is_some_and(|value| value.id != *id)
            {
                return Err(
                    "Restore blocked: a material draft conflicts with the recovered document"
                        .into(),
                );
            }
        }
        for (id, draft) in &self.drafts.functions {
            if catalog.material_drafts.functions.contains_key(id)
                || !valid_path(&draft.path)
                || draft.current.as_ref().is_some_and(|value| value.id != *id)
            {
                return Err(
                    "Restore blocked: a function draft conflicts with the recovered document"
                        .into(),
                );
            }
        }
        Ok(())
    }
    pub(super) fn validate_redo(&self, current: &Self) -> Result<(), String> {
        if self.drafts.programs != current.drafts.programs
            || self.drafts.functions != current.drafts.functions
        {
            return Err("Redo blocked: restored drafts changed".into());
        }
        Ok(())
    }
    pub(super) fn close(&self, world: &mut World) {
        let remaining = self
            .without_drafts(world.resource::<ProjectEffectCatalog>())
            .material_drafts;
        world.resource_mut::<ProjectEffectCatalog>().material_drafts = remaining.clone();
        let mut session = world.resource_mut::<EditorSession>();
        session.set_material_drafts(remaining);
        if self.target.as_ref() == Some(&session.material_target) {
            session.return_to_effect_material();
        }
        if self
            .effect
            .as_ref()
            .is_some_and(|(id, _)| *id == session.effect.id)
        {
            // Keep the current effect in memory as Untitled; Save now requires Save As.
            session.source_path = None;
        }
        session.ui_revision += 1;
    }
    pub(super) fn restore(&self, world: &mut World) {
        let mut catalog = world.resource_mut::<ProjectEffectCatalog>();
        if !self.drafts.is_empty() {
            catalog.material_drafts.root = self.drafts.root.clone();
        }
        catalog
            .material_drafts
            .programs
            .extend(self.drafts.programs.clone());
        catalog
            .material_drafts
            .functions
            .extend(self.drafts.functions.clone());
        let drafts = catalog.material_drafts.clone();
        let mut session = world.resource_mut::<EditorSession>();
        session.set_material_drafts(drafts);
        if let Some(target) = &self.target {
            session.material_target = target.clone();
            session.material_history_active = true;
        }
        if let Some((id, path)) = &self.effect
            && session.effect.id == *id
            && session.source_path.is_none()
        {
            session.accept_external_source_path(path.clone());
        }
        session.ui_revision += 1;
    }
}
