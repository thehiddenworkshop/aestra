//! UI-independent semantic editing for Aestra effect assets.

mod command;
mod diff;
mod executor;
mod history;
mod selection;

pub use command::{EffectCommand, EffectTransaction};
pub use diff::{ChangeKind, EffectDiff, SemanticChange};
pub use executor::{CommandError, CommandExecutor, TransactionOutcome, TransactionPreview};
pub use history::{CommandHistory, HistoryResult};
pub use selection::{LockState, Selection, SemanticTarget};
