//! Native Bevy WESL composition around the engine-neutral shader bodies.
use aestra_gpu::volume::volume_interface_wgsl_with_scene_lighting;

const VOLUME_IMPORTS: &str = r#"import bevy_pbr::render::{
    mesh_functions,
    mesh_view_bindings::{view},
    mesh_view_bindings as volume_scene,
    clustered_forward as volume_clusters,
    view_transformations::{position_world_to_clip, position_ndc_to_world, frag_coord_to_ndc},
};
@if(DEPTH_PREPASS)
import bevy_pbr::prepass::utils as prepass_utils;"#;

const MATERIAL_IMPORTS: &str = r#"@if(AESTRA_SCENE_POINT_LIGHTING)
import bevy_pbr::render::mesh_view_bindings::view;
@if(AESTRA_SCENE_POINT_LIGHTING)
import bevy_pbr::render::mesh_view_bindings as aestra_sprite_scene;
@if(AESTRA_SCENE_POINT_LIGHTING)
import bevy_pbr::render::clustered_forward as aestra_sprite_clusters;
"#;

pub(crate) fn compose_volume(program_wgsl: &str, entry_point: &str) -> String {
    format!(
        r#"{imports}
{interface}
{program_wgsl}

struct AestraVolumeVertex {{
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
}}

struct AestraVolumeVarying {{
    @builtin(position) clip: vec4<f32>,
    @location(0) local: vec3<f32>,
    @location(1) @interpolate(flat) instance: u32,
}}

@vertex
fn vertex(vertex: AestraVolumeVertex) -> AestraVolumeVarying {{
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    let world = world_from_local * vec4<f32>(vertex.position, 1.0);
    var out: AestraVolumeVarying;
    out.clip = position_world_to_clip(world.xyz);
    out.local = vertex.position;
    out.instance = vertex.instance_index;
    return out;
}}

@fragment
fn fragment(in: AestraVolumeVarying) -> @location(0) vec4<f32> {{
    // The box is a unit cube centred on its origin, scaled to the grid: local + 0.5 is uvw.
    aestra_volume_world_from_grid = mesh_functions::get_world_from_local(in.instance);
    let local_from_world = mesh_functions::get_local_from_world(in.instance);
    let camera = (local_from_world * vec4<f32>(view.world_position, 1.0)).xyz;
    let size = aestra_volume.size.xyz;
    let travel = (in.local - camera) * size;
    let distance = length(travel);
    if (distance <= 0.0) {{
        discard;
    }}
    let origin = camera + vec3<f32>(0.5);
    let direction = (in.local - camera) / distance;
    var span = aestra_volume_box(origin, direction);
    span.x = max(span.x, 0.0);
{depth_start}
    let depth = prepass_utils::prepass_depth(in.clip, 0u);
    if (depth > 0.0) {{
        let scene = position_ndc_to_world(frag_coord_to_ndc(vec4<f32>(in.clip.xy, depth, 1.0)));
        let scene_local = (local_from_world * vec4<f32>(scene, 1.0)).xyz;
        span.y = min(span.y, length((scene_local - camera) * size));
    }}
{depth_end}
    if (span.y <= span.x) {{
        discard;
    }}
    return {entry_point}(AestraVolumeRay(origin, direction, span.x, span.y, size, in.clip.xy));
}}
"#,
        imports = VOLUME_IMPORTS,
        depth_start = "@if(DEPTH_PREPASS) {",
        depth_end = "}",
        interface = volume_interface_wgsl_with_scene_lighting(
            "constants::MATERIAL_BIND_GROUP",
            SCENE_LIGHTING_WGSL,
        ),
    )
}

// Invocation-private sample transforms and the fixed visited-entry budget are unchanged.
const SCENE_LIGHTING_WGSL: &str = include_str!("volume_lighting.wgsl");

pub(crate) fn compose_material(wgsl: &str, enabled: bool) -> String {
    if !enabled {
        return wgsl.to_owned();
    }
    // include_str! shader bytes may use CRLF after a Windows checkout. Match
    // generated declarations independently of checkout formatting, not of ABI.
    let mut wgsl = wgsl.replace("\r\n", "\n");
    let scene_callback = "fn aestra_scene_point_irradiance(";
    let sprite_callback = "fn aestra_sprite_point_irradiance(";
    let has_scene_callback = wgsl.contains(scene_callback);
    let has_sprite_callback = wgsl.contains(sprite_callback);
    assert!(
        has_scene_callback || has_sprite_callback,
        "scene-light callback missing"
    );
    if has_scene_callback {
        replace_light_callback(
            &mut wgsl,
            scene_callback,
            r#"{
    @if(AESTRA_SCENE_POINT_LIGHTING) { return aestra_bevy_point_irradiance(world, pixel); }
    @else {
        @if(AESTRA_POINT_LIGHTING_2D) { return aestra_point_irradiance_2d(world.xy); }
        @else { return vec3<f32>(0.0); }
    }
}"#,
        );
    }
    if has_sprite_callback {
        replace_light_callback(
            &mut wgsl,
            sprite_callback,
            r#"{
    @if(AESTRA_POINT_LIGHTING_2D) {
        return aestra_normal_point_irradiance_2d(world, quad, normal);
    }
    @else { return vec3<f32>(0.0); }
}"#,
        );
    }
    let mut source = String::with_capacity(wgsl.len() + 3000);
    if has_scene_callback {
        source.push_str(MATERIAL_IMPORTS);
    }
    source.push_str(&wgsl);
    source.push_str("\n@if(AESTRA_POINT_LIGHTING_2D) {\n");
    source.push_str(include_str!("lighting_2d.wgsl"));
    source.push_str("\n}\n");
    if !has_scene_callback {
        return source;
    }
    source.push_str("\n@if(AESTRA_SCENE_POINT_LIGHTING) {\n");
    source.push_str(include_str!("material_lighting.wgsl"));
    source.push_str("\n}\n");
    // Cluster helpers also read the native view. Keep one binding-0 declaration in 3D;
    // two independently declared view globals are invalid even with compatible layouts.
    let declaration = "@group(0) @binding(0)\nvar<uniform> view: View;";
    assert_eq!(
        source.matches(declaration).count(),
        1,
        "portable view binding changed"
    );
    source.replace(
        declaration,
        &format!("@if(!AESTRA_SCENE_POINT_LIGHTING)\n{declaration}"),
    )
}

fn replace_light_callback(source: &mut String, signature: &str, body: &str) {
    let signature = source
        .find(signature)
        .expect("scene-light callback missing");
    let start = signature
        + source[signature..]
            .find('{')
            .expect("scene-light callback body missing");
    let end = start
        + source[start..]
            .find('}')
            .expect("scene-light callback end missing")
        + 1;
    source.replace_range(start..end, body);
}

// Insert into the shared sprite vertex body used by the legacy and material paths.
pub(crate) fn compose_pixel_grid(wesl: &str) -> String {
    const CLIP_POSITION: &str = "output.clip_position = view.clip_from_world * world_position;";
    const SNAP: &str = r#"
    @if(AESTRA_PIXEL_GRID) {
        let viewport_size = view.main_pass_viewport.zw;
        let center_clip = view.clip_from_world * world_center;
        if (all(viewport_size > vec2<f32>(0.0)) && center_clip.w > 0.0) {
            let ndc_to_pixel = vec2<f32>(0.5, -0.5);
            let pixel = (center_clip.xy / center_clip.w * ndc_to_pixel + 0.5) * viewport_size;
            let delta = floor(pixel + 0.5) - pixel;
            output.clip_position = vec4<f32>(
                output.clip_position.xy
                    + delta / (viewport_size * ndc_to_pixel) * output.clip_position.w,
                output.clip_position.zw,
            );
        }
    }
"#;
    wesl.replace(CLIP_POSITION, &format!("{CLIP_POSITION}{SNAP}"))
}

/// Keep the portable compiler/ABI engine-neutral. Only the native adapter wraps
/// the final alpha/additive fragments, after material coverage and lighting are evaluated.
pub(crate) fn compose_fragment_list(wesl: &str, semantic: bool) -> String {
    let position = if semantic {
        "fragment_position"
    } else {
        "clip_position"
    };
    let (entry, input_type) = if semantic {
        (
            aestra_gpu::material::MATERIAL_FRAGMENT_ENTRY_POINT,
            "MaterialFragmentInput",
        )
    } else {
        ("fragment_alpha", "VertexOutput")
    };
    let mut entries = vec![(entry, "aestra_oit_color")];
    if !semantic {
        entries.push(("fragment_additive", "aestra_oit_additive_color"));
    }
    // Shader files retain checkout line endings. Canonicalize before matching
    // entry points and multi-line bindings; keep the exact-once ABI guards.
    let mut source = wesl.replace("\r\n", "\n");
    let mut wrappers = String::new();
    for (entry, color_function) in entries {
        let signature =
            format!("@fragment\nfn {entry}(input: {input_type}) -> @location(0) vec4<f32>");
        assert_eq!(
            source.matches(&signature).count(),
            1,
            "native fragment ABI changed: {entry}"
        );
        source = source.replace(
            &signature,
            &format!("fn {color_function}(input: {input_type}) -> vec4<f32>"),
        );
        wrappers.push_str(&format!(
            r#"
@fragment
fn {entry}(input: {input_type}) -> @location(0) vec4<f32> {{
    let color = {color_function}(input);
    @if(AESTRA_FRAGMENT_LIST_OIT) {{
        if color.a <= 0.0 {{ discard; }}
        // Store coverage-weighted RGB, but use alpha for extinction, not emission
        // intensity: additive light must not absorb later layers.
        var extinction = color.a;
        @if(AESTRA_FRAGMENT_LIST_ADDITIVE) {{ extinction = 0.0; }}
        let payload = vec4<f32>(color.rgb * color.a, extinction);
        // Compare the actual stored representation, never a visibility epsilon.
        // Zero RGB mantissas AND zero extinction are the source-over identity.
        // Black absorbers and zero-extinction light must retain their nodes.
        if (pack_24bit_depth_8bit_alpha(0.0, payload.a) & 0xffu) != 0u ||
            (vec3_to_rgb9e5_(payload.rgb) & 0x07ffffffu) != 0u {{
            oit_draw(input.{position}, payload);
        }}
        discard;
    }}
    return color;
}}
"#
        ));
    }
    let view = if semantic {
        "@group(0) @binding(0)\nvar<uniform> view: View;"
    } else {
        "@group(0) @binding(0) var<uniform> view: View;"
    };
    assert_eq!(
        source.matches(view).count(),
        1,
        "native view binding changed"
    );
    let lit_view = format!("@if(!AESTRA_SCENE_POINT_LIGHTING)\n{view}");
    source = if source.contains(&lit_view) {
        source.replace(
            &lit_view,
            &format!("@if(!AESTRA_SCENE_POINT_LIGHTING && !AESTRA_FRAGMENT_LIST_OIT)\n{view}"),
        )
    } else {
        source.replace(view, &format!("@if(!AESTRA_FRAGMENT_LIST_OIT)\n{view}"))
    };
    format!(
        r#"@if(AESTRA_FRAGMENT_LIST_OIT)
import bevy_core_pipeline::oit::draw::{{oit_draw, pack_24bit_depth_8bit_alpha}};
@if(AESTRA_FRAGMENT_LIST_OIT)
import bevy_pbr::render::rgb9e5::vec3_to_rgb9e5_;
@if(AESTRA_FRAGMENT_LIST_OIT && !AESTRA_SCENE_POINT_LIGHTING)
import bevy_pbr::render::mesh_view_bindings::view;
{source}
{wrappers}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_volume_keeps_body_and_depth_condition_local() {
        let march = "fn march(ray: AestraVolumeRay) -> vec4<f32> { return vec4<f32>(ray.t_far); }";
        let wesl = compose_volume(march, "march");
        assert!(wesl.contains("@if(DEPTH_PREPASS) {\n    let depth"));
        assert!(wesl.contains("import bevy_pbr::prepass::utils as prepass_utils;"));
        assert!(wesl.contains("@group(constants::MATERIAL_BIND_GROUP)"));
        assert!(!wesl.contains('#'));
        assert!(wesl.contains(march));
        assert!(wesl.contains("return march(AestraVolumeRay("));
        assert!(wesl.contains(SCENE_LIGHTING_WGSL));
    }

    #[test]
    fn fragment_list_wraps_alpha_and_additive_without_changing_portable_source() {
        let source = compose_fragment_list(aestra_gpu::shader::SPRITE_RENDER_WESL, false);
        assert!(source.contains("let payload = vec4<f32>(color.rgb * color.a, extinction)"));
        assert!(source.contains("oit_draw(input.clip_position, payload)"));
        assert!(source.contains("fn aestra_oit_color(input: VertexOutput)"));
        assert!(source.contains("fn fragment_additive(input: VertexOutput)"));
        assert!(source.contains("fn aestra_oit_additive_color(input: VertexOutput)"));
        assert_eq!(
            source
                .matches("@if(AESTRA_FRAGMENT_LIST_ADDITIVE) { extinction = 0.0; }")
                .count(),
            2
        );
        assert!(!aestra_gpu::shader::SPRITE_RENDER_WESL.contains("oit_draw"));
    }

    #[test]
    fn fragment_list_legacy_composition_is_independent_of_checkout_line_endings() {
        // include_str! preserves checkout bytes, unlike Rust source literals.
        let lf = aestra_gpu::shader::SPRITE_RENDER_WESL.replace("\r\n", "\n");
        let expected = compose_fragment_list(&lf, false);
        for input in [
            lf.replace('\n', "\r\n"),
            lf.replace("@fragment\n", "@fragment\r\n"),
        ] {
            assert_eq!(compose_fragment_list(&input, false), expected);
        }
    }

    #[test]
    fn fragment_list_still_rejects_changed_or_duplicate_fragment_signatures() {
        let lf = aestra_gpu::shader::SPRITE_RENDER_WESL.replace("\r\n", "\n");
        for input in [
            lf.replace(
                "fragment_alpha(input: VertexOutput)",
                "fragment_alpha(input: UnexpectedInput)",
            ),
            format!(
                "{lf}\n@fragment\nfn fragment_alpha(input: VertexOutput) -> @location(0) vec4<f32> {{}}"
            ),
        ] {
            assert!(std::panic::catch_unwind(|| compose_fragment_list(&input, false)).is_err());
        }
    }

    #[test]
    fn fragment_list_semantic_bridge_validates_the_fragment_position_abi() {
        let program = aestra_core::material::MaterialProgram::additive_sprite("OIT interface");
        let ir = aestra_compiler::MaterialCompiler.compile(&program).unwrap();
        let compiled = aestra_gpu::material::MaterialShaderCompiler
            .compile(
                &ir,
                &aestra_gpu::material::MaterialBackendCapabilities::portable_minimum(),
            )
            .unwrap();
        let lf = compiled.shader.wgsl.replace("\r\n", "\n");
        let source = compose_fragment_list(&lf, true);
        for input in [
            lf.replace('\n', "\r\n"),
            lf.replace("@fragment\n", "@fragment\r\n"),
        ] {
            assert_eq!(compose_fragment_list(&input, true), source);
        }
        // Check the actual input struct with Naga, not only a generated string.
        // Native tests separately qualify Bevy's real imported implementation.
        let source = source.replace("@if(AESTRA_FRAGMENT_LIST_OIT)\nimport bevy_core_pipeline::oit::draw::{oit_draw, pack_24bit_depth_8bit_alpha};", "")
            .replace("@if(AESTRA_FRAGMENT_LIST_OIT)\nimport bevy_pbr::render::rgb9e5::vec3_to_rgb9e5_;", "")
            .replace("@if(AESTRA_FRAGMENT_LIST_OIT && !AESTRA_SCENE_POINT_LIGHTING)\nimport bevy_pbr::render::mesh_view_bindings::view;", "")
            .replace("@if(!AESTRA_FRAGMENT_LIST_OIT)", "")
            .replace("@if(AESTRA_FRAGMENT_LIST_OIT)", "")
            .replace("@if(AESTRA_FRAGMENT_LIST_ADDITIVE)", "");
        let source = format!(
            "{source}\nfn oit_draw(position: vec4<f32>, color: vec4<f32>) {{}}\nfn pack_24bit_depth_8bit_alpha(depth: f32, alpha: f32) -> u32 {{ return 0u; }}\nfn vec3_to_rgb9e5_(rgb: vec3<f32>) -> u32 {{ return 0u; }}"
        );
        let module = naga::front::wgsl::parse_str(&source).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .unwrap();
    }
}
