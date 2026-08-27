use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
};

pub(crate) const SETTINGS_FORMAT_VERSION: u32 = 2;

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
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            confirm_unsaved_changes: true,
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
    pub(crate) fn load() -> (EditorSettings, Self) {
        Self::load_from(settings_path())
    }

    fn load_from(path: PathBuf) -> (EditorSettings, Self) {
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
                (
                    settings.normalized(),
                    Self {
                        path,
                        writable: true,
                        diagnostic: migrated.then(|| {
                            format!(
                                "Settings will migrate to format {SETTINGS_FORMAT_VERSION} when changed"
                            )
                        }),
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
    if let Some(path) = std::env::var_os("AESTRA_CONFIG_DIR") {
        return PathBuf::from(path).join("settings.ron");
    }
    #[cfg(target_os = "windows")]
    if let Some(path) = std::env::var_os("APPDATA") {
        return PathBuf::from(path).join("Aestra").join("settings.ron");
    }
    #[cfg(not(target_os = "windows"))]
    if let Some(path) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(path).join("aestra").join("settings.ron");
    }
    PathBuf::from(".aestra").join("settings.ron")
}

fn write_settings(path: &Path, settings: &EditorSettings) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let source = ron::ser::to_string_pretty(
        &settings.clone().normalized(),
        ron::ser::PrettyConfig::default(),
    )
    .map_err(io::Error::other)?;
    let temporary = sibling_path(path, "tmp");
    let backup = sibling_path(path, "previous");
    let mut file = File::create(&temporary)?;
    file.write_all(source.as_bytes())?;
    file.sync_all()?;

    if !path.exists() {
        return fs::rename(temporary, path);
    }
    if backup.exists() {
        fs::remove_file(&backup)?;
    }
    fs::rename(path, &backup)?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::rename(&backup, path);
        return Err(error);
    }
    let _ = fs::remove_file(backup);
    Ok(())
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
    fn settings_round_trip_and_normalize() {
        let path = test_path("round-trip");
        let mut settings = EditorSettings::default();
        settings.preview.show_grid = false;
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
}
