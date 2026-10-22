use std::path::{Path, PathBuf};

/// Distribution: `apps/fireworks/fireworks`, `assets/test`, optional `audio`.
/// Standard Cargo builds: `target/{profile}/examples/fireworks`, with source
/// assets at the workspace root and the optional WAV pack inside aestra-bevy.
/// Cargo's runtime manifest variable supports custom target directories. No
/// compile-time checkout path or current working directory is used as a fallback.
pub(super) fn default_paths(
    executable: &Path,
    cargo_manifest: Option<&Path>,
) -> (PathBuf, PathBuf) {
    if let Some(manifest) = cargo_manifest.filter(|path| !path.as_os_str().is_empty()) {
        return (
            manifest.join("../../assets/test"),
            manifest.join("assets/audio/fireworks"),
        );
    }
    let directory = executable.parent().expect("executable has no parent");
    if directory.file_name().is_some_and(|name| name == "examples") {
        let root = directory.join("../../..");
        (
            root.join("assets/test"),
            root.join("bevy/aestra-bevy/assets/audio/fireworks"),
        )
    } else {
        let root = directory.join("../..");
        (root.join("assets/test"), root.join("audio"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_defaults_move_together_and_do_not_require_files_to_exist() {
        for package in ["original/package", "renamed package/new location"] {
            let executable = Path::new(package).join("apps/fireworks/fireworks.exe");
            let (project, audio) = default_paths(&executable, None);
            assert_eq!(
                project,
                Path::new(package).join("apps/fireworks/../../assets/test")
            );
            assert_eq!(audio, Path::new(package).join("apps/fireworks/../../audio"));
        }
    }

    #[test]
    fn cargo_examples_and_hashed_test_binaries_use_source_assets() {
        for (profile, binary) in [
            ("debug", "fireworks.exe"),
            ("release", "fireworks-hash.exe"),
        ] {
            let directory = Path::new("checkout/target").join(profile).join("examples");
            let (project, audio) = default_paths(&directory.join(binary), None);
            assert_eq!(project, directory.join("../../../assets/test"));
            assert_eq!(
                audio,
                directory.join("../../../bevy/aestra-bevy/assets/audio/fireworks")
            );
        }
    }

    #[test]
    fn cargo_runtime_manifest_supports_external_target_directories() {
        let manifest = Path::new("checkout/bevy/aestra-bevy");
        let (project, audio) = default_paths(
            Path::new("external/build/examples/fireworks.exe"),
            Some(manifest),
        );
        assert_eq!(project, manifest.join("../../assets/test"));
        assert_eq!(audio, manifest.join("assets/audio/fireworks"));
        let executable = Path::new("package/apps/fireworks/fireworks.exe");
        assert_eq!(
            default_paths(executable, Some(Path::new(""))),
            default_paths(executable, None)
        );
    }
}
