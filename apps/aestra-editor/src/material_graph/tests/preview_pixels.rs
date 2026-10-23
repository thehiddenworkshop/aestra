use super::*;
use aestra_compiler::{MaterialCompiler, MaterialGraphCreateKind};

#[test]
fn cancellation_is_observed_between_scanlines() {
    let program = MaterialProgram::additive_sprite("Cancel scanlines").normalized();
    let calls = std::cell::Cell::new(0);
    let result = render_material_preview_pixels(
        &program,
        None,
        &aestra_compiler::MaterialFunctionLibrary::default(),
        MaterialGraphPreviewTarget::Output,
        None,
        8,
        false,
        || {
            calls.set(calls.get() + 1);
            calls.get() == 3
        },
    );
    assert_eq!(result.unwrap_err(), "Cancelled");
    assert_eq!(calls.get(), 3);
}

#[test]
fn unresolved_output_keeps_permissive_graph_fallback_but_strict_assets_reject_it() {
    let mut program = MaterialProgram::additive_sprite("Missing sample").normalized();
    program.outputs.color = MaterialExpressionId::new();
    let library = aestra_compiler::MaterialFunctionLibrary::default();
    assert!(
        render_material_preview_pixels(
            &program,
            None,
            &library,
            MaterialGraphPreviewTarget::Output,
            None,
            16,
            true,
            || false
        )
        .is_err()
    );
    let rgba = render_material_preview_pixels(
        &program,
        None,
        &library,
        MaterialGraphPreviewTarget::Output,
        None,
        16,
        false,
        || false,
    )
    .unwrap();
    assert_eq!(rgba.len(), 16 * 16 * 4);
    assert!(
        rgba.as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[0] == p[1] && p[1] == p[2] && p[3] == 255)
    );
    assert_ne!(&rgba[..4], &rgba[10 * 4..11 * 4]);
}

#[test]
fn material_graph_preview_raster_has_expected_size_and_visible_content() {
    let program = MaterialProgram::from_ron(crate::MATERIAL_GRAPH_LAB_PROGRAM_SOURCE)
        .expect("material graph lab program should parse");
    let image = render_material_graph_preview(
        &program,
        None,
        &aestra_compiler::MaterialFunctionLibrary::default(),
        MaterialGraphPreviewTarget::Output,
        Some(MaterialValueType::Color),
    );

    assert_eq!(image.texture_descriptor.size.width, MATERIAL_PREVIEW_SIZE);
    assert_eq!(image.texture_descriptor.size.height, MATERIAL_PREVIEW_SIZE);
    let pixels = image.data.as_ref().expect("preview should be CPU-backed");
    assert_eq!(
        pixels.len(),
        (MATERIAL_PREVIEW_SIZE * MATERIAL_PREVIEW_SIZE * 4) as usize
    );
    assert!(pixels.windows(4).any(|pixel| pixel[0] != pixel[1]));
}

#[test]
fn mesh_basis_previews_are_orthonormal_even_at_the_poles() {
    for normal in [
        Vec3::Z,
        Vec3::Y,
        -Vec3::Y,
        Vec3::new(1.0, 2.0, 3.0).normalize(),
    ] {
        let context = MaterialPreviewContext {
            uv: Vec2::ZERO,
            normal,
        };
        let direction = |input| {
            let PreviewValue::Numeric(value, 3) = preview_input(input, context) else {
                panic!("expected Vec3 preview")
            };
            Vec3::new(value[0], value[1], value[2])
        };
        let tangent = direction(MaterialInput::Tangent);
        let bitangent = direction(MaterialInput::Bitangent);
        assert!((tangent.length() - 1.0).abs() < 1e-5);
        assert!((bitangent.length() - 1.0).abs() < 1e-5);
        assert!(normal.dot(bitangent).abs() < 1e-5);
        assert!(tangent.dot(bitangent).abs() < 1e-5);
        assert!(tangent.cross(bitangent).abs_diff_eq(normal, 1e-5));
    }
}

#[test]
fn normal_map_node_preview_decodes_inline_values_in_the_mesh_basis() {
    let mut program = MaterialProgram::additive_sprite("Normal preview");
    program.domain = aestra_core::material::MaterialDomain::Mesh;
    let plan = MaterialCompiler
        .plan_graph_node_creation(
            &program,
            MaterialGraphCreateKind::Function(
                aestra_core::material::MaterialGraphFunction::NormalMap,
            ),
            None,
        )
        .unwrap();
    let mut program = plan.replacement;
    let MaterialExpressionKind::NormalMap { sample, flip_y, .. } = program
        .expressions
        .iter()
        .find(|node| node.id == plan.expression)
        .unwrap()
        .kind
    else {
        panic!("normal map")
    };
    program
        .expressions
        .iter_mut()
        .find(|node| node.id == sample)
        .unwrap()
        .kind = MaterialExpressionKind::Constant(MaterialValue::Vec4([0.5, 0.8, 0.9, 0.0]));
    for flipped in [false, true] {
        program
            .expressions
            .iter_mut()
            .find(|node| node.id == flip_y)
            .unwrap()
            .kind = MaterialExpressionKind::Constant(MaterialValue::Bool(flipped));
        let functions = aestra_compiler::MaterialFunctionLibrary::default();
        let evaluator = MaterialPreviewEvaluator {
            program: &program,
            instance: None,
            functions: &functions,
        };
        let Some(PreviewValue::Numeric(value, 3)) = evaluator.evaluate(
            plan.expression,
            MaterialPreviewContext {
                uv: Vec2::ZERO,
                normal: Vec3::Z,
            },
        ) else {
            panic!("Vec3 preview")
        };
        let expected = Vec3::new(0.0, if flipped { -0.6 } else { 0.6 }, 0.8);
        assert!(Vec3::new(value[0], value[1], value[2]).abs_diff_eq(expected, 1e-5));
    }
}
