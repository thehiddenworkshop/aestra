//! Host bindings HB1 adds `bindings` to authored format v4 as an optional field: every existing
//! effect must load and re-save exactly as before.

use aestra_core::EffectAsset;
use std::path::Path;

#[test]
fn existing_v4_effects_resave_byte_identically() {
    let effects = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sample-project/effects");
    let mut checked = 0;
    for entry in std::fs::read_dir(&effects).unwrap() {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with(".aestra.ron") {
            continue;
        }
        let source = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\r\n", "\n");
        let effect = EffectAsset::from_ron(&source)
            .unwrap_or_else(|error| panic!("{} loads: {error}", path.display()));
        assert!(effect.bindings.is_empty());
        // The RON pretty printer uses the platform line ending; compare content, not newlines.
        assert_eq!(
            effect
                .to_pretty_ron()
                .unwrap()
                .replace("\r\n", "\n")
                .trim_end(),
            source.trim_end(),
            "{} re-saves byte-identically",
            path.display()
        );
        checked += 1;
    }
    assert!(checked >= 6, "the sample effects were checked");
}
