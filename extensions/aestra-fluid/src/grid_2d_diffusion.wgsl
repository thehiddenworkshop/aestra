@group(0) @binding(0) var<storage, read> density: array<f32>;
@group(0) @binding(1) var<storage, read_write> next_density: array<f32>;
@group(0) @binding(2) var<storage, read> params: array<u32>;
@group(0) @binding(3) var<storage, read> frame: array<u32>;

@compute @workgroup_size(8, 8, 1)
fn diffuse_grid_2d(@builtin(global_invocation_id) id: vec3<u32>) {
    let width = params[0];
    let height = params[1];
    if id.x >= width || id.y >= height {
        return;
    }
    let index = id.y * width + id.x;
    if frame[0] == 0u {
        let size = vec2<f32>(f32(width), f32(height));
        let radius = max(1.0, 0.18 * min(size.x, size.y));
        let delta = (vec2<f32>(id.xy) + vec2<f32>(0.5) - 0.5 * size) / radius;
        let pulse = max(1.0 - dot(delta, delta), 0.0);
        next_density[index] = pulse * pulse;
        return;
    }

    let left = select(id.x - 1u, width - 1u, id.x == 0u);
    let right = select(id.x + 1u, 0u, id.x == width - 1u);
    let bottom = select(id.y - 1u, height - 1u, id.y == 0u);
    let top = select(id.y + 1u, 0u, id.y == height - 1u);
    let center = density[index];
    let neighbors = density[id.y * width + left] + density[id.y * width + right]
        + density[bottom * width + id.x] + density[top * width + id.x];
    let rate = bitcast<f32>(params[2]);
    next_density[index] = center + rate * (neighbors - 4.0 * center);
}
