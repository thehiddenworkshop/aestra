use aestra_core::EffectAsset;
use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tempfile::NamedTempFile;

use crate::settings::config_dir;

const RECOVERY_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoverySnapshot {
    version: u32,
    saved_at_unix_millis: u64,
    source_path: Option<PathBuf>,
    effect: EffectAsset,
}

#[derive(Debug)]
pub(crate) struct RecoveryCandidate {
    path: PathBuf,
    snapshot: RecoverySnapshot,
    modified: SystemTime,
}

impl RecoveryCandidate {
    pub(crate) fn effect(&self) -> &EffectAsset {
        &self.snapshot.effect
    }

    pub(crate) fn source_path(&self) -> Option<&Path> {
        self.snapshot.source_path.as_deref()
    }
}

#[derive(Resource, Debug)]
pub(crate) struct RecoveryPersistence {
    directory: PathBuf,
    active_path: Option<PathBuf>,
}

impl RecoveryPersistence {
    pub(crate) fn discover() -> (Self, Option<RecoveryCandidate>, Option<String>) {
        Self::discover_in(config_dir().join("recovery"))
    }

    fn discover_in(directory: PathBuf) -> (Self, Option<RecoveryCandidate>, Option<String>) {
        let mut candidates = Vec::new();
        let mut rejected = 0usize;
        match fs::read_dir(&directory) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !is_recovery_path(&path) {
                        continue;
                    }
                    match load_candidate(&path) {
                        Ok(candidate) if candidate_is_newer_than_source(&candidate) => {
                            candidates.push(candidate);
                        }
                        Ok(_) => {
                            let _ = fs::remove_file(path);
                        }
                        Err(_) => rejected += 1,
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return (
                    Self {
                        directory,
                        active_path: None,
                    },
                    None,
                    Some(format!("Recovery directory could not be read: {error}")),
                );
            }
        }
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.modified));
        let diagnostic = (rejected > 0).then(|| {
            format!(
                "Ignored {rejected} malformed or unsupported recovery {}",
                if rejected == 1 { "file" } else { "files" }
            )
        });
        (
            Self {
                directory,
                active_path: None,
            },
            candidates.into_iter().next(),
            diagnostic,
        )
    }

    pub(crate) fn activate(&mut self, candidate: &RecoveryCandidate) {
        self.active_path = Some(candidate.path.clone());
    }

    pub(crate) fn persist(
        &mut self,
        effect: &EffectAsset,
        source_path: Option<&Path>,
    ) -> io::Result<PathBuf> {
        let snapshot = RecoverySnapshot {
            version: RECOVERY_FORMAT_VERSION,
            saved_at_unix_millis: unix_millis(SystemTime::now()),
            source_path: source_path.map(Path::to_owned),
            effect: effect.clone(),
        };
        let source = ron::ser::to_string_pretty(&snapshot, ron::ser::PrettyConfig::default())
            .map_err(io::Error::other)?;
        let path = self.path_for(effect);
        atomic_write(&path, source.as_bytes())?;
        self.active_path = Some(path.clone());
        Ok(path)
    }

    pub(crate) fn discard_candidate(&mut self, candidate: &RecoveryCandidate) -> io::Result<()> {
        self.active_path = Some(candidate.path.clone());
        self.clear_active()
    }

    pub(crate) fn clear_active(&mut self) -> io::Result<()> {
        self.clear_active_with(remove_if_present)
    }

    pub(crate) fn has_active(&self) -> bool {
        self.active_path.is_some()
    }

    fn clear_active_with(
        &mut self,
        remove: impl FnOnce(&Path) -> io::Result<()>,
    ) -> io::Result<()> {
        let Some(path) = self.active_path.as_deref() else {
            return Ok(());
        };
        remove(path)?;
        self.active_path = None;
        Ok(())
    }

    fn path_for(&self, effect: &EffectAsset) -> PathBuf {
        self.directory.join(format!("{}.recovery.ron", effect.id))
    }

    #[cfg(test)]
    pub(crate) fn for_test(directory: PathBuf, active_path: Option<PathBuf>) -> Self {
        Self {
            directory,
            active_path,
        }
    }
}

fn is_recovery_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".recovery.ron"))
}

fn load_candidate(path: &Path) -> io::Result<RecoveryCandidate> {
    let source = fs::read_to_string(path)?;
    let snapshot: RecoverySnapshot = ron::from_str(&source).map_err(io::Error::other)?;
    if snapshot.version != RECOVERY_FORMAT_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported recovery format {}; expected {RECOVERY_FORMAT_VERSION}",
                snapshot.version
            ),
        ));
    }
    if snapshot.effect.format_version != aestra_core::CURRENT_FORMAT_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "unsupported recovered effect format {}; expected {}",
                snapshot.effect.format_version,
                aestra_core::CURRENT_FORMAT_VERSION
            ),
        ));
    }
    let modified = fs::metadata(path)?.modified().unwrap_or(UNIX_EPOCH);
    Ok(RecoveryCandidate {
        path: path.to_owned(),
        snapshot,
        modified,
    })
}

fn candidate_is_newer_than_source(candidate: &RecoveryCandidate) -> bool {
    let Some(source_path) = candidate.source_path() else {
        return true;
    };
    let Ok(source_modified) = fs::metadata(source_path).and_then(|metadata| metadata.modified())
    else {
        return true;
    };
    candidate.modified > source_modified
}

fn unix_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(contents)?;
    temporary.as_file().sync_all()?;
    let persisted = temporary.persist(path).map_err(|error| error.error)?;
    persisted.sync_all()?;
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    fn test_directory(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "aestra-recovery-{name}-{}",
            unix_millis(SystemTime::now())
        ))
    }

    #[test]
    fn recovery_round_trip_is_valid_and_discardable() {
        let directory = test_directory("round-trip");
        let effect = EffectAsset::new("Recovered effect", 2.0);
        let mut persistence = RecoveryPersistence {
            directory: directory.clone(),
            active_path: None,
        };
        let path = persistence.persist(&effect, None).unwrap();

        let (mut discovered, candidate, diagnostic) =
            RecoveryPersistence::discover_in(directory.clone());
        let candidate = candidate.expect("the recovery should be discovered");
        assert!(diagnostic.is_none());
        assert_eq!(candidate.effect(), &effect);
        discovered.activate(&candidate);
        discovered.clear_active().unwrap();
        assert!(!path.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn source_newer_than_recovery_makes_snapshot_stale() {
        let directory = test_directory("stale");
        let source_path = directory.join("effect.aestra.ron");
        let effect = EffectAsset::new("Saved effect", 2.0);
        let mut persistence = RecoveryPersistence {
            directory: directory.clone(),
            active_path: None,
        };
        let recovery_path = persistence.persist(&effect, Some(&source_path)).unwrap();
        thread::sleep(Duration::from_millis(15));
        effect.save_ron(&source_path).unwrap();

        let (_, candidate, diagnostic) = RecoveryPersistence::discover_in(directory.clone());
        assert!(candidate.is_none());
        assert!(diagnostic.is_none());
        assert!(!recovery_path.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn malformed_recovery_is_preserved_and_reported() {
        let directory = test_directory("malformed");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("broken.recovery.ron");
        fs::write(&path, "not valid ron").unwrap();

        let (_, candidate, diagnostic) = RecoveryPersistence::discover_in(directory.clone());
        assert!(candidate.is_none());
        assert!(diagnostic.is_some());
        assert_eq!(fs::read_to_string(&path).unwrap(), "not valid ron");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn failed_cleanup_keeps_the_snapshot_tracked_for_retry() {
        let path = PathBuf::from("active.recovery.ron");
        let mut persistence = RecoveryPersistence {
            directory: PathBuf::new(),
            active_path: Some(path.clone()),
        };

        let error = persistence
            .clear_active_with(|_| Err(io::Error::other("temporary failure")))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(persistence.active_path.as_ref(), Some(&path));

        let mut removed = None;
        persistence
            .clear_active_with(|path| {
                removed = Some(path.to_owned());
                Ok(())
            })
            .unwrap();
        assert_eq!(removed.as_ref(), Some(&path));
        assert!(!persistence.has_active());
    }

    #[test]
    fn failed_startup_discard_becomes_the_active_cleanup_target() {
        let directory = test_directory("discard-retry");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("blocked.recovery.ron");
        fs::create_dir(&path).unwrap();
        let candidate = RecoveryCandidate {
            path: path.clone(),
            snapshot: RecoverySnapshot {
                version: RECOVERY_FORMAT_VERSION,
                saved_at_unix_millis: unix_millis(SystemTime::now()),
                source_path: None,
                effect: EffectAsset::new("Discarded recovery", 1.0),
            },
            modified: SystemTime::now(),
        };
        let mut persistence = RecoveryPersistence {
            directory: directory.clone(),
            active_path: None,
        };

        assert!(persistence.discard_candidate(&candidate).is_err());
        assert_eq!(persistence.active_path.as_ref(), Some(&path));

        fs::remove_dir(&path).unwrap();
        fs::write(&path, "pending snapshot").unwrap();
        persistence.clear_active().unwrap();
        assert!(!path.exists());
        assert!(!persistence.has_active());
        fs::remove_dir_all(directory).unwrap();
    }
}
