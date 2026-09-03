#import bevy_ui::ui_vertex_output::UiVertexOutput

struct GridUniforms {
    pan: vec2<f32>,
    zoom: f32,
    spacing: f32,
};

@group(1) @binding(0)
var<uniform> grid: GridUniforms;

fn grid_line(coordinate: vec2<f32>, spacing: f32, pixel_width: f32) -> f32 {
    let cell = coordinate / spacing;
    let distance_to_line = abs(fract(cell + 0.5) - 0.5) * spacing;
    let distance = min(distance_to_line.x, distance_to_line.y);
    return 1.0 - smoothstep(pixel_width * 0.35, pixel_width * 1.35, distance);
}

@fragment
fn fragment(input: UiVertexOutput) -> @location(0) vec4<f32> {
    let graph_position = (input.uv * input.size - grid.pan) / max(grid.zoom, 0.001);
    let graph_pixel = 1.0 / max(grid.zoom, 0.001);
    let minor = grid_line(graph_position, grid.spacing, graph_pixel);
    let major = grid_line(graph_position, grid.spacing * 4.0, graph_pixel * 1.2);
    let alpha = max(minor * 0.10, major * 0.20);
    return vec4<f32>(0.36, 0.40, 0.53, alpha);
}
