// Tangents are directions transformed by the linear matrix, unlike inverse-transpose normals.
// Re-orthogonalize after nonuniform scale; reflection is already carried by the matrix.
fn aestra_mesh_tangent(tangent: vec3<f32>, normal: vec3<f32>, transform: mat4x4<f32>) -> vec3<f32> {
    let world_tangent = (transform * vec4<f32>(tangent, 0.0)).xyz;
    return normalize(world_tangent - normal * dot(normal, world_tangent));
}
