use aestra_authoring::{MaterialAuthoringDocument, migrate_legacy_sprite_materials};
use aestra_compiler::{EffectCompiler, MaterialCompiler};
use aestra_core::{
    EffectAsset, RendererProperties,
    material::{MaterialExpressionKind, MaterialInput},
};
use aestra_gpu::{
    GpuEffectArtifact,
    material::{MaterialBackendCapabilities, MaterialShaderCompiler},
};
use aestra_runtime::EffectInstance;
use std::{collections::BTreeMap, sync::Arc};

const SHOWCASES: [(&str, &str); 3] = [
    (
        "Ember Sigil",
        include_str!("../../../assets/effects/ember_sigil.aestra.ron"),
    ),
    (
        "Plasma Burst",
        include_str!("../../../assets/effects/plasma_burst.aestra.ron"),
    ),
    (
        "Prism Bloom",
        include_str!("../../../assets/effects/prism_bloom.aestra.ron"),
    ),
];

#[test]
fn showcase_materials_migrate_through_commands_and_compile_for_portable_gpu() {
    for (name, source) in SHOWCASES {
        let effect = EffectAsset::from_ron(source).unwrap();
        let legacy_materials = effect.materials.clone();
        let legacy_renderer_count = effect
            .emitters
            .iter()
            .flat_map(|emitter| &emitter.renderers)
            .filter(|renderer| {
                legacy_materials
                    .iter()
                    .any(|material| material.id == renderer.material)
            })
            .count();
        let mut document = MaterialAuthoringDocument::new(effect, Vec::new());

        let (plan, _) = migrate_legacy_sprite_materials(&mut document).unwrap();
        assert!(!plan.is_empty(), "{name} should contain legacy materials");
        assert_eq!(document.effect.materials, legacy_materials);
        assert_eq!(
            plan.mappings
                .iter()
                .map(|mapping| mapping.renderers.len())
                .sum::<usize>(),
            legacy_renderer_count
        );
        assert!(document.validate().is_ok());
        let programs = document
            .programs
            .iter()
            .cloned()
            .map(|program| (program.id, program))
            .collect::<BTreeMap<_, _>>();
        EffectCompiler::default()
            .compile_with_material_programs(&document.effect, &programs)
            .unwrap_or_else(|error| panic!("{name} migrated effect failed compilation: {error}"));

        for program in &document.programs {
            let ir = MaterialCompiler
                .compile(program)
                .unwrap_or_else(|error| panic!("{name} material IR failed: {error}"));
            let compiled = MaterialShaderCompiler
                .compile(&ir, &MaterialBackendCapabilities::portable_minimum())
                .unwrap_or_else(|error| panic!("{name} material GPU compile failed: {error}"));
            assert!(compiled.shader.wgsl.contains("fn fragment_material"));
        }
    }
}

#[test]
fn migrated_flipbook_material_samples_renderer_resolved_uv0() {
    let effect = EffectAsset::from_ron(SHOWCASES[1].1).unwrap();
    let flipbook_renderers = effect
        .emitters
        .iter()
        .flat_map(|emitter| &emitter.renderers)
        .filter(|renderer| matches!(renderer.properties, RendererProperties::Flipbook { .. }))
        .map(|renderer| renderer.id)
        .collect::<Vec<_>>();
    assert!(!flipbook_renderers.is_empty());
    let mut document = MaterialAuthoringDocument::new(effect, Vec::new());

    let (plan, _) = migrate_legacy_sprite_materials(&mut document).unwrap();
    let mapping = plan
        .mappings
        .iter()
        .find(|mapping| {
            mapping
                .renderers
                .iter()
                .any(|renderer| flipbook_renderers.contains(renderer))
        })
        .expect("flipbook renderer must migrate to a semantic material");
    let program = document
        .programs
        .iter()
        .find(|program| program.id == mapping.program)
        .unwrap();
    let uv0 = program
        .expressions
        .iter()
        .find(|expression| {
            matches!(
                expression.kind,
                MaterialExpressionKind::Input(MaterialInput::Uv0)
            )
        })
        .expect("migrated flipbook material must consume renderer-resolved UV0")
        .id;
    assert!(program.expressions.iter().any(|expression| {
        matches!(
            expression.kind,
            MaterialExpressionKind::SampleTexture { uv, .. } if uv == uv0
        )
    }));

    let ir = MaterialCompiler.compile(program).unwrap();
    let material_shader = MaterialShaderCompiler
        .compile(&ir, &MaterialBackendCapabilities::portable_minimum())
        .unwrap();
    assert_eq!(
        material_shader.reflection.required_vertex_inputs,
        vec![MaterialInput::Uv0]
    );
    assert!(material_shader.shader.wesl.contains("input.uv0"));

    let programs = document
        .programs
        .iter()
        .cloned()
        .map(|program| (program.id, program))
        .collect::<BTreeMap<_, _>>();
    let compiled = Arc::new(
        EffectCompiler::default()
            .compile_with_material_programs(&document.effect, &programs)
            .unwrap(),
    );
    let artifact = GpuEffectArtifact::from_instance(&EffectInstance::new(compiled)).unwrap();
    let renderer = artifact
        .renderers
        .iter()
        .find(|renderer| renderer.renderer_kind == 1)
        .expect("migrated flipbook renderer must remain packed as a flipbook");
    assert!(renderer.frame_count > 1);
    assert!(renderer.textured != 0);
    assert_ne!(renderer.frames[0], renderer.frames[1]);
}
