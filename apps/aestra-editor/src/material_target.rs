//! Project-scoped material editing identity, shared by host routing and qualification.
use aestra_core::MaterialProgramId;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) enum MaterialEditingTarget {
    #[default]
    EffectInstance,
    Program {
        root: PathBuf,
        id: MaterialProgramId,
    },
    Function {
        root: PathBuf,
        id: aestra_core::MaterialFunctionId,
    },
}

impl MaterialEditingTarget {
    /// The shared material program this target edits, if it is a program target.
    pub(crate) fn program(&self) -> Option<MaterialProgramId> {
        match self {
            Self::Program { id, .. } => Some(*id),
            _ => None,
        }
    }

    /// The material function this target edits, if it is a function target.
    pub(crate) fn function(&self) -> Option<aestra_core::MaterialFunctionId> {
        match self {
            Self::Function { id, .. } => Some(*id),
            _ => None,
        }
    }
}
