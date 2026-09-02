//! Diagnostic-first, machine-readable material compilation reports.

use crate::{
    MaterialAuthoringDocument, MaterialInspectionError, MaterialInspectionTarget, MaterialInspector,
};
use aestra_compiler::{MaterialCompiler, MaterialIrProgram};
use aestra_core::{MaterialId, MaterialProgramId, ValidationReport};
use serde::{Deserialize, Serialize};

/// Serializable compilation result for one stable material target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialCompilationReport {
    pub target: MaterialInspectionTarget,
    pub program: MaterialProgramId,
    pub instance: Option<MaterialId>,
    /// Backend-neutral IR, including its source map and optimization statistics.
    ///
    /// This is absent whenever target validation or compilation reports an error.
    pub ir: Option<MaterialIrProgram>,
    pub diagnostics: ValidationReport,
}

impl MaterialCompilationReport {
    pub fn is_valid(&self) -> bool {
        self.diagnostics.is_valid()
    }
}

#[derive(Debug, Default)]
pub struct MaterialCompilationReporter;

impl MaterialCompilationReporter {
    /// Compiles a stable program or instance target without mutating authoring state.
    ///
    /// Instances share their program IR, but must pass their own override and external-binding
    /// validation before that IR is returned.
    pub fn compile(
        document: &MaterialAuthoringDocument,
        target: MaterialInspectionTarget,
    ) -> Result<MaterialCompilationReport, MaterialInspectionError> {
        let inspection = MaterialInspector::inspect(document, target)?;
        let program = inspection.program.id;
        let instance = inspection.instance.as_ref().map(|instance| instance.id);
        let mut diagnostics = inspection.diagnostics;
        let ir = if diagnostics.is_valid() {
            match MaterialCompiler.compile(&inspection.program) {
                Ok(ir) => Some(ir),
                Err(error) => {
                    diagnostics = error.report().clone();
                    None
                }
            }
        } else {
            None
        };
        Ok(MaterialCompilationReport {
            target,
            program,
            instance,
            ir,
            diagnostics,
        })
    }
}
