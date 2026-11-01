use super::*;
use crate::material_graph::preview_pixels::{MaterialPreviewContext, MaterialPreviewEvaluator};
use aestra_core::{
    material::MaterialFunctionRef,
    noise::{NoiseFunction, NoiseSettings},
};

#[test]
fn noise_nodes_preview_the_shared_cpu_field_and_produce_saved_thumbnails() {
    let functions = aestra_compiler::MaterialFunctionLibrary::default();
    for kind in NoiseFunction::ALL {
        let base = MaterialProgram::additive_sprite(kind.name());
        let mut program = MaterialCompiler
            .plan_graph_node_creation_with_functions(
                &base,
                MaterialGraphCreateKind::FunctionCall {
                    function: MaterialFunctionRef::BuiltIn(kind.id()),
                    output: aestra_core::noise::OUTPUT,
                },
                None,
                &functions,
            )
            .unwrap()
            .replacement;
        let id = program.expressions.last().unwrap().id;
        program.outputs.alpha = id;
        let position = match &program.expressions.last().unwrap().kind {
            MaterialExpressionKind::FunctionCall { arguments, .. } => {
                arguments[&aestra_core::noise::POSITION]
            }
            _ => unreachable!(),
        };
        program
            .expressions
            .iter_mut()
            .find(|e| e.id == position)
            .unwrap()
            .kind = MaterialExpressionKind::Constant(if kind.dimensions == 2 {
            MaterialValue::Vec2([-1.25, 0.5])
        } else {
            MaterialValue::Vec3([-1.25, 0.5, -0.75])
        });
        let context = MaterialPreviewContext {
            uv: Vec2::new(0.2, 0.7),
            normal: Vec3::Z,
        };
        let evaluator = MaterialPreviewEvaluator {
            program: &program,
            instance: None,
            functions: &functions,
        };
        let actual = evaluator
            .evaluate(id, context)
            .unwrap()
            .numeric()
            .unwrap()
            .0[0];
        let expected = aestra_runtime::noise::sample(
            [-1.25, 0.5, -0.75],
            &NoiseSettings {
                basis: kind.basis,
                dimensions: kind.dimensions,
                octaves: if kind.fractal { 4 } else { 1 },
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(actual, expected);
        let saved = MaterialProgram::from_ron(&program.to_pretty_ron().unwrap()).unwrap();
        let thumbnail =
            asset_preview::render_material_asset_preview(&saved, &functions, 16, || false).unwrap();
        assert_eq!(thumbnail.len(), 16 * 16 * 4);
    }
}

#[test]
fn nested_noise_smoke_is_editable_and_has_a_deterministic_nonempty_thumbnail() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sample-project");
    let function = aestra_core::material::MaterialFunction::load_ron(
        root.join("materials/noise_smoke_density.aestra.material-function.ron"),
    )
    .unwrap();
    assert!(function.custom_wesl.is_none());
    let functions = aestra_compiler::MaterialFunctionLibrary::new([function]);
    let mut program =
        MaterialProgram::load_ron(root.join("materials/noise_smoke.aestra.material.ron")).unwrap();
    MaterialCompiler
        .compile_with_functions(&program, &functions)
        .unwrap();
    let render = |p: &MaterialProgram| {
        asset_preview::render_material_asset_preview(p, &functions, 32, || false).unwrap()
    };
    let pixels = render(&program);
    if let Some(path) = std::env::var_os("AESTRA_NOISE_PREVIEW") {
        image::save_buffer(path, &pixels, 32, 32, image::ColorType::Rgba8).unwrap();
    }
    assert_eq!(pixels, render(&program));
    let colors = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    assert!(colors.len() > 10, "noise preview must contain structure");
    let seed = program
        .parameters
        .iter_mut()
        .find(|p| p.name == "Seed")
        .unwrap();
    seed.default = Some(MaterialValue::Float(91.0));
    assert_ne!(pixels, render(&program));
    let effect =
        aestra_core::EffectAsset::load_ron(root.join("effects/noise_smoke_lab.aestra.ron"))
            .unwrap();
    ProjectEffectCatalog::scan(&root)
        .compile_project(&effect)
        .unwrap();
}
