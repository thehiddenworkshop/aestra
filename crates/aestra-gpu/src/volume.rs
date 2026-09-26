//! The interface a volume presentation's march function is written against (fluid F3).
//!
//! A plugin's [`aestra_runtime::VolumePresentation`] names a WGSL function
//! `fn(ray: AestraVolumeRay) -> vec4<f32>` returning a premultiplied colour. The backend composes it
//! after [`volume_interface_wgsl`], which declares the bindings and the helpers the function may call:
//!
//! - `aestra_volume_field(slot, uvw) -> vec4<f32>` — field `slot` (the presentation's `fields` order)
//!   sampled with trilinear filtering at `uvw` in `[0, 1]³` over the grid box; scalar fields read `.x`,
//!   vector fields `.xyz`;
//! - `aestra_volume_constant(index) -> u32` and `aestra_volume_constant_f32(index) -> f32` — the
//!   presentation's constant words;
//! - `AestraVolumeRay` — the view ray through the pixel, in grid (`uvw`) coordinates, already clipped
//!   to the box and to the opaque scene. `origin + direction * t` is the point at distance `t` in the
//!   grid's own length unit (the effect's units), so `t_far - t_near` is the length to integrate over.

use aestra_runtime::{MAX_VOLUME_CONSTANTS, MAX_VOLUME_FIELDS};

/// The binding index of each field slot (slot 0 shares its sampler with every field).
pub const VOLUME_FIELD_BINDINGS: [u32; MAX_VOLUME_FIELDS] = [1, 3, 4, 5];
/// The binding of the uniform parameters, and of the shared trilinear sampler.
pub const VOLUME_PARAMS_BINDING: u32 = 0;
pub const VOLUME_SAMPLER_BINDING: u32 = 2;

/// The uniform the volume interface reads, as `u32` words: `size.xyz` (the box's size in the grid's
/// length unit) and the cell size in `size.w`; `dims.xyz` (the grid resolution) and the field count in
/// `dims.w`; then [`MAX_VOLUME_CONSTANTS`] constant words.
pub const VOLUME_PARAMS_WORDS: usize = 8 + MAX_VOLUME_CONSTANTS;

/// The interface declarations for bind group `group` — a number, or a shader-def placeholder such as
/// `#{MATERIAL_BIND_GROUP}` for a composing preprocessor.
pub fn volume_interface_wgsl(group: &str) -> String {
    let [field_0, field_1, field_2, field_3] = VOLUME_FIELD_BINDINGS;
    let constant_vectors = MAX_VOLUME_CONSTANTS / 4;
    format!(
        r#"
struct AestraVolumeParams {{
    size: vec4<f32>,
    dims: vec4<u32>,
    constants: array<vec4<u32>, {constant_vectors}>,
}}

@group({group}) @binding({VOLUME_PARAMS_BINDING}) var<uniform> aestra_volume: AestraVolumeParams;
@group({group}) @binding({field_0}) var aestra_volume_field_0: texture_3d<f32>;
@group({group}) @binding({VOLUME_SAMPLER_BINDING}) var aestra_volume_sampler: sampler;
@group({group}) @binding({field_1}) var aestra_volume_field_1: texture_3d<f32>;
@group({group}) @binding({field_2}) var aestra_volume_field_2: texture_3d<f32>;
@group({group}) @binding({field_3}) var aestra_volume_field_3: texture_3d<f32>;

// The view ray in grid coordinates: `origin + direction * t` for `t` in `[t_near, t_far]`, `t` in the
// grid's length unit. `size` is the box's size in that unit; `pixel` the fragment position.
struct AestraVolumeRay {{
    origin: vec3<f32>,
    direction: vec3<f32>,
    t_near: f32,
    t_far: f32,
    size: vec3<f32>,
    pixel: vec2<f32>,
}}

fn aestra_volume_constant(index: u32) -> u32 {{
    return aestra_volume.constants[index / 4u][index % 4u];
}}

fn aestra_volume_constant_f32(index: u32) -> f32 {{
    return bitcast<f32>(aestra_volume_constant(index));
}}

fn aestra_volume_field(slot: u32, uvw: vec3<f32>) -> vec4<f32> {{
    switch slot {{
        case 0u: {{ return textureSampleLevel(aestra_volume_field_0, aestra_volume_sampler, uvw, 0.0); }}
        case 1u: {{ return textureSampleLevel(aestra_volume_field_1, aestra_volume_sampler, uvw, 0.0); }}
        case 2u: {{ return textureSampleLevel(aestra_volume_field_2, aestra_volume_sampler, uvw, 0.0); }}
        default: {{ return textureSampleLevel(aestra_volume_field_3, aestra_volume_sampler, uvw, 0.0); }}
    }}
}}

// Where a ray from `origin` along `direction` (grid coordinates per length unit) is inside the unit
// box: `(t_enter, t_exit)`, empty when `t_exit < t_enter`.
fn aestra_volume_box(origin: vec3<f32>, direction: vec3<f32>) -> vec2<f32> {{
    let inverse = 1.0 / direction;
    let a = (vec3<f32>(0.0) - origin) * inverse;
    let b = (vec3<f32>(1.0) - origin) * inverse;
    let near = min(a, b);
    let far = max(a, b);
    return vec2<f32>(max(max(near.x, near.y), near.z), min(min(far.x, far.y), far.z));
}}
"#
    )
}

/// A compute pass that copies one grid field (an `array<f32>` of `components` per cell, `vec3`
/// padded to 4) into an `rgba16float` 3-D storage texture of the grid's size: x, y, z map to the
/// texture's u, v, w, so `uvw` in the interface is the grid position over its extent.
pub const FIELD_TO_VOLUME_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read> field: array<f32>;
@group(0) @binding(1) var<storage, read> params: array<u32>;
@group(0) @binding(2) var volume: texture_storage_3d<rgba16float, write>;

@compute @workgroup_size(4, 4, 4)
fn copy_field(@builtin(global_invocation_id) id: vec3<u32>) {
    let dims = vec3<u32>(params[0], params[1], params[2]);
    if (any(id >= dims)) { return; }
    let components = params[3];
    let stride = select(components, 4u, components == 3u);
    let base = ((id.z * dims.y + id.y) * dims.x + id.x) * stride;
    var value = vec4<f32>(field[base], 0.0, 0.0, 0.0);
    if (components > 1u) {
        value = vec4<f32>(field[base], field[base + 1u], field[base + 2u], 0.0);
    }
    if (components > 3u) {
        value.w = field[base + 3u];
    }
    textureStore(volume, vec3<i32>(id), value);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn validate(source: &str) {
        let module = naga::front::wgsl::parse_str(source).expect("parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .expect("validates");
    }

    #[test]
    fn the_interface_and_the_copy_pass_validate() {
        validate(&format!(
            "{}\n@fragment fn main() -> @location(0) vec4<f32> {{\n    \
             let span = aestra_volume_box(vec3<f32>(0.5), vec3<f32>(1.0, 0.0, 0.0));\n    \
             return aestra_volume_field(1u, vec3<f32>(0.5)) * aestra_volume_constant_f32(3u) * span.y;\n}}",
            volume_interface_wgsl("0")
        ));
        validate(FIELD_TO_VOLUME_WGSL);
    }
}
