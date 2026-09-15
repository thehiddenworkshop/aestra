//! Standalone-material thumbnails via a synthesized one-instance GPU scene (M-MG2).
//!
//! The CPU rasterizer (`material_graph::asset_preview`) renders simple materials and rejects
//! ones that sample textures, use screen derivatives, or displace vertices — those route here.
//! We wrap the material program in a minimal scene by domain — a single static camera-facing
//! sprite (Sprite), a unit sphere (Mesh), or a short gravity-curved strand (Ribbon) — and feed
//! it through the shared [`effect::assemble`] + `GpuJob` scaffolding.
//!
//! A *standalone* material has no concrete texture — the texture is an instance/effect-level
//! input — so we bind a semantically **neutral** texture per slot (white for color, flat normal
//! for data). That is the faithful preview of the material program in isolation; borrowing a
//! consuming effect's texture would misrepresent one arbitrary use as the material's own.
use super::effect::{self, Assembled, Prepared};
use super::*;
use aestra_core::{
    AssetDefinition, AssetId, AssetKind, ColorKey, Curve, CurveKey, EffectAsset,
    EffectPlaybackMode, Emitter, EmitterShape, Gradient, MaterialFunctionId, MaterialId,
    ModuleInstance, ModuleParameters, RENDERER_MESH, RENDERER_RIBBON, RendererId, RendererInstance,
    RendererProperties, RendererTypeId, ScalarRange,
    material::{
        MaterialDomain, MaterialExpressionKind, MaterialFunction, MaterialInput, MaterialInstance,
        MaterialProgram, MaterialProgramRef, MaterialTextureColorSpace, MaterialValue,
        MaterialValueType,
    },
};
use aestra_project::ResolvedEffectProject;
use bevy::{
    math::primitives::Sphere,
    mesh::{Meshable, VertexAttributeValues},
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

const PREVIEW_DURATION: f32 = 2.0;

/// Whether this material needs the GPU scene preview instead of the CPU rasterizer: a
/// Sprite- or Mesh-domain material that samples a texture, uses screen derivatives, or (Mesh
/// only) displaces vertices — and does not call functions/custom WESL (still not run in
/// background previews). Everything else stays on the fast CPU path.
pub(super) fn wants_gpu(program: &MaterialProgram) -> bool {
    if !matches!(
        program.domain,
        MaterialDomain::Sprite | MaterialDomain::Mesh | MaterialDomain::Ribbon
    ) || program.expressions.len() > 256
        || program.parameters.len() > 128
    {
        return false;
    }
    // Split what needs the real renderer (textures, screen derivatives) from what the CPU sphere
    // preview also renders faithfully (graph functions, view-dependent inputs like a fresnel).
    let mut gpu_scene = false;
    let mut cpu_scene = false;
    for expression in &program.expressions {
        match expression.kind {
            // Inline custom WESL is never compiled in a background preview; a graph FunctionCall is
            // fine (the CPU preview inlines it, and the GPU scene is given the library).
            MaterialExpressionKind::CustomWeslCall { .. }
            | MaterialExpressionKind::FunctionInput(_) => return false,
            MaterialExpressionKind::SampleTexture { .. }
            | MaterialExpressionKind::SampleTextureLevel { .. }
            | MaterialExpressionKind::SampleTextureGradient { .. }
            | MaterialExpressionKind::DerivativeX { .. }
            | MaterialExpressionKind::DerivativeY { .. } => gpu_scene = true,
            MaterialExpressionKind::FunctionCall { .. } => cpu_scene = true,
            // Scene-dependent inputs — surface geometry, view direction, screen position.
            MaterialExpressionKind::Input(
                MaterialInput::LocalPosition
                | MaterialInput::WorldPosition
                | MaterialInput::Normal
                | MaterialInput::Tangent
                | MaterialInput::ViewDirection
                | MaterialInput::ScreenUv,
            ) => cpu_scene = true,
            _ => {}
        }
    }
    let displaces = program.outputs.vertex_offset.is_some();
    match program.domain {
        // A mesh needs real geometry for any scene-dependent shading, and for displacement.
        MaterialDomain::Mesh => gpu_scene || cpu_scene || displaces,
        // A ribbon needs real strand geometry the CPU can't synthesize.
        MaterialDomain::Ribbon => (gpu_scene || cpu_scene) && !displaces,
        // A sprite is a flat billboard: only real textures/derivatives need the GPU. Fresnel,
        // graph functions and other view-dependent shading render better (a curved surface) and
        // faster on the CPU sphere preview, so they stay off the GPU.
        MaterialDomain::Sprite => gpu_scene && !displaces,
        _ => false,
    }
}

/// Synthesizes a one-instance scene around `program` (sprite quad or unit sphere by domain),
/// assembles it through the shared effect scaffolding, and binds a generated neutral texture for
/// every texture the material references. Returns the same [`Prepared`] the effect `GpuJob` consumes.
pub(super) fn prepare(
    program: MaterialProgram,
    functions: &BTreeMap<MaterialFunctionId, MaterialFunction>,
    root: &Path,
    cancelled: &AtomicBool,
) -> Result<Prepared, String> {
    check_cancelled(cancelled)?;
    if !matches!(
        program.domain,
        MaterialDomain::Sprite | MaterialDomain::Mesh | MaterialDomain::Ribbon
    ) {
        return Err("Only Sprite, Mesh, and Ribbon materials preview in the background".into());
    }
    let (resolved, neutrals, injected_meshes) = synthesize(program, functions, root);
    let assembled: Assembled = effect::assemble(resolved, root, cancelled, &injected_meshes, true)?;
    let textures = assembled
        .texture_paths
        .iter()
        .map(|path| {
            let color_space = neutrals
                .get(path)
                .copied()
                .unwrap_or(MaterialTextureColorSpace::SrgbColor);
            (path.clone(), neutral_image(color_space))
        })
        .collect();
    Ok(Prepared {
        players: assembled.players,
        center: assembled.center,
        radius: assembled.radius,
        textures,
        meshes: assembled.meshes,
    })
}

/// Wraps `program` in a minimal single-instance effect. Registers a neutral texture asset for
/// each texture id the material references, and (for the Mesh domain) an injected procedural
/// unit sphere. Returns the resolved project, the neutral color space per texture path, and the
/// procedural meshes to inject into [`effect::assemble`].
fn synthesize(
    program: MaterialProgram,
    functions: &BTreeMap<MaterialFunctionId, MaterialFunction>,
    root: &Path,
) -> (
    ResolvedEffectProject,
    BTreeMap<PathBuf, MaterialTextureColorSpace>,
    BTreeMap<PathBuf, (Mesh, f32)>,
) {
    // Every texture the material references, by asset id, with the color space to fill it with.
    let mut texture_assets: BTreeMap<AssetId, MaterialTextureColorSpace> = BTreeMap::new();
    for parameter in &program.parameters {
        if let MaterialValueType::Texture2D(descriptor) = &parameter.value_type
            && let Some(MaterialValue::Texture2D(id)) = parameter.default
            && !id.is_nil()
        {
            texture_assets.entry(id).or_insert(descriptor.color_space);
        }
    }
    for expression in &program.expressions {
        if let MaterialExpressionKind::Constant(MaterialValue::Texture2D(id)) = expression.kind
            && !id.is_nil()
        {
            texture_assets
                .entry(id)
                .or_insert(MaterialTextureColorSpace::SrgbColor);
        }
    }

    let mut assets = Vec::new();
    let mut neutrals = BTreeMap::new();
    for (index, (id, color_space)) in texture_assets.into_iter().enumerate() {
        let path = format!("aestra-preview/neutral-{index}.png");
        neutrals.insert(root.join(&path), color_space);
        assets.push(AssetDefinition {
            id,
            name: format!("Preview Neutral {index}"),
            kind: AssetKind::Texture,
            path,
        });
    }

    let program_id = program.id;
    let domain = program.domain;
    // The program's policy dictates the legal render states; its default is always allowed.
    // Hardcoding one risks "render state is not allowed by its program" on compile.
    let render_state = program.render_state_policy.default;
    let instance_id = MaterialId::new();
    let mut injected_meshes = BTreeMap::new();
    let renderer = match domain {
        MaterialDomain::Mesh => {
            let mesh_asset = AssetId::new();
            let path = "aestra-preview/unit-sphere.mesh".to_string();
            injected_meshes.insert(root.join(&path), unit_sphere());
            assets.push(AssetDefinition {
                id: mesh_asset,
                name: "Preview Unit Sphere".into(),
                kind: AssetKind::Mesh,
                path,
            });
            RendererInstance {
                id: RendererId::new(),
                renderer_type: RendererTypeId::new(RENDERER_MESH),
                enabled: true,
                material: instance_id,
                properties: RendererProperties::Mesh { asset: mesh_asset },
            }
        }
        MaterialDomain::Ribbon => RendererInstance {
            id: RendererId::new(),
            renderer_type: RendererTypeId::new(RENDERER_RIBBON),
            enabled: true,
            material: instance_id,
            // A single strand connects the arc's particles into one clean strip.
            properties: RendererProperties::Ribbon {
                width: 0.35,
                strand_count: 1,
            },
        },
        _ => RendererInstance::sprite(instance_id),
    };

    let instance = MaterialInstance {
        id: instance_id,
        program: MaterialProgramRef::Project(program_id),
        values: BTreeMap::new(),
        render_state,
    };
    let mut effect = EffectAsset::new("Material Preview", PREVIEW_DURATION);
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;
    effect.material_instances = vec![instance];
    effect.assets = assets;
    // A ribbon strip needs a strand of moving particles; sprite/mesh want one static instance.
    effect.emitters = vec![if domain == MaterialDomain::Ribbon {
        ribbon_emitter(renderer)
    } else {
        preview_emitter(renderer)
    }];

    let material_programs = BTreeMap::from([(program_id, program)]);
    (
        ResolvedEffectProject {
            root: effect,
            dependencies: BTreeMap::new(),
            material_programs,
            // The project's function library, so the material's graph FunctionCalls resolve.
            material_functions: functions.clone(),
        },
        neutrals,
        injected_meshes,
    )
}

/// A single static, full-size, opaque-white particle at the origin carrying `renderer`, so the
/// preview shows one clean instance of the material rather than an animated swarm. Tunes the
/// standard sprite module set in place (rather than rebuilding it) so every module the compiler
/// requires — emission, shape, initialize, motion, appearance — stays present.
fn preview_emitter(renderer: RendererInstance) -> Emitter {
    let mut emitter = Emitter::basic_sprite("Material Preview", PREVIEW_DURATION);
    emitter.max_particles = 1;
    for module in &mut emitter.modules {
        match &mut module.parameters {
            // One particle at start, no continuous emission.
            ModuleParameters::Emission {
                spawn_rate,
                burst_count,
            } => {
                *spawn_rate = 0.0;
                *burst_count = 1;
            }
            ModuleParameters::Shape { shape } => *shape = EmitterShape::Point,
            // Lives the whole preview, motionless.
            ModuleParameters::Initialize {
                lifetime,
                speed,
                direction,
                spread_degrees,
                angular_velocity,
            } => {
                *lifetime = ScalarRange::new(PREVIEW_DURATION * 8.0, PREVIEW_DURATION * 8.0);
                *speed = ScalarRange::new(0.0, 0.0);
                *direction = [0.0, 0.0, 1.0];
                *spread_degrees = 0.0;
                *angular_velocity = ScalarRange::new(0.0, 0.0);
            }
            ModuleParameters::Motion {
                gravity,
                drag,
                turbulence,
            } => {
                *gravity = [0.0; 3];
                *drag = 0.0;
                *turbulence = 0.0;
            }
            // Constant unit size and opacity, neutral white so the material's own color dominates.
            ModuleParameters::Appearance {
                size,
                opacity,
                color,
            } => {
                *size = Curve::new(vec![CurveKey::new(0.0, 1.0)]);
                *opacity = Curve::new(vec![CurveKey::new(0.0, 1.0)]);
                *color = Gradient::new(vec![ColorKey::new(0.0, [1.0, 1.0, 1.0, 1.0])]);
            }
            _ => {}
        }
    }
    emitter.renderers = vec![renderer];
    emitter
}

/// A single ribbon strand: particles emitted along a gravity-curved arc so the renderer connects
/// them into one short, opaque-white strip (mirrors the `ribbon_lab` fixture's arc, but one strand
/// at full opacity so the whole strip shows). The strip's own material shading dominates.
fn ribbon_emitter(renderer: RendererInstance) -> Emitter {
    let mut emitter = Emitter::basic_sprite("Material Preview", PREVIEW_DURATION);
    emitter.max_particles = 128;
    emitter.modules = vec![
        // Continuous emission traces the strand; no initial burst.
        ModuleInstance::emission(30.0, 0),
        ModuleInstance::shape(EmitterShape::Point),
        ModuleInstance::initialize(
            ScalarRange::new(PREVIEW_DURATION * 1.2, PREVIEW_DURATION * 1.2),
            ScalarRange::new(55.0, 55.0),
            [0.8, 0.6, 0.0],
            0.0,
            ScalarRange::new(0.0, 0.0),
        ),
        ModuleInstance::motion([0.0, -35.0, 0.0], 0.0, 0.0),
        ModuleInstance::appearance(
            Curve::new(vec![
                CurveKey::new(0.0, 3.0),
                CurveKey::new(0.35, 12.0),
                CurveKey::new(1.0, 1.5),
            ]),
            Curve::new(vec![CurveKey::new(0.0, 1.0)]),
            Gradient::new(vec![ColorKey::new(0.0, [1.0, 1.0, 1.0, 1.0])]),
        ),
    ];
    emitter.renderers = vec![renderer];
    emitter
}

/// A radius-1 sphere with the full attribute set the glTF mesh loader produces (normals, UV0,
/// generated tangents for normal maps, a duplicated UV1, and white vertex colors), so any mesh
/// material's inputs resolve against a generic preview surface.
fn unit_sphere() -> (Mesh, f32) {
    let mut mesh = Sphere::new(1.0).mesh().build();
    let _ = mesh.generate_tangents();
    let vertices = mesh.count_vertices();
    if let Some(VertexAttributeValues::Float32x2(uv0)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
        let uv1 = uv0.clone();
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv1);
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0f32; 4]; vertices]);
    (mesh, 1.0)
}

/// A 1×1 neutral texture: white for color maps, flat normal for linear data maps, in the
/// matching color space so the material samples a truthful stand-in for its absent input.
fn neutral_image(color_space: MaterialTextureColorSpace) -> Image {
    let (rgba, format) = match color_space {
        MaterialTextureColorSpace::SrgbColor => {
            ([255u8, 255, 255, 255], TextureFormat::Rgba8UnormSrgb)
        }
        MaterialTextureColorSpace::LinearData => {
            ([128u8, 128, 255, 255], TextureFormat::Rgba8Unorm)
        }
    };
    let mut image = Image::new(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba.to_vec(),
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::linear();
    image
}

#[cfg(test)]
mod tests;
