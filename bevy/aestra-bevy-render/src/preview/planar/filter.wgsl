@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var destination: texture_storage_2d<rgba16float, write>;

// Area-weighted box reduction preserves HDR radiance, including odd dimensions.
// Each destination interval overlaps at most three source texels per axis.
@compute @workgroup_size(8, 8)
fn downsample(@builtin(global_invocation_id) invocation: vec3<u32>) {
    let size = textureDimensions(destination);
    if any(invocation.xy >= size) { return; }
    let ratio = vec2<f32>(textureDimensions(source)) / vec2<f32>(size);
    let start = vec2<f32>(invocation.xy) * ratio;
    let end = vec2<f32>(invocation.xy + vec2<u32>(1u)) * ratio;
    let first = vec2<i32>(floor(start));
    var color = vec4<f32>(0.0);
    for (var y = 0; y < 3; y += 1) {
        for (var x = 0; x < 3; x += 1) {
            let texel = first + vec2<i32>(x, y);
            let coverage = max(vec2<f32>(0.0), min(end, vec2<f32>(texel + vec2<i32>(1))) - max(start, vec2<f32>(texel)));
            if coverage.x * coverage.y > 0.0 {
                color += textureLoad(source, texel, 0) * coverage.x * coverage.y;
            }
        }
    }
    textureStore(destination, vec2<i32>(invocation.xy), color / (ratio.x * ratio.y));
}
