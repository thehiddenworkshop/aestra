use aestra_core::{
    EffectAsset, EffectId, MaterialExpressionId, MaterialFunctionId, MaterialFunctionOutputId,
    MaterialPresetId,
    material::{
        MaterialExpression, MaterialExpressionKind, MaterialFunction, MaterialFunctionOutput,
        MaterialPresetCategory, MaterialPresetDescriptor, MaterialPresetRecipe,
        MaterialPresetSchemaVersion, MaterialProgram, MaterialSchemaVersion,
        MaterialStackModifierKind, MaterialValue, MaterialValueType,
    },
};
use aestra_project::{
    ProjectAssetDiagnosticCode, ProjectAssetId, ProjectAssetIndex, ProjectAssetIndexAvailability,
    ProjectContent, ProjectContentResolveError, ProjectEffectStatus,
    ProjectFileClassification as Class, ProjectSourceKind, ProjectSourceTree,
};
use std::{fs, path::Path};

fn classification(content: &ProjectContent, path: &str) -> Class {
    let source = content.source_tree().at_relative_path(path).unwrap();
    let ProjectSourceKind::File(file) = &source.kind else {
        panic!("expected file")
    };
    file.classification
}

fn write_effect(path: &Path, id: EffectId) {
    let mut effect = EffectAsset::new("Fire", 1.0);
    effect.id = id;
    effect.save_ron(path).unwrap();
}

fn function() -> MaterialFunction {
    let expression = MaterialExpressionId::new();
    MaterialFunction {
        schema_version: MaterialSchemaVersion::CURRENT,
        id: MaterialFunctionId::new(),
        name: "Constant function".into(),
        inputs: vec![],
        outputs: vec![MaterialFunctionOutput {
            id: MaterialFunctionOutputId::new(),
            name: "Value".into(),
            value_type: MaterialValueType::Float,
            expression,
        }],
        expressions: vec![MaterialExpression {
            id: expression,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        }],
        custom_wesl: None,
    }
}

fn preset() -> MaterialPresetDescriptor {
    MaterialPresetDescriptor {
        schema_version: MaterialPresetSchemaVersion::CURRENT,
        id: MaterialPresetId::new(),
        display_name: "Fire preset".into(),
        description: "Fixture".into(),
        category: MaterialPresetCategory::Shaping,
        tags: vec![],
        recipe: MaterialPresetRecipe::Stack {
            modifiers: vec![MaterialStackModifierKind::Remap],
            defaults: vec![],
        },
    }
}

#[test]
fn mixed_content_has_one_source_hierarchy_and_all_four_semantic_types() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    for folder in ["fire/nested", "empty", "other"] {
        fs::create_dir_all(root.join(folder)).unwrap();
    }
    let mut effect = EffectAsset::new("Fire", 1.0);
    let texture = aestra_core::AssetDefinition::texture("Noise", "fire/noise.PNG");
    effect.flipbooks.push(aestra_core::FlipbookDefinition::grid(
        "Noise frames",
        texture.id,
        4,
        1,
        12.0,
    ));
    effect.assets.push(texture);
    effect.save_ron(root.join("fire/fire.aestra.ron")).unwrap();
    let material = MaterialProgram::additive_sprite("Fire material");
    material
        .save_ron(root.join("fire/fire.aestra.material.ron"))
        .unwrap();
    let function = function();
    function
        .save_ron(root.join("fire/nested/fire.aestra.material-function.ron"))
        .unwrap();
    let preset = preset();
    preset
        .save_ron(root.join("fire/nested/fire.aestra.material-preset.ron"))
        .unwrap();
    for path in [
        "fire/noise.PNG",
        "other/noise.PNG",
        "fire/cube.glb",
        "fire/custom.wesl",
        "notes.txt",
        "settings.ron",
        ".visible",
    ] {
        fs::write(root.join(path), "ordinary source").unwrap();
    }

    let content = ProjectContent::scan(root);
    assert!(content.asset_index().diagnostics().is_empty());
    let expected = [
        ("fire/fire.aestra.ron", ProjectAssetId::Effect(effect.id)),
        (
            "fire/fire.aestra.material.ron",
            ProjectAssetId::MaterialProgram(material.id),
        ),
        (
            "fire/nested/fire.aestra.material-function.ron",
            ProjectAssetId::MaterialFunction(function.id),
        ),
        (
            "fire/nested/fire.aestra.material-preset.ron",
            ProjectAssetId::MaterialPreset(preset.id),
        ),
    ];
    for (path, asset) in expected {
        let source = content.source_tree().at_relative_path(path).unwrap();
        assert_eq!(content.asset_for_source(source.id), Some(asset));
        assert_eq!(content.sources_for_asset(asset), &[source.id]);
        assert_eq!(
            content.unique_source_for_asset(asset).unwrap().id,
            source.id
        );
    }
    let index = content.asset_index();
    let loaded = index.load_effect(effect.id.into()).unwrap();
    assert_eq!(loaded.assets, effect.assets);
    assert_eq!(loaded.flipbooks, effect.flipbooks);
    assert_eq!(
        index.effects()[0].id,
        content
            .unique_source_for_asset(ProjectAssetId::Effect(effect.id))
            .unwrap()
            .id
    );
    assert_eq!(
        index.material_programs()[0].id,
        content
            .unique_source_for_asset(ProjectAssetId::MaterialProgram(material.id))
            .unwrap()
            .id
    );
    assert_eq!(
        index.material_functions()[0].id,
        content
            .unique_source_for_asset(ProjectAssetId::MaterialFunction(function.id))
            .unwrap()
            .id
    );
    assert_eq!(
        index.material_presets()[0].id,
        content
            .unique_source_for_asset(ProjectAssetId::MaterialPreset(preset.id))
            .unwrap()
            .id
    );
    assert_eq!(classification(&content, "fire/noise.PNG"), Class::Texture);
    assert_eq!(classification(&content, "fire/cube.glb"), Class::Mesh);
    assert_eq!(classification(&content, "fire/custom.wesl"), Class::Shader);
    assert_eq!(classification(&content, "settings.ron"), Class::Generic);
    let tree = content.source_tree();
    let folder = tree.at_relative_path("empty").unwrap();
    assert_eq!(folder.kind, ProjectSourceKind::Directory);
    assert!(folder.error.is_none());
    assert_eq!(tree.children(folder.id).count(), 0);
    assert!(tree.at_relative_path(".visible").is_some());
    assert_ne!(
        tree.at_relative_path("fire/noise.PNG").unwrap().id,
        tree.at_relative_path("other/noise.PNG").unwrap().id
    );
    let image = tree.at_relative_path("fire/noise.PNG").unwrap();
    assert_eq!(image.metadata.as_ref().unwrap().bytes, 15);
    assert_eq!(content.asset_for_source(image.id), None);
    assert_eq!(
        tree.source(image.parent.unwrap()).unwrap().relative_path,
        Path::new("fire")
    );
    let children: Vec<_> = tree
        .children(tree.root())
        .map(|source| source.name.clone())
        .collect();
    assert_eq!(&children[..3], &["empty", "fire", "other"]);
    let second = ProjectContent::scan(root);
    assert_eq!(
        tree.entries().collect::<Vec<_>>(),
        second.source_tree().entries().collect::<Vec<_>>()
    );
}

#[test]
fn legacy_ron_effects_survive_without_treating_every_ron_file_as_an_effect() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let id = EffectId::new();
    write_effect(&root.join("legacy.ron"), id);
    fs::write(root.join("settings.ron"), "(enabled: true)").unwrap();
    fs::write(root.join("broken.aestra.ron"), "invalid").unwrap();
    fs::write(root.join("broken.aestra.material.ron"), "invalid").unwrap();
    fs::write(root.join("future.aestra.ron"), "(format_version: 999)").unwrap();
    let content = ProjectContent::scan(root);
    assert_eq!(content.asset_index().effects().len(), 3);
    assert_eq!(classification(&content, "legacy.ron"), Class::Effect);
    assert_eq!(classification(&content, "settings.ron"), Class::Generic);
    assert_eq!(
        classification(&content, "broken.aestra.material.ron"),
        Class::MaterialProgram
    );
    assert_eq!(classification(&content, "future.aestra.ron"), Class::Effect);
    assert!(
        content
            .asset_index()
            .diagnostics()
            .iter()
            .all(|diagnostic| diagnostic.path.as_deref()
                != Some(root.join("settings.ron").as_path()))
    );
    assert!(
        content
            .asset_index()
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.code == ProjectAssetDiagnosticCode::UnsupportedFormat)
    );
    assert_eq!(content.asset_index().load_effect(id.into()).unwrap().id, id);
    let broken = content
        .source_tree()
        .at_relative_path("broken.aestra.ron")
        .unwrap();
    assert_eq!(content.asset_for_source(broken.id), None);
    assert!(matches!(
        content.asset_index().entry(broken.id).unwrap().status,
        ProjectEffectStatus::Invalid { .. }
    ));

    // Characterization: the compatibility index API still exposes failed arbitrary RON candidates.
    let legacy = ProjectAssetIndex::scan(root);
    assert_eq!(legacy.effects().len(), 4);
    assert!(
        legacy
            .diagnostics()
            .iter()
            .any(|diagnostic| diagnostic.path.as_deref()
                == Some(root.join("settings.ron").as_path()))
    );
    assert_eq!(
        legacy.resolve(id.into()).unwrap().id,
        content
            .unique_source_for_asset(ProjectAssetId::Effect(id))
            .unwrap()
            .id
    );
}

#[test]
fn duplicate_semantic_identity_keeps_every_source_and_never_picks_one() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let id = EffectId::new();
    write_effect(&root.join("second.aestra.ron"), id);
    write_effect(&root.join("first.aestra.ron"), id);
    let content = ProjectContent::scan(root);
    let asset = ProjectAssetId::Effect(id);
    let sources = content.sources_for_asset(asset);
    assert_eq!(sources.len(), 2);
    assert_ne!(sources[0], sources[1]);
    assert_eq!(
        content.source(sources[0]).unwrap().relative_path,
        Path::new("first.aestra.ron")
    );
    for source in sources {
        assert_eq!(content.asset_for_source(*source), Some(asset));
    }
    assert_eq!(
        content.unique_source_for_asset(asset).unwrap_err(),
        ProjectContentResolveError::Ambiguous {
            asset,
            sources: sources.to_vec()
        }
    );
    assert!(content.asset_index().resolve(id.into()).is_err());
    assert!(matches!(
        content.unique_source_for_asset(ProjectAssetId::Effect(EffectId::new())),
        Err(ProjectContentResolveError::Missing { .. })
    ));
}

#[test]
fn duplicate_material_program_function_and_preset_sources_keep_their_shared_tree_ids() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let program = MaterialProgram::additive_sprite("Program");
    let function = function();
    let preset = preset();
    for name in ["first", "second"] {
        program
            .save_ron(root.join(format!("{name}.aestra.material.ron")))
            .unwrap();
        function
            .save_ron(root.join(format!("{name}.aestra.material-function.ron")))
            .unwrap();
        preset
            .save_ron(root.join(format!("{name}.aestra.material-preset.ron")))
            .unwrap();
    }
    let content = ProjectContent::scan(root);
    for asset in [
        ProjectAssetId::MaterialProgram(program.id),
        ProjectAssetId::MaterialFunction(function.id),
        ProjectAssetId::MaterialPreset(preset.id),
    ] {
        let sources = content.sources_for_asset(asset);
        assert_eq!(sources.len(), 2);
        assert_ne!(sources[0], sources[1]);
        for source in sources {
            assert_eq!(content.asset_for_source(*source), Some(asset));
        }
        assert_eq!(
            content.unique_source_for_asset(asset).unwrap_err(),
            ProjectContentResolveError::Ambiguous {
                asset,
                sources: sources.to_vec(),
            }
        );
    }
    for entry in content.asset_index().material_programs() {
        assert_eq!(content.source(entry.id).unwrap().path, entry.path);
    }
    for entry in content.asset_index().material_functions() {
        assert_eq!(content.source(entry.id).unwrap().path, entry.path);
    }
    for entry in content.asset_index().material_presets() {
        assert_eq!(content.source(entry.id).unwrap().path, entry.path);
    }
}

#[test]
fn move_changes_location_without_changing_semantics_and_snapshots_are_read_only() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path();
    let id = EffectId::new();
    let before_path = root.join("before.aestra.ron");
    write_effect(&before_path, id);
    let bytes = fs::read(&before_path).unwrap();
    let before = ProjectContent::scan(root);
    assert_eq!(fs::read(&before_path).unwrap(), bytes);
    assert!(!root.join(".aestra").exists());
    fs::create_dir(root.join("nested")).unwrap();
    fs::rename(&before_path, root.join("nested/after.aestra.ron")).unwrap();
    let after = ProjectContent::scan(root);
    let asset = ProjectAssetId::Effect(id);
    assert_ne!(
        before.unique_source_for_asset(asset).unwrap().id,
        after.unique_source_for_asset(asset).unwrap().id
    );
    assert_eq!(
        after.unique_source_for_asset(asset).unwrap().relative_path,
        Path::new("nested/after.aestra.ron")
    );
    assert_eq!(
        before.unique_source_for_asset(asset).unwrap().path,
        before_path
    );
    assert_eq!(after.asset_index().load_effect(id.into()).unwrap().id, id);
    assert_eq!(
        fs::read(root.join("nested/after.aestra.ron")).unwrap(),
        bytes
    );
}

#[test]
fn internal_directories_are_excluded_but_other_hidden_and_unknown_files_remain() {
    let temporary = tempfile::tempdir().unwrap();
    for folder in [".aestra", ".git", ".hg", ".svn", "nested/.aestra"] {
        fs::create_dir_all(temporary.path().join(folder)).unwrap();
        write_effect(
            &temporary.path().join(folder).join("hidden.aestra.ron"),
            EffectId::new(),
        );
    }
    fs::write(temporary.path().join(".notes"), "notes").unwrap();
    fs::write(temporary.path().join("unknown.bin"), [0, 255]).unwrap();
    let content = ProjectContent::scan(temporary.path());
    assert!(content.asset_index().effects().is_empty());
    assert!(
        ProjectAssetIndex::scan(temporary.path())
            .effects()
            .is_empty()
    );
    assert!(content.source_tree().at_relative_path(".aestra").is_none());
    assert!(content.source_tree().at_relative_path(".notes").is_some());
    assert_eq!(classification(&content, "unknown.bin"), Class::Generic);
}

#[test]
fn missing_or_file_root_is_unavailable_not_an_empty_ready_project() {
    let temporary = tempfile::tempdir().unwrap();
    // Even a valid semantic source cannot be used in place of a directory root.
    write_effect(&temporary.path().join("file.ron"), EffectId::new());
    for root in [
        temporary.path().join("missing"),
        temporary.path().join("file.ron"),
    ] {
        let content = ProjectContent::scan(&root);
        assert!(ProjectSourceTree::validate_root(&root).is_err());
        assert!(content.asset_index().effects().is_empty());
        assert!(ProjectAssetIndex::scan(&root).effects().is_empty());
        assert!(matches!(
            content.source_tree().availability(),
            ProjectAssetIndexAvailability::Unavailable { .. }
        ));
        assert_eq!(
            content.asset_index().availability(),
            content.source_tree().availability()
        );
        assert!(
            content
                .source_tree()
                .source(content.source_tree().root())
                .unwrap()
                .error
                .is_some()
        );
        assert!(matches!(
            content.unique_source_for_asset(ProjectAssetId::Effect(EffectId::new())),
            Err(ProjectContentResolveError::Unavailable { .. })
        ));
    }
    let empty = ProjectContent::scan(temporary.path());
    assert_eq!(
        empty.source_tree().availability(),
        &ProjectAssetIndexAvailability::Ready
    );
    assert!(empty.source_tree().at_relative_path("../outside").is_none());
    assert!(
        empty
            .source_tree()
            .at_relative_path(temporary.path())
            .is_none()
    );
}

#[test]
fn classifications_use_compound_suffixes_and_case_insensitive_extensions() {
    for (path, expected) in [
        ("A.AESTRA.RON", Class::Effect),
        ("A.aestra.material.ron", Class::MaterialProgram),
        ("A.aestra.material-function.ron", Class::MaterialFunction),
        ("A.aestra.material-preset.ron", Class::MaterialPreset),
        ("sheet.PNG", Class::Texture),
        ("part.GLTF", Class::Mesh),
        ("node.WESL", Class::Shader),
        ("settings.ron", Class::Generic),
        ("sheet.aestra.flipbook.ron", Class::Generic),
        ("noextension", Class::Generic),
    ] {
        assert_eq!(Class::for_path(Path::new(path)), expected, "{path}");
    }
}

#[test]
fn source_tree_retains_exact_filename_case() {
    let temporary = tempfile::tempdir().unwrap();
    fs::write(temporary.path().join("MixedCase.PNG"), "image").unwrap();
    let tree = ProjectSourceTree::scan(temporary.path());
    let entry = tree.at_relative_path("MixedCase.PNG").unwrap();
    assert_eq!(entry.name, "MixedCase.PNG");
    assert_eq!(entry.path, temporary.path().join("MixedCase.PNG"));
    assert!(tree.at_relative_path("mixedcase.png").is_none());
}

#[cfg(any(unix, windows))]
#[test]
fn links_and_linked_roots_are_not_followed_or_semantically_indexed() {
    let temporary = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    write_effect(&outside.path().join("outside.aestra.ron"), EffectId::new());
    let link = temporary.path().join("external");
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(outside.path(), &link);
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_dir(outside.path(), &link);
    if let Err(error) = result {
        #[cfg(windows)]
        if error.raw_os_error() == Some(1314) {
            eprintln!("SKIPPED native link test: symlink privilege unavailable");
            return;
        }
        panic!("cannot create link fixture: {error}");
    }
    let content = ProjectContent::scan(temporary.path());
    assert_eq!(
        content
            .source_tree()
            .at_relative_path("external")
            .unwrap()
            .kind,
        ProjectSourceKind::Link
    );
    assert!(
        content
            .source_tree()
            .at_relative_path("external/outside.aestra.ron")
            .is_none()
    );
    assert!(content.asset_index().effects().is_empty());
    assert!(
        ProjectAssetIndex::scan(temporary.path())
            .effects()
            .is_empty()
    );
    let linked_root = ProjectContent::scan(&link);
    assert!(ProjectSourceTree::validate_root(&link).is_err());
    assert!(matches!(
        linked_root.source_tree().availability(),
        ProjectAssetIndexAvailability::Unavailable { .. }
    ));
    // Remove only the link, never its target directory.
    #[cfg(windows)]
    fs::remove_dir(link).unwrap();
    #[cfg(unix)]
    fs::remove_file(link).unwrap();
    assert!(outside.path().join("outside.aestra.ron").exists());
}

#[cfg(unix)]
#[test]
fn lossy_display_names_and_case_folded_hashes_do_not_alias_native_paths() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt, path::PathBuf};
    let temporary = tempfile::tempdir().unwrap();
    let names = [
        OsString::from_vec(vec![0xff]),
        OsString::from_vec(vec![0xfe]),
        "A.txt".into(),
        "a.txt".into(),
    ];
    for name in &names {
        fs::write(temporary.path().join(name), "data").unwrap();
    }
    let tree = ProjectSourceTree::scan(temporary.path());
    let ids = names
        .iter()
        .map(|name| tree.at_relative_path(PathBuf::from(name)).unwrap().id)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), names.len());
}
