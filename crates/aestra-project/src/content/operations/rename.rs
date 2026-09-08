//! Shared filename relocation for fully understood semantic projects.
use super::*;
use crate::ProjectAssetId;
use std::collections::BTreeMap;

/// WGSL has no file imports. Accept WESL only when it parses as this import-free subset,
/// rather than guessing from strings (which misses comments, escapes and directives).
pub(super) fn non_referencing_shader(source: &str) -> bool {
    naga::front::wgsl::parse_str(source).is_ok()
}

fn plain_svg_paint(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '#')
}

/// Only core, self-contained glTF is understood here. Extension payloads may introduce
/// other dependencies, so even optional extensions remain conservative blockers.
fn non_referencing_gltf(source: &[u8]) -> bool {
    fn no_extensions(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::Object(fields) => fields
                .iter()
                .all(|(key, value)| key != "extensions" && no_extensions(value)),
            serde_json::Value::Array(values) => values.iter().all(no_extensions),
            _ => true,
        }
    }
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(source) else {
        return false;
    };
    if !no_extensions(&json) {
        return false;
    }
    let Ok(document) = gltf::Gltf::from_slice(source) else {
        return false;
    };
    document.buffers().all(|buffer| match buffer.source() {
        gltf::buffer::Source::Bin => false, // This helper accepts JSON glTF, not GLB.
        gltf::buffer::Source::Uri(uri) => uri.starts_with("data:"),
    }) && document.images().all(|image| match image.source() {
        gltf::image::Source::View { .. } => true,
        gltf::image::Source::Uri { uri, .. } => uri.starts_with("data:"),
    })
}

/// Prove an unindexed asset has no external links; unknown formats remain blocked.
/// SVG is a drawing-only subset: scripts, general CSS, hrefs and unknown tags still block.
pub(super) fn non_referencing_asset(path: &Path) -> bool {
    if super::super::ProjectFileClassification::for_path(path)
        == super::super::ProjectFileClassification::Texture
    {
        return true;
    }
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    let Ok(source) = fs::read_to_string(path) else {
        return false;
    };
    if extension.eq_ignore_ascii_case("gltf") {
        return non_referencing_gltf(source.as_bytes());
    }
    if extension.eq_ignore_ascii_case("wgsl") || extension.eq_ignore_ascii_case("wesl") {
        return non_referencing_shader(&source);
    }
    if !extension.eq_ignore_ascii_case("svg") {
        return false;
    }
    // Ignore only the standard SVG 1.1 declaration used by exported icons. Never
    // enable DTD loading/entity resolution or accept a custom external declaration.
    let source = source.replacen(
        r#"<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd">"#,
        "",
        1,
    );
    let Ok(document) = roxmltree::Document::parse(&source) else {
        return false;
    };
    document.descendants().all(|node| {
        if node.is_pi() {
            return false;
        }
        if !node.is_element() {
            return true;
        }
        if node.tag_name().namespace() != Some("http://www.w3.org/2000/svg")
            || !matches!(
                node.tag_name().name(),
                "svg"
                    | "g"
                    | "defs"
                    | "path"
                    | "circle"
                    | "ellipse"
                    | "rect"
                    | "line"
                    | "polygon"
                    | "polyline"
                    | "title"
                    | "desc"
            )
        {
            return false;
        }
        node.attributes().all(|attribute| {
            if attribute.namespace() == Some("http://www.w3.org/XML/1998/namespace")
                && attribute.name() == "space"
            {
                return matches!(attribute.value(), "preserve" | "default");
            }
            // Sketch's exported shape-kind metadata is not a link. Do not extend
            // this exemption to arbitrary namespaced attributes such as xlink:href.
            if attribute.namespace() == Some("http://www.bohemiancoding.com/sketch/ns")
                && attribute.name() == "type"
            {
                return true;
            }
            if attribute.namespace().is_some() {
                return false;
            }
            match attribute.name() {
                "fill" | "stroke" => plain_svg_paint(attribute.value()),
                // This is deliberately not a general CSS parser. Only literal paint
                // declarations pass; URLs, escapes, comments and all other properties block.
                "style" => attribute
                    .value()
                    .split(';')
                    .filter(|part| !part.trim().is_empty())
                    .all(|part| {
                        part.split_once(':').is_some_and(|(name, value)| {
                            matches!(name.trim(), "fill" | "stroke")
                                && plain_svg_paint(value.trim())
                        })
                    }),
                "d"
                | "points"
                | "viewBox"
                | "width"
                | "height"
                | "x"
                | "y"
                | "x1"
                | "x2"
                | "y1"
                | "y2"
                | "cx"
                | "cy"
                | "r"
                | "rx"
                | "ry"
                | "transform"
                | "stroke-width"
                | "stroke-linecap"
                | "stroke-linejoin"
                | "stroke-miterlimit"
                | "stroke-dasharray"
                | "stroke-dashoffset"
                | "fill-rule"
                | "clip-rule"
                | "opacity"
                | "fill-opacity"
                | "stroke-opacity"
                | "version"
                | "id"
                | "preserveAspectRatio" => true,
                _ => false,
            }
        })
    })
}

/// Compare root-relative paths using filesystem identity when available, and normalized
/// lexical keys for missing/case-only aliases. Unresolvable/outside-root paths stay blocked.
pub(super) fn path_targets(root: &Path, target: &Path, reference: &Path) -> Result<bool, String> {
    let raw = reference.to_string_lossy().replace('\\', "/");
    let reference = Path::new(&raw);
    let candidate = if reference.is_absolute() {
        reference.to_owned()
    } else {
        root.join(reference)
    };
    let resolved = candidate.canonicalize();
    if let Err(error) = &resolved
        && error.kind() != std::io::ErrorKind::NotFound
    {
        return Err(error.to_string());
    }
    if let Ok(canonical) = resolved {
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        if !canonical.starts_with(&root) {
            return Err("path is outside the project".into());
        }
        return Ok(canonical == target.canonicalize().map_err(|e| e.to_string())?);
    }
    fn key(path: &Path) -> Result<Vec<String>, String> {
        let mut parts = Vec::new();
        for component in path.components() {
            match component {
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    if parts.pop().is_none() {
                        return Err("path escapes its root".into());
                    }
                }
                other => parts.push(other.as_os_str().to_string_lossy().to_lowercase()),
            }
        }
        Ok(parts)
    }
    let candidate = key(&candidate)?;
    if !candidate.starts_with(&key(root)?) {
        return Err("path is outside the project".into());
    }
    Ok(candidate == key(target)?)
}

#[derive(Debug)]
pub struct RenamePlan {
    pub(super) destination: OperationPlan,
    pub(super) source: PathBuf,
    pub(super) inventory: BTreeMap<PathBuf, Vec<u8>>,
    pub(super) asset: ProjectAssetId,
    journaled: bool,
}

#[derive(Debug)]
pub struct RenameResult {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub asset: ProjectAssetId,
}

pub(super) fn inventory(root: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>, OperationError> {
    let tree = ProjectSourceTree::scan(root);
    let mut result = BTreeMap::new();
    for entry in tree.entries() {
        if entry.error.is_some() {
            return Err(blocked("Project discovery is incomplete"));
        }
        if entry.kind == ProjectSourceKind::Directory {
            continue;
        }
        checked_parent(
            root,
            entry
                .path
                .parent()
                .ok_or_else(|| blocked("Missing parent"))?,
        )?;
        let metadata = fs::symlink_metadata(&entry.path)?;
        if !metadata.is_file() || super::super::source_tree::is_link(&metadata) {
            return Err(blocked("Linked or unreadable sources block rename"));
        }
        result.insert(entry.path.clone(), fs::read(&entry.path)?);
    }
    Ok(result)
}

impl ProjectContent {
    /// Host must recheck its draft/session guard immediately before apply. No path rewrites
    /// are supported: unknown references and known filename references block this operation.
    pub fn plan_asset_rename(
        &self,
        request: OperationRequest,
        drafts: &[(ProjectSourceId, DraftDocument)],
        complete: bool,
    ) -> Result<RenamePlan, OperationError> {
        self.plan_asset_relocation(request, drafts, complete, false, false)
    }

    /// Single-file, same-project move. Identity and bytes stay unchanged. Sources
    /// requiring path-reference rewrites remain blocked, just as for rename.
    pub fn plan_asset_move(
        &self,
        request: OperationRequest,
        drafts: &[(ProjectSourceId, DraftDocument)],
        complete: bool,
    ) -> Result<RenamePlan, OperationError> {
        self.plan_asset_relocation(request, drafts, complete, true, false)
    }

    pub(super) fn plan_asset_relocation(
        &self,
        request: OperationRequest,
        drafts: &[(ProjectSourceId, DraftDocument)],
        complete: bool,
        journaled: bool,
        rewrite_paths: bool,
    ) -> Result<RenamePlan, OperationError> {
        let (source, rename, parent) = match request {
            OperationRequest::Rename { source, name } if !journaled => (source, Some(name), None),
            OperationRequest::Move { source, parent } if journaled => (source, None, Some(parent)),
            _ => return Err(blocked("Expected a matching rename or move request")),
        };
        if !complete || drafts.iter().any(|(owner, _)| *owner == source) {
            return Err(blocked(
                "Save the source and resolve pending drafts before renaming or moving",
            ));
        }
        let asset = self
            .asset_for_source(source)
            .ok_or_else(|| blocked("Unsupported source"))?;
        let suffix = self
            .asset_operation_suffix(source)
            .ok_or_else(|| blocked("This asset does not support filename relocation"))?;
        if rename.as_ref().is_some_and(|name| !valid_name(name)) {
            return Err(blocked("Use a portable filename stem"));
        }
        let entry = self
            .unique_source_for_asset(asset)
            .map_err(|_| blocked("Source identity is ambiguous"))?;
        let root = self.source_tree().root_path();
        super::transaction::ensure_idle(root)?;
        if journaled {
            super::move_journal::recover(root)?;
        }
        let baseline = inventory(root)?;
        let fresh = ProjectContent::scan(root);
        // Reject stale selected documents instead of silently renaming their replacements.
        let fresh_entry = fresh
            .unique_source_for_asset(asset)
            .map_err(|_| blocked("Source changed; refresh first"))?;
        let same = fresh.documents.get(&source) == self.documents.get(&source);
        if fresh_entry.id != source || !same {
            return Err(blocked("Source changed; refresh first"));
        }
        let report = fresh.reference_preflight_inner(source, drafts, complete, true, rewrite_paths);
        if let Some(reason) = report.incomplete.first() {
            return Err(blocked(&format!("Relocation blocked: {reason}")));
        }
        if fs::metadata(&entry.path)?.permissions().readonly() {
            return Err(blocked("Source is read-only"));
        }
        let destination = fresh.plan_operation(OperationRequest::CreateFolder {
            parent: parent
                .or(entry.parent)
                .ok_or_else(|| blocked("Missing source parent"))?,
            name: rename.map_or_else(
                || entry.name.to_string_lossy().into_owned(),
                |name| format!("{name}{suffix}"),
            ),
        })?;
        if inventory(root)? != baseline {
            return Err(blocked("Project changed during preflight"));
        }
        Ok(RenamePlan {
            destination,
            source: entry.path.clone(),
            inventory: baseline,
            asset,
            journaled,
        })
    }
}

impl RenamePlan {
    pub fn apply(self) -> Result<RenameResult, OperationError> {
        let plan = self.destination;
        super::transaction::ensure_idle(&plan.root)?;
        checked_parent(&plan.root, &plan.parent)?;
        if inventory(&plan.root)? != self.inventory {
            return Err(blocked("Project files changed; rename cancelled"));
        }
        if fs::metadata(&self.source)?.permissions().readonly() {
            return Err(blocked("Source is read-only"));
        }
        vacant(&plan.parent, &plan.destination)?;
        let journal = if self.journaled {
            Some(super::move_journal::prepare(
                &plan.root,
                &self.source,
                &plan.destination,
            )?)
        } else {
            None
        };
        // Creating the journal must not hide a concurrent project change.
        if inventory(&plan.root)? != self.inventory {
            return Err(blocked("Project files changed; relocation cancelled"));
        }
        rename_exclusive(&self.source, &plan.destination)?;
        if let Some(journal) = journal {
            // Publication already succeeded. Never report a failed move and leave
            // the editor saving to the old path just because acknowledgement fails.
            // The next recovery pass reconciles this exact-byte atomic outcome.
            let _ = super::move_journal::finish(&journal);
        }
        Ok(RenameResult {
            source: self.source,
            destination: plan.destination,
            asset: self.asset,
        })
    }
}

#[cfg(windows)]
pub(super) fn rename_exclusive(source: &Path, destination: &Path) -> Result<(), OperationError> {
    use std::os::windows::ffi::OsStrExt;
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: both pointers reference live NUL-terminated UTF-16 buffers. Zero flags means
    // no replacement and no copy fallback; same-filesystem publication is a single rename.
    if unsafe {
        windows_sys::Win32::Storage::FileSystem::MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            0,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn rename_exclusive(source: &Path, destination: &Path) -> Result<(), OperationError> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        source,
        rustix::fs::CWD,
        destination,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|e| OperationError::Io(e.to_string()))
}

#[cfg(not(any(windows, target_os = "linux")))]
pub(super) fn rename_exclusive(_: &Path, _: &Path) -> Result<(), OperationError> {
    Err(blocked(
        "Exclusive rename is not implemented on this platform",
    ))
}

#[cfg(all(test, any(windows, target_os = "linux")))]
mod tests {
    use super::*;
    use aestra_core::{EffectAsset, MaterialId, material::*};

    #[test]
    fn semantic_asset_moves_share_identity_bytes_and_destination_guards() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("destination")).unwrap();
        let effect = EffectAsset::new("Effect", 1.0);
        let program = MaterialProgram::additive_sprite("Material");
        let function = MaterialFunction::from_ron(include_str!(
            "../../../../../assets/materials/dissolve_edge.aestra.material-function.ron"
        ))
        .unwrap();
        effect
            .save_ron(root.path().join("effect.aestra.ron"))
            .unwrap();
        program
            .save_ron(root.path().join("material.aestra.material.ron"))
            .unwrap();
        function
            .save_ron(root.path().join("function.aestra.material-function.ron"))
            .unwrap();
        for asset in [
            ProjectAssetId::Effect(effect.id),
            ProjectAssetId::MaterialProgram(program.id),
            ProjectAssetId::MaterialFunction(function.id),
        ] {
            let content = ProjectContent::scan(root.path());
            let source = content.unique_source_for_asset(asset).unwrap();
            let original = source.path.clone();
            let bytes = fs::read(&original).unwrap();
            let parent = content
                .source_tree()
                .at_relative_path(Path::new("destination"))
                .unwrap()
                .id;
            let request = OperationRequest::Move {
                source: source.id,
                parent,
            };
            let plan = content.plan_asset_move(request.clone(), &[], true).unwrap();
            // Replacing any project bytes after planning must cancel publication.
            fs::write(&original, b"concurrent edit").unwrap();
            assert!(plan.apply().is_err());
            fs::write(&original, &bytes).unwrap();
            let result = content
                .plan_asset_move(request, &[], true)
                .unwrap()
                .apply()
                .unwrap();
            assert!(!original.exists());
            assert_eq!(fs::read(&result.destination).unwrap(), bytes);
            assert_eq!(result.asset, asset);
            let fresh = ProjectContent::scan(root.path());
            assert_eq!(
                fresh.unique_source_for_asset(asset).unwrap().path,
                result.destination
            );
            assert!(
                fresh
                    .plan_asset_move(
                        OperationRequest::Move {
                            source: fresh.unique_source_for_asset(asset).unwrap().id,
                            parent
                        },
                        &[],
                        true
                    )
                    .is_err()
            );
        }
        assert_eq!(
            ProjectContent::scan(root.path())
                .recover_asset_moves()
                .unwrap(),
            0
        );
    }

    fn request(content: &ProjectContent, asset: ProjectAssetId, name: &str) -> OperationRequest {
        OperationRequest::Rename {
            source: content.unique_source_for_asset(asset).unwrap().id,
            name: name.into(),
        }
    }

    #[test]
    fn filename_rename_preserves_bytes_identity_and_effect_resolution() {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Display name");
        let original = root.path().join("original.aestra.material.ron");
        program.save_ron(&original).unwrap();
        let bytes = fs::read(&original).unwrap();
        let mut effect = EffectAsset::new("User", 1.0);
        fs::write(root.path().join("unrelated.png"), []).unwrap();
        fs::write(
            root.path().join("icon.svg"),
            include_str!("../../../../../assets/icons/folder-plus.svg"),
        )
        .unwrap();
        effect.assets.push(aestra_core::AssetDefinition::texture(
            "Unrelated",
            "./unrelated.png",
        ));
        effect.material_instances.push(MaterialInstance {
            id: MaterialId::new(),
            program: MaterialProgramRef::Project(program.id),
            values: Default::default(),
            render_state: MaterialRenderState::additive_sprite(),
        });
        effect
            .save_ron(root.path().join("user.aestra.ron"))
            .unwrap();
        let content = ProjectContent::scan(root.path());
        let result = content
            .plan_asset_rename(
                request(
                    &content,
                    ProjectAssetId::MaterialProgram(program.id),
                    "renamed",
                ),
                &[],
                true,
            )
            .unwrap()
            .apply()
            .unwrap();
        assert!(!original.exists());
        assert_eq!(fs::read(&result.destination).unwrap(), bytes);
        assert_eq!(
            MaterialProgram::load_ron(&result.destination).unwrap().name,
            program.name
        );
        let fresh = ProjectContent::scan(root.path());
        assert!(fresh.asset_index().resolve_effect_project(&effect).is_ok());
        assert_eq!(
            fresh.unique_source_for_asset(result.asset).unwrap().path,
            result.destination
        );
        let mut index = crate::ProjectAssetIndex::scan(root.path());
        let loaded = index
            .load_material_program(MaterialProgramRef::Project(program.id))
            .unwrap();
        let mut edited = loaded.clone();
        edited.name = "Edited after reopen".into();
        let source = fresh.unique_source_for_asset(result.asset).unwrap().id;
        index
            .replace_material_program_source(source, &loaded, &edited)
            .unwrap();
        assert_eq!(
            MaterialProgram::load_ron(&result.destination).unwrap().name,
            edited.name
        );
        assert!(!original.exists());
        assert!(index.resolve_effect_project(&effect).is_ok());
    }

    #[test]
    fn path_aliases_and_draft_only_filename_references_block() {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Original");
        let target = root.path().join("original.aestra.material.ron");
        program.save_ron(&target).unwrap();
        for alias in [
            "./original.aestra.material.ron",
            "sub/../original.aestra.material.ron",
            "sub\\..\\ORIGINAL.aestra.material.ron",
        ] {
            assert!(path_targets(root.path(), &target, Path::new(alias)).unwrap());
        }
        assert!(!path_targets(root.path(), &target, Path::new("unrelated.png")).unwrap());
        assert!(path_targets(root.path(), &target, Path::new("../outside.png")).is_err());
        let effect = EffectAsset::new("Owner", 1.0);
        effect
            .save_ron(root.path().join("owner.aestra.ron"))
            .unwrap();
        let content = ProjectContent::scan(root.path());
        let owner = content
            .unique_source_for_asset(ProjectAssetId::Effect(effect.id))
            .unwrap()
            .id;
        let mut draft = effect;
        draft.assets.push(aestra_core::AssetDefinition::texture(
            "Path alias",
            "./original.aestra.material.ron",
        ));
        let error = content
            .plan_asset_rename(
                request(
                    &content,
                    ProjectAssetId::MaterialProgram(program.id),
                    "renamed",
                ),
                &[(owner, DraftDocument::Effect(Box::new(draft)))],
                true,
            )
            .unwrap_err();
        assert!(error.to_string().contains("requires a path rewrite"));
        assert!(target.exists());
    }

    #[test]
    fn only_proven_drawing_svg_is_non_referencing() {
        let root = tempfile::tempdir().unwrap();
        let icon = root.path().join("icon.svg");
        for body in [
            r#"<image href="original.aestra.material.ron"/>"#,
            r#"<style>path {fill: url(other.svg)}</style>"#,
            r#"<path fill="u&#114;l(other.svg)"/>"#,
            "<script/>",
            "<defs><script/></defs>",
            r#"<defs><image href="original.aestra.material.ron"/></defs>"#,
            r#"<defs onload="run()"/>"#,
            r#"<defs><unknown/></defs>"#,
            r#"<path style="fill:url(original.aestra.material.ron)"/>"#,
            r#"<path style="fill:u\72l(original.aestra.material.ron)"/>"#,
            r#"<path style="fill:#fff;filter:url(other.svg)"/>"#,
            r#"<path xmlns:xlink="http://www.w3.org/1999/xlink" xlink:href="other.svg"/>"#,
        ] {
            fs::write(
                &icon,
                format!(r#"<svg xmlns="http://www.w3.org/2000/svg">{body}</svg>"#),
            )
            .unwrap();
            assert!(!non_referencing_asset(&icon));
        }
        fs::write(
            &icon,
            r##"<svg xmlns="http://www.w3.org/2000/svg"><path fill="#fff" d="M0 0"/></svg>"##,
        )
        .unwrap();
        assert!(non_referencing_asset(&icon));
        for declaration in [
            r#"<!DOCTYPE svg SYSTEM "original.aestra.material.ron">"#,
            r#"<!DOCTYPE svg [<!ENTITY link SYSTEM "original.aestra.material.ron">]>"#,
        ] {
            fs::write(
                &icon,
                format!(r#"{declaration}<svg xmlns="http://www.w3.org/2000/svg"/>"#),
            )
            .unwrap();
            assert!(!non_referencing_asset(&icon));
        }
    }

    #[test]
    fn bundled_icons_are_proven_drawing_only_including_pause_defs() {
        let icons = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/icons");
        for entry in fs::read_dir(icons).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|ext| ext == "svg") {
                assert!(non_referencing_asset(&path), "{}", path.display());
            }
        }
    }

    #[test]
    fn self_contained_meshes_and_shaders_do_not_block_but_external_sources_do() {
        assert!(non_referencing_gltf(include_bytes!(
            "../../../../../assets/meshes/lab_cube.gltf"
        )));
        for source in [
            r#"{"asset":{"version":"2.0"},"buffers":[{"byteLength":4,"uri":"original.aestra.material.ron"}]}"#,
            r#"{"asset":{"version":"2.0"},"images":[{"uri":"original.aestra.material.ron"}]}"#,
            r#"{"asset":{"version":"2.0"},"extensions":{"CUSTOM":{"uri":"external"}}}"#,
            r#"{"asset":{"version":"2.0"},"nodes":[{"extensions":{"CUSTOM":{}}}]}"#,
            "invalid json",
        ] {
            assert!(!non_referencing_gltf(source.as_bytes()), "{source}");
        }
        assert!(non_referencing_shader(include_str!(
            "../../../../../assets/shaders/preview_grid.wesl"
        )));
        for source in [
            "import package::external; fn main() {}",
            "#include \"original.aestra.material.ron\"",
            "not a shader",
        ] {
            assert!(!non_referencing_shader(source), "{source}");
        }
    }

    #[test]
    fn custom_function_imports_in_saved_sources_and_drafts_block_rename() {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Original");
        program
            .save_ron(root.path().join("original.aestra.material.ron"))
            .unwrap();
        let mut function = MaterialFunction::from_ron(include_str!(
            "../../../../../assets/materials/pulse_wave.aestra.material-function.ron"
        ))
        .unwrap();
        let path = root.path().join("pulse.aestra.material-function.ron");
        function.save_ron(&path).unwrap();
        let content = ProjectContent::scan(root.path());
        let asset = ProjectAssetId::MaterialProgram(program.id);
        assert!(
            content
                .plan_asset_rename(request(&content, asset, "renamed"), &[], true)
                .is_ok()
        );
        let owner = content
            .unique_source_for_asset(ProjectAssetId::MaterialFunction(function.id))
            .unwrap()
            .id;
        function
            .custom_wesl
            .as_mut()
            .unwrap()
            .source
            .insert_str(0, "import package::external;\n");
        assert!(
            content
                .plan_asset_rename(
                    request(&content, asset, "renamed"),
                    &[(owner, DraftDocument::Function(Box::new(function.clone())))],
                    true
                )
                .is_err()
        );
        function.save_ron(&path).unwrap();
        let content = ProjectContent::scan(root.path());
        assert!(
            content
                .plan_asset_rename(request(&content, asset, "renamed"), &[], true)
                .is_err()
        );
    }

    #[test]
    fn bundled_project_supports_shared_asset_filename_rename() {
        // Exercise the complete shipping project, not an empty synthetic folder:
        // icons, mesh, shader and custom-function sources must all be accounted for.
        // All publication happens in the temporary copy, never in the workspace.
        let root = tempfile::tempdir().unwrap();
        let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
        let tree = ProjectSourceTree::scan(&assets);
        for entry in tree.entries() {
            let destination = root.path().join(&entry.relative_path);
            if entry.kind == ProjectSourceKind::Directory {
                fs::create_dir_all(destination).unwrap();
            } else {
                fs::create_dir_all(destination.parent().unwrap()).unwrap();
                fs::copy(&entry.path, destination).unwrap();
            }
        }
        for filename in [
            "materials/dissolve_edge.aestra.material-function.ron",
            "materials/material_graph_lab.aestra.material.ron",
            "effects/prism_bloom.aestra.ron",
        ] {
            let content = ProjectContent::scan(root.path());
            let path = root.path().join(filename);
            let entry = content
                .source_tree()
                .entries()
                .find(|entry| entry.path == path)
                .unwrap();
            let asset = content.asset_for_source(entry.id).unwrap();
            let bytes = fs::read(&path).unwrap();
            let result = content
                .plan_asset_rename(request(&content, asset, "renamed"), &[], true)
                .unwrap()
                .apply()
                .unwrap();
            assert!(!path.exists());
            assert_eq!(fs::read(&result.destination).unwrap(), bytes);
            let fresh = ProjectContent::scan(root.path());
            assert_eq!(
                fresh.unique_source_for_asset(asset).unwrap().path,
                result.destination
            );
        }
    }

    #[test]
    fn effect_filename_rename_preserves_nested_references_and_rejects_dirty_sources() {
        for filename in ["original.aestra.ron", "original.ron"] {
            let root = tempfile::tempdir().unwrap();
            let effect = EffectAsset::new("Authored name", 2.0);
            let original = root.path().join(filename);
            effect.save_ron(&original).unwrap();
            let mut owner = EffectAsset::new("Parent", 2.0);
            owner
                .effect_clips
                .push(aestra_core::EffectClip::new(effect.id, 0.0, 2.0));
            owner
                .save_ron(root.path().join("owner.aestra.ron"))
                .unwrap();
            let content = ProjectContent::scan(root.path());
            let asset = ProjectAssetId::Effect(effect.id);
            let source = content.unique_source_for_asset(asset).unwrap().id;
            assert!(
                content
                    .plan_asset_rename(
                        request(&content, asset, "renamed"),
                        &[(source, DraftDocument::Effect(Box::new(effect.clone())))],
                        true
                    )
                    .is_err()
            );
            let before = fs::read(&original).unwrap();
            let result = content
                .plan_asset_rename(request(&content, asset, "renamed"), &[], true)
                .unwrap()
                .apply()
                .unwrap();
            assert_eq!(fs::read(&result.destination).unwrap(), before);
            assert_eq!(EffectAsset::load_ron(&result.destination).unwrap(), effect);
            let fresh = ProjectContent::scan(root.path());
            assert!(fresh.asset_index().resolve_effect_project(&owner).is_ok());
            assert_eq!(result.destination.extension().unwrap(), "ron");
            assert_eq!(
                result.destination.file_name().unwrap(),
                filename.replace("original", "renamed").as_str()
            );
        }
    }

    #[test]
    fn function_identity_and_all_signatures_survive_rename() {
        let root = tempfile::tempdir().unwrap();
        let function = MaterialFunction::from_ron(include_str!(
            "../../../../../assets/materials/dissolve_edge.aestra.material-function.ron"
        ))
        .unwrap();
        function
            .save_ron(root.path().join("function.aestra.material-function.ron"))
            .unwrap();
        let content = ProjectContent::scan(root.path());
        let result = content
            .plan_asset_rename(
                request(
                    &content,
                    ProjectAssetId::MaterialFunction(function.id),
                    "renamed",
                ),
                &[],
                true,
            )
            .unwrap()
            .apply()
            .unwrap();
        assert_eq!(
            MaterialFunction::load_ron(result.destination).unwrap(),
            function
        );
    }

    #[test]
    fn stale_dirty_unknown_and_colliding_renames_preserve_original() {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Original");
        let original = root.path().join("original.aestra.material.ron");
        program.save_ron(&original).unwrap();
        let content = ProjectContent::scan(root.path());
        let asset = ProjectAssetId::MaterialProgram(program.id);
        let source = content.unique_source_for_asset(asset).unwrap().id;
        for name in ["original", "ORIGINAL", "../escape", "NUL", ""] {
            assert!(
                content
                    .plan_asset_rename(request(&content, asset, name), &[], true)
                    .is_err()
            );
        }
        assert!(
            content
                .plan_asset_rename(request(&content, asset, "renamed"), &[], false)
                .is_err()
        );
        assert!(
            content
                .plan_asset_rename(
                    request(&content, asset, "renamed"),
                    &[(source, DraftDocument::Program(Box::new(program)))],
                    true
                )
                .is_err()
        );
        let plan = content
            .plan_asset_rename(request(&content, asset, "renamed"), &[], true)
            .unwrap();
        fs::write(root.path().join("unknown.wgsl"), "unknown includes").unwrap();
        assert!(plan.apply().is_err());
        assert!(
            content
                .plan_asset_rename(request(&content, asset, "renamed"), &[], true)
                .is_err()
        );
        assert!(original.exists());
    }

    #[test]
    fn exclusive_primitive_never_replaces_destination() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        fs::write(&source, "original").unwrap();
        fs::write(&destination, "existing").unwrap();
        assert!(rename_exclusive(&source, &destination).is_err());
        assert_eq!(fs::read_to_string(source).unwrap(), "original");
        assert_eq!(fs::read_to_string(destination).unwrap(), "existing");
    }
}
