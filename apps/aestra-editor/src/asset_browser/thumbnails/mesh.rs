//! Bounded static geometry previews. Reuses glTF's parser without its unbounded import
//! path or AssetServer scene spawning; no external images/materials are loaded.
use super::*;
mod primitive;
use base64::{Engine, engine::general_purpose::STANDARD};
pub(super) use primitive::load as load_primitive;

const TRIANGLES: usize = 20_000;
const VERTICES: usize = 100_000;
const BUFFER_BYTES: usize = 32 * 1024 * 1024;
const PIXEL_TESTS: usize = 4_000_000;
const BACKGROUND: [u8; 4] = [24, 26, 33, 255];

fn load_document(
    root: &Path,
    relative: &Path,
    flag: &AtomicBool,
) -> Result<(gltf::Gltf, Vec<Vec<u8>>), String> {
    if !relative
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("gltf") || ext.eq_ignore_ascii_case("glb"))
    {
        return Err("Mesh previews support glTF and GLB".into());
    }
    let source = read_source(root, relative, flag)?;
    let document = gltf::Gltf::from_slice(&source).map_err(|e| format!("Invalid glTF/GLB: {e}"))?;
    if document.nodes().len() > 512
        || document.meshes().len() > 256
        || document.accessors().len() > 2048
        || document.views().len() > 2048
        || document.buffers().len() > 64
    {
        return Err("Mesh preview exceeds the document complexity limit".into());
    }
    let mut buffers = Vec::new();
    let mut total = 0;
    for buffer in document.buffers() {
        check_cancelled(flag)?;
        if buffer.length() > FILE_LIMIT as usize {
            return Err("Mesh buffer exceeds 16 MiB".into());
        }
        let data = match buffer.source() {
            gltf::buffer::Source::Bin => document
                .blob
                .as_ref()
                .ok_or("GLB buffer is missing")?
                .clone(),
            gltf::buffer::Source::Uri(uri) if uri.starts_with("data:") => {
                let (header, data) = uri.split_once(',').ok_or("Invalid buffer data URI")?;
                if !matches!(
                    header,
                    "data:application/octet-stream;base64" | "data:application/gltf-buffer;base64"
                ) {
                    return Err("Unsupported mesh buffer data URI".into());
                }
                if data.len() > (FILE_LIMIT as usize).div_ceil(3) * 4 {
                    return Err("Mesh buffer exceeds 16 MiB".into());
                }
                STANDARD.decode(data).map_err(|e| e.to_string())?
            }
            gltf::buffer::Source::Uri(uri) => {
                read_source(root, &buffer_path(relative, uri)?, flag)?
            }
        };
        total += data.len();
        if total > BUFFER_BYTES {
            return Err("Mesh preview buffer budget is 32 MiB".into());
        }
        if data.len() < buffer.length() {
            return Err("Mesh buffer is truncated".into());
        }
        buffers.push(data);
    }
    Ok((document, buffers))
}

pub(super) fn render(root: &Path, relative: &Path, flag: &AtomicBool) -> Result<Vec<u8>, String> {
    let (document, buffers) = load_document(root, relative, flag)?;
    let mut geometry = Geometry {
        triangles: Vec::new(),
        vertices: 0,
        nodes: 0,
        flag,
        buffers: &buffers,
    };
    if let Some(scene) = document
        .default_scene()
        .or_else(|| document.scenes().next())
    {
        for node in scene.nodes() {
            geometry.node(node, Mat4::IDENTITY, &mut BTreeSet::new())?;
        }
    } else {
        // Mesh-only documents have no scene transforms; preview all mesh definitions once.
        for mesh in document.meshes() {
            geometry.mesh(mesh, Mat4::IDENTITY)?;
        }
    }
    rasterize(&geometry.triangles, flag)
}

fn buffer_path(source: &Path, uri: &str) -> Result<PathBuf, String> {
    let mut decoded = Vec::new();
    let mut bytes = uri.as_bytes().iter().copied();
    while let Some(byte) = bytes.next() {
        decoded.push(if byte == b'%' {
            let high = bytes
                .next()
                .and_then(|v| (v as char).to_digit(16))
                .ok_or("Invalid buffer URI escape")?;
            let low = bytes
                .next()
                .and_then(|v| (v as char).to_digit(16))
                .ok_or("Invalid buffer URI escape")?;
            (high * 16 + low) as u8
        } else {
            byte
        });
    }
    let decoded = String::from_utf8(decoded).map_err(|e| e.to_string())?;
    if decoded.contains([':', '\\', '\0', '?', '#']) {
        return Err("Only local relative mesh buffers are supported".into());
    }
    let mut path = source.parent().unwrap_or(Path::new("")).to_owned();
    for component in Path::new(&decoded).components() {
        match component {
            PathComponent::Normal(value) => path.push(value),
            PathComponent::CurDir => {}
            PathComponent::ParentDir if path.pop() => {}
            _ => return Err("Mesh buffer escapes the project".into()),
        }
    }
    Ok(path)
}

// Validate accessors ourselves before slicing. Sparse/compressed geometry is explicitly
// unsupported; a malformed accessor must fail, never panic in a loader iterator.
fn elements<'a>(
    accessor: &gltf::Accessor<'_>,
    buffers: &'a [Vec<u8>],
    size: usize,
) -> Result<impl Iterator<Item = &'a [u8]>, String> {
    if accessor.sparse().is_some() {
        return Err("Sparse mesh accessors are not previewed".into());
    }
    let view = accessor.view().ok_or("Missing mesh buffer view")?;
    let buffer = buffers
        .get(view.buffer().index())
        .ok_or("Missing mesh buffer")?;
    let stride = view.stride().unwrap_or(size);
    if stride < size {
        return Err("Invalid mesh accessor stride".into());
    }
    let start = view
        .offset()
        .checked_add(accessor.offset())
        .ok_or("Invalid mesh offset")?;
    let end = accessor
        .count()
        .saturating_sub(1)
        .checked_mul(stride)
        .and_then(|n| n.checked_add(size))
        .and_then(|n| start.checked_add(n))
        .ok_or("Invalid mesh accessor length")?;
    let view_end = view
        .offset()
        .checked_add(view.length())
        .ok_or("Invalid mesh view length")?;
    if end > view_end || view_end > buffer.len() || view_end > view.buffer().length() {
        return Err("Mesh accessor is outside its buffer".into());
    }
    Ok((0..accessor.count()).map(move |i| &buffer[start + i * stride..start + i * stride + size]))
}

struct Geometry<'a> {
    triangles: Vec<[Vec3; 3]>,
    vertices: usize,
    nodes: usize,
    buffers: &'a [Vec<u8>],
    flag: &'a AtomicBool,
}
impl Geometry<'_> {
    fn node(
        &mut self,
        node: gltf::Node<'_>,
        parent: Mat4,
        active: &mut BTreeSet<usize>,
    ) -> Result<(), String> {
        check_cancelled(self.flag)?;
        self.nodes += 1;
        if self.nodes > 512 || active.len() >= 64 || !active.insert(node.index()) {
            return Err("Mesh scene exceeds node/depth limits or contains a cycle".into());
        }
        if node.skin().is_some() {
            return Err("Skinned meshes need an animated scene preview".into());
        }
        let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        if !transform.is_finite() {
            return Err("Non-finite mesh transform".into());
        }
        if let Some(mesh) = node.mesh() {
            self.mesh(mesh, transform)?;
        }
        for child in node.children() {
            self.node(child, transform, active)?;
        }
        active.remove(&node.index());
        Ok(())
    }

    fn mesh(&mut self, mesh: gltf::Mesh<'_>, transform: Mat4) -> Result<(), String> {
        for primitive in mesh.primitives() {
            check_cancelled(self.flag)?;
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                return Err("Only triangle mesh geometry is previewed".into());
            }
            if primitive.morph_targets().next().is_some() {
                return Err("Morph targets need an animated scene preview".into());
            }
            let positions = primitive
                .get(&gltf::Semantic::Positions)
                .ok_or("Missing mesh positions")?;
            self.vertices = self.vertices.saturating_add(positions.count());
            if self.vertices > VERTICES {
                return Err("Mesh preview limit: 100,000 vertices".into());
            }
            if positions.data_type() != gltf::accessor::DataType::F32
                || positions.dimensions() != gltf::accessor::Dimensions::Vec3
            {
                return Err("Mesh positions must be float3".into());
            }
            let vertices = elements(&positions, self.buffers, 12)?
                .map(|bytes| {
                    let point = Vec3::from_array(std::array::from_fn(|i| {
                        f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                    }));
                    transform.transform_point3(point)
                })
                .collect::<Vec<_>>();
            if vertices.iter().any(|v| !v.is_finite()) {
                return Err("Non-finite mesh positions".into());
            }
            let indices = if let Some(accessor) = primitive.indices() {
                if accessor.count() > TRIANGLES * 3
                    || accessor.dimensions() != gltf::accessor::Dimensions::Scalar
                {
                    return Err("Mesh preview index budget exceeded".into());
                }
                let size = match accessor.data_type() {
                    gltf::accessor::DataType::U8 => 1,
                    gltf::accessor::DataType::U16 => 2,
                    gltf::accessor::DataType::U32 => 4,
                    _ => return Err("Invalid mesh index type".into()),
                };
                elements(&accessor, self.buffers, size)?
                    .map(|bytes| {
                        let mut value = [0u8; 4];
                        value[..size].copy_from_slice(bytes);
                        u32::from_le_bytes(value) as usize
                    })
                    .collect::<Vec<_>>()
            } else {
                (0..vertices.len()).collect()
            };
            if !indices.len().is_multiple_of(3)
                || self.triangles.len() + indices.len() / 3 > TRIANGLES
            {
                return Err("Mesh preview limit: 20,000 triangles".into());
            }
            for indices in indices.as_chunks::<3>().0 {
                let mut triangle = [Vec3::ZERO; 3];
                for (point, index) in triangle.iter_mut().zip(indices) {
                    *point = *vertices
                        .get(*index)
                        .ok_or("Mesh index is outside positions")?;
                }
                self.triangles.push(triangle);
            }
        }
        Ok(())
    }
}

fn rasterize(triangles: &[[Vec3; 3]], flag: &AtomicBool) -> Result<Vec<u8>, String> {
    check_cancelled(flag)?;
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for point in triangles.iter().flatten() {
        min = min.min(*point);
        max = max.max(*point);
    }
    let extent = max - min;
    if !extent.is_finite() || extent.max_element() <= 0.0 {
        return Err("Mesh has no visible geometry".into());
    }
    let center = min + extent * 0.5;
    let forward = Vec3::new(1.6, 1.2, 2.0).normalize();
    let right = Vec3::Y.cross(forward).normalize();
    let up = forward.cross(right);
    let project = |point: Vec3| {
        let p = (point - center) / extent.max_element();
        Vec3::new(p.dot(right), p.dot(up), p.dot(forward))
    };
    let mut bounds = Vec2::ZERO;
    for point in triangles.iter().flatten() {
        bounds = bounds.max(project(*point).truncate().abs());
    }
    let scale = EDGE as f32 * 0.42 / bounds.max_element().max(0.001);
    let screen = |point| {
        let p = project(point);
        Vec3::new(
            EDGE as f32 * 0.5 + p.x * scale,
            EDGE as f32 * 0.5 - p.y * scale,
            p.z,
        )
    };
    let mut pixels = BACKGROUND.repeat((EDGE * EDGE) as usize);
    let mut depth = vec![f32::NEG_INFINITY; (EDGE * EDGE) as usize];
    let mut tests = 0;
    let mut drawn = false;
    let edge = |a: Vec3, b: Vec3, p: Vec2| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
    for triangle in triangles {
        check_cancelled(flag)?;
        let [a, b, c] = triangle.map(screen);
        let area = edge(a, b, c.truncate());
        if area.abs() < 0.0001 {
            continue;
        }
        let normal = ((triangle[1] - triangle[0]) / extent.max_element())
            .cross((triangle[2] - triangle[0]) / extent.max_element())
            .normalize_or_zero();
        let shade = 0.25 + 0.75 * normal.dot(Vec3::new(0.4, 0.8, 0.6).normalize()).abs();
        let color = [155.0, 186.0, 218.0].map(|v| (v * shade).round() as u8);
        let lo = a.min(b).min(c).truncate().floor().max(Vec2::ZERO);
        let hi = a
            .max(b)
            .max(c)
            .truncate()
            .ceil()
            .min(Vec2::splat((EDGE - 1) as f32));
        for y in lo.y as u32..=hi.y as u32 {
            check_cancelled(flag)?;
            for x in lo.x as u32..=hi.x as u32 {
                tests += 1;
                if tests > PIXEL_TESTS {
                    return Err("Mesh preview raster work budget exceeded".into());
                }
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let weights = Vec3::new(edge(b, c, p), edge(c, a, p), edge(a, b, p)) / area;
                if weights.min_element() < 0.0 {
                    continue;
                }
                let z = weights.dot(Vec3::new(a.z, b.z, c.z));
                let offset = (y * EDGE + x) as usize;
                if z > depth[offset] {
                    depth[offset] = z;
                    pixels[offset * 4..offset * 4 + 3].copy_from_slice(&color);
                    drawn = true;
                }
            }
        }
    }
    if !drawn {
        return Err("Mesh has no visible triangles".into());
    }
    Ok(pixels)
}

#[cfg(test)]
mod tests;
