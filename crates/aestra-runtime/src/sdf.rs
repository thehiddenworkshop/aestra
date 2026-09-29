//! Signed distance volumes (fluid F11, host bindings HB10): scene geometry as a regular grid of signed
//! distances, negative inside. A host supplies one as the *world SDF* its simulations collide with
//! (see [`crate::AESTRA_RESOURCE_WORLD_SDF`]); level geometry can be baked offline from triangle
//! meshes ([`SdfVolume::bake_triangle_mesh`]) and stored as an asset ([`SdfVolume::to_bytes`], the
//! `.aestra-sdf` format).
//!
//! Engine-neutral: no mesh loader, no GPU. The GPU ABI and its WGSL accessors live in `aestra-gpu`.

/// A grid of signed distances: `dims` voxels, x fastest, voxel `i`'s centre at
/// `origin + (i + ½) · voxel_size`. Negative inside the geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct SdfVolume {
    pub dims: [u32; 3],
    pub origin: [f32; 3],
    pub voxel_size: f32,
    pub distances: Vec<f32>,
}

/// The first bytes of an `.aestra-sdf` file.
pub const SDF_MAGIC: [u8; 4] = *b"ASDF";
/// The `.aestra-sdf` format version this build writes and reads.
pub const SDF_FORMAT_VERSION: u32 = 1;
/// Voxels an SDF volume holds at most (256³).
pub const MAX_SDF_VOXELS: u64 = 1 << 24;

impl SdfVolume {
    /// Voxels in the grid.
    pub fn voxels(&self) -> u64 {
        self.dims.iter().map(|&n| u64::from(n)).product()
    }

    /// Whether the volume is well formed: a non-empty grid within [`MAX_SDF_VOXELS`], a positive
    /// voxel size, finite placement, one distance per voxel, none of them NaN.
    pub fn validate(&self) -> Result<(), String> {
        if self.dims.contains(&0) || self.voxels() > MAX_SDF_VOXELS {
            return Err(format!(
                "an SDF holds between 1 and {MAX_SDF_VOXELS} voxels, got {:?}",
                self.dims
            ));
        }
        if !(self.voxel_size.is_finite() && self.voxel_size > 0.0)
            || !self.origin.iter().all(|axis| axis.is_finite())
        {
            return Err("an SDF's voxel size must be positive and its origin finite".into());
        }
        if self.distances.len() as u64 != self.voxels() {
            return Err(format!(
                "an SDF of {} voxels holds {} distances",
                self.voxels(),
                self.distances.len()
            ));
        }
        if self.distances.iter().any(|d| d.is_nan()) {
            return Err("an SDF's distances must be numbers".into());
        }
        Ok(())
    }

    fn at(&self, cell: [i64; 3]) -> f32 {
        let [nx, ny, _] = self.dims.map(i64::from);
        let index = (cell[2] * ny + cell[1]) * nx + cell[0];
        self.distances[index as usize]
    }

    /// The signed distance at `p`: trilinear between voxel centres inside the grid; outside it, the
    /// value at the nearest point of the grid plus the distance to that point (never less than the
    /// true distance for a volume whose border voxels are outside the geometry). Mirrors
    /// `aestra_world_sdf_distance` in `aestra_gpu::world_sdf`.
    pub fn sample(&self, p: [f32; 3]) -> f32 {
        let mut g = [0.0f32; 3];
        let mut clamped = [0.0f32; 3];
        for axis in 0..3 {
            g[axis] = (p[axis] - self.origin[axis]) / self.voxel_size - 0.5;
            clamped[axis] = g[axis].clamp(0.0, (self.dims[axis] - 1) as f32);
        }
        let base = clamped.map(|v| v.floor());
        let t: Vec<f32> = (0..3).map(|axis| clamped[axis] - base[axis]).collect();
        let b = base.map(|v| v as i64);
        let upper = |axis: usize| (b[axis] + 1).min(i64::from(self.dims[axis]) - 1);
        let corner = |x: bool, y: bool, z: bool| {
            self.at([
                if x { upper(0) } else { b[0] },
                if y { upper(1) } else { b[1] },
                if z { upper(2) } else { b[2] },
            ])
        };
        let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
        let x0 = lerp(
            corner(false, false, false),
            corner(true, false, false),
            t[0],
        );
        let x1 = lerp(corner(false, true, false), corner(true, true, false), t[0]);
        let x2 = lerp(corner(false, false, true), corner(true, false, true), t[0]);
        let x3 = lerp(corner(false, true, true), corner(true, true, true), t[0]);
        let value = lerp(lerp(x0, x1, t[1]), lerp(x2, x3, t[1]), t[2]);
        let outside = (0..3)
            .map(|axis| ((g[axis] - clamped[axis]) * self.voxel_size).powi(2))
            .sum::<f32>()
            .sqrt();
        value + outside
    }

    /// The outward unit normal at `p`: the distance's gradient by central differences half a voxel
    /// apart, zero where it vanishes. Mirrors the GPU's world-SDF normal.
    pub fn normal(&self, p: [f32; 3]) -> [f32; 3] {
        let h = 0.5 * self.voxel_size;
        let difference = |axis: usize| {
            let mut ahead = p;
            let mut behind = p;
            ahead[axis] += h;
            behind[axis] -= h;
            self.sample(ahead) - self.sample(behind)
        };
        let gradient = [difference(0), difference(1), difference(2)];
        let length_squared =
            gradient[0] * gradient[0] + gradient[1] * gradient[1] + gradient[2] * gradient[2];
        if length_squared < 1e-20 {
            return [0.0; 3];
        }
        let length = length_squared.sqrt();
        gradient.map(|component| component / length)
    }

    /// The volume as an `.aestra-sdf` file: the magic, the format version, dims, origin and voxel size,
    /// then the distances, all little-endian.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(36 + self.distances.len() * 4);
        bytes.extend(SDF_MAGIC);
        bytes.extend(SDF_FORMAT_VERSION.to_le_bytes());
        for dim in self.dims {
            bytes.extend(dim.to_le_bytes());
        }
        for axis in self.origin {
            bytes.extend(axis.to_le_bytes());
        }
        bytes.extend(self.voxel_size.to_le_bytes());
        for distance in &self.distances {
            bytes.extend(distance.to_le_bytes());
        }
        bytes
    }

    /// Reads an `.aestra-sdf` file, validating it.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let word = |index: usize| -> Result<[u8; 4], String> {
            bytes
                .get(index * 4..index * 4 + 4)
                .map(|slice| slice.try_into().expect("four bytes"))
                .ok_or_else(|| "the SDF file is truncated".to_string())
        };
        if word(0)? != SDF_MAGIC {
            return Err("not an .aestra-sdf file".into());
        }
        let version = u32::from_le_bytes(word(1)?);
        if version != SDF_FORMAT_VERSION {
            return Err(format!(
                "SDF format version {version} is not supported (this build reads {SDF_FORMAT_VERSION})"
            ));
        }
        let u = |index| word(index).map(u32::from_le_bytes);
        let f = |index| word(index).map(f32::from_le_bytes);
        let dims = [u(2)?, u(3)?, u(4)?];
        let volume_voxels: u64 = dims.iter().map(|&n| u64::from(n)).product();
        if volume_voxels > MAX_SDF_VOXELS {
            return Err(format!("an SDF holds at most {MAX_SDF_VOXELS} voxels"));
        }
        if bytes.len() as u64 != 36 + volume_voxels * 4 {
            return Err(format!(
                "an SDF of {volume_voxels} voxels is {} bytes, the file {}",
                36 + volume_voxels * 4,
                bytes.len()
            ));
        }
        let volume = Self {
            dims,
            origin: [f(5)?, f(6)?, f(7)?],
            voxel_size: f(8)?,
            distances: bytes[36..]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|chunk| f32::from_le_bytes(*chunk))
                .collect(),
        };
        volume.validate()?;
        Ok(volume)
    }

    /// Bakes a triangle mesh: the grid covers the mesh's bounds grown by `padding` on every side,
    /// in voxels of `voxel_size`. Each voxel holds the distance from its centre to the nearest
    /// triangle, negative where the magnitude of the mesh's generalized winding number exceeds ½ —
    /// inside a closed mesh whichever way its triangles wind, and robustly so for small holes. Exact and brute force (every voxel against
    /// every triangle): meant for offline baking and modest runtime bakes.
    pub fn bake_triangle_mesh(
        positions: &[[f32; 3]],
        triangles: &[[u32; 3]],
        voxel_size: f32,
        padding: f32,
    ) -> Result<Self, String> {
        if triangles.is_empty() {
            return Err("an SDF bake needs at least one triangle".into());
        }
        if !(voxel_size.is_finite() && voxel_size > 0.0) || padding.is_nan() || padding < 0.0 {
            return Err(
                "an SDF bake needs a positive voxel size and a non-negative padding".into(),
            );
        }
        let vertex = |index: u32| -> Result<[f32; 3], String> {
            positions
                .get(index as usize)
                .copied()
                .ok_or_else(|| format!("triangle vertex {index} is out of range"))
        };
        let mut tris = Vec::with_capacity(triangles.len());
        for triangle in triangles {
            tris.push([
                vertex(triangle[0])?,
                vertex(triangle[1])?,
                vertex(triangle[2])?,
            ]);
        }
        let mut low = [f32::INFINITY; 3];
        let mut high = [f32::NEG_INFINITY; 3];
        for point in tris.iter().flatten() {
            for axis in 0..3 {
                low[axis] = low[axis].min(point[axis]);
                high[axis] = high[axis].max(point[axis]);
            }
        }
        let origin = low.map(|v| v - padding);
        let dims: [u32; 3] = std::array::from_fn(|axis| {
            (((high[axis] + padding - origin[axis]) / voxel_size).ceil() as u32).max(1)
        });
        let voxels: u64 = dims.iter().map(|&n| u64::from(n)).product();
        if voxels > MAX_SDF_VOXELS {
            return Err(format!(
                "the bake needs {voxels} voxels, more than {MAX_SDF_VOXELS}: use larger voxels"
            ));
        }
        let mut distances = Vec::with_capacity(voxels as usize);
        for z in 0..dims[2] {
            for y in 0..dims[1] {
                for x in 0..dims[0] {
                    let p = [
                        origin[0] + (x as f32 + 0.5) * voxel_size,
                        origin[1] + (y as f32 + 0.5) * voxel_size,
                        origin[2] + (z as f32 + 0.5) * voxel_size,
                    ];
                    let mut nearest = f32::INFINITY;
                    let mut winding = 0.0f64;
                    for tri in &tris {
                        nearest = nearest.min(triangle_distance_squared(p, tri));
                        winding += solid_angle(p, tri);
                    }
                    let distance = nearest.sqrt();
                    let inside = (winding / (4.0 * std::f64::consts::PI)).abs() > 0.5;
                    distances.push(if inside { -distance } else { distance });
                }
            }
        }
        let volume = Self {
            dims,
            origin,
            voxel_size,
            distances,
        };
        volume.validate()?;
        Ok(volume)
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The squared distance from `p` to the triangle `t` (Ericson, *Real-Time Collision Detection*,
/// closest point on a triangle).
fn triangle_distance_squared(p: [f32; 3], t: &[[f32; 3]; 3]) -> f32 {
    let [a, b, c] = *t;
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    let closest = |q: [f32; 3]| {
        let d = sub(p, q);
        dot(d, d)
    };
    let along = |origin: [f32; 3], edge: [f32; 3], t: f32| {
        [
            origin[0] + edge[0] * t,
            origin[1] + edge[1] * t,
            origin[2] + edge[2] * t,
        ]
    };
    if d1 <= 0.0 && d2 <= 0.0 {
        return closest(a);
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return closest(b);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return closest(along(a, ab, d1 / (d1 - d3)));
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return closest(c);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return closest(along(a, ac, d2 / (d2 - d6)));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return closest(along(b, sub(c, b), (d4 - d3) / ((d4 - d3) + (d5 - d6))));
    }
    let denominator = 1.0 / (va + vb + vc);
    let (v, w) = (vb * denominator, vc * denominator);
    closest([
        a[0] + ab[0] * v + ac[0] * w,
        a[1] + ab[1] * v + ac[1] * w,
        a[2] + ab[2] * v + ac[2] * w,
    ])
}

/// The signed solid angle the triangle `t` subtends at `p` (Van Oosterom and Strackee, 1983), in
/// double precision; its sign follows the triangle's winding as seen from `p`.
fn solid_angle(p: [f32; 3], t: &[[f32; 3]; 3]) -> f64 {
    let v = |q: [f32; 3]| {
        [
            f64::from(q[0] - p[0]),
            f64::from(q[1] - p[1]),
            f64::from(q[2] - p[2]),
        ]
    };
    let (a, b, c) = (v(t[0]), v(t[1]), v(t[2]));
    let length = |x: [f64; 3]| (x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt();
    let dot = |x: [f64; 3], y: [f64; 3]| x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
    let cross = |x: [f64; 3], y: [f64; 3]| {
        [
            x[1] * y[2] - x[2] * y[1],
            x[2] * y[0] - x[0] * y[2],
            x[0] * y[1] - x[1] * y[0],
        ]
    };
    let (la, lb, lc) = (length(a), length(b), length(c));
    let numerator = dot(a, cross(b, c));
    let denominator = la * lb * lc + dot(a, b) * lc + dot(b, c) * la + dot(c, a) * lb;
    2.0 * numerator.atan2(denominator)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An axis-aligned box as 12 outward-facing (counter-clockwise from outside) triangles.
    fn box_mesh(low: [f32; 3], high: [f32; 3]) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
        let corner = |i: u32| {
            [
                if i & 1 != 0 { high[0] } else { low[0] },
                if i & 2 != 0 { high[1] } else { low[1] },
                if i & 4 != 0 { high[2] } else { low[2] },
            ]
        };
        let positions = (0..8).map(corner).collect();
        let triangles = vec![
            [0, 2, 1],
            [1, 2, 3], // -z
            [4, 5, 6],
            [5, 7, 6], // +z
            [0, 1, 4],
            [1, 5, 4], // -y
            [2, 6, 3],
            [3, 6, 7], // +y
            [0, 4, 2],
            [2, 4, 6], // -x
            [1, 3, 5],
            [3, 7, 5], // +x
        ];
        (positions, triangles)
    }

    #[test]
    fn a_baked_box_is_negative_inside_and_measures_its_distance_outside() {
        let (positions, triangles) = box_mesh([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]);
        let sdf = SdfVolume::bake_triangle_mesh(&positions, &triangles, 0.25, 1.0).unwrap();
        assert_eq!(sdf.dims, [16, 16, 16]);
        assert!(
            (sdf.sample([0.0, 0.0, 0.0]) + 1.0).abs() < 0.2,
            "the centre is 1 inside"
        );
        assert!(
            (sdf.sample([1.5, 0.0, 0.0]) - 0.5).abs() < 0.05,
            "half a unit off a face"
        );
        assert!(sdf.sample([0.9, 0.9, 0.9]) < 0.0);
        // Beyond the grid, the distance keeps growing.
        assert!(sdf.sample([5.0, 0.0, 0.0]) > 3.5);
        // Wound the other way (clockwise from outside), the box is the same solid.
        let flipped: Vec<[u32; 3]> = triangles.iter().map(|t| [t[0], t[2], t[1]]).collect();
        let inverted = SdfVolume::bake_triangle_mesh(&positions, &flipped, 0.25, 1.0).unwrap();
        assert_eq!(inverted, sdf);
    }

    #[test]
    fn an_sdf_file_round_trips_and_rejects_what_is_not_one() {
        let (positions, triangles) = box_mesh([0.0, 0.0, 0.0], [2.0, 1.0, 1.0]);
        let sdf = SdfVolume::bake_triangle_mesh(&positions, &triangles, 0.5, 0.5).unwrap();
        let bytes = sdf.to_bytes();
        assert_eq!(SdfVolume::from_bytes(&bytes).unwrap(), sdf);
        assert!(SdfVolume::from_bytes(b"nope").is_err());
        assert!(SdfVolume::from_bytes(&bytes[..bytes.len() - 4]).is_err());
        let mut future = bytes.clone();
        future[4] = 2;
        assert!(
            SdfVolume::from_bytes(&future)
                .unwrap_err()
                .contains("version")
        );
    }
}
