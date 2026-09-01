use std::{fs, path::Path};

#[test]
fn bevy_runtime_adapter_is_only_referenced_by_the_preview_bridge() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let allowed = source_root.join("bevy_preview.rs");
    let mut violations = Vec::new();
    collect_adapter_imports(&source_root, &allowed, &mut violations);
    assert!(
        violations.is_empty(),
        "editor modules must use owning Aestra crates instead of aestra-bevy re-exports: {}",
        violations.join(", ")
    );
}

fn collect_adapter_imports(directory: &Path, allowed: &Path, violations: &mut Vec<String>) {
    for entry in fs::read_dir(directory).expect("editor source directory must be readable") {
        let entry = entry.expect("editor source entry must be readable");
        let path = entry.path();
        if path.is_dir() {
            collect_adapter_imports(&path, allowed, violations);
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && path != allowed
            && fs::read_to_string(&path)
                .expect("editor source file must be readable")
                .contains("aestra_bevy")
        {
            violations.push(
                path.strip_prefix(directory)
                    .unwrap_or(&path)
                    .display()
                    .to_string(),
            );
        }
    }
}
