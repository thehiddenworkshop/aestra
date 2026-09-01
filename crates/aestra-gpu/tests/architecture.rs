use std::{fs, path::Path};

const PORTABLE_CRATES: &[&str] = &[
    "aestra-core",
    "aestra-authoring",
    "aestra-project",
    "aestra-runtime",
    "aestra-compiler",
    "aestra-gpu",
    "aestra-artifact",
];

fn collect_portable_sources(path: &Path, output: &mut String) {
    for entry in fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_portable_sources(&path, output);
        } else if matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("rs" | "wesl")
        ) {
            output.push_str(&fs::read_to_string(path).unwrap());
        }
    }
}

#[test]
fn portable_crates_have_no_engine_or_graphics_api_dependency() {
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("aestra-gpu must live under the workspace crates directory");

    for crate_name in PORTABLE_CRATES {
        let root = workspace_root.join("crates").join(crate_name);
        let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
        let mut source = String::new();
        collect_portable_sources(&root.join("src"), &mut source);

        for forbidden in ["bevy", "wgpu"] {
            assert!(
                !manifest.contains(forbidden),
                "{crate_name} manifest must not depend on {forbidden}"
            );
            assert!(
                !source.contains(&format!("{forbidden}::")),
                "{crate_name} source must not import {forbidden}"
            );
        }
    }
}
