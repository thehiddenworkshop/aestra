//! One-shot authored-format **v3 → v4** converter (extensible-stages redesign M3).
//!
//! Aestra is pre-release and carries no runtime migrations, so this is a dev tool run **once** over the
//! in-repo assets to rewrite them from the flat v3 shape into the nested v4 shape, after which the v4
//! files are committed. It reads a v3 effect with the flat derive (the shape `EffectAsset` had at v3),
//! stamps the current format version, and saves it through `save_ron`, which now serializes the nested
//! v4 document.
//!
//! Usage: `cargo run -p aestra-core --example migrate_v3_to_v4 -- <file.aestra.ron>...`

use aestra_core::{detect_effect_format, EffectAsset, CURRENT_FORMAT_VERSION};
use std::path::Path;

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: migrate_v3_to_v4 <file.aestra.ron>...");
        std::process::exit(2);
    }
    let (mut converted, mut skipped) = (0u32, 0u32);
    for path in &paths {
        let source = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));
        let version = detect_effect_format(&source).unwrap_or_else(|e| panic!("detect {path}: {e}"));
        if version == CURRENT_FORMAT_VERSION {
            println!("skip (already v{version}): {path}");
            skipped += 1;
            continue;
        }
        assert_eq!(version, 3, "{path}: only v3 -> v4 conversion is supported (found v{version})");
        // Read the flat v3 shape directly (not through `from_ron`, which now expects v4).
        let mut asset: EffectAsset =
            ron::from_str(&source).unwrap_or_else(|e| panic!("parse flat v3 {path}: {e}"));
        asset.format_version = CURRENT_FORMAT_VERSION;
        asset
            .save_ron(Path::new(path))
            .unwrap_or_else(|e| panic!("write v4 {path}: {e}"));
        println!("converted v3 -> v4: {path}");
        converted += 1;
    }
    println!("done: {converted} converted, {skipped} skipped");
}
