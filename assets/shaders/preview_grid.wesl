struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
};

struct PreviewGridUniform {
    minor_color: vec4<f32>,
    major_color: vec4<f32>,
    x_axis_color: vec4<f32>,
    z_axis_color: vec4<f32>,
    parameters: vec4<f32>,
    focus: vec4<f32>,
};

@group(3) @binding(0)
var<uniform> grid: PreviewGridUniform;

fn grid_strength(
    position: vec2<f32>,
    spacing: f32,
    fade_start: f32,
    fade_end: f32,
) -> f32 {
    let coordinate = position / spacing;
    let derivative = max(fwidth(coordinate), vec2<f32>(0.000001));
    let distance_to_line = abs(fract(coordinate - 0.5) - 0.5) / derivative;
    let coverage = vec2<f32>(1.0) - min(distance_to_line, vec2<f32>(1.0));
    let density_fade = vec2<f32>(1.0) - smoothstep(
        vec2<f32>(fade_start),
        vec2<f32>(fade_end),
        derivative,
    );
    return max(coverage.x * density_fade.x, coverage.y * density_fade.y);
}

fn axis_coverage(value: f32) -> f32 {
    let derivative = max(fwidth(value), 0.000001);
    return 1.0 - min(abs(value) / derivative, 1.0);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let world_position = in.world_position.xz;
    let minor = grid_strength(world_position, grid.parameters.x, 0.08, 0.35);
    let major = grid_strength(
        world_position,
        grid.parameters.x * grid.parameters.w,
        0.12,
        0.55,
    );
    let x_axis = axis_coverage(world_position.y);
    let z_axis = axis_coverage(world_position.x);

    var strength = minor * grid.minor_color.a;
    var color = grid.minor_color.rgb;
    let major_strength = major * grid.major_color.a;
    if major_strength > strength {
        strength = major_strength;
        color = grid.major_color.rgb;
    }
    let x_axis_strength = x_axis * grid.x_axis_color.a;
    if x_axis_strength > strength {
        strength = x_axis_strength;
        color = grid.x_axis_color.rgb;
    }
    let z_axis_strength = z_axis * grid.z_axis_color.a;
    if z_axis_strength > strength {
        strength = z_axis_strength;
        color = grid.z_axis_color.rgb;
    }

    let distance_from_focus = length(world_position - grid.focus.xy);
    let distance_fade = 1.0 - smoothstep(
        grid.parameters.y * 0.18,
        grid.parameters.y * 0.75,
        distance_from_focus,
    );
    let alpha = strength * distance_fade * grid.parameters.z;
    if alpha <= 0.001 {
        discard;
    }
    return vec4<f32>(color, alpha);
}
