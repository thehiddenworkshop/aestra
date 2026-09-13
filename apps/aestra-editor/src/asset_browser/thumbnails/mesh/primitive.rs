//! The exact static primitive referenced by a mesh renderer, with bounded vertex attributes.
use super::*;

pub(in super::super) fn load(
    root: &Path,
    reference: &str,
    flag: &AtomicBool,
) -> Result<(Mesh, f32), String> {
    let (file, label) = reference
        .split_once('#')
        .map_or((reference, None), |(file, label)| (file, Some(label)));
    let (document, buffers) = load_document(root, Path::new(file), flag)?;
    let (mesh_index, primitive_index) = if let Some(label) = label {
        let (mesh, primitive) = label.split_once('/').ok_or("Expected #MeshN/PrimitiveN")?;
        (
            mesh.strip_prefix("Mesh")
                .ok_or("Expected MeshN")?
                .parse::<usize>()
                .map_err(|_| "Invalid mesh index")?,
            primitive
                .strip_prefix("Primitive")
                .ok_or("Expected PrimitiveN")?
                .parse::<usize>()
                .map_err(|_| "Invalid primitive index")?,
        )
    } else if document.meshes().len() == 1
        && document.meshes().next().unwrap().primitives().len() == 1
    {
        (0, 0)
    } else {
        return Err("Mesh renderer needs an explicit #MeshN/PrimitiveN label".into());
    };
    let primitive = document
        .meshes()
        .nth(mesh_index)
        .ok_or("Missing mesh")?
        .primitives()
        .nth(primitive_index)
        .ok_or("Missing mesh primitive")?;
    if primitive.mode() != gltf::mesh::Mode::Triangles || primitive.morph_targets().next().is_some()
    {
        return Err("Effect thumbnails require static triangle mesh primitives".into());
    }
    let positions = attribute::<3>(&primitive, gltf::Semantic::Positions, &buffers)?
        .ok_or("Missing mesh positions")?;
    let normals = attribute::<3>(&primitive, gltf::Semantic::Normals, &buffers)?;
    let uv = attribute::<2>(&primitive, gltf::Semantic::TexCoords(0), &buffers)?;
    let uv1 = attribute::<2>(&primitive, gltf::Semantic::TexCoords(1), &buffers)?;
    let tangents = attribute::<4>(&primitive, gltf::Semantic::Tangents, &buffers)?;
    // Keep unsupported encoded attributes explicit instead of silently misrepresenting a material.
    let colors = if let Some(accessor) = primitive.get(&gltf::Semantic::Colors(0)) {
        if accessor.dimensions() == gltf::accessor::Dimensions::Vec3 {
            attribute::<3>(&primitive, gltf::Semantic::Colors(0), &buffers)?.map(|colors| {
                colors
                    .into_iter()
                    .map(|c| [c[0], c[1], c[2], 1.0])
                    .collect::<Vec<_>>()
            })
        } else {
            attribute::<4>(&primitive, gltf::Semantic::Colors(0), &buffers)?
        }
    } else {
        None
    };
    if normals.as_ref().is_some_and(|a| a.len() != positions.len())
        || uv.as_ref().is_some_and(|a| a.len() != positions.len())
        || uv1.as_ref().is_some_and(|a| a.len() != positions.len())
        || tangents
            .as_ref()
            .is_some_and(|a| a.len() != positions.len())
        || colors.as_ref().is_some_and(|a| a.len() != positions.len())
    {
        return Err("Mesh attribute counts disagree".into());
    }
    let indices: Vec<usize> = if let Some(accessor) = primitive.indices() {
        if accessor.count() > TRIANGLES * 3
            || accessor.dimensions() != gltf::accessor::Dimensions::Scalar
        {
            return Err("Mesh index budget exceeded".into());
        }
        let size = match accessor.data_type() {
            gltf::accessor::DataType::U8 => 1,
            gltf::accessor::DataType::U16 => 2,
            gltf::accessor::DataType::U32 => 4,
            _ => return Err("Invalid mesh indices".into()),
        };
        elements(&accessor, &buffers, size)?
            .map(|bytes| {
                let mut value = [0; 4];
                value[..size].copy_from_slice(bytes);
                u32::from_le_bytes(value) as usize
            })
            .collect()
    } else {
        (0..positions.len()).collect()
    };
    if indices.is_empty()
        || !indices.len().is_multiple_of(3)
        || indices.len() > TRIANGLES * 3
        || indices.iter().any(|i| *i >= positions.len())
    {
        return Err("Invalid or oversized mesh triangles".into());
    }
    let mut p = Vec::new();
    let mut n = Vec::new();
    let mut t = Vec::new();
    let mut c = Vec::new();
    let mut t1 = Vec::new();
    let mut tangent = Vec::new();
    let mut radius = 0.0f32;
    for triangle in indices.as_chunks::<3>().0 {
        check_cancelled(flag)?;
        let points = triangle.map(|i| Vec3::from_array(positions[i]));
        let normal = (points[1] - points[0])
            .cross(points[2] - points[0])
            .normalize_or_zero()
            .to_array();
        for i in triangle {
            radius = radius.max(Vec3::from_array(positions[*i]).length());
            p.push(positions[*i]);
            n.push(normals.as_ref().map_or(normal, |a| a[*i]));
            t.push(uv.as_ref().map_or([0.0; 2], |a| a[*i]));
            t1.push(uv1.as_ref().map_or([0.0; 2], |a| a[*i]));
            tangent.push(tangents.as_ref().map_or([1.0, 0.0, 0.0, 1.0], |a| a[*i]));
            c.push(colors.as_ref().map_or([1.0; 4], |a| a[*i]));
        }
    }
    if !radius.is_finite() || radius <= 0.0 || radius > 1.0e6 {
        return Err("Invalid mesh bounds".into());
    }
    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, p)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, n)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, t)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, c);
    // Missing optional geometry inputs must retain the native renderer's validation/fallback,
    // not appear to satisfy a material with invented UVs or tangents.
    if uv1.is_some() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, t1);
    }
    if tangents.is_some() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tangent);
    }
    Ok((mesh, radius))
}

fn attribute<const N: usize>(
    primitive: &gltf::Primitive<'_>,
    semantic: gltf::Semantic,
    buffers: &[Vec<u8>],
) -> Result<Option<Vec<[f32; N]>>, String> {
    let Some(accessor) = primitive.get(&semantic) else {
        return Ok(None);
    };
    if accessor.count() > VERTICES
        || accessor.data_type() != gltf::accessor::DataType::F32
        || accessor.dimensions().multiplicity() != N
    {
        return Err("Mesh preview requires bounded floating-point vertex attributes".into());
    }
    let values: Vec<[f32; N]> = elements(&accessor, buffers, N * 4)?
        .map(|bytes| {
            std::array::from_fn(|i| f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()))
        })
        .collect();
    if values.iter().flatten().any(|v| !v.is_finite()) {
        return Err("Non-finite mesh attribute".into());
    }
    Ok(Some(values))
}
