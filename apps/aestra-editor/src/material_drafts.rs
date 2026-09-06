//! Unsaved shared material sources and their exact disk baselines.

use aestra_core::{
    MaterialFunctionId, MaterialProgramId,
    material::{MaterialFunction, MaterialFunctionRef, MaterialProgram, MaterialProgramRef},
};
use aestra_project::ProjectAssetIndex;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, btree_map::Entry},
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Draft<T> {
    pub path: PathBuf,
    original: Option<T>,
    bytes: Option<Vec<u8>>,
    pub current: Option<T>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct MaterialDrafts {
    pub root: Option<PathBuf>,
    pub programs: BTreeMap<MaterialProgramId, Draft<MaterialProgram>>,
    pub functions: BTreeMap<MaterialFunctionId, Draft<MaterialFunction>>,
}

impl MaterialDrafts {
    pub fn is_empty(&self) -> bool {
        self.programs.is_empty() && self.functions.is_empty()
    }
    pub fn count(&self) -> usize {
        self.programs.len() + self.functions.len()
    }

    pub fn replace_program(
        &mut self,
        index: &ProjectAssetIndex,
        expected: &MaterialProgram,
        replacement: &MaterialProgram,
    ) -> Result<(), String> {
        if expected.id != replacement.id {
            return Err("Material identity cannot change".into());
        }
        if let Entry::Vacant(slot) = self.programs.entry(expected.id) {
            let entry = index
                .resolve_material_program(MaterialProgramRef::Project(expected.id))
                .map_err(|error| error.to_string())?;
            let bytes = fs::read(&entry.path).map_err(|error| error.to_string())?;
            let original =
                MaterialProgram::load_ron(&entry.path).map_err(|error| error.to_string())?;
            if original != expected.normalized() {
                return Err("Material changed outside the editor; reopen it before editing".into());
            }
            slot.insert(Draft {
                path: entry.path.clone(),
                original: Some(original.clone()),
                current: Some(original),
                bytes: Some(bytes),
            });
        }
        let draft = self.programs.get_mut(&expected.id).unwrap();
        if draft.current.as_ref() != Some(&expected.normalized()) {
            return Err("Material draft changed since this edit was prepared".into());
        }
        draft.current = Some(replacement.normalized());
        if draft.current == draft.original {
            self.programs.remove(&expected.id);
        }
        self.root = Some(index.root().to_owned());
        Ok(())
    }

    pub fn create_function(
        &mut self,
        index: &ProjectAssetIndex,
        function: &MaterialFunction,
    ) -> Result<(), String> {
        if let Some(draft) = self.functions.get_mut(&function.id) {
            if draft.current.is_some() {
                return Err("Material function already exists".into());
            }
            draft.current = Some(function.clone());
            if draft.current == draft.original {
                self.functions.remove(&function.id);
            }
        } else {
            if index
                .resolve_material_function(MaterialFunctionRef::Project(function.id))
                .is_ok()
            {
                return Err("Material function already exists".into());
            }
            let path = index
                .root()
                .join(format!("{}.aestra.material-function.ron", function.id));
            if path.exists() {
                return Err(format!("{} already exists", path.display()));
            }
            self.functions.insert(
                function.id,
                Draft {
                    path,
                    original: None,
                    bytes: None,
                    current: Some(function.clone()),
                },
            );
        }
        self.root = Some(index.root().to_owned());
        Ok(())
    }

    pub fn delete_function(
        &mut self,
        index: &ProjectAssetIndex,
        function: &MaterialFunction,
    ) -> Result<(), String> {
        if let Entry::Vacant(slot) = self.functions.entry(function.id) {
            let entry = index
                .resolve_material_function(MaterialFunctionRef::Project(function.id))
                .map_err(|error| error.to_string())?;
            let original =
                MaterialFunction::load_ron(&entry.path).map_err(|error| error.to_string())?;
            if original != *function {
                return Err("Material function changed outside the editor".into());
            }
            slot.insert(Draft {
                path: entry.path.clone(),
                bytes: Some(fs::read(&entry.path).map_err(|error| error.to_string())?),
                original: Some(original.clone()),
                current: Some(original),
            });
        }
        let draft = self.functions.get_mut(&function.id).unwrap();
        if draft.current.as_ref() != Some(function) {
            return Err("Material function draft changed".into());
        }
        draft.current = None;
        if draft.original.is_none() {
            self.functions.remove(&function.id);
        }
        self.root = Some(index.root().to_owned());
        Ok(())
    }

    pub fn preflight(&self) -> Result<(), String> {
        for (path, expected) in self
            .programs
            .values()
            .map(|draft| (&draft.path, &draft.bytes))
            .chain(
                self.functions
                    .values()
                    .map(|draft| (&draft.path, &draft.bytes)),
            )
        {
            let actual = match fs::read(path) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(format!("{}: {error}", path.display())),
            };
            if actual != *expected {
                return Err(format!(
                    "{} changed outside the editor. Unsaved material changes were kept; discard them and reopen the source to resolve the conflict.",
                    path.display()
                ));
            }
        }
        Ok(())
    }

    /// Each file is replaced atomically. Successfully saved files leave the draft set even if a
    /// later I/O operation fails, so retry/discard accurately describe the remaining changes.
    pub fn save(&mut self) -> Result<(), String> {
        self.preflight()?;
        for id in self.functions.keys().copied().collect::<Vec<_>>() {
            let draft = &self.functions[&id];
            if let Some(function) = &draft.current {
                function
                    .save_ron(&draft.path)
                    .map_err(|error| error.to_string())?;
                self.functions.remove(&id);
            }
        }
        for id in self.programs.keys().copied().collect::<Vec<_>>() {
            let draft = &self.programs[&id];
            draft
                .current
                .as_ref()
                .ok_or("Cannot delete a material program")?
                .save_ron(&draft.path)
                .map_err(|error| error.to_string())?;
            self.programs.remove(&id);
        }
        for id in self.functions.keys().copied().collect::<Vec<_>>() {
            fs::remove_file(&self.functions[&id].path).map_err(|error| error.to_string())?;
            self.functions.remove(&id);
        }
        Ok(())
    }

    pub fn validate_root(&self, root: &Path) -> bool {
        let Ok(root) = root.canonicalize() else {
            return false;
        };
        self.programs
            .values()
            .map(|draft| &draft.path)
            .chain(self.functions.values().map(|draft| &draft.path))
            .all(|path| {
                path.parent()
                    .and_then(|parent| parent.canonicalize().ok())
                    .is_some_and(|parent| parent.starts_with(&root))
                    && !path
                        .components()
                        .any(|part| matches!(part, std::path::Component::ParentDir))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_comment_changes_block_the_entire_material_save() {
        let directory = tempfile::tempdir().unwrap();
        let first = MaterialProgram::additive_sprite("First");
        let second = MaterialProgram::additive_sprite("Second");
        let a = directory.path().join("first.aestra.material.ron");
        let b = directory.path().join("second.aestra.material.ron");
        first.save_ron(&a).unwrap();
        second.save_ron(&b).unwrap();
        let bytes = fs::read(&a).unwrap();
        let index = ProjectAssetIndex::scan(directory.path());
        let mut drafts = MaterialDrafts::default();
        let mut replacement = first.clone();
        replacement.name = "Edited first".into();
        drafts
            .replace_program(&index, &first, &replacement)
            .unwrap();
        let mut replacement = second.clone();
        replacement.name = "Edited second".into();
        drafts
            .replace_program(&index, &second, &replacement)
            .unwrap();
        let external = format!("// External comment\n{}", fs::read_to_string(&b).unwrap());
        fs::write(&b, &external).unwrap();
        assert!(drafts.save().unwrap_err().contains("changed outside"));
        assert_eq!(fs::read(&a).unwrap(), bytes);
        assert_eq!(fs::read_to_string(&b).unwrap(), external);
        assert_eq!(drafts.count(), 2);
    }
}
