
// Mesh positions/normals/UVs use the host mesh's vertex buffer, not billboard corners.
struct MeshVertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

struct MeshVertexData {
    particle: SpriteVertexData,
    normal: vec3<f32>,
    world_position: vec3<f32>,
    local_position: vec3<f32>,
    view_direction: vec3<f32>,
}

fn aestra_mesh_vertex(vertex: MeshVertexInput, instance_index: u32) -> MeshVertexData {
    let index = alive_indices[params.alive_offset + instance_index];
    let rotation = particles[index].rotation;
    let s = sin(rotation);
    let c = cos(rotation);
    let local = vec3<f32>(vertex.position.x * c - vertex.position.y * s,
        vertex.position.x * s + vertex.position.y * c, vertex.position.z) * particles[index].size;
    let rotated_normal = vec3<f32>(vertex.normal.x * c - vertex.normal.y * s,
        vertex.normal.x * s + vertex.normal.y * c, vertex.normal.z);
    let mesh_transform = globals.world_from_effect * params.mesh_from_local;
    let world = globals.world_from_effect * vec4<f32>(particles[index].position, 1.0)
        + mesh_transform * vec4<f32>(local, 0.0);
    // Cofactor transform is inverse-transpose up to determinant, including mirrored transforms.
    let x = mesh_transform[0].xyz;
    let y = mesh_transform[1].xyz;
    let z = mesh_transform[2].xyz;
    let determinant_sign = select(-1.0, 1.0, dot(x, cross(y, z)) >= 0.0);
    var output: MeshVertexData;
    output.particle = aestra_sprite_vertex(0u, instance_index);
    output.particle.clip_position = view.clip_from_world * world;
    output.particle.uv = vertex.uv;
    output.normal = normalize((cross(y, z) * rotated_normal.x + cross(z, x) * rotated_normal.y
        + cross(x, y) * rotated_normal.z) * determinant_sign);
    output.world_position = world.xyz;
    output.local_position = vertex.position;
    output.view_direction = view.world_from_view[3].xyz - world.xyz;
    return output;
}
