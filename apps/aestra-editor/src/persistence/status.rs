//! Production settings persistence and localized document/status feedback.
use crate::{
    EditorSession,
    localization::Localizer,
    settings::{EditorSettings, SettingsPersistence},
};
use fluent_bundle::FluentArgs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PersistenceStatus {
    CreatedUntitled,
    Opened(String),
    OpenCancelled,
    OpenFailed(String),
    MigrationCancelled,
    Migrated {
        path: String,
        backup: String,
        from: u32,
        to: u32,
    },
    Saved(String),
    SaveCancelled,
    SaveFailed(String),
    RecoveryRestored(String),
    RecoveryDiscarded,
    RecoveryDiscardFailed(String),
    RecoveryAutosaveFailed(String),
    RecoveryDiagnostic(String),
    SettingsSaved,
    SettingsSaveFailed(String),
    SettingsDiagnostic(String),
}

pub(super) fn set_persistence_status(
    session: &mut EditorSession,
    localizer: &Localizer,
    status: PersistenceStatus,
) {
    session.status = localize_persistence_status(status, localizer);
}

pub(super) fn localize_persistence_status(
    status: PersistenceStatus,
    localizer: &Localizer,
) -> String {
    if let PersistenceStatus::Migrated {
        path,
        backup,
        from,
        to,
    } = status
    {
        let mut args = FluentArgs::new();
        args.set("path", path);
        args.set("backup", backup);
        args.set("from", i64::from(from));
        args.set("to", i64::from(to));
        return localizer.text_with("persistence-status-migrated", &args);
    }
    let (message_id, argument) = match status {
        PersistenceStatus::CreatedUntitled => {
            return localizer.text("persistence-status-created-untitled");
        }
        PersistenceStatus::Opened(path) => ("persistence-status-opened", ("path", path)),
        PersistenceStatus::OpenCancelled => {
            return localizer.text("persistence-status-open-cancelled");
        }
        PersistenceStatus::OpenFailed(error) => {
            ("persistence-status-open-failed", ("error", error))
        }
        PersistenceStatus::MigrationCancelled => {
            return localizer.text("persistence-status-migration-cancelled");
        }
        PersistenceStatus::Saved(path) => ("persistence-status-saved", ("path", path)),
        PersistenceStatus::SaveCancelled => {
            return localizer.text("persistence-status-save-cancelled");
        }
        PersistenceStatus::SaveFailed(error) => {
            ("persistence-status-save-failed", ("error", error))
        }
        PersistenceStatus::RecoveryRestored(effect) => {
            ("persistence-status-recovery-restored", ("effect", effect))
        }
        PersistenceStatus::RecoveryDiscarded => {
            return localizer.text("persistence-status-recovery-discarded");
        }
        PersistenceStatus::RecoveryDiscardFailed(error) => (
            "persistence-status-recovery-discard-failed",
            ("error", error),
        ),
        PersistenceStatus::RecoveryAutosaveFailed(error) => (
            "persistence-status-recovery-autosave-failed",
            ("error", error),
        ),
        PersistenceStatus::RecoveryDiagnostic(detail) => {
            ("persistence-status-recovery-diagnostic", ("detail", detail))
        }
        PersistenceStatus::SettingsSaved => {
            return localizer.text("persistence-status-settings-saved");
        }
        PersistenceStatus::SettingsSaveFailed(error) => {
            ("persistence-status-settings-save-failed", ("error", error))
        }
        PersistenceStatus::SettingsDiagnostic(detail) => {
            ("persistence-status-settings-diagnostic", ("detail", detail))
        }
        PersistenceStatus::Migrated { .. } => unreachable!("handled above"),
    };
    let mut args = FluentArgs::new();
    args.set(argument.0, argument.1);
    localizer.text_with(message_id, &args)
}

pub(crate) fn persist_editor_settings(
    settings: &EditorSettings,
    persistence: &mut SettingsPersistence,
    session: &mut EditorSession,
    localizer: &Localizer,
) {
    match persistence.persist(settings) {
        Ok(()) => set_persistence_status(session, localizer, PersistenceStatus::SettingsSaved),
        Err(error) => set_persistence_status(
            session,
            localizer,
            PersistenceStatus::SettingsSaveFailed(error.to_string()),
        ),
    }
}
