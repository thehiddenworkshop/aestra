
fn aestra_effect_position_xy(world_position: vec3<f32>) -> vec2<f32> {
    let transform = globals.world_from_effect;
    let x = transform[0].xyz;
    let y = transform[1].xyz;
    let z = transform[2].xyz;
    let row_x = cross(y, z);
    let determinant = dot(x, row_x);
    if determinant == 0.0 {
        return vec2<f32>(0.0);
    }
    let relative = world_position - transform[3].xyz;
    return vec2<f32>(dot(row_x, relative), dot(cross(z, x), relative)) / determinant;
}
