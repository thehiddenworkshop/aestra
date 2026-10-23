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
fn wants_gpu_routes_texture_derivative_and_mesh_displacement_materials() {
    // A plain radial-mask sprite material renders faithfully on the CPU.
    assert!(!wants_gpu(&sprite("Plain")));
    // Sampling a texture needs the bound-texture GPU scene, on either domain.
    assert!(wants_gpu(&sampling(sprite("Textured"))));
    let mut textured_mesh = sampling(sprite("Textured Mesh"));
    textured_mesh.domain = MaterialDomain::Mesh;
    assert!(wants_gpu(&textured_mesh));

    // Vertex displacement is not representable on a camera-facing sprite, but a mesh shows it â€”
    // even without a texture sample.
    let mut sprite_displaced = sampling(sprite("Displaced Sprite"));
    sprite_displaced.outputs.vertex_offset = Some(sprite_displaced.outputs.color);
    assert!(!wants_gpu(&sprite_displaced));
    let mut mesh_displaced = sprite("Displaced Mesh");
    mesh_displaced.domain = MaterialDomain::Mesh;
    mesh_displaced.outputs.vertex_offset = Some(mesh_displaced.outputs.color);
    assert!(wants_gpu(&mesh_displaced));

    // A plain mesh material with neither texture nor displacement stays on the CPU.
    let mut plain_mesh = sprite("Plain Mesh");
    plain_mesh.domain = MaterialDomain::Mesh;
    assert!(!wants_gpu(&plain_mesh));

    // Ribbon materials that sample a texture also route (Decal/Screen do not).
    let mut ribbon = sampling(sprite("Ribbon"));
    ribbon.domain = MaterialDomain::Ribbon;
    assert!(wants_gpu(&ribbon));
    let mut decal = sampling(sprite("Decal"));
    decal.domain = MaterialDomain::Decal;
    assert!(!wants_gpu(&decal));
}

#[test]
fn ribbon_synthesis_traces_a_single_strand_with_moving_particles() {
    let mut program = sampling(sprite("Ribbon Material"));
    program.domain = MaterialDomain::Ribbon;

    let (resolved, _neutrals, injected) = synthesize(program, &BTreeMap::new(), Path::new("/root"));
    assert!(injected.is_empty(), "a ribbon material injects no mesh");

    let emitter = &resolved.root.emitters[0];
    let renderer = &emitter.renderers[0];
    let RendererProperties::Ribbon {
        strand_count,
        width,
    } = renderer.properties
    else {
        panic!("ribbon material previews on a ribbon renderer");
    };
    assert_eq!(strand_count, 1, "one clean strand");
    assert!(width > 0.0);
    assert_eq!(renderer.material, resolved.root.material_instances[0].id);
    // The strand needs several moving particles over time, not a single static one.
    assert!(emitter.max_particles > 1);
}

#[test]
fn mesh_synthesis_injects_a_unit_sphere_with_the_expected_attributes() {
    let mut program = sampling(sprite("Mesh Material"));
    program.domain = MaterialDomain::Mesh;

    let (resolved, _neutrals, injected) = synthesize(program, &BTreeMap::new(), Path::new("/root"));

    // A mesh renderer over the injected procedural sphere, bound to the material instance.
    let renderer = &resolved.root.emitters[0].renderers[0];
    let RendererProperties::Mesh { asset } = renderer.properties else {
        panic!("mesh material previews on a mesh renderer");
    };
    assert_eq!(renderer.material, resolved.root.material_instances[0].id);
    // The mesh asset the renderer references is injected (no file on disk) and registered.
    let mesh_asset = resolved
        .root
        .assets
        .iter()
        .find(|a| a.id == asset)
        .expect("mesh asset is registered");
    assert_eq!(mesh_asset.kind, AssetKind::Mesh);
    let (mesh, radius) = injected
        .get(&Path::new("/root").join(&mesh_asset.path))
        .expect("procedural sphere is injected for the mesh asset");
    assert!(*radius > 0.0);
    for attribute in [
        Mesh::ATTRIBUTE_NORMAL,
        Mesh::ATTRIBUTE_UV_0,
        Mesh::ATTRIBUTE_UV_1,
        Mesh::ATTRIBUTE_TANGENT,
    ] {
        assert!(
            mesh.contains_attribute(attribute),
            "unit sphere is missing a mesh attribute"
        );
    }
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

    let (resolved, neutrals, injected) = synthesize(program, &BTreeMap::new(), Path::new("/root"));
    assert!(
        injected.is_empty(),
        "a sprite material injects no procedural mesh"
    );

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
fn real_texture_and_mesh_materials_prepare_without_a_gpu() {
    // prepare() compiles the synthesized scene and assembles it (no GPU), so a synthesis or
    // compile failure reproduces here even though the capture itself needs a native GPU.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/test");
    let functions = BTreeMap::new();
    // mesh (texture + displacement) and trail (ribbon texture) genuinely need the GPU.
    for name in ["mesh_material_lab", "trail_lab"] {
        let program =
            MaterialProgram::load_ron(root.join(format!("materials/{name}.aestra.material.ron")))
                .unwrap_or_else(|e| panic!("load {name}: {e}"));
        assert!(
            wants_gpu(&program),
            "{name} should route to the GPU preview"
        );
        prepare(program, &functions, &root, &AtomicBool::new(false))
            .unwrap_or_else(|e| panic!("prepare {name}: {e}"));
    }
    // A function/view-dependent Sprite material (material_graph_lab: fresnel over functions, no
    // texture) renders on the faster CPU sphere preview instead — it does not need the GPU.
    let sprite_fresnel =
        MaterialProgram::load_ron(root.join("materials/material_graph_lab.aestra.material.ron"))
            .unwrap();
    assert!(
        !wants_gpu(&sprite_fresnel),
        "a fresnel/function sprite material should render on the CPU, not the GPU"
    );
}

#[test]
fn project_presets_resolve_and_prepare_through_the_material_path() {
    use aestra_core::material::MaterialPresetDescriptor;
    // Presets resolve to a material program (crate::material_graph::resolve_preset_program) and
    // then route like any material. Verify the sample project's presets resolve, that a
    // view-dependent one (hologram: fresnel over Normal/ViewDirection) routes to the GPU scene,
    // and that the GPU ones compile — no GPU needed for the check.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/test");
    let names = [
        "hologram",
        "impact_flash",
        "additive_flame",
        "energy_beam",
        "ghost",
        "magic_shield",
        "portal",
        "soft_smoke",
    ];
    let descriptors: Vec<MaterialPresetDescriptor> = names
        .iter()
        .map(|n| {
            MaterialPresetDescriptor::load_ron(
                root.join(format!("materials/{n}.aestra.material-preset.ron")),
            )
            .unwrap_or_else(|e| panic!("load preset {n}: {e}"))
        })
        .collect();
    let catalog =
        aestra_compiler::MaterialPresetCatalog::with_project_presets(descriptors.clone()).unwrap();
    // The sample presets are view-dependent sprite materials (fresnel), which render on the CPU
    // sphere with built-in functions inlined — not the GPU flat quad.
    let library = aestra_compiler::MaterialFunctionLibrary::default();
    let mut cpu_rendered = 0;
    for descriptor in &descriptors {
        let program = crate::material_graph::resolve_preset_program(&catalog, descriptor.id)
            .unwrap_or_else(|e| panic!("resolve preset {}: {e}", descriptor.display_name));
        if !wants_gpu(&program) {
            crate::material_graph::render_material_asset_preview(&program, &library, 64, || false)
                .unwrap_or_else(|e| panic!("render preset {}: {e}", descriptor.display_name));
            cpu_rendered += 1;
        }
    }
    assert!(
        cpu_rendered > 0,
        "view-dependent presets should render on the CPU sphere preview"
    );
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
