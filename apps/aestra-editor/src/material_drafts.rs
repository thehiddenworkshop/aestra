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
    /// Resolve moved sources by unique identity without changing the recovered byte baseline.
    /// Missing/ambiguous sources remain drafts; they are never rebound arbitrarily.
    pub(crate) fn recover_paths(
        &mut self,
        index: &ProjectAssetIndex,
    ) -> Result<Vec<String>, String> {
        let root = index
            .root()
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let stored_root = self
            .root
            .as_deref()
            .ok_or("Recovery has no material project root")?;
        if stored_root
            .canonicalize()
            .map_err(|error| error.to_string())?
            != root
        {
            return Err("Recovery drafts belong to a different project".into());
        }
        let mut warnings = Vec::new();
        let validate = |path: &Path| -> Result<(), String> {
            if !path.is_absolute()
                || path
                    .components()
                    .any(|part| matches!(part, std::path::Component::ParentDir))
                || !(path.starts_with(stored_root) || path.starts_with(&root))
                || !path
                    .ancestors()
                    .find_map(|ancestor| ancestor.canonicalize().ok())
                    .is_some_and(|ancestor| ancestor.starts_with(&root))
            {
                return Err("Recovery material paths are outside their project".into());
            }
            Ok(())
        };
        for (id, draft) in &mut self.programs {
            validate(&draft.path)?;
            if draft
                .original
                .as_ref()
                .is_some_and(|program| program.id != *id)
                || draft
                    .current
                    .as_ref()
                    .is_some_and(|program| program.id != *id)
            {
                return Err("Recovered material identity does not match its draft".into());
            }
            match index.resolve_material_program(MaterialProgramRef::Project(*id)) {
                Ok(entry) => {
                    validate(&entry.path)?;
                    draft.path.clone_from(&entry.path);
                }
                Err(error) => warnings.push(error.to_string()),
            }
        }
        for (id, draft) in &mut self.functions {
            validate(&draft.path)?;
            if draft
                .original
                .as_ref()
                .is_some_and(|function| function.id != *id)
                || draft
                    .current
                    .as_ref()
                    .is_some_and(|function| function.id != *id)
            {
                return Err("Recovered function identity does not match its draft".into());
            }
            // A newly extracted, not-yet-saved function has no source to resolve.
            if draft.original.is_none() {
                continue;
            }
            match index.resolve_material_function(MaterialFunctionRef::Project(*id)) {
                Ok(entry) => {
                    validate(&entry.path)?;
                    draft.path.clone_from(&entry.path);
                }
                Err(error) => warnings.push(error.to_string()),
            }
        }
        self.root = Some(root);
        if let Err(error) = self.preflight() {
            warnings.push(error);
        }
        Ok(warnings)
    }
    /// Worker-only receipts for sources successfully written by a possibly partial save.
    pub(crate) fn saved_baselines(before: &Self, remaining: &Self) -> Self {
        fn receipt<T: Clone>(draft: &Draft<T>, bytes: Option<Vec<u8>>) -> Draft<T> {
            Draft {
                path: draft.path.clone(),
                original: draft.current.clone(),
                current: draft.current.clone(),
                bytes,
            }
        }
        Self {
            root: before.root.clone(),
            programs: before
                .programs
                .iter()
                .filter(|(id, _)| !remaining.programs.contains_key(id))
                .map(|(id, draft)| {
                    (
                        *id,
                        receipt(
                            draft,
                            draft.current.as_ref().map(|value| {
                                value
                                    .to_pretty_ron()
                                    .expect("saved program serialized")
                                    .into_bytes()
                            }),
                        ),
                    )
                })
                .collect(),
            functions: before
                .functions
                .iter()
                .filter(|(id, _)| !remaining.functions.contains_key(id))
                .map(|(id, draft)| {
                    (
                        *id,
                        receipt(
                            draft,
                            draft.current.as_ref().map(|value| {
                                value
                                    .to_pretty_ron()
                                    .expect("saved function serialized")
                                    .into_bytes()
                            }),
                        ),
                    )
                })
                .collect(),
        }
    }

    /// Preserve edits made during a save but rebase them onto exactly what that save wrote.
    pub(crate) fn accept_saved_baselines(&mut self, before: &Self, receipts: Self) {
        fn merge<K: Ord, T: Clone + PartialEq>(
            live: &mut BTreeMap<K, Draft<T>>,
            before: &BTreeMap<K, Draft<T>>,
            receipts: BTreeMap<K, Draft<T>>,
        ) {
            for (id, receipt) in receipts {
                // Undo during a save can remove the draft entirely. Recreate that unsaved
                // intent against the new disk baseline instead of silently accepting the save.
                match live.entry(id) {
                    Entry::Vacant(slot) => {
                        let mut reverted = receipt;
                        reverted.current = before[slot.key()].original.clone();
                        if reverted.current != reverted.original {
                            slot.insert(reverted);
                        }
                    }
                    Entry::Occupied(mut slot) => {
                        let draft = slot.get_mut();
                        draft.original = receipt.original;
                        draft.bytes = receipt.bytes;
                        if draft.current == draft.original {
                            slot.remove();
                        }
                    }
                }
            }
        }
        merge(&mut self.programs, &before.programs, receipts.programs);
        merge(&mut self.functions, &before.functions, receipts.functions);
        if !self.is_empty() {
            self.root.clone_from(&before.root);
        }
    }

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

    fn edited_program(root: &Path) -> (MaterialDrafts, MaterialProgram, MaterialProgram) {
        let original = MaterialProgram::additive_sprite("Original").normalized();
        original
            .save_ron(root.join("program.aestra.material.ron"))
            .unwrap();
        let mut edited = original.clone();
        edited.name = "Saved edit".into();
        let mut drafts = MaterialDrafts::default();
        drafts
            .replace_program(&ProjectAssetIndex::scan(root), &original, &edited)
            .unwrap();
        (drafts, original, edited)
    }

    #[test]
    fn save_receipt_rebases_newer_edits_and_removes_only_unchanged_drafts() {
        let directory = tempfile::tempdir().unwrap();
        let (before, _, edited) = edited_program(directory.path());
        let mut worker = before.clone();
        worker.save().unwrap();
        let receipts = MaterialDrafts::saved_baselines(&before, &worker);
        let mut live = before.clone();
        live.programs
            .get_mut(&edited.id)
            .unwrap()
            .current
            .as_mut()
            .unwrap()
            .name = "Newer edit".into();
        live.accept_saved_baselines(&before, receipts.clone());
        assert_eq!(live.programs[&edited.id].original.as_ref(), Some(&edited));
        assert_eq!(
            live.programs[&edited.id].current.as_ref().unwrap().name,
            "Newer edit"
        );
        live.preflight().unwrap();
        let mut unchanged = before.clone();
        unchanged.accept_saved_baselines(&before, receipts);
        assert!(unchanged.is_empty());
    }

    #[test]
    fn undo_during_save_remains_an_unsaved_edit_against_the_new_baseline() {
        let directory = tempfile::tempdir().unwrap();
        let (before, original, edited) = edited_program(directory.path());
        let mut worker = before.clone();
        worker.save().unwrap();
        let mut undone = MaterialDrafts::default();
        undone.accept_saved_baselines(&before, MaterialDrafts::saved_baselines(&before, &worker));
        assert_eq!(undone.programs[&edited.id].original.as_ref(), Some(&edited));
        assert_eq!(
            undone.programs[&edited.id].current.as_ref(),
            Some(&original)
        );
        undone.save().unwrap();
        assert_eq!(
            MaterialProgram::load_ron(directory.path().join("program.aestra.material.ron"))
                .unwrap(),
            original
        );
    }

    #[test]
    fn partial_save_receipts_keep_failed_drafts_and_accept_successful_ones() {
        let directory = tempfile::tempdir().unwrap();
        let mut first = MaterialProgram::additive_sprite("First");
        first.id = MaterialProgramId::from_u128(1);
        let mut second = MaterialProgram::additive_sprite("Second");
        second.id = MaterialProgramId::from_u128(2);
        first
            .save_ron(directory.path().join("first.aestra.material.ron"))
            .unwrap();
        let second_path = directory.path().join("second.aestra.material.ron");
        second.save_ron(&second_path).unwrap();
        let second_bytes = fs::read(&second_path).unwrap();
        let index = ProjectAssetIndex::scan(directory.path());
        let mut before = MaterialDrafts::default();
        for original in [&first, &second] {
            let mut edited = original.clone();
            edited.name.push_str(" edited");
            before.replace_program(&index, original, &edited).unwrap();
        }
        // A structural error in the second draft fails after the first source is written.
        before
            .programs
            .get_mut(&second.id)
            .unwrap()
            .current
            .as_mut()
            .unwrap()
            .id = MaterialProgramId::from_u128(0);
        let mut worker = before.clone();
        assert!(worker.save().is_err());
        assert_eq!(worker.count(), 1);
        let mut live = before.clone();
        live.accept_saved_baselines(&before, MaterialDrafts::saved_baselines(&before, &worker));
        assert_eq!(live, worker);
        assert_eq!(fs::read(second_path).unwrap(), second_bytes);
        live.preflight().unwrap();
    }

    #[test]
    fn receipts_use_our_written_bytes_not_an_external_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let (before, _, edited) = edited_program(directory.path());
        let mut worker = before.clone();
        worker.save().unwrap();
        let path = &before.programs[&edited.id].path;
        let external = format!("// External comment\n{}", fs::read_to_string(path).unwrap());
        fs::write(path, external).unwrap();
        let mut live = before.clone();
        live.programs
            .get_mut(&edited.id)
            .unwrap()
            .current
            .as_mut()
            .unwrap()
            .name = "New edit".into();
        live.accept_saved_baselines(&before, MaterialDrafts::saved_baselines(&before, &worker));
        assert!(live.preflight().unwrap_err().contains("changed outside"));
    }

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
