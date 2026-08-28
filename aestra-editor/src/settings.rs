use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
use tempfile::{Builder as TempFileBuilder, NamedTempFile};

pub(crate) const SETTINGS_FORMAT_VERSION: u32 = 4;

#[derive(Resource, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct EditorSettings {
    pub(crate) version: u32,
    pub(crate) general: GeneralSettings,
    pub(crate) preview: PreviewSettings,
    pub(crate) performance: PerformanceSettings,
    pub(crate) capture: CaptureSettings,
    pub(crate) appearance: AppearanceSettings,
    pub(crate) inspector: InspectorSettings,
    pub(crate) language: LanguageSettings,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            version: SETTINGS_FORMAT_VERSION,
            general: GeneralSettings::default(),
            preview: PreviewSettings::default(),
            performance: PerformanceSettings::default(),
            capture: CaptureSettings::default(),
            appearance: AppearanceSettings::default(),
            inspector: InspectorSettings::default(),
            language: LanguageSettings::default(),
        }
    }
}

impl EditorSettings {
    pub(crate) fn normalized(mut self) -> Self {
        self.version = SETTINGS_FORMAT_VERSION;
        self.performance.preview_particle_limit =
            self.performance.preview_particle_limit.clamp(64, 384);
        self.general.autosave_interval_seconds =
            self.general.autosave_interval_seconds.clamp(5, 600);
        self.capture.frame_rate = self.capture.frame_rate.clamp(1, 240);
        self.capture.contact_sheet_columns = self.capture.contact_sheet_columns.clamp(1, 16);
        self.appearance.ui_scale = self.appearance.ui_scale.clamp(0.75, 1.50);
        self.language.locale = self.language.locale.trim().to_string();
        if self.language.locale.is_empty() {
            self.language.locale = "en-US".into();
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct GeneralSettings {
    pub(crate) confirm_unsaved_changes: bool,
    pub(crate) autosave_enabled: bool,
    pub(crate) autosave_interval_seconds: u16,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            confirm_unsaved_changes: true,
            autosave_enabled: true,
            autosave_interval_seconds: 30,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct PreviewSettings {
    pub(crate) show_grid: bool,
    pub(crate) play_on_open: bool,
}

impl Default for PreviewSettings {
    fn default() -> Self {
        Self {
            show_grid: true,
            play_on_open: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct PerformanceSettings {
    pub(crate) preview_particle_limit: usize,
}

impl Default for PerformanceSettings {
    fn default() -> Self {
        Self {
            preview_particle_limit: 384,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct CaptureSettings {
    pub(crate) frame_rate: u16,
    pub(crate) contact_sheet_columns: u8,
}

impl Default for CaptureSettings {
    fn default() -> Self {
        Self {
            frame_rate: 60,
            contact_sheet_columns: 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AppearanceSettings {
    pub(crate) ui_scale: f32,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self { ui_scale: 1.0 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct InspectorSettings {
    /// User expansion choices keyed by stable module or renderer type.
    pub(crate) section_expansion: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct LanguageSettings {
    pub(crate) locale: String,
}

impl Default for LanguageSettings {
    fn default() -> Self {
        Self {
            locale: "en-US".into(),
        }
    }
}

#[derive(Resource, Debug)]
pub(crate) struct SettingsPersistence {
    path: PathBuf,
    writable: bool,
    diagnostic: Option<String>,
}

impl SettingsPersistence {
    #[cfg(test)]
    pub(crate) fn for_test(path: PathBuf) -> Self {
        Self {
            path,
            writable: true,
            diagnostic: None,
        }
    }

    pub(crate) fn load() -> (EditorSettings, Self) {
        Self::load_from(settings_path())
    }

    fn load_from(path: PathBuf) -> (EditorSettings, Self) {
        let mut lifecycle_diagnostic = match recover_interrupted_replacement(&path) {
            Ok(diagnostic) => diagnostic,
            Err(error) => {
                return (
                    EditorSettings::default(),
                    Self {
                        diagnostic: Some(format!(
                            "Interrupted settings replacement could not be recovered: {error}"
                        )),
                        path,
                        writable: false,
                    },
                );
            }
        };
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return (
                    EditorSettings::default(),
                    Self {
                        path,
                        writable: true,
                        diagnostic: None,
                    },
                );
            }
            Err(error) => {
                return (
                    EditorSettings::default(),
                    Self {
                        diagnostic: Some(format!("Settings could not be read: {error}")),
                        path,
                        writable: false,
                    },
                );
            }
        };
        match ron::from_str::<EditorSettings>(&source) {
            Ok(settings) if settings.version <= SETTINGS_FORMAT_VERSION => {
                let migrated = settings.version < SETTINGS_FORMAT_VERSION;
                if let Err(error) = cleanup_replacement_artifacts(&path) {
                    lifecycle_diagnostic = join_diagnostics(
                        lifecycle_diagnostic,
                        Some(format!(
                            "Obsolete settings replacement files could not be removed: {error}"
                        )),
                    );
                }
                (
                    settings.normalized(),
                    Self {
                        path,
                        writable: true,
                        diagnostic: join_diagnostics(
                            lifecycle_diagnostic,
                            migrated.then(|| {
                                format!(
                                "Settings will migrate to format {SETTINGS_FORMAT_VERSION} when changed"
                            )
                            }),
                        ),
                    },
                )
            }
            Ok(settings) => (
                EditorSettings::default(),
                Self {
                    diagnostic: Some(format!(
                        "Settings format {} is newer than supported format {}; using defaults without overwriting it",
                        settings.version, SETTINGS_FORMAT_VERSION
                    )),
                    path,
                    writable: false,
                },
            ),
            Err(error) => (
                EditorSettings::default(),
                Self {
                    diagnostic: Some(format!(
                        "Settings are malformed; using defaults without overwriting them: {error}"
                    )),
                    path,
                    writable: false,
                },
            ),
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn diagnostic(&self) -> Option<&str> {
        self.diagnostic.as_deref()
    }

    pub(crate) fn persist(&mut self, settings: &EditorSettings) -> io::Result<()> {
        if !self.writable {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the existing settings file must be reset explicitly before it can be replaced",
            ));
        }
        write_settings(&self.path, settings)?;
        self.diagnostic = None;
        Ok(())
    }

    pub(crate) fn replace_with_defaults(&mut self) -> io::Result<EditorSettings> {
        if !self.writable && self.path.exists() {
            preserve_existing_file(&self.path)?;
        }
        let settings = EditorSettings::default();
        write_settings(&self.path, &settings)?;
        self.writable = true;
        self.diagnostic = None;
        Ok(settings)
    }
}

fn settings_path() -> PathBuf {
    config_dir().join("settings.ron")
}

pub(crate) fn config_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("AESTRA_CONFIG_DIR") {
        return PathBuf::from(path);
    }
    #[cfg(target_os = "windows")]
    if let Some(path) = std::env::var_os("APPDATA") {
        return PathBuf::from(path).join("Aestra");
    }
    #[cfg(not(target_os = "windows"))]
    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(path).join("aestra");
    }
    PathBuf::from(".aestra")
}

fn write_settings(path: &Path, settings: &EditorSettings) -> io::Result<()> {
    let temporary = stage_settings(path, settings)?;
    let persisted = temporary.persist(path).map_err(|error| error.error)?;
    persisted.sync_all()?;
    sync_parent_directory(path)?;
    Ok(())
}

fn stage_settings(path: &Path, settings: &EditorSettings) -> io::Result<NamedTempFile> {
    let parent = settings_parent(path);
    fs::create_dir_all(parent)?;
    let source = ron::ser::to_string_pretty(
        &settings.clone().normalized(),
        ron::ser::PrettyConfig::default(),
    )
    .map_err(io::Error::other)?;
    let mut temporary = TempFileBuilder::new()
        .prefix(&settings_temporary_prefix(path))
        .tempfile_in(parent)?;
    temporary.write_all(source.as_bytes())?;
    temporary.as_file().sync_all()?;
    Ok(temporary)
}

fn recover_interrupted_replacement(path: &Path) -> io::Result<Option<String>> {
    if path.exists() {
        return Ok(None);
    }
    let backup = sibling_path(path, "previous");
    if backup.exists() {
        fs::rename(&backup, path)?;
        sync_parent_directory(path)?;
        return Ok(Some(
            "Recovered settings from an interrupted replacement".into(),
        ));
    }

    let Some(staged) = newest_staged_settings(path)? else {
        return Ok(None);
    };
    let source = fs::read_to_string(&staged)?;
    let settings: EditorSettings = ron::from_str(&source).map_err(io::Error::other)?;
    if settings.version > SETTINGS_FORMAT_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "staged settings format {} is newer than supported format {SETTINGS_FORMAT_VERSION}",
                settings.version
            ),
        ));
    }
    write_settings(path, &settings)?;
    remove_if_present(&staged)?;
    Ok(Some(
        "Recovered settings from an interrupted initial write".into(),
    ))
}

fn newest_staged_settings(path: &Path) -> io::Result<Option<PathBuf>> {
    let parent = settings_parent(path);
    let mut candidates = Vec::new();
    let legacy = sibling_path(path, "tmp");
    if legacy.exists() {
        candidates.push(legacy);
    }
    match fs::read_dir(parent) {
        Ok(entries) => {
            let prefix = settings_temporary_prefix(path);
            for entry in entries.flatten() {
                let candidate = entry.path();
                if candidate
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&prefix))
                {
                    candidates.push(candidate);
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    candidates.sort_by_key(|candidate| {
        std::cmp::Reverse(
            fs::metadata(candidate)
                .and_then(|metadata| metadata.modified())
                .ok(),
        )
    });
    Ok(candidates.into_iter().next())
}

fn cleanup_replacement_artifacts(path: &Path) -> io::Result<()> {
    remove_if_present(&sibling_path(path, "previous"))?;
    remove_if_present(&sibling_path(path, "tmp"))?;
    let parent = settings_parent(path);
    let prefix = settings_temporary_prefix(path);
    match fs::read_dir(parent) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let candidate = entry.path();
                if candidate
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&prefix))
                {
                    remove_if_present(&candidate)?;
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(())
}

fn settings_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn settings_temporary_prefix(path: &Path) -> String {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("settings.ron");
    format!(".{name}.tmp-")
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn sync_parent_directory(_path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    fs::File::open(settings_parent(_path))?.sync_all()?;
    Ok(())
}

fn join_diagnostics(first: Option<String>, second: Option<String>) -> Option<String> {
    match (first, second) {
        (Some(first), Some(second)) => Some(format!("{first}. {second}")),
        (Some(diagnostic), None) | (None, Some(diagnostic)) => Some(diagnostic),
        (None, None) => None,
    }
}

fn preserve_existing_file(path: &Path) -> io::Result<PathBuf> {
    let mut index = 0;
    loop {
        let suffix = if index == 0 {
            "backup".to_string()
        } else {
            format!("backup-{index}")
        };
        let backup = sibling_path(path, &suffix);
        if !backup.exists() {
            fs::copy(path, &backup)?;
            return Ok(backup);
        }
        index += 1;
    }
}

fn sibling_path(path: &Path, suffix: &str) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("settings.ron");
    path.with_file_name(format!("{name}.{suffix}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!("aestra-settings-{name}-{unique}"))
            .join("settings.ron")
    }

    #[test]
    fn grid_is_enabled_by_default() {
        assert!(EditorSettings::default().preview.show_grid);
        assert!(PreviewSettings::default().show_grid);
        assert!(EditorSettings::default().preview.play_on_open);
        assert!(EditorSettings::default().general.autosave_enabled);
        assert!(PreviewSettings::default().play_on_open);
        assert_eq!(
            EditorSettings::default().general.autosave_interval_seconds,
            30
        );
    }

    #[test]
    fn missing_grid_preference_defaults_to_enabled() {
        let path = test_path("grid-default");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            format!("(version: {SETTINGS_FORMAT_VERSION}, preview: (play_on_open: true))"),
        )
        .unwrap();

        let (settings, state) = SettingsPersistence::load_from(path.clone());

        assert!(settings.preview.show_grid);
        assert!(state.diagnostic().is_none());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn settings_round_trip_and_normalize() {
        let path = test_path("round-trip");
        let mut settings = EditorSettings::default();
        settings.preview.show_grid = false;
        settings.general.autosave_enabled = false;
        settings.general.autosave_interval_seconds = 1;
        settings.appearance.ui_scale = 8.0;
        settings
            .inspector
            .section_expansion
            .insert("module/aestra.update.motion".into(), true);
        settings.language.locale = "fr-FR".into();
        let mut persistence = SettingsPersistence {
            path: path.clone(),
            writable: true,
            diagnostic: None,
        };
        persistence.persist(&settings).unwrap();

        let (loaded, state) = SettingsPersistence::load_from(path.clone());
        assert!(!loaded.preview.show_grid);
        assert!(!loaded.general.autosave_enabled);
        assert_eq!(loaded.general.autosave_interval_seconds, 5);
        assert_eq!(loaded.appearance.ui_scale, 1.5);
        assert_eq!(
            loaded
                .inspector
                .section_expansion
                .get("module/aestra.update.motion"),
            Some(&true)
        );
        assert_eq!(loaded.language.locale, "fr-FR");
        assert!(state.diagnostic().is_none());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn malformed_settings_are_preserved_until_explicit_reset() {
        let path = test_path("malformed");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "this is not ron").unwrap();
        let (settings, mut state) = SettingsPersistence::load_from(path.clone());

        assert_eq!(settings, EditorSettings::default());
        assert!(state.diagnostic().is_some());
        assert!(state.persist(&settings).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "this is not ron");

        state.replace_with_defaults().unwrap();
        assert!(sibling_path(&path, "backup").exists());
        assert!(ron::from_str::<EditorSettings>(&fs::read_to_string(&path).unwrap()).is_ok());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn newer_settings_are_loaded_read_only() {
        let path = test_path("future");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let source = format!("(version: {},)", SETTINGS_FORMAT_VERSION + 1);
        fs::write(&path, &source).unwrap();
        let (settings, mut state) = SettingsPersistence::load_from(path.clone());

        assert_eq!(settings, EditorSettings::default());
        assert!(state.persist(&settings).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), source);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn unknown_fields_leave_the_file_read_only_and_untouched() {
        let path = test_path("unknown-fields");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"(
                version: 1,
                preview: (show_grid: true, experimental_preview: 42),
                plugin_setting: "preserve me",
            )"#,
        )
        .unwrap();
        let original = fs::read_to_string(&path).unwrap();
        let (settings, mut state) = SettingsPersistence::load_from(path.clone());

        assert!(state.diagnostic().is_some());
        assert!(state.persist(&settings).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn staged_settings_use_unique_files_without_moving_the_canonical_file() {
        let path = test_path("unique-staging");
        let original = EditorSettings::default();
        write_settings(&path, &original).unwrap();
        let original_source = fs::read_to_string(&path).unwrap();
        let first = stage_settings(&path, &original).unwrap();
        let second = stage_settings(&path, &original).unwrap();

        assert_ne!(first.path(), second.path());
        assert!(path.exists());
        assert_eq!(fs::read_to_string(&path).unwrap(), original_source);

        drop(first);
        drop(second);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn interrupted_legacy_replacement_restores_the_canonical_settings() {
        let path = test_path("interrupted-replacement");
        let original = EditorSettings::default();
        write_settings(&path, &original).unwrap();
        let backup = sibling_path(&path, "previous");
        let staged = sibling_path(&path, "tmp");
        let mut replacement = original.clone();
        replacement.language.locale = "fr-FR".into();
        fs::write(
            &staged,
            ron::ser::to_string_pretty(&replacement, ron::ser::PrettyConfig::default()).unwrap(),
        )
        .unwrap();
        fs::rename(&path, &backup).unwrap();

        let (loaded, persistence) = SettingsPersistence::load_from(path.clone());

        assert_eq!(loaded, original);
        assert!(path.exists());
        assert!(!backup.exists());
        assert!(!staged.exists());
        assert!(
            persistence
                .diagnostic()
                .is_some_and(|diagnostic| diagnostic.contains("Recovered settings"))
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn interrupted_initial_write_promotes_a_valid_staged_file() {
        let path = test_path("interrupted-initial-write");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let staged = sibling_path(&path, "tmp");
        let settings = EditorSettings {
            language: LanguageSettings {
                locale: "fr-FR".into(),
            },
            ..Default::default()
        };
        fs::write(
            &staged,
            ron::ser::to_string_pretty(&settings, ron::ser::PrettyConfig::default()).unwrap(),
        )
        .unwrap();

        let (loaded, persistence) = SettingsPersistence::load_from(path.clone());

        assert_eq!(loaded, settings);
        assert!(path.exists());
        assert!(!staged.exists());
        assert!(
            persistence
                .diagnostic()
                .is_some_and(|diagnostic| diagnostic.contains("interrupted initial write"))
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn repeated_settings_replacements_leave_only_the_latest_canonical_file() {
        let path = test_path("repeated-replacement");
        let mut persistence = SettingsPersistence {
            path: path.clone(),
            writable: true,
            diagnostic: None,
        };
        let mut expected = EditorSettings::default();

        for index in 0..8 {
            expected.general.autosave_interval_seconds = 30 + index;
            persistence.persist(&expected).unwrap();
            assert!(path.exists());
            assert!(!sibling_path(&path, "previous").exists());
            assert!(!sibling_path(&path, "tmp").exists());
            assert!(newest_staged_settings(&path).unwrap().is_none());
        }

        let (loaded, state) = SettingsPersistence::load_from(path.clone());
        assert_eq!(loaded, expected);
        assert!(state.diagnostic().is_none());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
