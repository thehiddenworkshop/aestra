use serde::{Deserialize, Serialize};
use std::{error::Error, fmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticCode {
    UnsupportedFormat,
    NilId,
    DuplicateId,
    InvalidDuration,
    InvalidTiming,
    InvalidCapacity,
    MissingModule,
    DuplicateModule,
    StageMismatch,
    InvalidValue,
    MissingRenderer,
    InvalidReference,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub code: DiagnosticCode,
    pub path: String,
    pub message: String,
}

impl Diagnostic {
    pub fn error(
        code: DiagnosticCode,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity: DiagnosticSeverity::Error,
            code,
            path: path.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub diagnostics: Vec<Diagnostic>,
}

impl ValidationReport {
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub fn is_valid(&self) -> bool {
        !self
            .diagnostics
            .iter()
            .any(|item| item.severity == DiagnosticSeverity::Error)
    }

    pub fn into_result(self) -> Result<(), Self> {
        if self.is_valid() { Ok(()) } else { Err(self) }
    }
}

impl fmt::Display for ValidationReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.diagnostics.first() {
            Some(first) if self.diagnostics.len() > 1 => write!(
                formatter,
                "{} (and {} more diagnostics)",
                first.message,
                self.diagnostics.len() - 1
            ),
            Some(first) => formatter.write_str(&first.message),
            None => formatter.write_str("effect validation failed"),
        }
    }
}

impl Error for ValidationReport {}
