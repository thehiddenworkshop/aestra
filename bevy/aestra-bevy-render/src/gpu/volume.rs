//! Volume presentations of extension stages (fluid F3).
//!
//! A stage lowerer may present its grid fields as a volume ([`VolumePresentation`]): the plugin ships a
//! WGSL march function, and this module does the rest, generically:
//!
//! - **Fields become 3-D textures.** Each presented field gets an `rgba16float` 3-D image; after the
//!   stages advance each frame, a compute pass copies the field's buffer into it
//!   ([`aestra_gpu::volume::FIELD_TO_VOLUME_WGSL`]), so the march samples with hardware trilinear
//!   filtering. A bricked grid's fields (fluid F7) become brick atlases, sized by the grid's slots,
//!   not its extent, with an `r32uint` table image the interface finds each brick's block with.
//! - **The box is a mesh.** A unit cube, a child of the effect scaled to the grid's extent, draws with
//!   [`VolumeMaterial`]. Only its back faces are rasterized and the depth test is off: the fragment
//!   rebuilds the view ray in the box's space, clips it to the box (the camera may be inside) and to the
//!   opaque scene through the depth prepass Aestra enables on 3-D cameras, then calls the plugin's
//!   march function, which returns premultiplied colour.
//! - **One material type serves every plugin.** The composed shader (Bevy prelude + the portable
//!   interface from [`aestra_gpu::volume`] + the plugin's WGSL + the entry points) is created once per
//!   program and march function and swapped in at pipeline specialization.
//!
//! Volumes need a 3-D camera and Bevy's PBR plugin; without them the debug field slices remain.

use super::*;
use aestra_compiler::ExtensionRegistry;
use aestra_core::{ComputeProgramId, ResourceTypeId};
use aestra_gpu::volume::volume_interface_wgsl_with_scene_lighting;
use aestra_runtime::{
    CompiledEffect, FieldLayout, MAX_VOLUME_CONSTANTS, StagePresentation, VolumePresentation,
};
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    mesh::MeshVertexBufferLayoutRef,
    pbr::{Material, MaterialPipeline, MaterialPipelineKey, MaterialPlugin, MeshMaterial3d},
    render::render_resource::{
        AsBindGroup, CompareFunction, Face, RenderPipelineDescriptor, ShaderType,
        SpecializedMeshPipelineError, TextureUsages,
    },
    shader::Shader,
};
use std::collections::HashMap;

/// The uniform [`VolumeMaterial`] binds, in the layout of the interface's `AestraVolumeParams`.
#[derive(Clone, Copy, PartialEq, ShaderType)]
pub(crate) struct VolumeParams {
    size: Vec4,
    dims: UVec4,
    bricks: UVec4,
    constants: [UVec4; MAX_VOLUME_CONSTANTS / 4],
}

impl VolumeParams {
    fn new(layout: &FieldLayout, fields: usize, constants: &[u32]) -> Self {
        let mut words = [UVec4::ZERO; MAX_VOLUME_CONSTANTS / 4];
        for (index, word) in constants.iter().take(MAX_VOLUME_CONSTANTS).enumerate() {
            words[index / 4][index % 4] = *word;
        }
        Self {
            size: Vec3::from(layout.dims.map(|cells| cells as f32 * layout.cell_size))
                .extend(layout.cell_size),
            dims: UVec3::from(layout.dims).extend(fields as u32),
            bricks: layout.bricks.as_ref().map_or(UVec4::ZERO, |bricks| {
                UVec4::new(
                    bricks.edge,
                    aestra_gpu::volume::brick_atlas_bricks(bricks.slots),
                    0,
                    0,
                )
            }),
            constants: words,
        }
    }
}

/// Draws one volume presentation: its parameters, field textures and composed shader.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
#[bind_group_data(VolumeMaterialKey)]
pub(crate) struct VolumeMaterial {
    #[uniform(0)]
    params: VolumeParams,
    #[texture(1, dimension = "3d")]
    #[sampler(2)]
    field_0: Handle<Image>,
    #[texture(3, dimension = "3d")]
    field_1: Handle<Image>,
    #[texture(4, dimension = "3d")]
    field_2: Handle<Image>,
    #[texture(5, dimension = "3d")]
    field_3: Handle<Image>,
    /// A bricked grid's brick table (fluid F7); a placeholder otherwise.
    #[texture(6, dimension = "3d", sample_type = "u_int")]
    table: Handle<Image>,
    shader: Handle<Shader>,
}

/// The pipeline key: which composed shader draws the volume.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct VolumeMaterialKey {
    shader: Handle<Shader>,
}

impl From<&VolumeMaterial> for VolumeMaterialKey {
    fn from(material: &VolumeMaterial) -> Self {
        Self {
            shader: material.shader.clone(),
        }
    }
}

impl Material for VolumeMaterial {
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Premultiplied
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let shader = key.bind_group_data.shader;
        descriptor.vertex.shader = shader.clone();
        descriptor.vertex.entry_point = Some("vertex".into());
        if let Some(fragment) = descriptor.fragment.as_mut() {
            fragment.shader = shader;
            fragment.entry_point = Some("fragment".into());
        }
        // Back faces only, whatever the depth: the fragment clips the ray to the box and the scene.
        descriptor.primitive.cull_mode = Some(Face::Front);
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_compare = Some(CompareFunction::Always);
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

/// The full shader of a march function: Bevy's view and mesh prelude, the portable interface, the
/// plugin's WGSL, and the vertex and fragment entry points that set up the ray.
pub(crate) fn compose_volume_shader(program_wgsl: &str, entry_point: &str) -> String {
    format!(
        r#"#import bevy_pbr::mesh_functions
#import bevy_pbr::mesh_view_bindings::view
#import bevy_pbr::mesh_view_bindings as volume_scene
#import bevy_pbr::clustered_forward as volume_clusters
#import bevy_pbr::view_transformations::{{position_world_to_clip, position_ndc_to_world, frag_coord_to_ndc}}
#ifdef DEPTH_PREPASS
#import bevy_pbr::prepass_utils
#endif
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
#ifdef DEPTH_PREPASS
    let depth = prepass_utils::prepass_depth(in.clip, 0u);
    if (depth > 0.0) {{
        let scene = position_ndc_to_world(frag_coord_to_ndc(vec4<f32>(in.clip.xy, depth, 1.0)));
        let scene_local = (local_from_world * vec4<f32>(scene, 1.0)).xyz;
        span.y = min(span.y, length((scene_local - camera) * size));
    }}
#endif
    if (span.y <= span.x) {{
        discard;
    }}
    return {entry_point}(AestraVolumeRay(origin, direction, span.x, span.y, size, in.clip.xy));
}}
"#,
        interface = volume_interface_wgsl_with_scene_lighting(
            "#{MATERIAL_BIND_GROUP}",
            SCENE_LIGHTING_WGSL,
        ),
    )
}

// Invocation-private state: each ray converts uvw with its own full box transform, including
// nested rotation/nonuniform scale. No light positions or particle selection are read back.
// Point lights only, unshadowed isotropic single scattering. Each sample visits at most 32 cluster
// entries (including inactive entries); this is a cost bound, not strongest-light selection.
const SCENE_LIGHTING_WGSL: &str = include_str!("volume_lighting.wgsl");

/// A field the render world copies into a volume texture each frame.
#[derive(Clone)]
pub(super) struct VolumeFieldTarget {
    pub stage: usize,
    pub layout: FieldLayout,
    pub image: AssetId<Image>,
    /// A bricked field's table image (fluid F7).
    pub table: Option<AssetId<Image>>,
}

/// What one presented volume was built for; a change rebuilds its entity and textures.
#[derive(Clone, PartialEq)]
struct VolumeKey {
    stage: usize,
    program: ComputeProgramId,
    entry_point: String,
    fields: Vec<ResourceTypeId>,
    layouts: Vec<FieldLayout>,
}

struct VolumeView {
    key: VolumeKey,
    entity: Entity,
    material: Handle<VolumeMaterial>,
    images: Vec<Handle<Image>>,
    table: Option<Handle<Image>>,
}

/// Main-world state of an effect's volumes.
#[derive(Component, Default)]
pub(super) struct VolumeViews {
    views: Vec<VolumeView>,
}

impl VolumeViews {
    /// The field copies the render world performs for these volumes.
    pub(super) fn targets(&self) -> Vec<VolumeFieldTarget> {
        self.views
            .iter()
            .flat_map(|view| {
                view.key
                    .layouts
                    .iter()
                    .zip(&view.images)
                    .map(|(layout, image)| VolumeFieldTarget {
                        stage: view.key.stage,
                        layout: layout.clone(),
                        image: image.id(),
                        table: view.table.as_ref().map(Handle::id),
                    })
            })
            .collect()
    }

    /// Whether any volume of the effect is drawn (its automatic debug slices then stay off).
    pub(super) fn draws_any(&self) -> bool {
        !self.views.is_empty()
    }
}

/// Every volume the effect's stages present, with its stage index and field layouts.
fn presented_volumes(effect: &CompiledEffect) -> Vec<(VolumeKey, &VolumePresentation)> {
    effect
        .all_extension_stages()
        .enumerate()
        .flat_map(|(stage, compiled)| {
            compiled
                .presentations
                .iter()
                .filter_map(move |presentation| {
                    let StagePresentation::Volume(volume) = presentation;
                    let layouts = volume.layouts(&compiled.block).ok()?;
                    Some((
                        VolumeKey {
                            stage,
                            program: volume.program.clone(),
                            entry_point: volume.entry_point.clone(),
                            fields: volume.fields.clone(),
                            layouts: layouts.into_iter().cloned().collect(),
                        },
                        volume,
                    ))
                })
        })
        .collect()
}

/// Composed volume shaders by program and march function.
#[derive(Resource, Default)]
struct VolumeShaders(HashMap<(ComputeProgramId, String), Handle<Shader>>);

/// The texture a field is copied into: the grid's extent, or a bricked grid's atlas.
fn field_image(layout: &FieldLayout) -> Image {
    match &layout.bricks {
        Some(bricks) => {
            volume_image([aestra_gpu::volume::brick_atlas_texels(bricks.slots, bricks.edge); 3])
        }
        None => volume_image(layout.dims),
    }
}

/// A bricked grid's table image: one `u32` texel per brick, read without filtering.
fn table_image(grid: [u32; 3]) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: grid[0],
            height: grid[1],
            depth_or_array_layers: grid[2],
        },
        TextureDimension::D3,
        &[0; 4],
        TextureFormat::R32Uint,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage |= TextureUsages::STORAGE_BINDING;
    image
}

fn volume_image(dims: [u32; 3]) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width: dims[0],
            height: dims[1],
            depth_or_array_layers: dims[2],
        },
        TextureDimension::D3,
        &[0; 8],
        TextureFormat::Rgba16Float,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage |= TextureUsages::STORAGE_BINDING;
    let clamp = ImageAddressMode::ClampToEdge;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: clamp,
        address_mode_v: clamp,
        address_mode_w: clamp,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

/// The asset stores volumes need; absent without Bevy's PBR plugin.
type VolumeAssets<'w> = (
    Option<ResMut<'w, Assets<Image>>>,
    Option<ResMut<'w, Assets<Mesh>>>,
    Option<ResMut<'w, Assets<VolumeMaterial>>>,
    Option<ResMut<'w, Assets<Shader>>>,
);

/// Creates, updates or removes each effect's volume entities.
#[allow(clippy::type_complexity)]
fn sync_volume_views(
    mut commands: Commands,
    effects: Query<(
        Entity,
        &PresentedEffect,
        Option<&mut VolumeViews>,
        Option<&RenderLayers>,
    )>,
    cameras_3d: Query<(), With<Camera3d>>,
    mut shaders_cache: ResMut<VolumeShaders>,
    // One-texel 3-D images bound to unused field slots and, for a grid storing every cell, the table.
    mut placeholders: Local<Option<(Handle<Image>, Handle<Image>)>>,
    mut unit_cube: Local<Option<Handle<Mesh>>>,
    assets: VolumeAssets,
) {
    let (Some(mut images), Some(mut meshes), Some(mut materials), Some(mut shaders)) = assets
    else {
        return;
    };
    let (placeholder, placeholder_table) = placeholders
        .get_or_insert_with(|| {
            (
                images.add(volume_image([1, 1, 1])),
                images.add(table_image([1, 1, 1])),
            )
        })
        .clone();
    let unit_cube = unit_cube
        .get_or_insert_with(|| meshes.add(Cuboid::from_length(1.0)))
        .clone();
    let has_3d_camera = !cameras_3d.is_empty();
    for (entity, presented, views, layers) in effects {
        let wanted = if has_3d_camera {
            presented_volumes(presented.effect())
        } else {
            Vec::new()
        };
        if views.is_none() && wanted.is_empty() {
            continue;
        }
        let mut fresh = None;
        let views: &mut VolumeViews = match views {
            Some(views) => views.into_inner(),
            None => fresh.insert(VolumeViews::default()),
        };
        let unchanged = views.views.len() == wanted.len()
            && views
                .views
                .iter()
                .zip(&wanted)
                .all(|(view, (key, _))| &view.key == key);
        if unchanged {
            // Only the constants can differ: a look edit updates the material in place.
            for (view, (key, volume)) in views.views.iter().zip(&wanted) {
                let params =
                    VolumeParams::new(&key.layouts[0], key.fields.len(), &volume.constants);
                if materials
                    .get(&view.material)
                    .is_some_and(|material| material.params != params)
                    && let Some(mut material) = materials.get_mut(&view.material)
                {
                    material.params = params;
                }
            }
            continue;
        }
        for view in views.views.drain(..) {
            commands.entity(view.entity).despawn();
        }
        for (key, volume) in wanted {
            let shader = shaders_cache
                .0
                .entry((key.program.clone(), key.entry_point.clone()))
                .or_insert_with(|| {
                    let programs = ExtensionRegistry::linked().programs;
                    let wgsl = programs
                        .get(&key.program)
                        .map_or_else(String::new, |program| program.wgsl.clone());
                    shaders.add(Shader::from_wgsl(
                        compose_volume_shader(&wgsl, &key.entry_point),
                        format!("aestra-volume/{}/{}", key.program.as_str(), key.entry_point),
                    ))
                })
                .clone();
            let layout = &key.layouts[0];
            let table = layout
                .bricks
                .as_ref()
                .map(|bricks| images.add(table_image(bricks.grid(layout.dims))));
            let images: Vec<Handle<Image>> = key
                .layouts
                .iter()
                .map(|field| images.add(field_image(field)))
                .collect();
            let slot = |index: usize| images.get(index).unwrap_or(&placeholder).clone();
            let material = materials.add(VolumeMaterial {
                params: VolumeParams::new(layout, key.fields.len(), &volume.constants),
                field_0: slot(0),
                field_1: slot(1),
                field_2: slot(2),
                field_3: slot(3),
                table: table.clone().unwrap_or_else(|| placeholder_table.clone()),
                shader,
            });
            let size = Vec3::from(layout.dims.map(|cells| cells as f32 * layout.cell_size));
            let center = Vec3::from(layout.origin) + size * 0.5;
            let mut volume_entity = commands.spawn((
                Mesh3d(unit_cube.clone()),
                MeshMaterial3d(material.clone()),
                Transform::from_translation(center).with_scale(size),
                ChildOf(entity),
            ));
            if let Some(layers) = layers {
                volume_entity.insert(layers.clone());
            }
            views.views.push(VolumeView {
                entity: volume_entity.id(),
                key,
                material,
                images,
                table,
            });
        }
        if let Some(fresh) = fresh {
            commands.entity(entity).insert(fresh);
        }
    }
}

pub(super) fn install(app: &mut App) {
    if !app.is_plugin_added::<bevy::pbr::PbrPlugin>() {
        return; // no 3-D materials: volumes are not drawn, debug slices remain
    }
    app.add_plugins(MaterialPlugin::<VolumeMaterial>::default())
        .init_resource::<VolumeShaders>()
        .add_systems(
            Update,
            sync_volume_views
                .before(super::extension_stages::sync_stage_inputs)
                .in_set(crate::AestraRenderSet::Prepare),
        );
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app.add_systems(RenderStartup, init_field_volume_pipeline);
}

/// Copies grid fields into volume textures (render world).
#[derive(Resource)]
pub(super) struct FieldVolume(pub crate::execution::FieldVolumePipeline);

fn init_field_volume_pipeline(mut commands: Commands, device: Res<RenderDevice>) {
    commands.insert_resource(FieldVolume(crate::execution::FieldVolumePipeline::new(
        device.wgpu_device(),
    )));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_light_helper_validates_with_cluster_binding_contract() {
        // Parse the real adapter helper even in GPU-free CI. Stand-ins only replace Bevy's
        // imported namespaces/types; native image tests additionally exercise the real bindings.
        let helper = SCENE_LIGHTING_WGSL
            .replace("volume_clusters::", "volume_clusters_")
            .replace("volume_scene::", "volume_scene_");
        let source = format!(
            r#"
struct View {{ view_from_world: mat4x4<f32>, clip_from_view: mat4x4<f32> }}
struct Light {{ position_radius: vec4<f32>, color_inverse_square_range: vec4<f32>, range: f32 }}
struct Lights {{ data: array<Light, 64> }}
struct Indices {{ first_point_light_index_offset: u32, first_spot_light_index_offset: u32 }}
@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> volume_scene_clustered_lights: Lights;
fn volume_clusters_view_fragment_cluster_index(pixel: vec2<f32>, z: f32, ortho: bool) -> u32 {{ return 0u; }}
fn volume_clusters_unpack_clusterable_object_index_ranges(index: u32) -> Indices {{ return Indices(0u, 1u); }}
fn volume_clusters_get_clusterable_object_id(index: u32) -> u32 {{ return index; }}
{helper}
@fragment fn main(@builtin(position) pixel: vec4<f32>) -> @location(0) vec4<f32> {{
    return vec4<f32>(aestra_volume_scene_lighting(vec3<f32>(0.5), pixel.xy, 8u), 1.0);
}}
"#
        );
        let module = naga::front::wgsl::parse_str(&source).expect("scene lighting parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .expect("scene lighting validates");
        assert!(helper.contains("min(max_lights, 32u)"));
        assert!(helper.contains("view.view_from_world * world"));
    }

    #[test]
    fn a_composed_volume_shader_parses_and_validates_without_the_bevy_prelude() {
        // The Bevy imports are resolved by Bevy's composer at run time; the interface, a march
        // function and the entry-point body must be valid WGSL on their own.
        let composed = compose_volume_shader(
            "fn march(ray: AestraVolumeRay) -> vec4<f32> {\n    \
             return aestra_volume_field(0u, ray.origin) * (ray.t_far - ray.t_near);\n}",
            "march",
        );
        assert!(composed.contains("return march(AestraVolumeRay("));
        assert!(composed.contains(SCENE_LIGHTING_WGSL));
        assert!(composed.contains(
            "aestra_volume_world_from_grid = mesh_functions::get_world_from_local(in.instance)"
        ));
        // Keep validating the actual composed interface/march, replacing only the separately
        // validated Bevy-dependent helper with the neutral portable implementation.
        let portable = composed.replace(
            SCENE_LIGHTING_WGSL,
            "fn aestra_volume_scene_lighting(uvw: vec3<f32>, pixel: vec2<f32>, max_lights: u32) -> vec3<f32> { return vec3<f32>(0.0); }",
        );
        let interface_and_march = portable
            .split("\nstruct AestraVolumeVertex")
            .next()
            .unwrap()
            .lines()
            .filter(|line| !line.starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n")
            .replace("#{MATERIAL_BIND_GROUP}", "0");
        let module = naga::front::wgsl::parse_str(&interface_and_march).expect("parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .expect("validates");
    }

    fn smoke(look: Option<(&str, aestra_core::Value)>) -> Arc<CompiledEffect> {
        let mut registry = ExtensionRegistry::builtin();
        registry.install(&aestra_fluid::FluidExtension).unwrap();
        let mut effect = aestra_fluid::smoke_effect(&registry);
        if let Some((name, value)) = look {
            let module = effect.simulation_stages[0]
                .modules
                .iter_mut()
                .find(|module| module.module_type.0 == aestra_fluid::MODULE_VOLUME_LOOK)
                .unwrap();
            let aestra_core::ModuleParameters::Custom(values) = &mut module.parameters else {
                unreachable!("plugin modules carry a generic payload");
            };
            values.insert(name.into(), value);
        }
        Arc::new(
            aestra_compiler::EffectCompiler::with_extensions(registry)
                .compile(&effect)
                .unwrap(),
        )
    }

    fn volume_app() -> App {
        aestra_fluid::link();
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .init_asset::<Image>()
            .init_asset::<Mesh>()
            .init_asset::<Shader>()
            .init_asset::<VolumeMaterial>()
            .init_resource::<VolumeShaders>()
            .add_systems(Update, sync_volume_views);
        app.world_mut().spawn(Camera3d::default());
        app
    }

    fn volume_entities(app: &mut App) -> Vec<(Entity, Handle<VolumeMaterial>)> {
        app.world_mut()
            .query::<(Entity, &MeshMaterial3d<VolumeMaterial>)>()
            .iter(app.world())
            .map(|(entity, material)| (entity, material.0.clone()))
            .collect()
    }

    #[test]
    fn a_presented_volume_gets_a_box_its_textures_and_live_look_edits() {
        let mut app = volume_app();
        let effect = app
            .world_mut()
            .spawn(PresentedEffect::new(smoke(None)))
            .id();
        app.update();
        let [(volume, material)] = volume_entities(&mut app).try_into().unwrap();
        let world = app.world();
        assert_eq!(world.get::<ChildOf>(volume).unwrap().parent(), effect);
        // The box spans the grid: 32 cells of 3 units around (0, 48, 0).
        let transform = world.get::<Transform>(volume).unwrap();
        assert_eq!(transform.scale, Vec3::splat(96.0));
        assert_eq!(transform.translation, Vec3::new(0.0, 48.0, 0.0));
        let targets = world.get::<VolumeViews>(effect).unwrap().targets();
        assert_eq!(targets.len(), 1, "the density field is copied each frame");
        assert_eq!(targets[0].layout.dims, [32; 3]);
        let image = world
            .resource::<Assets<Image>>()
            .get(targets[0].image)
            .unwrap();
        assert_eq!(image.texture_descriptor.dimension, TextureDimension::D3);
        let steps = |app: &App| {
            app.world()
                .resource::<Assets<VolumeMaterial>>()
                .get(&material)
                .unwrap()
                .params
                .constants[0]
                .x
        };
        assert_eq!(steps(&app), 48);

        // A look edit (the editor swaps the player in place) updates the same material.
        *app.world_mut().get_mut::<PresentedEffect>(effect).unwrap() =
            PresentedEffect::new(smoke(Some(("steps", aestra_core::Value::U32(96)))));
        app.update();
        assert_eq!(volume_entities(&mut app), [(volume, material.clone())]);
        assert_eq!(steps(&app), 96);

        for (name, value, index, expected) in [
            (
                "scene_light_intensity",
                aestra_core::Value::Scalar(1.0),
                21,
                1.0_f32.to_bits(),
            ),
            ("scene_light_limit", aestra_core::Value::U32(0), 22, 0),
        ] {
            *app.world_mut().get_mut::<PresentedEffect>(effect).unwrap() =
                PresentedEffect::new(smoke(Some((name, value))));
            app.update();
            assert_eq!(volume_entities(&mut app), [(volume, material.clone())]);
            assert_eq!(
                app.world().get::<VolumeViews>(effect).unwrap().targets()[0].image,
                targets[0].image
            );
            let params = &app
                .world()
                .resource::<Assets<VolumeMaterial>>()
                .get(&material)
                .unwrap()
                .params;
            assert_eq!(params.constants[index / 4][index % 4], expected);
        }

        // Without a 3-D camera there is nothing to draw it with.
        let cameras: Vec<Entity> = app
            .world_mut()
            .query_filtered::<Entity, With<Camera3d>>()
            .iter(app.world())
            .collect();
        for camera in cameras {
            app.world_mut().despawn(camera);
        }
        app.update();
        app.update();
        assert!(volume_entities(&mut app).is_empty());
        assert!(!app.world().get::<VolumeViews>(effect).unwrap().draws_any());
    }

    #[test]
    fn volume_params_pack_the_grid_and_the_constants() {
        let layout = FieldLayout {
            resource: ResourceTypeId::new("test::resource/density"),
            dims: [16, 8, 4],
            components: 1,
            origin: [0.0; 3],
            cell_size: 0.5,
            staggered: false,
            bricks: None,
        };
        let params = VolumeParams::new(&layout, 2, &[7, 8, 9, 10, 11]);
        assert_eq!(params.size, Vec4::new(8.0, 4.0, 2.0, 0.5));
        assert_eq!(params.dims, UVec4::new(16, 8, 4, 2));
        assert_eq!(params.constants[0], UVec4::new(7, 8, 9, 10));
        assert_eq!(params.constants[1], UVec4::new(11, 0, 0, 0));
        let legacy = VolumeParams::new(&layout, 1, &[0; 21]);
        assert_eq!(
            legacy.constants[5].y, 0,
            "old artifacts have zero scene gain"
        );
        assert_eq!(
            legacy.constants[5].z, 0,
            "old artifacts have zero scene budget"
        );
    }
}
