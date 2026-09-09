//! Additive material source creation: snapshot affordance, guarded no-clobber publication.
use super::*;
use aestra_core::material::MaterialProgram;
use std::io::Write;

#[derive(Debug)]
pub struct MaterialCreatePlan {
    destination: OperationPlan,
    program: MaterialProgram,
}

impl ProjectContent {
    /// Snapshot-only validation suitable for a dialog. Applying still rechecks the filesystem.
    pub fn material_creation_destination(
        &self,
        parent: ProjectSourceId,
        name: &str,
    ) -> Result<PathBuf, OperationError> {
        if !valid_name(name) {
            return Err(blocked(
                "Enter a portable material name without separators or reserved characters",
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
        let filename = format!("{name}.aestra.material.ron");
        if self
            .source_tree()
            .children(parent)
            .any(|child| child.name.to_string_lossy().to_lowercase() == filename.to_lowercase())
        {
            return Err(blocked("A file or folder with this name already exists"));
        }
        Ok(entry.relative_path.join(filename))
    }

    /// The host validates compiler/domain compatibility before creating a source.
    pub fn plan_material_creation(
        &self,
        parent: ProjectSourceId,
        name: &str,
        program: &MaterialProgram,
    ) -> Result<MaterialCreatePlan, OperationError> {
        self.material_creation_destination(parent, name)?;
        if self.source_tree().entries().any(|entry| {
            self.asset_for_source(entry.id)
                == Some(crate::ProjectAssetId::MaterialProgram(program.id))
        }) {
            return Err(blocked("Material identity already exists"));
        }
        // Serialization validates the authored program before any write.
        program
            .to_pretty_ron()
            .map_err(|error| blocked(&error.to_string()))?;
        let destination = self.plan_operation(OperationRequest::CreateFolder {
            parent,
            name: format!("{name}.aestra.material.ron"),
        })?;
        Ok(MaterialCreatePlan {
            destination,
            program: program.clone(),
        })
    }
}

impl MaterialCreatePlan {
    /// Existing files are never replaced. The saved reusable source has its own lifetime;
    /// undoing its renderer assignment does not delete it.
    pub fn apply(self) -> Result<PathBuf, OperationError> {
        let plan = self.destination;
        transaction::ensure_idle(&plan.root)?;
        checked_parent(&plan.root, &plan.parent)?;
        vacant(&plan.parent, &plan.destination)?;
        let fresh = ProjectContent::scan(&plan.root);
        if fresh.source_tree().entries().any(|entry| {
            fresh.asset_for_source(entry.id)
                == Some(crate::ProjectAssetId::MaterialProgram(self.program.id))
        }) {
            return Err(blocked("Material identity appeared after preflight"));
        }
        let bytes = self
            .program
            .to_pretty_ron()
            .map_err(|error| blocked(&error.to_string()))?;
        let mut staged = tempfile::NamedTempFile::new_in(&plan.parent)?;
        staged.write_all(bytes.as_bytes())?;
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
    fn material_creation_validates_names_identity_and_late_collisions() {
        let root = tempfile::tempdir().unwrap();
        let content = ProjectContent::scan(root.path());
        let parent = content.source_tree().root();
        let program = MaterialProgram::additive_sprite("Created");
        for name in ["", "../escape", "a/b", "CON", ".hidden", "trailing."] {
            assert!(
                content
                    .plan_material_creation(parent, name, &program)
                    .is_err()
            );
        }
        let plan = content
            .plan_material_creation(parent, "created", &program)
            .unwrap();
        let path = plan.apply().unwrap();
        assert_eq!(
            MaterialProgram::load_ron(&path).unwrap(),
            program.normalized()
        );
        assert!(
            content
                .plan_material_creation(parent, "created", &program)
                .is_err()
        );
        let fresh = ProjectContent::scan(root.path());
        assert!(
            fresh
                .plan_material_creation(parent, "other", &program)
                .is_err()
        );
        let other = MaterialProgram::additive_sprite("Other");
        let plan = fresh
            .plan_material_creation(parent, "other", &other)
            .unwrap();
        let conflict = root.path().join("OTHER.aestra.material.ron");
        fs::write(&conflict, "user file").unwrap();
        assert!(plan.apply().is_err());
        assert_eq!(fs::read_to_string(conflict).unwrap(), "user file");
    }
}
