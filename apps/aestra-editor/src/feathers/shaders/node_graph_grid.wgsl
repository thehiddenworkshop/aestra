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
    // Cross-fade between power-of-two subdivisions as the graph zooms. The fine level disappears
    // before its cells become noisy, while the coarser level is already aligned underneath it.
    let desired_screen_spacing = 28.0;
    let base_screen_spacing = grid.spacing * grid.zoom;
    let continuous_level = max(
        0.0,
        log2(desired_screen_spacing / max(base_screen_spacing, 0.001)),
    );
    let level = floor(continuous_level);
    let blend = fract(continuous_level);
    let fine_spacing = grid.spacing * exp2(level);
    let coarse_spacing = fine_spacing * 2.0;
    let fine = grid_line(graph_position, fine_spacing, graph_pixel);
    let coarse = grid_line(graph_position, coarse_spacing, graph_pixel * 1.1);
    let major = grid_line(graph_position, coarse_spacing * 4.0, graph_pixel * 1.25);
    let alpha = max(max(fine * 0.10 * (1.0 - blend), coarse * 0.10), major * 0.20);
    return vec4<f32>(0.36, 0.40, 0.53, alpha);
}
