//! Additive effect creation with snapshot preflight and atomic no-clobber publication.
use super::*;
use aestra_core::EffectAsset;
use std::io::Write;

#[derive(Debug)]
pub struct EffectCreatePlan {
    destination: OperationPlan,
    bytes: String,
    id: aestra_core::EffectId,
}

impl ProjectContent {
    pub fn effect_creation_destination(
        &self,
        parent: ProjectSourceId,
        name: &str,
    ) -> Result<PathBuf, OperationError> {
        if !valid_name(name) {
            return Err(blocked(
                "Enter a portable effect name without separators or reserved characters",
            ));
        }
        let entry = self
            .source(parent)
            .ok_or_else(|| blocked("Choose an existing project folder"))?;
        if entry.kind != ProjectSourceKind::Directory
            || entry.error.is_some()
            || entry
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.readonly)
        {
            return Err(blocked("Choose a writable project folder, not a link"));
        }
        let filename = format!("{name}.aestra.ron");
        if self
            .source_tree()
            .children(parent)
            .any(|child| child.name.to_string_lossy().to_lowercase() == filename.to_lowercase())
        {
            return Err(blocked("A file or folder with this name already exists"));
        }
        Ok(entry.relative_path.join(filename))
    }

    /// The host compiles the starter before publishing it.
    pub fn plan_effect_creation(
        &self,
        parent: ProjectSourceId,
        name: &str,
        effect: &EffectAsset,
    ) -> Result<EffectCreatePlan, OperationError> {
        self.effect_creation_destination(parent, name)?;
        if self.source_tree().entries().any(|entry| {
            self.asset_for_source(entry.id) == Some(crate::ProjectAssetId::Effect(effect.id))
        }) {
            return Err(blocked("Effect identity already exists"));
        }
        let bytes = effect
            .to_pretty_ron()
            .map_err(|error| blocked(&error.to_string()))?;
        let destination = self.plan_operation(OperationRequest::CreateFolder {
            parent,
            name: format!("{name}.aestra.ron"),
        })?;
        Ok(EffectCreatePlan {
            destination,
            bytes,
            id: effect.id,
        })
    }
}

impl EffectCreatePlan {
    pub fn apply(self) -> Result<PathBuf, OperationError> {
        let plan = self.destination;
        transaction::ensure_idle(&plan.root)?;
        checked_parent(&plan.root, &plan.parent)?;
        vacant(&plan.parent, &plan.destination)?;
        let fresh = ProjectContent::scan(&plan.root);
        if fresh.source_tree().entries().any(|entry| {
            fresh.asset_for_source(entry.id) == Some(crate::ProjectAssetId::Effect(self.id))
        }) {
            return Err(blocked("Effect identity appeared after preflight"));
        }
        let mut staged = tempfile::NamedTempFile::new_in(&plan.parent)?;
        staged.write_all(self.bytes.as_bytes())?;
        staged.as_file().sync_all()?;
        checked_parent(&plan.root, &plan.parent)?;
        vacant(&plan.parent, &plan.destination)?;
        staged
            .persist_noclobber(&plan.destination)
            .map_err(|error| OperationError::Io(error.error.to_string()))?;
        Ok(plan.destination)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creation_is_independent_and_never_overwrites_even_after_preflight() {
        let root = tempfile::tempdir().unwrap();
        let content = ProjectContent::scan(root.path());
        let parent = content.source_tree().root();
        let effect = EffectAsset::new("First", 2.0);
        for name in ["", "../escape", "a/b", "CON", ".hidden", "trailing."] {
            assert!(content.plan_effect_creation(parent, name, &effect).is_err());
        }
        let path = content
            .plan_effect_creation(parent, "First", &effect)
            .unwrap()
            .apply()
            .unwrap();
        assert_eq!(EffectAsset::load_ron(path).unwrap(), effect);
        let fresh = ProjectContent::scan(root.path());
        assert!(
            fresh
                .plan_effect_creation(parent, "Other", &effect)
                .is_err()
        );
        let other = EffectAsset::new("Other", 2.0);
        let plan = fresh.plan_effect_creation(parent, "Other", &other).unwrap();
        let conflict = root.path().join("OTHER.aestra.ron");
        fs::write(&conflict, "keep").unwrap();
        assert!(plan.apply().is_err());
        assert_eq!(fs::read_to_string(conflict).unwrap(), "keep");
    }
}
