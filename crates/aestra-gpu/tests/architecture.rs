use std::{fs, path::Path};

#[test]
fn gpu_lowering_has_no_engine_or_graphics_api_dependency() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let source = fs::read_to_string(root.join("src/lib.rs")).unwrap();

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
