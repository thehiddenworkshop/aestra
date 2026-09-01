use aestra_authoring::{MaterialAuthoringDocument, migrate_legacy_sprite_materials};
use aestra_compiler::{EffectCompiler, MaterialCompiler};
use aestra_core::EffectAsset;
use aestra_gpu::material::{MaterialBackendCapabilities, MaterialShaderCompiler};

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
        EffectCompiler::default()
            .compile(&document.effect)
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
