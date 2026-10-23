use std::{fs, path::Path};

#[test]
fn external_unit_tests_are_grouped_by_subsystem() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut violations = Vec::new();
    for namespace in ["apps", "bevy", "crates", "extensions"] {
        for entry in fs::read_dir(workspace.join(namespace)).expect("workspace namespace") {
            let source = entry.expect("workspace entry").path().join("src");
            if source.is_dir() {
                collect_loose_tests(&source, &mut violations);
            }
        }
    }
    violations.sort();
    assert!(
        violations.is_empty(),
        "external unit tests belong in subsystem tests/ folders (see docs/UNIT_TEST_LAYOUT.md): {}",
        violations.join(", ")
    );
}

fn collect_loose_tests(directory: &Path, violations: &mut Vec<String>) {
    for entry in fs::read_dir(directory).expect("source directory") {
        let path = entry.expect("source entry").path();
        if path.is_dir() {
            if path.file_name().is_none_or(|name| name != "tests") {
                collect_loose_tests(&path, violations);
            }
        } else if let Some(name) = path.file_name().and_then(|name| name.to_str())
            && (name == "tests.rs" || name.ends_with("_tests.rs"))
        {
            violations.push(path.display().to_string());
        }
    }
}
