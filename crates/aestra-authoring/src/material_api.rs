//! Unified, serializable request/response facade for semantic material tooling.

use crate::{
    MaterialAuthoringDocument, MaterialCommandError, MaterialCompilationReport,
    MaterialCompilationReporter, MaterialInspectionError, MaterialInspectionReport,
    MaterialInspectionTarget, MaterialInspector, MaterialToolCommand, MaterialToolError,
    MaterialToolPlan, MaterialToolPlanner,
};
use aestra_compiler::MaterialStackEditError;
use aestra_core::ValidationReport;
use serde::{Deserialize, Serialize};

/// One machine-readable request at the non-mutating material tool boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub enum MaterialApiRequest {
    Inspect { target: MaterialInspectionTarget },
    PlanEdit { command: MaterialToolCommand },
    Compile { target: MaterialInspectionTarget },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialApiErrorCode {
    NotFound,
    NotExposed,
    InvalidRequest,
    IncompatibleEdit,
    AmbiguousEdit,
    ValidationFailed,
}

/// Stable failure payload suitable for transport across process and tool boundaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialApiError {
    pub code: MaterialApiErrorCode,
    pub message: String,
    pub diagnostics: ValidationReport,
}

/// One machine-readable response. Errors are values so they serialize like successful results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload")]
pub enum MaterialApiResponse {
    Inspection(Box<MaterialInspectionReport>),
    EditPlan(Box<MaterialToolPlan>),
    Compilation(Box<MaterialCompilationReport>),
    Error(MaterialApiError),
}

#[derive(Debug, Default)]
pub struct MaterialApi;

impl MaterialApi {
    /// Handles a request without mutating the supplied authoring document.
    pub fn handle(
        document: &MaterialAuthoringDocument,
        request: MaterialApiRequest,
    ) -> MaterialApiResponse {
        match request {
            MaterialApiRequest::Inspect { target } => MaterialInspector::inspect(document, target)
                .map_or_else(
                    |error| MaterialApiResponse::Error(inspection_error(error)),
                    |report| MaterialApiResponse::Inspection(Box::new(report)),
                ),
            MaterialApiRequest::PlanEdit { command } => {
                MaterialToolPlanner::plan(document, command).map_or_else(
                    |error| MaterialApiResponse::Error(tool_error(error)),
                    |plan| MaterialApiResponse::EditPlan(Box::new(plan)),
                )
            }
            MaterialApiRequest::Compile { target } => {
                MaterialCompilationReporter::compile(document, target).map_or_else(
                    |error| MaterialApiResponse::Error(inspection_error(error)),
                    |report| MaterialApiResponse::Compilation(Box::new(report)),
                )
            }
        }
    }
}

fn inspection_error(error: MaterialInspectionError) -> MaterialApiError {
    MaterialApiError {
        code: MaterialApiErrorCode::NotFound,
        message: error.to_string(),
        diagnostics: ValidationReport::default(),
    }
}

fn tool_error(error: MaterialToolError) -> MaterialApiError {
    let message = error.to_string();
    let (code, diagnostics) = match error {
        MaterialToolError::ProgramNotFound(_)
        | MaterialToolError::InstanceNotFound(_)
        | MaterialToolError::ParameterNotFound { .. }
        | MaterialToolError::BindingParameterNotFound(_)
        | MaterialToolError::InsertionAnchorNotFound(_)
        | MaterialToolError::SourceExpressionNotFound(_)
        | MaterialToolError::DestinationExpressionNotFound(_) => {
            (MaterialApiErrorCode::NotFound, ValidationReport::default())
        }
        MaterialToolError::BindingParameterNotExposed(_) => (
            MaterialApiErrorCode::NotExposed,
            ValidationReport::default(),
        ),
        MaterialToolError::InvalidFresnelSettings(_)
        | MaterialToolError::EmptyExpressionSelection => (
            MaterialApiErrorCode::InvalidRequest,
            ValidationReport::default(),
        ),
        MaterialToolError::IncompatibleWrap { .. }
        | MaterialToolError::IncompatibleSource { .. }
        | MaterialToolError::ExpressionCannotBeDeleted(_)
        | MaterialToolError::ConnectionCannotBeDisconnected(_) => (
            MaterialApiErrorCode::IncompatibleEdit,
            ValidationReport::default(),
        ),
        MaterialToolError::AmbiguousWrap { .. } => (
            MaterialApiErrorCode::AmbiguousEdit,
            ValidationReport::default(),
        ),
        MaterialToolError::StackEdit(MaterialStackEditError::Compile(error)) => (
            MaterialApiErrorCode::ValidationFailed,
            error.report().clone(),
        ),
        MaterialToolError::StackEdit(_) => (
            MaterialApiErrorCode::IncompatibleEdit,
            ValidationReport::default(),
        ),
        MaterialToolError::Transaction(error) => command_error(error),
    };
    MaterialApiError {
        code,
        message,
        diagnostics,
    }
}

fn command_error(error: MaterialCommandError) -> (MaterialApiErrorCode, ValidationReport) {
    match error {
        MaterialCommandError::Validation(report) => {
            (MaterialApiErrorCode::ValidationFailed, report)
        }
        MaterialCommandError::NotFound { .. } => {
            (MaterialApiErrorCode::NotFound, ValidationReport::default())
        }
        MaterialCommandError::IndexOutOfBounds { .. }
        | MaterialCommandError::IdentityChanged { .. }
        | MaterialCommandError::InvalidExpressionInput { .. } => (
            MaterialApiErrorCode::InvalidRequest,
            ValidationReport::default(),
        ),
    }
}
