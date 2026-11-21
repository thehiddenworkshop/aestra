import bevy_sprite_render::mesh2d::view_bindings::view;

@if(SRGB_OUTPUT)
import bevy_render::color_operations::linear_to_srgb;
@if(OKLAB_OUTPUT)
import bevy_render::color_operations::linear_rgb_to_oklab;

struct GridVertex {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_xy: vec2<f32>,
};

@vertex
fn vertex(@location(0) position: vec3<f32>) -> GridVertex {
    // Cover the current viewport at its far depth. Reconstruct world XY
    // from the same projection as the effects, including pan and resize.
    let clip = vec4<f32>(position.xy, 0.0, 1.0);
    let world = view.world_from_clip * clip;
    return GridVertex(clip, world.xy / world.w);
}

fn coverage(distance_pixels: vec2<f32>) -> vec2<f32> {
    return 1.0 - smoothstep(vec2<f32>(0.15), vec2<f32>(1.15), distance_pixels);
}

fn grid_line(position: vec2<f32>, spacing: f32, pixel: vec2<f32>) -> f32 {
    let cell = position / spacing;
    let distance = abs(fract(cell + 0.5) - 0.5) * spacing;
    let line = coverage(distance / pixel);
    return max(line.x, line.y);
}

@fragment
fn fragment(input: GridVertex) -> @location(0) vec4<f32> {
    let pixel = max(fwidth(input.world_xy), vec2<f32>(0.000001));
    let level = log2(28.0 * max(pixel.x, pixel.y));
    let fine = exp2(floor(level));
    let coarse = fine * 2.0;
    let blend = fract(level);

    // Interpolate between aligned power-of-two subdivisions while zooming.
    let minor = mix(grid_line(input.world_xy, fine, pixel), grid_line(input.world_xy, coarse, pixel), blend);
    let major = mix(grid_line(input.world_xy, fine * 8.0, pixel), grid_line(input.world_xy, coarse * 8.0, pixel), blend);
    var strength = max(minor * 0.18, major * 0.34);
    var color = vec3<f32>(0.27, 0.30, 0.39);
    let axes = coverage(abs(input.world_xy) / pixel) * 0.72;
    if axes.y > strength {
        strength = axes.y;
        color = vec3<f32>(0.58, 0.20, 0.27);
    }
    if axes.x > strength {
        strength = axes.x;
        color = vec3<f32>(0.19, 0.53, 0.32);
    }

    @if(SRGB_OUTPUT) color = linear_to_srgb(color);
    @if(OKLAB_OUTPUT) color = linear_rgb_to_oklab(color);
    return vec4<f32>(color, strength);
}
