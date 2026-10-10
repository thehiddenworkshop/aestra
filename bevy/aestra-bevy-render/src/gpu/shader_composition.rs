//! Engine-boundary shader composition. The shader bodies are shared verbatim.
//! The 0.20 dialect is qualified against real Bevy modules in patches-020, but is
//! test-only until the renderer/editor engine switch selects Shader::from_wesl.
use aestra_gpu::volume::volume_interface_wgsl_with_scene_lighting;

#[derive(Clone, Copy)]
pub(crate) enum Dialect {
    Bevy019,
    #[cfg(test)]
    Bevy020,
}

const VOLUME_IMPORTS_019: &str = r#"#import bevy_pbr::mesh_functions
#import bevy_pbr::mesh_view_bindings::view
#import bevy_pbr::mesh_view_bindings as volume_scene
#import bevy_pbr::clustered_forward as volume_clusters
#import bevy_pbr::view_transformations::{position_world_to_clip, position_ndc_to_world, frag_coord_to_ndc}
#ifdef DEPTH_PREPASS
#import bevy_pbr::prepass_utils
#endif"#;

#[cfg(test)]
const VOLUME_IMPORTS_020: &str = r#"import bevy_pbr::render::{
    mesh_functions,
    mesh_view_bindings::{view},
    mesh_view_bindings as volume_scene,
    clustered_forward as volume_clusters,
    view_transformations::{position_world_to_clip, position_ndc_to_world, frag_coord_to_ndc},
};
@if(DEPTH_PREPASS)
import bevy_pbr::prepass::utils as prepass_utils;"#;

#[cfg(test)]
const MATERIAL_IMPORTS_020: &str = r#"@if(AESTRA_SCENE_POINT_LIGHTING)
import bevy_pbr::render::mesh_view_bindings::view;
@if(AESTRA_SCENE_POINT_LIGHTING)
import bevy_pbr::render::mesh_view_bindings as aestra_sprite_scene;
@if(AESTRA_SCENE_POINT_LIGHTING)
import bevy_pbr::render::clustered_forward as aestra_sprite_clusters;
"#;

pub(crate) fn compose_volume(program_wgsl: &str, entry_point: &str, dialect: Dialect) -> String {
    let (imports, depth_start, depth_end, group) = match dialect {
        Dialect::Bevy019 => (
            VOLUME_IMPORTS_019,
            "#ifdef DEPTH_PREPASS",
            "#endif",
            "#{MATERIAL_BIND_GROUP}",
        ),
        #[cfg(test)]
        Dialect::Bevy020 => (
            VOLUME_IMPORTS_020,
            "@if(DEPTH_PREPASS) {",
            "}",
            "constants::MATERIAL_BIND_GROUP",
        ),
    };
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
        interface = volume_interface_wgsl_with_scene_lighting(group, SCENE_LIGHTING_WGSL,),
    )
}

// Invocation-private sample transforms and the fixed visited-entry budget are unchanged.
const SCENE_LIGHTING_WGSL: &str = include_str!("volume_lighting.wgsl");

pub(crate) fn compose_material(wgsl: &str, enabled: bool, dialect: Dialect) -> String {
    if !enabled {
        return wgsl.to_owned();
    }
    // The portable compiler owns this callback ABI. Fail loudly if its definition changes;
    // silently leaving a neutral shader would make a newly authored lit material look broken.
    let start = wgsl
        .find("fn aestra_scene_point_irradiance(")
        .expect("scene-light callback missing");
    let body = start
        + wgsl[start..]
            .find('{')
            .expect("scene-light callback body missing");
    let end = body
        + wgsl[body..]
            .find('}')
            .expect("scene-light callback end missing")
        + 1;
    let mut source = String::with_capacity(wgsl.len() + 3000);
    source.push_str(match dialect {
        Dialect::Bevy019 => "#ifdef AESTRA_SCENE_POINT_LIGHTING\n#import bevy_pbr::mesh_view_bindings::view\n#import bevy_pbr::mesh_view_bindings as aestra_sprite_scene\n#import bevy_pbr::clustered_forward as aestra_sprite_clusters\n#endif\n",
        #[cfg(test)]
        Dialect::Bevy020 => MATERIAL_IMPORTS_020,
    });
    source.push_str(&wgsl[..body]);
    source.push_str(match dialect {
        Dialect::Bevy019 => "{\n#ifdef AESTRA_SCENE_POINT_LIGHTING\n    return aestra_bevy_point_irradiance(world, pixel);\n#else\n    return vec3<f32>(0.0);\n#endif\n}",
        #[cfg(test)]
        Dialect::Bevy020 => "{\n    @if(AESTRA_SCENE_POINT_LIGHTING) { return aestra_bevy_point_irradiance(world, pixel); }\n    @else { return vec3<f32>(0.0); }\n}",
    });
    source.push_str(&wgsl[end..]);
    source.push_str(match dialect {
        Dialect::Bevy019 => "\n#ifdef AESTRA_SCENE_POINT_LIGHTING\n",
        #[cfg(test)]
        Dialect::Bevy020 => "\n@if(AESTRA_SCENE_POINT_LIGHTING) {\n",
    });
    source.push_str(include_str!("material_lighting.wgsl"));
    source.push_str(match dialect {
        Dialect::Bevy019 => "\n#endif\n",
        #[cfg(test)]
        Dialect::Bevy020 => "\n}\n",
    });
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
        &match dialect {
            Dialect::Bevy019 => {
                format!("#ifndef AESTRA_SCENE_POINT_LIGHTING\n{declaration}\n#endif")
            }
            #[cfg(test)]
            Dialect::Bevy020 => format!("@if(!AESTRA_SCENE_POINT_LIGHTING)\n{declaration}"),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialects_share_volume_body_and_keep_depth_condition_local() {
        let march = "fn march(ray: AestraVolumeRay) -> vec4<f32> { return vec4<f32>(ray.t_far); }";
        let legacy = compose_volume(march, "march", Dialect::Bevy019);
        let wesl = compose_volume(march, "march", Dialect::Bevy020);
        assert!(
            legacy.contains("#import bevy_pbr::view_transformations::{position_world_to_clip,")
        );
        assert!(legacy.contains("#ifdef DEPTH_PREPASS\n    let depth"));
        assert!(wesl.contains("@if(DEPTH_PREPASS) {\n    let depth"));
        assert!(wesl.contains("import bevy_pbr::prepass::utils as prepass_utils;"));
        assert!(wesl.contains("@group(constants::MATERIAL_BIND_GROUP)"));
        assert!(!wesl.contains('#'));
        for source in [&legacy, &wesl] {
            assert!(source.contains(march));
            assert!(source.contains("return march(AestraVolumeRay("));
            assert!(source.contains(SCENE_LIGHTING_WGSL));
        }
    }
}
