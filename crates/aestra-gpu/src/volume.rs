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
//! - `aestra_volume_scene_lighting(uvw, pixel, max_lights) -> vec3<f32>` — optional host illumination
//!   at a grid sample, in linear RGB, including the scattering phase. The portable default is zero;
//!   a supporting backend supplies the implementation. This is not a promise of shadows or a
//!   particular light type. Plugins must explicitly opt in and pass a bounded per-sample budget.
//! - `AestraVolumeRay` — the view ray through the pixel, in grid (`uvw`) coordinates, already clipped
//!   to the box and to the opaque scene. `origin + direction * t` is the point at distance `t` in the
//!   grid's own length unit (the effect's units), so `t_far - t_near` is the length to integrate over.
//!
//! A bricked grid (fluid F7) is uploaded as a **brick atlas**: each stored brick, with a one-cell
//! apron copied from its neighbours, becomes a block of the field textures, and a `u32` table texture
//! gives each brick's slot. `aestra_volume_field` samples it the same way — trilinear filtering stays
//! exact across brick borders thanks to the apron — and returns zero where no brick is stored.

use aestra_runtime::{MAX_VOLUME_CONSTANTS, MAX_VOLUME_FIELDS};

/// The binding index of each field slot (slot 0 shares its sampler with every field).
pub const VOLUME_FIELD_BINDINGS: [u32; MAX_VOLUME_FIELDS] = [1, 3, 4, 5];
/// The binding of the uniform parameters, and of the shared trilinear sampler.
pub const VOLUME_PARAMS_BINDING: u32 = 0;
pub const VOLUME_SAMPLER_BINDING: u32 = 2;
/// The binding of a bricked grid's table texture (`texture_3d<u32>`, one texel per brick).
pub const VOLUME_TABLE_BINDING: u32 = 6;

/// The uniform the volume interface reads, as `u32` words: `size.xyz` (the box's size in the grid's
/// length unit) and the cell size in `size.w`; `dims.xyz` (the grid resolution) and the field count in
/// `dims.w`; `bricks`: a bricked grid's brick edge (0 when every cell is stored) and its atlas's bricks
/// per axis; then [`MAX_VOLUME_CONSTANTS`] constant words.
pub const VOLUME_PARAMS_WORDS: usize = 12 + MAX_VOLUME_CONSTANTS;

/// Bricks along each axis of the atlas holding `slots` bricks: the smallest cube that fits them.
pub fn brick_atlas_bricks(slots: u32) -> u32 {
    let mut per_axis = 1;
    while u64::from(per_axis).pow(3) < u64::from(slots) {
        per_axis += 1;
    }
    per_axis
}

/// Texels along each axis of the atlas of `slots` bricks of `edge`³ cells, each with a one-cell apron.
pub fn brick_atlas_texels(slots: u32, edge: u32) -> u32 {
    brick_atlas_bricks(slots) * (edge + 2)
}

/// The interface declarations for bind group `group` — a number, or a shader-def placeholder such as
/// `#{MATERIAL_BIND_GROUP}` for a composing preprocessor.
pub fn volume_interface_wgsl(group: &str) -> String {
    volume_interface_wgsl_with_scene_lighting(
        group,
        "fn aestra_volume_scene_lighting(uvw: vec3<f32>, pixel: vec2<f32>, max_lights: u32) -> vec3<f32> { return vec3<f32>(0.0); }",
    )
}

/// Compose the portable interface with a backend-provided scene-lighting function.
///
/// `scene_lighting_wgsl` must define `aestra_volume_scene_lighting` with the signature documented
/// above. The backend owns grid-to-world conversion and any view bindings; the ray ABI and field
/// bindings remain unchanged. Unsupported backends should use [`volume_interface_wgsl`].
pub fn volume_interface_wgsl_with_scene_lighting(group: &str, scene_lighting_wgsl: &str) -> String {
    let [field_0, field_1, field_2, field_3] = VOLUME_FIELD_BINDINGS;
    let constant_vectors = MAX_VOLUME_CONSTANTS / 4;
    format!(
        r#"
struct AestraVolumeParams {{
    size: vec4<f32>,
    dims: vec4<u32>,
    bricks: vec4<u32>,
    constants: array<vec4<u32>, {constant_vectors}>,
}}

@group({group}) @binding({VOLUME_PARAMS_BINDING}) var<uniform> aestra_volume: AestraVolumeParams;
@group({group}) @binding({field_0}) var aestra_volume_field_0: texture_3d<f32>;
@group({group}) @binding({VOLUME_SAMPLER_BINDING}) var aestra_volume_sampler: sampler;
@group({group}) @binding({field_1}) var aestra_volume_field_1: texture_3d<f32>;
@group({group}) @binding({field_2}) var aestra_volume_field_2: texture_3d<f32>;
@group({group}) @binding({field_3}) var aestra_volume_field_3: texture_3d<f32>;
@group({group}) @binding({VOLUME_TABLE_BINDING}) var aestra_volume_table: texture_3d<u32>;

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

fn aestra_volume_texture(slot: u32, coords: vec3<f32>) -> vec4<f32> {{
    switch slot {{
        case 0u: {{ return textureSampleLevel(aestra_volume_field_0, aestra_volume_sampler, coords, 0.0); }}
        case 1u: {{ return textureSampleLevel(aestra_volume_field_1, aestra_volume_sampler, coords, 0.0); }}
        case 2u: {{ return textureSampleLevel(aestra_volume_field_2, aestra_volume_sampler, coords, 0.0); }}
        default: {{ return textureSampleLevel(aestra_volume_field_3, aestra_volume_sampler, coords, 0.0); }}
    }}
}}

fn aestra_volume_field(slot: u32, uvw: vec3<f32>) -> vec4<f32> {{
    let edge = aestra_volume.bricks.x;
    if (edge == 0u) {{
        return aestra_volume_texture(slot, uvw);
    }}
    // A bricked grid: the brick holding the point, its slot, and the point in its atlas block (cell
    // centres at +0.5, after the one-cell apron).
    let dims = aestra_volume.dims.xyz;
    let p = clamp(uvw, vec3<f32>(0.0), vec3<f32>(1.0)) * vec3<f32>(dims);
    let brick = min(vec3<u32>(p) / edge, dims / edge - vec3<u32>(1u));
    let stored = textureLoad(aestra_volume_table, vec3<i32>(brick), 0).x;
    if (stored == 0u) {{
        return vec4<f32>(0.0);
    }}
    let per_axis = aestra_volume.bricks.y;
    let span = edge + 2u;
    let block = vec3<u32>(stored % per_axis, (stored / per_axis) % per_axis, stored / (per_axis * per_axis));
    let texel = vec3<f32>(block * span) + vec3<f32>(1.0) + (p - vec3<f32>(brick * edge));
    return aestra_volume_texture(slot, texel / f32(per_axis * span));
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

{scene_lighting_wgsl}
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

/// The copies of a bricked grid field (fluid F7) into its volume textures. `copy_bricks` fills the
/// brick atlas: every slot's block, the brick's cells with a one-cell apron from the neighbouring
/// cells (clamped at the grid's edge, like the dense texture's sampler; zero from a brick not stored);
/// an unused slot's block is zero. `copy_table` copies the brick table into the `r32uint` table
/// texture. `params`: `[dims.xyz, components, edge, atlas bricks per axis, table word, slot bricks
/// word, slots]`.
pub fn bricks_to_volume_wgsl() -> String {
    format!(
        "{BRICKS_TO_VOLUME_WGSL}{}",
        crate::brick_cell_wgsl("volume_brick_cell", "bricks_table")
    )
}

const BRICKS_TO_VOLUME_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read> field: array<f32>;
@group(0) @binding(1) var<storage, read> params: array<u32>;
@group(0) @binding(2) var volume: texture_storage_3d<rgba16float, write>;
@group(0) @binding(3) var<storage, read> bricks_table: array<u32>;
@group(0) @binding(4) var table_image: texture_storage_3d<r32uint, write>;

fn brick_dims() -> vec3<u32> {
    return vec3<u32>(params[0], params[1], params[2]);
}

@compute @workgroup_size(4, 4, 4)
fn copy_bricks(@builtin(global_invocation_id) id: vec3<u32>) {
    let edge = params[4];
    let per_axis = params[5];
    let span = edge + 2u;
    if (any(id >= vec3<u32>(per_axis * span))) { return; }
    let block = id / span;
    let slot = (block.z * per_axis + block.y) * per_axis + block.x;
    var value = vec4<f32>(0.0);
    if (slot > 0u && slot < params[8]) {
        let packed = bricks_table[params[7] + slot];
        if (packed != 0u) {
            let brick = vec3<i32>(
                i32((packed - 1u) & 1023u),
                i32(((packed - 1u) >> 10u) & 1023u),
                i32((packed - 1u) >> 20u),
            );
            let dims = brick_dims();
            let local = vec3<i32>(id % span) - vec3<i32>(1);
            let cell = clamp(brick * i32(edge) + local, vec3<i32>(0), vec3<i32>(dims) - vec3<i32>(1));
            let element = volume_brick_cell(vec3<u32>(cell), dims, edge, params[6]);
            if (element != 0xffffffffu) {
                let components = params[3];
                let stride = select(components, 4u, components == 3u);
                let base = element * stride;
                value = vec4<f32>(field[base], 0.0, 0.0, 0.0);
                if (components > 1u) {
                    value = vec4<f32>(field[base], field[base + 1u], field[base + 2u], 0.0);
                }
                if (components > 3u) {
                    value.w = field[base + 3u];
                }
            }
        }
    }
    textureStore(volume, vec3<i32>(id), value);
}

@compute @workgroup_size(4, 4, 4)
fn copy_table(@builtin(global_invocation_id) id: vec3<u32>) {
    let grid = brick_dims() / params[4];
    if (any(id >= grid)) { return; }
    let slot = bricks_table[params[6] + (id.z * grid.y + id.y) * grid.x + id.x];
    textureStore(table_image, vec3<i32>(id), vec4<u32>(slot, 0u, 0u, 0u));
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
        validate(&bricks_to_volume_wgsl());
    }

    #[test]
    fn a_backend_can_supply_scene_lighting_without_changing_the_ray_or_bindings() {
        let function = "fn aestra_volume_scene_lighting(uvw: vec3<f32>, pixel: vec2<f32>, max_lights: u32) -> vec3<f32> { return uvw * f32(min(max_lights, 1u)); }";
        let source = volume_interface_wgsl_with_scene_lighting("0", function);
        assert_eq!(
            source.matches("fn aestra_volume_scene_lighting(").count(),
            1
        );
        validate(&format!(
            "{source}\n@fragment fn main() -> @location(0) vec4<f32> {{ return vec4<f32>(aestra_volume_scene_lighting(vec3<f32>(0.5), vec2<f32>(0.0), 1u), 1.0); }}"
        ));
    }

    #[test]
    fn a_brick_atlas_is_the_smallest_cube_of_bricks_that_holds_every_slot() {
        assert_eq!(brick_atlas_bricks(1), 1);
        assert_eq!(brick_atlas_bricks(8), 2);
        assert_eq!(brick_atlas_bricks(9), 3);
        assert_eq!(brick_atlas_bricks(4097), 17);
        assert_eq!(brick_atlas_texels(4097, 8), 170);
    }
}
