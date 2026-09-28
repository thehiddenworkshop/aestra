//! The world-SDF ABI (fluid F11, host bindings HB10): the host's scene geometry, an
//! [`aestra_runtime::SdfVolume`], packed into the `array<u32>` a stage reads as
//! [`aestra_runtime::AESTRA_RESOURCE_WORLD_SDF`]:
//!
//! - word 0: 1 when a world is supplied, 0 when absent (everything then reads as far outside);
//! - words 1..4: the grid's dims; words 4..7: its origin; word 7: its voxel size (floats as bits);
//! - from word 8: the distances, x fastest.

use std::sync::Arc;

/// Words before the distances.
pub const WORLD_SDF_HEADER_WORDS: usize = 8;

/// A world SDF packed for upload, with the host's revision of it: an executor uploads it again only
/// when the revision changes, so a static world costs nothing per tick.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuWorldSdf {
    pub revision: u64,
    pub words: Arc<[u32]>,
}

impl GpuWorldSdf {
    /// Packs `volume` as revision `revision`.
    pub fn new(volume: &aestra_runtime::SdfVolume, revision: u64) -> Self {
        let mut words = Vec::with_capacity(WORLD_SDF_HEADER_WORDS + volume.distances.len());
        words.push(1);
        words.extend(volume.dims);
        words.extend(volume.origin.map(f32::to_bits));
        words.push(volume.voxel_size.to_bits());
        words.extend(volume.distances.iter().map(|distance| distance.to_bits()));
        Self {
            revision,
            words: words.into(),
        }
    }

    /// No world: a header saying so. Revision 0.
    pub fn absent() -> Self {
        Self {
            revision: 0,
            words: vec![0; WORLD_SDF_HEADER_WORDS].into(),
        }
    }

    /// Size of the storage buffer in bytes.
    pub fn byte_len(&self) -> u64 {
        (self.words.len() * 4) as u64
    }

    /// Little-endian bytes for upload.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.words
            .iter()
            .flat_map(|word| word.to_le_bytes())
            .collect()
    }
}

/// WGSL accessors for the world SDF. The consuming shader declares the buffer as
/// `var<storage, read> aestra_world_sdf: array<u32>;` at the group/binding it chooses, then appends
/// these functions. Positions and distances are in world space.
pub const WORLD_SDF_WGSL: &str = r#"
// Aestra world SDF (fluid F11, host bindings HB10).
const AESTRA_WORLD_SDF_HEADER: u32 = 8u;
const AESTRA_WORLD_SDF_FAR: f32 = 1.0e30;

fn aestra_world_sdf_present() -> bool {
    return aestra_world_sdf[0] != 0u;
}

fn aestra_world_sdf_voxel_size() -> f32 {
    return bitcast<f32>(aestra_world_sdf[7]);
}

fn aestra_world_sdf_dims() -> vec3<i32> {
    return vec3<i32>(i32(aestra_world_sdf[1]), i32(aestra_world_sdf[2]), i32(aestra_world_sdf[3]));
}

fn aestra_world_sdf_voxel(cell: vec3<i32>) -> f32 {
    let dims = aestra_world_sdf_dims();
    let c = clamp(cell, vec3<i32>(0), dims - vec3<i32>(1));
    let index = u32((c.z * dims.y + c.y) * dims.x + c.x);
    return bitcast<f32>(aestra_world_sdf[AESTRA_WORLD_SDF_HEADER + index]);
}

// The signed distance at world point `p` (negative inside): trilinear between voxel centres; outside
// the grid, the value at its nearest point plus the distance to it; far when no world is supplied.
// Mirrors aestra_runtime::SdfVolume::sample.
fn aestra_world_sdf_distance(p: vec3<f32>) -> f32 {
    if (!aestra_world_sdf_present()) {
        return AESTRA_WORLD_SDF_FAR;
    }
    let origin = vec3<f32>(
        bitcast<f32>(aestra_world_sdf[4]),
        bitcast<f32>(aestra_world_sdf[5]),
        bitcast<f32>(aestra_world_sdf[6]),
    );
    let voxel = bitcast<f32>(aestra_world_sdf[7]);
    let g = (p - origin) / voxel - vec3<f32>(0.5);
    let clamped = clamp(g, vec3<f32>(0.0), vec3<f32>(aestra_world_sdf_dims() - vec3<i32>(1)));
    let base = floor(clamped);
    let t = clamped - base;
    let b = vec3<i32>(base);
    let x0 = mix(aestra_world_sdf_voxel(b), aestra_world_sdf_voxel(b + vec3<i32>(1, 0, 0)), t.x);
    let x1 = mix(aestra_world_sdf_voxel(b + vec3<i32>(0, 1, 0)), aestra_world_sdf_voxel(b + vec3<i32>(1, 1, 0)), t.x);
    let x2 = mix(aestra_world_sdf_voxel(b + vec3<i32>(0, 0, 1)), aestra_world_sdf_voxel(b + vec3<i32>(1, 0, 1)), t.x);
    let x3 = mix(aestra_world_sdf_voxel(b + vec3<i32>(0, 1, 1)), aestra_world_sdf_voxel(b + vec3<i32>(1, 1, 1)), t.x);
    let value = mix(mix(x0, x1, t.y), mix(x2, x3, t.y), t.z);
    return value + length((g - clamped) * voxel);
}

// The outward unit normal at world point `p`: the distance's gradient by central differences half a
// voxel apart (zero where it vanishes or no world is supplied).
fn aestra_world_sdf_normal(p: vec3<f32>) -> vec3<f32> {
    if (!aestra_world_sdf_present()) {
        return vec3<f32>(0.0);
    }
    let h = 0.5 * bitcast<f32>(aestra_world_sdf[7]);
    let gradient = vec3<f32>(
        aestra_world_sdf_distance(p + vec3<f32>(h, 0.0, 0.0)) - aestra_world_sdf_distance(p - vec3<f32>(h, 0.0, 0.0)),
        aestra_world_sdf_distance(p + vec3<f32>(0.0, h, 0.0)) - aestra_world_sdf_distance(p - vec3<f32>(0.0, h, 0.0)),
        aestra_world_sdf_distance(p + vec3<f32>(0.0, 0.0, h)) - aestra_world_sdf_distance(p - vec3<f32>(0.0, 0.0, h)),
    );
    let length_squared = dot(gradient, gradient);
    if (length_squared < 1e-20) {
        return vec3<f32>(0.0);
    }
    return gradient / sqrt(length_squared);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_world_sdf_packs_its_header_and_the_accessors_validate() {
        let volume = aestra_runtime::SdfVolume {
            dims: [2, 1, 1],
            origin: [-1.0, 0.0, 0.0],
            voxel_size: 1.0,
            distances: vec![-0.5, 0.5],
        };
        let world = GpuWorldSdf::new(&volume, 7);
        assert_eq!(
            world.words[..WORLD_SDF_HEADER_WORDS],
            [1, 2, 1, 1, (-1.0f32).to_bits(), 0, 0, 1.0f32.to_bits()]
        );
        assert_eq!(world.words[WORLD_SDF_HEADER_WORDS], (-0.5f32).to_bits());
        assert_eq!(world.byte_len(), 40);
        assert_eq!(GpuWorldSdf::absent().words[0], 0);

        let source = format!(
            "@group(0) @binding(0) var<storage, read> aestra_world_sdf: array<u32>;\n\
             @group(0) @binding(1) var<storage, read_write> out: array<f32>;\n\
             {WORLD_SDF_WGSL}\n\
             @compute @workgroup_size(1)\n\
             fn probe() {{\n    \
             out[0] = aestra_world_sdf_distance(vec3<f32>(0.0));\n    \
             out[1] = aestra_world_sdf_normal(vec3<f32>(0.0)).x;\n}}"
        );
        let module = naga::front::wgsl::parse_str(&source).expect("parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .expect("validates");
    }
}
