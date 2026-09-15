//! Standalone-material thumbnails via a synthesized one-instance GPU scene (M-MG2, Sprite domain).
//!
//! The CPU rasterizer (`material_graph::asset_preview`) renders simple materials and rejects
//! ones that sample textures or use screen derivatives — those route here. We wrap the material
//! program in a minimal sprite effect and feed it through the shared [`effect::assemble`] +
//! `GpuJob` scaffolding.
//!
//! A *standalone* material has no concrete texture — the texture is an instance/effect-level
//! input — so we bind a semantically **neutral** texture per slot (white for color, flat normal
//! for data). That is the faithful preview of the material program in isolation; borrowing a
//! consuming effect's texture would misrepresent one arbitrary use as the material's own.
use super::effect::{self, Assembled, Prepared};
use super::*;
use aestra_core::{
    AssetDefinition, AssetId, AssetKind, BlendMode, EffectAsset, EffectPlaybackMode, Emitter,
    MaterialId,
    material::{
        MaterialCullMode, MaterialDepthTest, MaterialDomain, MaterialExpressionKind,
        MaterialInstance, MaterialProgram, MaterialProgramRef, MaterialRenderState,
        MaterialTextureColorSpace, MaterialValue, MaterialValueType,
    },
};
use aestra_project::ResolvedEffectProject;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

const PREVIEW_DURATION: f32 = 2.0;

/// Whether this material needs the GPU scene preview instead of the CPU rasterizer: a
/// Sprite-domain material that samples a texture or uses screen derivatives, without function/
/// custom-WESL calls (still not run in background previews) or vertex displacement (a mesh
/// concern handled by a later milestone). Everything else stays on the fast CPU path.
pub(super) fn wants_gpu(program: &MaterialProgram) -> bool {
    if program.domain != MaterialDomain::Sprite
        || program.expressions.len() > 256
        || program.parameters.len() > 128
        || program.outputs.vertex_offset.is_some()
    {
        return false;
    }
    let mut needs_scene = false;
    for expression in &program.expressions {
        match expression.kind {
            MaterialExpressionKind::FunctionCall { .. }
            | MaterialExpressionKind::CustomWeslCall { .. }
            | MaterialExpressionKind::FunctionInput(_) => return false,
            MaterialExpressionKind::SampleTexture { .. }
            | MaterialExpressionKind::SampleTextureLevel { .. }
            | MaterialExpressionKind::SampleTextureGradient { .. }
            | MaterialExpressionKind::DerivativeX { .. }
            | MaterialExpressionKind::DerivativeY { .. } => needs_scene = true,
            _ => {}
        }
    }
    needs_scene
}

/// Synthesizes a one-instance sprite scene around `program`, assembles it through the shared
/// effect scaffolding, and binds a generated neutral texture for every texture the material
/// references. Returns the same [`Prepared`] the effect `GpuJob` consumes.
pub(super) fn prepare(
    program: MaterialProgram,
    root: &Path,
    cancelled: &AtomicBool,
) -> Result<Prepared, String> {
    check_cancelled(cancelled)?;
    if program.domain != MaterialDomain::Sprite {
        return Err("Only Sprite-domain materials preview in the background".into());
    }
    let (resolved, neutrals) = synthesize(program, root);
    let assembled: Assembled = effect::assemble(resolved, root, cancelled)?;
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

/// Wraps `program` in a minimal sprite effect: a default instance, one sprite emitter, and a
/// neutral texture asset registered for each texture id the material references (so the
/// compiler binds them and the render samples neutral pixels). Returns the resolved project and
/// the color space to generate for each neutral texture's absolute path.
fn synthesize(
    program: MaterialProgram,
    root: &Path,
) -> (
    ResolvedEffectProject,
    BTreeMap<PathBuf, MaterialTextureColorSpace>,
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
    let instance_id = MaterialId::new();
    let instance = MaterialInstance {
        id: instance_id,
        program: MaterialProgramRef::Project(program_id),
        values: BTreeMap::new(),
        render_state: MaterialRenderState {
            blend: BlendMode::Alpha,
            depth_test: MaterialDepthTest::LessEqual,
            depth_write: false,
            cull_mode: MaterialCullMode::None,
        },
    };
    let mut emitter = Emitter::basic_sprite("Material Preview", PREVIEW_DURATION);
    for renderer in &mut emitter.renderers {
        renderer.material = instance_id;
    }
    let mut effect = EffectAsset::new("Material Preview", PREVIEW_DURATION);
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;
    effect.material_instances = vec![instance];
    effect.assets = assets;
    effect.emitters = vec![emitter];

    let material_programs = BTreeMap::from([(program_id, program)]);
    (
        ResolvedEffectProject {
            root: effect,
            dependencies: BTreeMap::new(),
            material_programs,
            material_functions: BTreeMap::new(),
        },
        neutrals,
    )
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
