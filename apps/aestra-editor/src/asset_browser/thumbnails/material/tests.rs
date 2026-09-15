use super::*;
use aestra_core::material::{
    MaterialEvaluationDomain, MaterialExpression, MaterialParameter, MaterialTextureDescriptor,
};
use aestra_core::{MaterialExpressionId, MaterialParameterId};

fn sprite(name: &str) -> MaterialProgram {
    crate::material_graph::material_preset_base(name, MaterialDomain::Sprite)
}

fn sampling(mut program: MaterialProgram) -> MaterialProgram {
    let (texture, uv) = (program.outputs.color, program.outputs.alpha);
    program.expressions.push(MaterialExpression {
        id: MaterialExpressionId::new(),
        kind: MaterialExpressionKind::SampleTexture { texture, uv },
    });
    program
}

#[test]
fn wants_gpu_only_for_scene_needing_sprite_materials() {
    // A plain radial-mask sprite material renders faithfully on the CPU.
    assert!(!wants_gpu(&sprite("Plain")));
    // Sampling a texture needs the bound-texture GPU scene.
    assert!(wants_gpu(&sampling(sprite("Textured"))));
    // Mesh domain is a later milestone; only Sprite routes here for now.
    let mut mesh = sampling(sprite("Mesh"));
    mesh.domain = MaterialDomain::Mesh;
    assert!(!wants_gpu(&mesh));
    // Vertex displacement is not representable on a camera-facing sprite.
    let mut displaced = sampling(sprite("Displaced"));
    displaced.outputs.vertex_offset = Some(displaced.outputs.color);
    assert!(!wants_gpu(&displaced));
}

#[test]
fn synthesize_binds_a_neutral_texture_for_each_referenced_texture() {
    let mut program = sampling(sprite("Textured"));
    let texture_asset = AssetId::new();
    program.parameters.push(MaterialParameter {
        id: MaterialParameterId::new(),
        name: "Albedo".into(),
        value_type: MaterialValueType::Texture2D(MaterialTextureDescriptor {
            color_space: MaterialTextureColorSpace::LinearData,
            sampler: Default::default(),
        }),
        evaluation_domain: MaterialEvaluationDomain::Instance,
        default: Some(MaterialValue::Texture2D(texture_asset)),
    });
    let program_id = program.id;

    let (resolved, neutrals) = synthesize(program, Path::new("/root"));

    // The referenced texture id is registered as a (neutral) texture asset.
    let asset = resolved
        .root
        .assets
        .iter()
        .find(|a| a.id == texture_asset)
        .expect("referenced texture is registered");
    assert_eq!(asset.kind, AssetKind::Texture);
    // The neutral for that asset uses the parameter's declared color space.
    assert_eq!(
        neutrals.get(&Path::new("/root").join(&asset.path)),
        Some(&MaterialTextureColorSpace::LinearData)
    );
    // The program is resolvable and the sprite emitter's renderer targets its instance.
    assert!(resolved.material_programs.contains_key(&program_id));
    let instance = &resolved.root.material_instances[0];
    assert_eq!(instance.program.id(), program_id);
    assert_eq!(resolved.root.emitters[0].renderers[0].material, instance.id);
}

#[test]
fn neutral_images_use_the_matching_color_space_format() {
    assert_eq!(
        neutral_image(MaterialTextureColorSpace::SrgbColor)
            .texture_descriptor
            .format,
        TextureFormat::Rgba8UnormSrgb
    );
    assert_eq!(
        neutral_image(MaterialTextureColorSpace::LinearData)
            .texture_descriptor
            .format,
        TextureFormat::Rgba8Unorm
    );
}
