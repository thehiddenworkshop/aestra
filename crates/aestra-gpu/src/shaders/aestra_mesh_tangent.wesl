// Tangents are directions transformed by the linear matrix, unlike inverse-transpose normals.
// Re-orthogonalize after nonuniform scale; reflection is already carried by the matrix.
fn aestra_mesh_tangent(tangent: vec3<f32>, normal: vec3<f32>, transform: mat4x4<f32>) -> vec3<f32> {
    let world_tangent = (transform * vec4<f32>(tangent, 0.0)).xyz;
    return normalize(world_tangent - normal * dot(normal, world_tangent));
}

// Reconstruct an orthonormal basis, not a sheared transformed local bitangent.
// A reflection flips orientation independently of the imported tangent's UV handedness.
fn aestra_mesh_bitangent(normal: vec3<f32>, tangent: vec3<f32>, handedness: f32, transform: mat4x4<f32>) -> vec3<f32> {
    let orientation = dot(transform[0].xyz, cross(transform[1].xyz, transform[2].xyz));
    let mirror_sign = select(-1.0, 1.0, orientation >= 0.0);
    return normalize(cross(normal, tangent)) * handedness * mirror_sign;
}
