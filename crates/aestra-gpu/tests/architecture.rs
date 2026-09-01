use std::{fs, path::Path};

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
fn gpu_lowering_has_no_engine_or_graphics_api_dependency() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let mut source = String::new();
    collect_portable_sources(&root.join("src"), &mut source);

    for forbidden in ["bevy", "wgpu"] {
        assert!(
            !manifest.contains(forbidden),
            "aestra-gpu manifest must not depend on {forbidden}"
        );
        assert!(
            !source.contains(&format!("{forbidden}::")),
            "aestra-gpu source must not import {forbidden}"
        );
    }
}
