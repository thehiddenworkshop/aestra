struct SmokeParams {
    size: u32,
    cell_size: f32,
    airflow: vec2<f32>,
    source_position: vec2<f32>,
    turbulence: f32,
    dissipation: f32,
    source_radius: f32,
    density_rate: f32,
}

@group(0) @binding(0) var<storage, read> density: array<f32>;
@group(0) @binding(1) var<storage, read_write> next_density: array<f32>;
@group(0) @binding(2) var<storage, read_write> velocity: array<vec2<f32>>;
@group(0) @binding(3) var<storage, read> params: SmokeParams;
@group(0) @binding(4) var<storage, read> frame: array<u32>;

fn density_at(cell: vec2<i32>) -> f32 {
    if any(cell < vec2<i32>(0)) || any(cell >= vec2<i32>(i32(params.size))) {
        return 0.0;
    }
    return density[u32(cell.y) * params.size + u32(cell.x)];
}

// Open boundaries: there is no density outside the grid.
fn sample_density(cell: vec2<f32>) -> f32 {
    if any(cell <= vec2<f32>(-1.0)) || any(cell >= vec2<f32>(f32(params.size))) {
        return 0.0;
    }
    let lower = vec2<i32>(floor(cell));
    let weight = fract(cell);
    let bottom = mix(density_at(lower), density_at(lower + vec2<i32>(1, 0)), weight.x);
    let top = mix(density_at(lower + vec2<i32>(0, 1)), density_at(lower + vec2<i32>(1, 1)), weight.x);
    return mix(bottom, top, weight.y);
}

@compute @workgroup_size(8, 8, 1)
fn advect_smoke(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= vec2<u32>(params.size)) {
        return;
    }
    let index = id.y * params.size + id.x;
    let cell = vec2<f32>(id.xy);
    let uv = (cell + vec2<f32>(0.5)) / f32(params.size) - vec2<f32>(0.5);
    let time = bitcast<f32>(frame[2]);
    let dt = bitcast<f32>(frame[1]);
    // Each component varies across the other axis to keep the prescribed flow divergence-free.
    let eddy = vec2<f32>(
        sin(14.0 * uv.y - time * 1.8),
        0.45 * cos(12.0 * uv.x + time * 1.3),
    );
    let flow = params.airflow + params.turbulence * eddy;
    velocity[index] = flow;

    let upstream = cell - (flow * dt) / params.cell_size;
    let remaining = sample_density(upstream) * exp(-params.dissipation * dt);
    let position = uv * (f32(params.size) * params.cell_size);
    let radial = 1.0 - smoothstep(0.0, 1.0, distance(position, params.source_position) / params.source_radius);
    next_density[index] = min(1.0, remaining + radial * (params.density_rate * dt));
}
