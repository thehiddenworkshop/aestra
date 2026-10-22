use std::path::{Path, PathBuf};

/// Both `target/{profile}/aestra-editor` and the distributed
/// `apps/aestra-editor/aestra-editor` are two levels below their content root.
/// Cargo unit-test executables live one extra level down, in `deps`.
/// Cargo's runtime manifest variable also supports custom target directories;
/// it is not embedded into the shipped executable.
pub(super) fn project_root(executable: &Path, cargo_manifest: Option<&Path>) -> PathBuf {
    if let Some(manifest) = cargo_manifest.filter(|path| !path.as_os_str().is_empty()) {
        return manifest.join("../../sample-project");
    }
    let directory = executable.parent().expect("executable has no parent");
    let directory = if directory.file_name().is_some_and(|name| name == "deps") {
        directory
            .parent()
            .expect("test executable has no profile directory")
    } else {
        directory
    };
    directory.join("../../sample-project")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moved_package_selects_its_own_project_even_with_checkout_present() {
        let temporary = tempfile::tempdir().unwrap();
        let checkout = temporary.path().join("original checkout");
        let package = temporary.path().join("moved package");
        for root in [&checkout, &package] {
            std::fs::create_dir_all(root.join("sample-project")).unwrap();
            std::fs::create_dir_all(root.join("apps/aestra-editor")).unwrap();
        }
        let executable = package.join("apps/aestra-editor/aestra-editor.exe");
        assert_eq!(
            project_root(&executable, None).canonicalize().unwrap(),
            package.join("sample-project").canonicalize().unwrap()
        );
        std::fs::remove_dir(package.join("sample-project")).unwrap();
        assert!(project_root(&executable, None).canonicalize().is_err());
        assert!(checkout.join("sample-project").is_dir());
    }

    #[test]
    fn standard_cargo_build_and_unit_test_layouts_agree() {
        assert_eq!(
            project_root(Path::new("checkout/target/debug/aestra-editor.exe"), None),
            Path::new("checkout/target/debug/../../sample-project")
        );
        assert_eq!(
            project_root(
                Path::new("checkout/target/release/deps/editor-hash.exe"),
                None
            ),
            Path::new("checkout/target/release/../../sample-project")
        );
    }

    #[test]
    fn cargo_runtime_manifest_supports_external_target_directories() {
        assert_eq!(
            project_root(
                Path::new("external/build/editor.exe"),
                Some(Path::new("checkout/apps/aestra-editor"))
            ),
            Path::new("checkout/apps/aestra-editor/../../sample-project")
        );
        let executable = Path::new("package/apps/aestra-editor/aestra-editor.exe");
        assert_eq!(
            project_root(executable, Some(Path::new(""))),
            project_root(executable, None)
        );
    }
}
