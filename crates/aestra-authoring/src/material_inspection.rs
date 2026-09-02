//! Machine-readable material inspection composed from semantic core and compiler projections.

use crate::{MaterialAuthoringDocument, MaterialInsertionPoint};
use aestra_compiler::{
    MaterialCompiler, MaterialControlCatalog, MaterialStackModifierKind, MaterialStackPresetKind,
    MaterialStackProjection,
};
use aestra_core::{
    MaterialId, MaterialProgramId, ValidationReport,
    material::{MaterialInstance, MaterialProgram},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Stable target for a read-only material inspection request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialInspectionTarget {
    Program(MaterialProgramId),
    Instance(MaterialId),
}

/// One compiler-approved semantic operation and stable insertion edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialOperationAvailability {
    pub kind: MaterialStackModifierKind,
    pub placement: MaterialInsertionPoint,
}

/// One compiler-approved preset and stable insertion edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialPresetAvailability {
    pub preset: MaterialStackPresetKind,
    pub placement: MaterialInsertionPoint,
}

/// Serializable snapshot used by AI, CLI, and other tool clients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialInspectionReport {
    pub target: MaterialInspectionTarget,
    pub program: MaterialProgram,
    pub instance: Option<MaterialInstance>,
    /// Reflected controls are absent when target diagnostics prevent a reliable projection.
    pub controls: Option<MaterialControlCatalog>,
    /// The stack is absent only when the program itself cannot be projected.
    pub stack: Option<MaterialStackProjection>,
    pub operations: Vec<MaterialOperationAvailability>,
    pub presets: Vec<MaterialPresetAvailability>,
    pub diagnostics: ValidationReport,
}

impl MaterialInspectionReport {
    pub fn is_valid(&self) -> bool {
        self.diagnostics.is_valid()
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MaterialInspectionError {
    #[error("material program '{0}' was not found")]
    ProgramNotFound(MaterialProgramId),
    #[error("material instance '{0}' was not found")]
    InstanceNotFound(MaterialId),
}

#[derive(Debug, Default)]
pub struct MaterialInspector;

impl MaterialInspector {
    /// Inspects a material without mutating the authoring document.
    ///
    /// Invalid targets still return their authored snapshot and structured diagnostics. Compiler
    /// projections that would be misleading for that invalid state are omitted.
    pub fn inspect(
        document: &MaterialAuthoringDocument,
        target: MaterialInspectionTarget,
    ) -> Result<MaterialInspectionReport, MaterialInspectionError> {
        let (program_index, instance_index) = match target {
            MaterialInspectionTarget::Program(program) => {
                (find_program_index(document, program)?, None)
            }
            MaterialInspectionTarget::Instance(instance) => {
                let instance_index = document
                    .effect
                    .material_instances
                    .iter()
                    .position(|candidate| candidate.id == instance)
                    .ok_or(MaterialInspectionError::InstanceNotFound(instance))?;
                let program = document.effect.material_instances[instance_index]
                    .program
                    .id();
                (find_program_index(document, program)?, Some(instance_index))
            }
        };
        let program = &document.programs[program_index];
        let instance = instance_index.map(|index| &document.effect.material_instances[index]);
        let diagnostics = target_diagnostics(document, program_index, instance_index);
        let compiler = MaterialCompiler;
        let stack = compiler.project_stack(program).ok();
        let operations = stack.as_ref().map_or_else(Vec::new, |projection| {
            operation_availability(&compiler, program, projection)
        });
        let presets = stack.as_ref().map_or_else(Vec::new, |projection| {
            preset_availability(&compiler, program, projection)
        });
        let controls = diagnostics
            .is_valid()
            .then(|| compiler.reflect_controls(program, instance).ok())
            .flatten();

        Ok(MaterialInspectionReport {
            target,
            program: program.clone(),
            instance: instance.cloned(),
            controls,
            stack,
            operations,
            presets,
            diagnostics,
        })
    }
}

fn find_program_index(
    document: &MaterialAuthoringDocument,
    program: MaterialProgramId,
) -> Result<usize, MaterialInspectionError> {
    document
        .programs
        .iter()
        .position(|candidate| candidate.id == program)
        .ok_or(MaterialInspectionError::ProgramNotFound(program))
}

fn target_diagnostics(
    document: &MaterialAuthoringDocument,
    program_index: usize,
    instance_index: Option<usize>,
) -> ValidationReport {
    let program_prefix = format!("material_document.programs[{program_index}]");
    let instance_prefix = instance_index.map(|index| format!("effect.material_instances[{index}]"));
    let diagnostics = document
        .validation_report()
        .diagnostics
        .into_iter()
        .filter(|diagnostic| {
            diagnostic.path.starts_with(&program_prefix)
                || instance_prefix
                    .as_ref()
                    .is_some_and(|prefix| diagnostic.path.starts_with(prefix))
        })
        .collect();
    ValidationReport { diagnostics }
}

fn operation_availability(
    compiler: &MaterialCompiler,
    program: &MaterialProgram,
    projection: &MaterialStackProjection,
) -> Vec<MaterialOperationAvailability> {
    let MaterialStackProjection::Stack { entries } = projection else {
        return Vec::new();
    };
    compiler
        .stack_insert_targets(program)
        .unwrap_or_default()
        .into_iter()
        .map(|target| MaterialOperationAvailability {
            kind: target.kind,
            placement: stable_placement(entries, target.index),
        })
        .collect()
}

fn preset_availability(
    compiler: &MaterialCompiler,
    program: &MaterialProgram,
    projection: &MaterialStackProjection,
) -> Vec<MaterialPresetAvailability> {
    let MaterialStackProjection::Stack { entries } = projection else {
        return Vec::new();
    };
    compiler
        .stack_preset_targets(program)
        .unwrap_or_default()
        .into_iter()
        .map(|target| MaterialPresetAvailability {
            preset: target.preset,
            placement: stable_placement(entries, target.index),
        })
        .collect()
}

fn stable_placement(
    entries: &[aestra_compiler::MaterialStackEntry],
    index: usize,
) -> MaterialInsertionPoint {
    if index == entries.len() {
        MaterialInsertionPoint::End
    } else if index == 0 {
        MaterialInsertionPoint::Start
    } else {
        MaterialInsertionPoint::Before(entries[index].expression)
    }
}
