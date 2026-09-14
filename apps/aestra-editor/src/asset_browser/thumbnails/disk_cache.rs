//! Content-key derivation for the effect thumbnail disk cache (plan M-TC1).
//!
//! Pure, IO-free: given a resolved effect project and a way to look up the file
//! fingerprint of a referenced asset, produce a stable key that changes iff the
//! rendered thumbnail would change. The read/write paths (M-TC2/M-TC3) supply
//! the fingerprints and perform the actual disk access.
//!
use super::EDGE;
use aestra_project::ResolvedEffectProject;
use std::{
    fs,
    hash::{Hash, Hasher},
    io::Cursor,
    path::{Path, PathBuf},
};

/// Bump to invalidate every cached entry when the on-disk schema changes.
const CACHE_FORMAT_VERSION: u32 = 1;
/// Bump when the GPU render or framing path changes the produced pixels.
const RENDERER_VERSION: u32 = 1;

/// The content identity of an effect's rendered thumbnail.
///
/// Folds together everything that affects the rendered pixels:
/// - the parsed content of the root effect, its dependency effects, and the
///   material programs (already resolved and path-independent — `aestra-core`
///   uses no `HashMap`, so their serialization is deterministic), and
/// - the file fingerprint of every referenced texture and mesh.
///
/// Material *functions* are excluded: background previews do not run function
/// calls (`effect::prepare` clears them), so they never affect the render.
///
/// Returns `None` if any referenced asset's fingerprint is unavailable, so the
/// caller treats it as a cache miss and never writes a key that might not
/// reflect the real inputs (a stale hit is worse than a re-render).
pub(super) fn resolved_fingerprint(
    resolved: &ResolvedEffectProject,
    root: &Path,
    fingerprint_of: impl Fn(&Path) -> Option<u64>,
) -> Option<u64> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();

    serialize_hash(&resolved.root, &mut hasher)?;
    serialize_hash(&resolved.dependencies, &mut hasher)?;
    serialize_hash(&resolved.material_programs, &mut hasher)?;

    // File fingerprints of every referenced texture/mesh, order-independent.
    let mut asset_fingerprints = Vec::new();
    for effect in std::iter::once(&resolved.root).chain(resolved.dependencies.values()) {
        for asset in &effect.assets {
            asset_fingerprints.push(fingerprint_of(&root.join(&asset.path))?);
        }
    }
    asset_fingerprints.sort_unstable();
    asset_fingerprints.hash(&mut hasher);

    Some(hasher.finish())
}

fn serialize_hash<T: serde::Serialize>(value: &T, hasher: &mut impl Hasher) -> Option<()> {
    ron::to_string(value).ok()?.hash(hasher);
    Some(())
}

/// The on-disk filename stem for an effect thumbnail. Combines the content
/// fingerprint with the format/renderer versions so either kind of bump
/// invalidates every entry.
pub(super) fn cache_key(content_fingerprint: u64) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (CACHE_FORMAT_VERSION, RENDERER_VERSION, content_fingerprint).hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// User-level cache directory for baked thumbnails (kept out of the project so
/// it survives `git clean` and is shared across projects). `None` if the OS
/// cache location cannot be resolved.
fn cache_root() -> Option<PathBuf> {
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Caches"))
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
    }?;
    Some(base.join("aestra").join("thumbnails"))
}

/// Loads a cached thumbnail's `EDGE`×`EDGE` RGBA pixels, or `None` on any miss,
/// IO error, decode failure, or unexpected dimensions (all treated as a miss).
pub(super) fn read(key: &str) -> Option<Vec<u8>> {
    let path = cache_root()?.join(format!("{key}.png"));
    let image = image::load_from_memory(&fs::read(&path).ok()?)
        .ok()?
        .to_rgba8();
    (image.width() == EDGE && image.height() == EDGE).then(|| image.into_raw())
}

/// Persists a rendered thumbnail's RGBA pixels as PNG, best-effort. Writes to a
/// temp file and renames so a reader never sees a torn file; any error is
/// ignored (the in-memory cache still holds the pixels this session).
pub(super) fn write(key: &str, rgba: &[u8]) {
    if rgba.len() != (EDGE * EDGE * 4) as usize {
        return;
    }
    let Some(dir) = cache_root() else {
        return;
    };
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let Some(image) = image::RgbaImage::from_raw(EDGE, EDGE, rgba.to_vec()) else {
        return;
    };
    let mut png = Vec::new();
    if image
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .is_err()
    {
        return;
    }
    let temp = dir.join(format!("{key}.png.tmp"));
    if fs::write(&temp, &png).is_ok() {
        let _ = fs::rename(&temp, dir.join(format!("{key}.png")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_core::{AssetDefinition, EffectAsset};
    use std::collections::BTreeMap;

    fn sample() -> ResolvedEffectProject {
        ResolvedEffectProject {
            root: EffectAsset::new("Sample", 2.0),
            dependencies: BTreeMap::new(),
            material_programs: BTreeMap::new(),
            material_functions: BTreeMap::new(),
        }
    }

    fn key(project: &ResolvedEffectProject, fp: impl Fn(&Path) -> Option<u64>) -> Option<String> {
        resolved_fingerprint(project, Path::new("/root"), fp).map(cache_key)
    }

    #[test]
    fn identical_input_yields_identical_key() {
        let project = sample();
        assert_eq!(
            key(&project, |_| Some(0)),
            key(&project, |_| Some(0)),
        );
    }

    #[test]
    fn effect_content_change_changes_key() {
        let base = sample();
        let mut modified = base.clone();
        modified.root.duration = 3.0;
        assert_ne!(key(&base, |_| Some(0)), key(&modified, |_| Some(0)));
    }

    #[test]
    fn referenced_texture_fingerprint_change_changes_key() {
        let mut project = sample();
        project
            .root
            .assets
            .push(AssetDefinition::texture("spark", "textures/spark.png"));
        assert_ne!(key(&project, |_| Some(1)), key(&project, |_| Some(2)));
    }

    #[test]
    fn missing_asset_fingerprint_yields_no_key() {
        let mut project = sample();
        project
            .root
            .assets
            .push(AssetDefinition::texture("spark", "textures/spark.png"));
        assert!(key(&project, |_| None).is_none());
    }

    #[test]
    fn version_bump_is_folded_into_the_key() {
        // Different content fingerprints must not collide through the key.
        assert_ne!(cache_key(1), cache_key(2));
    }
}
