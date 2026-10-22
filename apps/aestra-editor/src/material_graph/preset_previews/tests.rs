use super::*;
use crate::material_graph::preview_pixels::MATERIAL_PREVIEW_SIZE;
use std::{collections::BTreeSet, path::PathBuf};

fn test_asset_root() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|root| {
            root.join("assets/test/materials/material_graph_lab.aestra.material.ron")
                .is_file()
        })
        .expect("repository test assets")
        .join("assets/test")
}

#[test]
fn every_registered_material_preset_produces_a_preview() {
    let asset_root = test_asset_root();
    let project_catalog =
        ProjectEffectCatalog::scan_project(&asset_root, asset_root.join("effects"));
    let catalog = project_catalog
        .material_preset_catalog()
        .expect("built-in and bundled project presets should merge");
    let mut images = Assets::<Image>::default();
    let curated = [
        "Additive Flame",
        "Soft Smoke",
        "Energy Beam",
        "Magic Shield",
        "Dissolve",
        "Hologram",
        "Ghost",
        "Portal",
        "Impact Flash",
    ];
    let mut curated_preview_hashes = BTreeSet::new();

    for preset in catalog.iter() {
        let status = build_material_preset_preview(preset, &catalog, &mut images);
        let MaterialPresetPreviewStatus::Ready(image) = status else {
            panic!("{} should produce a preview", preset.display_name);
        };
        let preview = images
            .get(&image)
            .expect("generated preview should be cached as an image asset");
        assert_eq!(preview.texture_descriptor.size.width, MATERIAL_PREVIEW_SIZE);
        assert_eq!(
            preview.texture_descriptor.size.height,
            MATERIAL_PREVIEW_SIZE
        );
        if curated.contains(&preset.display_name.as_str()) {
            let hash = preview
                .data
                .as_ref()
                .expect("preset preview should be CPU-backed")
                .iter()
                .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                    (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
                });
            curated_preview_hashes.insert(hash);
        }
    }
    for name in curated {
        assert!(
            catalog.iter().any(|preset| preset.display_name == name),
            "curated preset {name} should be registered"
        );
    }
    assert_eq!(
        curated_preview_hashes.len(),
        curated.len(),
        "each curated material look should have a distinct preview"
    );
}

#[test]
fn preset_previews_refresh_on_published_revision_without_resource_change_ticks() {
    let root = tempfile::tempdir().unwrap();
    let presets = MaterialCompiler.material_preset_catalog();
    let mut preset = presets
        .get(aestra_compiler::MATERIAL_PRESET_DISSOLVE)
        .unwrap()
        .clone();
    preset.id = aestra_core::MaterialPresetId::new();
    let path = root.path().join("preview.aestra.material-preset.ron");
    preset.save_ron(&path).unwrap();
    let mut app = App::new();
    app.insert_resource(ProjectEffectCatalog::scan(root.path()))
        .init_resource::<MaterialPresetPreviewState>()
        .init_resource::<Assets<Image>>()
        .add_systems(Update, rasterize_material_preset_previews);
    let label = app.world_mut().spawn(Text::default()).id();
    let entity = app
        .world_mut()
        .spawn(MaterialPresetPreviewRaster {
            preset: preset.id,
            status_label: label,
        })
        .id();
    app.update();
    let first = app.world().get::<ImageNode>(entity).unwrap().image.clone();
    let aestra_core::material::MaterialPresetRecipe::Stack { defaults, .. } = &mut preset.recipe
    else {
        panic!("stack recipe");
    };
    defaults[0].value = MaterialValue::Float(0.123);
    preset.save_ron(&path).unwrap();
    app.world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .bypass_change_detection()
        .refresh();
    app.update();
    assert_ne!(app.world().get::<ImageNode>(entity).unwrap().image, first);
}

#[test]
fn material_preset_preview_fingerprint_tracks_recipe_changes() {
    let catalog = MaterialCompiler.material_preset_catalog();
    let preset = catalog
        .get(aestra_compiler::MATERIAL_PRESET_DISSOLVE)
        .expect("Dissolve preset should be registered");
    let mut changed_metadata = preset.clone();
    changed_metadata.description.push_str(" Updated");
    let mut changed_recipe = preset.clone();
    let aestra_core::material::MaterialPresetRecipe::Stack { defaults, .. } =
        &mut changed_recipe.recipe
    else {
        panic!("Dissolve should use a stack recipe");
    };
    defaults[0].value = MaterialValue::Float(0.123);

    assert_eq!(
        material_preset_fingerprint(preset),
        material_preset_fingerprint(&changed_metadata)
    );
    assert_ne!(
        material_preset_fingerprint(preset),
        material_preset_fingerprint(&changed_recipe)
    );
}
