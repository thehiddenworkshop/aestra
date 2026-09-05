struct MeshWireOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) @interpolate(flat) visible: u32,
}

@vertex
fn vertex_mesh_wireframe(
    input: MeshVertexInput,
    @builtin(instance_index) instance_index: u32,
) -> MeshWireOutput {
    let mesh = aestra_mesh_vertex(input, instance_index);
    var output: MeshWireOutput;
    output.clip_position = mesh.particle.clip_position;
    output.color = mesh.particle.color;
    output.visible = mesh.particle.visible;
    return output;
}

@fragment
fn fragment_mesh_wireframe(input: MeshWireOutput) -> @location(0) vec4<f32> {
    if input.visible == 0u { discard; }
    return vec4<f32>(mix(input.color.rgb, vec3<f32>(0.72, 0.56, 1.0), 0.65), 0.92);
}
