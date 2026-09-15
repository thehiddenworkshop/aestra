//! Content-key derivation for the effect thumbnail disk cache (plan M-TC1).
//!
//! Pure, IO-free: given a resolved effect project and a way to look up the file
//! fingerprint of a referenced asset, produce a stable key that changes iff the
//! rendered thumbnail would change. The read/write paths (M-TC2/M-TC3) supply
//! the fingerprints and perform the actual disk access.
//!
use super::EDGE;
use aestra_project::ResolvedEffectProject;
use bevy::{
    camera::{OrthographicProjection, ScalingMode},
    math::{Quat, Vec3},
    transform::components::Transform,
};
use std::{
    fs,
    hash::{Hash, Hasher},
    io::Cursor,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

/// Maximum number of cached thumbnails kept on disk; the startup sweep evicts the
/// oldest beyond this.
const MAX_ENTRIES: usize = 4096;

/// The refined camera framing stored alongside a cached thumbnail so a
/// cache-loaded effect's hover preview matches the static image's size.
pub(super) type Framing = (Transform, OrthographicProjection);

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
            // A glTF mesh reference carries a `#MeshN/PrimitiveN` sub-asset label; the file
            // on disk is the part before `#` (matching how the mesh loader resolves it). Without
            // stripping it, the fingerprint lookup misses and the whole effect is treated as
            // un-fingerprintable, so any mesh-referencing effect is silently never disk-cached.
            let file = asset.path.split('#').next().unwrap_or(&asset.path);
            asset_fingerprints.push(fingerprint_of(&root.join(file))?);
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

/// The refined camera framing, stored as compact primitives (the other
/// projection fields are reconstructed from `OrthographicProjection::default_3d`).
#[derive(serde::Serialize, serde::Deserialize)]
struct CachedFraming {
    translation: [f32; 3],
    rotation: [f32; 4],
    viewport_height: f32,
    near: f32,
    far: f32,
}

impl CachedFraming {
    fn from_framing((transform, projection): &Framing) -> Option<Self> {
        let ScalingMode::FixedVertical { viewport_height } = projection.scaling_mode else {
            return None;
        };
        Some(Self {
            translation: transform.translation.to_array(),
            rotation: transform.rotation.to_array(),
            viewport_height,
            near: projection.near,
            far: projection.far,
        })
    }

    fn into_framing(self) -> Framing {
        (
            Transform {
                translation: Vec3::from_array(self.translation),
                rotation: Quat::from_array(self.rotation),
                scale: Vec3::ONE,
            },
            OrthographicProjection {
                scaling_mode: ScalingMode::FixedVertical {
                    viewport_height: self.viewport_height,
                },
                near: self.near,
                far: self.far,
                ..OrthographicProjection::default_3d()
            },
        )
    }
}

/// Loads the framing stored beside a cached thumbnail, or `None` if absent.
pub(super) fn read_framing(key: &str) -> Option<Framing> {
    let path = cache_root()?.join(format!("{key}.ron"));
    let cached: CachedFraming = ron::from_str(&fs::read_to_string(&path).ok()?).ok()?;
    Some(cached.into_framing())
}

/// Persists a rendered thumbnail's RGBA pixels as PNG (plus its framing as a
/// `.ron` sidecar), best-effort. Writes each via a temp file + rename so a reader
/// never sees a torn file; any error is ignored (the in-memory cache still holds
/// the pixels this session).
pub(super) fn write(key: &str, rgba: &[u8], framing: Option<&Framing>) {
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
    write_atomic(&dir, key, "png", &png);

    if let Some(cached) = framing.and_then(CachedFraming::from_framing)
        && let Ok(ron) = ron::to_string(&cached)
    {
        write_atomic(&dir, key, "ron", ron.as_bytes());
    }
}

fn write_atomic(dir: &Path, key: &str, extension: &str, bytes: &[u8]) {
    let temp = dir.join(format!("{key}.{extension}.tmp"));
    if fs::write(&temp, bytes).is_ok() {
        let _ = fs::rename(&temp, dir.join(format!("{key}.{extension}")));
    }
}

/// Evicts the oldest cached thumbnails beyond `MAX_ENTRIES` and clears any stale
/// temp files. Best-effort; intended to run once at startup on a worker thread.
pub(super) fn sweep() {
    let Some(dir) = cache_root() else {
        return;
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    let mut pngs: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("tmp") => {
                let _ = fs::remove_file(&path);
            }
            Some("png") => {
                let mtime = entry
                    .metadata()
                    .and_then(|meta| meta.modified())
                    .unwrap_or(UNIX_EPOCH);
                pngs.push((path, mtime));
            }
            _ => {}
        }
    }
    if pngs.len() <= MAX_ENTRIES {
        return;
    }
    pngs.sort_by_key(|(_, mtime)| *mtime);
    let excess = pngs.len() - MAX_ENTRIES;
    for (path, _) in pngs.into_iter().take(excess) {
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(path.with_extension("ron"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_core::{AssetDefinition, AssetId, AssetKind, EffectAsset};
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
        assert_eq!(key(&project, |_| Some(0)), key(&project, |_| Some(0)),);
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
    fn mesh_gltf_sub_asset_fragment_is_stripped_before_the_fingerprint_lookup() {
        // A glTF mesh reference carries a `#MeshN/PrimitiveN` label; the file on disk is the
        // part before `#`. If the fragment is not stripped, the lookup misses and the effect is
        // treated as un-fingerprintable, so it is silently never disk-cached (the original bug).
        let mut project = sample();
        project.root.assets.push(AssetDefinition {
            id: AssetId::new(),
            name: "Lab Cube".into(),
            kind: AssetKind::Mesh,
            path: "meshes/lab_cube.gltf#Mesh0/Primitive0".into(),
        });
        let key = key(&project, |path| {
            // The lookup must receive the real file, without the sub-asset fragment.
            assert!(
                !path.to_string_lossy().contains('#'),
                "the glTF fragment must be stripped before the file lookup"
            );
            assert!(path.ends_with("meshes/lab_cube.gltf"));
            Some(7)
        });
        assert!(
            key.is_some(),
            "a mesh-referencing effect must still produce a cache key"
        );
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

    #[test]
    fn framing_round_trips_through_ron() {
        let framing = (
            Transform {
                translation: Vec3::new(1.0, 2.0, 3.0),
                rotation: Quat::from_array([0.1, 0.2, 0.3, 0.4]),
                scale: Vec3::ONE,
            },
            OrthographicProjection {
                scaling_mode: ScalingMode::FixedVertical {
                    viewport_height: 4.5,
                },
                near: 0.01,
                far: 12.0,
                ..OrthographicProjection::default_3d()
            },
        );
        let ron = ron::to_string(&CachedFraming::from_framing(&framing).unwrap()).unwrap();
        let (transform, projection) = ron::from_str::<CachedFraming>(&ron).unwrap().into_framing();
        assert_eq!(transform.translation.to_array(), [1.0, 2.0, 3.0]);
        assert_eq!(transform.rotation.to_array(), [0.1, 0.2, 0.3, 0.4]);
        assert_eq!(projection.near, 0.01);
        assert_eq!(projection.far, 12.0);
        assert!(matches!(
            projection.scaling_mode,
            ScalingMode::FixedVertical { viewport_height } if viewport_height == 4.5
        ));
    }
}
