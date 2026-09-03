#import bevy_ui::ui_vertex_output::UiVertexOutput

struct WireUniforms {
    start: vec2<f32>,
    control_start: vec2<f32>,
    control_end: vec2<f32>,
    end: vec2<f32>,
    color: vec4<f32>,
    width: f32,
};

@group(1) @binding(0)
var<uniform> wire: WireUniforms;

fn cubic_bezier(t: f32) -> vec2<f32> {
    let inverse = 1.0 - t;
    return inverse * inverse * inverse * wire.start
        + 3.0 * inverse * inverse * t * wire.control_start
        + 3.0 * inverse * t * t * wire.control_end
        + t * t * t * wire.end;
}

fn segment_distance(point: vec2<f32>, start: vec2<f32>, end: vec2<f32>) -> f32 {
    let segment = end - start;
    let amount = clamp(dot(point - start, segment) / max(dot(segment, segment), 0.0001), 0.0, 1.0);
    return length(point - (start + segment * amount));
}

@fragment
fn fragment(input: UiVertexOutput) -> @location(0) vec4<f32> {
    let point = input.uv * input.size;
    var distance = 100000.0;
    var previous = wire.start;
    for (var index = 1u; index <= 32u; index += 1u) {
        let current = cubic_bezier(f32(index) / 32.0);
        distance = min(distance, segment_distance(point, previous, current));
        previous = current;
    }
    // Keep the requested width stable in screen pixels and retain derivative-based antialiasing
    // at viewport edges and fractional display scales.
    let local_pixel = max(max(fwidth(point.x), fwidth(point.y)), 0.001);
    let edge = distance - wire.width * 0.5 * local_pixel;
    let feather = max(fwidth(edge), 0.75 * local_pixel);
    let alpha = 1.0 - smoothstep(-feather, feather, edge);
    return vec4<f32>(wire.color.rgb, wire.color.a * alpha);
}
