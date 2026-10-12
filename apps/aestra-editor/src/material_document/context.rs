//! Actual standalone/effect graph authoring context and project identity guards.
use crate::material_target::MaterialEditingTarget;
use crate::{ProjectEffectCatalog, session::EditorSession};
use aestra_authoring::MaterialAuthoringDocument;
use aestra_core::{MaterialProgramId, material::MaterialProgram};
impl EditorSession {
    pub(crate) fn standalone_material(&self) -> Option<MaterialProgramId> {
        self.material_target.program()
    }

    pub(crate) fn standalone_function(&self) -> Option<aestra_core::MaterialFunctionId> {
        self.material_target.function()
    }

    pub(crate) fn open_material_function(
        &mut self,
        catalog: &ProjectEffectCatalog,
        id: aestra_core::MaterialFunctionId,
    ) -> Result<(), String> {
        catalog
            .content()
            .cached_material_function(aestra_core::material::MaterialFunctionRef::Project(id))
            .map_err(|error| error.to_string())?;
        let target = MaterialEditingTarget::Function {
            root: catalog.root().to_owned(),
            id,
        };
        if self.material_target != target {
            self.material_target = target;
            self.ui_revision += 1;
        }
        self.material_history_active = true;
        Ok(())
    }

    pub(crate) fn graph_function(
        &self,
        catalog: &ProjectEffectCatalog,
    ) -> Result<aestra_core::material::MaterialFunction, String> {
        self.graph_function_for(&self.material_target, catalog)
    }

    /// Resolves the function for an explicit target, independent of the session's current target.
    /// Per-view rendering (M4c) passes the view's target here; `graph_function` delegates with the
    /// session target.
    pub(crate) fn graph_function_for(
        &self,
        target: &MaterialEditingTarget,
        catalog: &ProjectEffectCatalog,
    ) -> Result<aestra_core::material::MaterialFunction, String> {
        let MaterialEditingTarget::Function { root, id } = target else {
            return Err("No function selected".into());
        };
        if root != catalog.root() {
            return Err("Function belongs to another project; reopen it from Assets".into());
        }
        // Resolve current source identity before consulting drafts.
        catalog
            .content()
            .cached_material_function(aestra_core::material::MaterialFunctionRef::Project(*id))
            .map_err(|error| error.to_string())?;
        catalog
            .material_functions()?
            .into_iter()
            .find(|function| function.id == *id)
            .ok_or_else(|| "Function unavailable".into())
    }

    pub(crate) fn open_material_program(
        &mut self,
        catalog: &ProjectEffectCatalog,
        id: MaterialProgramId,
    ) -> Result<(), String> {
        // Resolve identity against the published snapshot even if a draft exists. A duplicate
        // or removed source must not silently win by reusing an old cached draft.
        catalog.material_program(id)?;
        let target = MaterialEditingTarget::Program {
            root: catalog.root().to_owned(),
            id,
        };
        if self.material_target != target {
            self.material_target = target;
            self.ui_revision += 1;
        }
        self.material_history_active = true;
        Ok(())
    }

    pub(crate) fn return_to_effect_material(&mut self) {
        if self.material_target != MaterialEditingTarget::EffectInstance {
            self.material_target = MaterialEditingTarget::EffectInstance;
            self.material_history_active = false;
            self.ui_revision += 1;
        }
    }

    pub(crate) fn graph_material_programs(
        &self,
        catalog: &ProjectEffectCatalog,
    ) -> Result<Vec<MaterialProgram>, String> {
        self.graph_material_programs_for(&self.material_target, catalog)
    }

    pub(crate) fn graph_material_programs_for(
        &self,
        target: &MaterialEditingTarget,
        catalog: &ProjectEffectCatalog,
    ) -> Result<Vec<MaterialProgram>, String> {
        match target {
            MaterialEditingTarget::Function { .. } => Ok(Vec::new()),
            MaterialEditingTarget::EffectInstance => {
                catalog.material_programs_for_effect(&self.effect)
            }
            MaterialEditingTarget::Program { root, id } => {
                if root != catalog.root() {
                    return Err(
                        "The material belongs to another project; reopen it from Assets".into(),
                    );
                }
                Ok(vec![catalog.material_program(*id)?])
            }
        }
    }

    pub(crate) fn graph_authoring_document(
        &self,
        catalog: &ProjectEffectCatalog,
    ) -> Result<MaterialAuthoringDocument, String> {
        self.graph_authoring_document_for(&self.material_target, catalog)
    }

    /// Builds the authoring document for an explicit target, independent of the session's current
    /// target. Per-view rendering (M4c) uses this; `graph_authoring_document` delegates with the
    /// session target.
    pub(crate) fn graph_authoring_document_for(
        &self,
        target: &MaterialEditingTarget,
        catalog: &ProjectEffectCatalog,
    ) -> Result<MaterialAuthoringDocument, String> {
        if target.function().is_some() {
            self.graph_function_for(target, catalog)?;
        }
        let programs = self.graph_material_programs_for(target, catalog)?;
        let document = if target.program().is_some() || target.function().is_some() {
            MaterialAuthoringDocument::standalone(programs)
        } else {
            MaterialAuthoringDocument::new(self.effect.clone(), programs)
        };
        Ok(document.with_material_functions(catalog.material_functions()?))
    }
}
