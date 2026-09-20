use std::{fs, path::Path};

#[test]
fn editor_does_not_depend_on_the_bevy_runtime_adapter() {
    let manifest_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source_root = manifest_root.join("src");
    let mut violations = Vec::new();
    collect_adapter_imports(&source_root, &mut violations);
    assert!(
        violations.is_empty(),
        "editor modules must not import the aestra-bevy runtime adapter: {}",
        violations.join(", ")
    );
    let manifest = fs::read_to_string(manifest_root.join("Cargo.toml"))
        .expect("editor manifest must be readable");
    assert!(
        !manifest
            .lines()
            .any(|line| line.trim_start().starts_with("aestra-bevy =")),
        "aestra-editor must not depend on the aestra-bevy runtime crate"
    );
}

#[test]
fn editor_production_dependencies_do_not_include_the_reference_layout_backend() {
    let manifest_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = fs::read_to_string(manifest_root.join("Cargo.toml"))
        .expect("editor manifest must be readable");
    let production_dependencies = manifest
        .split("[dev-dependencies]")
        .next()
        .expect("manifest must have a production dependency section");

    assert!(
        !production_dependencies
            .lines()
            .any(|line| line.trim_start().starts_with("elkrs")),
        "aestra-editor must keep elkrs as a development/reference dependency only"
    );
}

#[test]
fn arrange_graph_uses_the_native_layout_backend() {
    let manifest_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let arrange = fs::read_to_string(manifest_root.join("src/material_graph/arrange.rs"))
        .expect("Arrange Graph controller must be readable");

    assert!(
        arrange.contains("AestraLayeredLayout.layout(&input)"),
        "Arrange Graph must dispatch the native layout engine"
    );
    assert!(
        !arrange.contains("ElkLayeredLayout"),
        "Arrange Graph must not dispatch the reference elkrs backend"
    );
}

fn collect_adapter_imports(directory: &Path, violations: &mut Vec<String>) {
    for entry in fs::read_dir(directory).expect("editor source directory must be readable") {
        let entry = entry.expect("editor source entry must be readable");
        let path = entry.path();
        if path.is_dir() {
            collect_adapter_imports(&path, violations);
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && fs::read_to_string(&path)
                .expect("editor source file must be readable")
                .contains("aestra_bevy::")
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
