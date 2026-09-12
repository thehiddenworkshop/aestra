//! Shared source identity for authoring drops and, later, compatible asset pickers.
//! A drag is a snapshot-bound intent, never authority to mutate a file/document.
use crate::*;
use aestra_project::{ProjectAssetId, ProjectContentVersion, ProjectSourceId};
use std::path::PathBuf;

/// Shared gate for typed authoring drops; filesystem operations have their own I/O guards.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct AuthoringDropGuard<'w> {
    keys: Option<Res<'w, ButtonInput<KeyCode>>>,
    protection: Option<Res<'w, DocumentProtectionState>>,
    tasks: Option<Res<'w, crate::project_content::io::ProjectIoTasks>>,
}

impl AuthoringDropGuard<'_> {
    pub(crate) fn check_release(&self) -> Result<(), String> {
        if super::cancelled(self.keys.as_deref()) {
            return Err("Asset drop cancelled".into());
        }
        self.check()
    }

    pub(crate) fn check(&self) -> Result<(), String> {
        if self
            .protection
            .as_ref()
            .is_some_and(|state| state.is_open())
            || !crate::project_content::io::idle(self.tasks.as_ref().map(Res::clone))
        {
            return Err("Finish the current document operation before dropping an asset".into());
        }
        Ok(())
    }
}

#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(crate) struct AssetPayload {
    root: PathBuf,
    version: ProjectContentVersion,
    source: ProjectSourceId,
    asset: Option<ProjectAssetId>,
}

impl AssetPayload {
    pub(crate) fn mesh_source<'a>(
        &self,
        catalog: &'a ProjectEffectCatalog,
    ) -> Result<&'a aestra_project::ProjectSourceEntry, String> {
        self.resolve(catalog)?;
        let source = catalog
            .content()
            .source(self.source)
            .ok_or("Mesh source disappeared")?;
        if !matches!(&source.kind, aestra_project::ProjectSourceKind::File(info)
            if info.classification == aestra_project::ProjectFileClassification::Mesh)
            || source.error.is_some()
        {
            return Err("Drop a glTF or GLB mesh onto this input".into());
        }
        let path = source
            .relative_path
            .to_str()
            .ok_or("Mesh path is not valid UTF-8")?;
        if path.contains(['#', ':']) {
            return Err("Mesh filename contains a loader-reserved character (# or :)".into());
        }
        let extension = source
            .path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if !["gltf", "glb"]
            .iter()
            .any(|supported| extension.eq_ignore_ascii_case(supported))
        {
            return Err(
                "Mesh drops support glTF and GLB; convert this file first (OBJ has no loader)"
                    .into(),
            );
        }
        Ok(source)
    }

    /// File-backed texture intent; format support comes from the runtime loader, not icons.
    pub(crate) fn texture_source<'a>(
        &self,
        catalog: &'a ProjectEffectCatalog,
    ) -> Result<&'a aestra_project::ProjectSourceEntry, String> {
        self.resolve(catalog)?;
        let source = catalog
            .content()
            .source(self.source)
            .ok_or("Texture source disappeared")?;
        if !matches!(&source.kind, aestra_project::ProjectSourceKind::File(info)
            if info.classification == aestra_project::ProjectFileClassification::Texture)
            || source.error.is_some()
        {
            return Err("Drop a texture file onto this input".into());
        }
        let path = source
            .relative_path
            .to_str()
            .ok_or("Texture path is not valid UTF-8")?;
        if path.contains(['#', ':']) {
            return Err("Texture path contains a loader-reserved character (# or :)".into());
        }
        let extension = source
            .path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default();
        if bevy::image::ImageFormat::from_extension(extension).is_none() {
            return Err(format!(
                "The texture loader does not support .{extension} in this build"
            ));
        }
        Ok(source)
    }

    pub(crate) fn capture(catalog: &ProjectEffectCatalog, source: ProjectSourceId) -> Self {
        Self {
            root: catalog.root().to_path_buf(),
            version: catalog.content_revision(),
            source,
            asset: catalog.content().asset_for_source(source),
        }
    }

    pub(crate) fn resolve(
        &self,
        catalog: &ProjectEffectCatalog,
    ) -> Result<Option<ProjectAssetId>, String> {
        if self.root != catalog.root() || self.version != catalog.content_revision() {
            return Err("Project changed; drag the asset again".into());
        }
        catalog
            .content()
            .source(self.source)
            .ok_or("Source no longer exists")?;
        if catalog.content().asset_for_source(self.source) != self.asset {
            return Err("Asset identity changed; drag the asset again".into());
        }
        if let Some(asset) = self.asset {
            let source = catalog
                .content()
                .unique_source_for_asset(asset)
                .map_err(|error| error.to_string())?;
            if source.id != self.source {
                return Err("Asset source changed; drag the asset again".into());
            }
        }
        Ok(self.asset)
    }

    pub(crate) fn timeline_effect(
        &self,
        catalog: &ProjectEffectCatalog,
    ) -> Result<(aestra_core::EffectAssetRef, String), String> {
        let Some(ProjectAssetId::Effect(id)) = self.resolve(catalog)? else {
            return Err("The timeline accepts effect assets, not this asset type".into());
        };
        let reference = aestra_core::EffectAssetRef::new(id);
        let effect = catalog.cached_effect(reference)?;
        Ok((reference, effect.name))
    }
}
