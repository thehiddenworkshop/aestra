//! UI-independent semantic editing for Aestra effect assets.

mod command;
mod diff;
mod executor;
mod history;
mod material_authoring;
mod material_migration;
mod material_tools;
mod selection;

pub use command::{EffectCommand, EffectTransaction};
pub use diff::{ChangeKind, EffectDiff, SemanticChange};
pub use executor::{CommandError, CommandExecutor, TransactionOutcome, TransactionPreview};
pub use history::{CommandHistory, HistoryResult};
pub use material_authoring::{
    MaterialAuthoringDocument, MaterialChangeKind, MaterialCommand, MaterialCommandError,
    MaterialCommandExecutor, MaterialCommandHistory, MaterialDiff, MaterialExpressionInput,
    MaterialHistoryResult, MaterialOutputSocket, MaterialSemanticChange, MaterialSemanticTarget,
    MaterialTransaction, MaterialTransactionOutcome,
};
pub use material_migration::{
    LegacyMaterialMigrationError, LegacyMaterialMigrationMapping, LegacyMaterialMigrationPlan,
    migrate_legacy_sprite_materials, plan_legacy_sprite_material_migration,
};
pub use material_tools::{
    MaterialConnectionTarget, MaterialInsertionPoint, MaterialParameterBinding,
    MaterialToolCommand, MaterialToolError, MaterialToolPlan, MaterialToolPlanner,
};
pub use selection::{LockState, Selection, SemanticTarget};
