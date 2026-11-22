@group(0) @binding(0) var<storage, read_write> velocity: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read> params: array<u32>;
@group(0) @binding(2) var<storage, read> frame: array<u32>;

@compute @workgroup_size(8, 8, 1)
fn write_grid_2d_velocity(@builtin(global_invocation_id) id: vec3<u32>) {
    let width = params[0];
    let height = params[1];
    if id.x >= width || id.y >= height {
        return;
    }

    let size = vec2<f32>(f32(width), f32(height));
    let radius = 0.5 * min(size.x, size.y);
    let relative = (vec2<f32>(id.xy) + vec2<f32>(0.5) - 0.5 * size) / radius;
    let edge = 1.0 - smoothstep(0.60, 1.0, length(relative));
    let time = bitcast<f32>(frame[2]);
    let speed = bitcast<f32>(params[2]) * (0.85 + 0.15 * cos(time * 1.5));
    velocity[id.y * width + id.x] = vec2<f32>(-relative.y, relative.x) * edge * speed;
}
