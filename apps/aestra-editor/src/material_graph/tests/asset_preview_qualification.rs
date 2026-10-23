//! The actual saved-material validation/evaluation entry point, without browser/GPU substitutes.
use super::*;
use crate::material_graph::preset_previews::material_preset_base;
use aestra_compiler::MaterialFunctionLibrary;
use aestra_core::material::{
    MaterialDomain, MaterialEvaluationDomain, MaterialFunction, MaterialFunctionInput,
    MaterialFunctionOutput, MaterialFunctionRef, MaterialParameter, MaterialSchemaVersion,
    MaterialValue, MaterialValueType,
};
use aestra_core::{
    MaterialFunctionId, MaterialFunctionInputId, MaterialFunctionOutputId, MaterialParameterId,
};

fn material() -> MaterialProgram {
    material_preset_base("Saved preview", MaterialDomain::Sprite)
}
fn render(program: &MaterialProgram) -> Result<Vec<u8>, String> {
    render_material_asset_preview(program, &MaterialFunctionLibrary::default(), 8, || false)
}
fn replace_color(program: &mut MaterialProgram, kind: MaterialExpressionKind) {
    program
        .expressions
        .iter_mut()
        .find(|node| node.id == program.outputs.color)
        .unwrap()
        .kind = kind;
}
fn chain(depth: usize) -> MaterialProgram {
    let mut program = material();
    let leaf = MaterialExpressionId::new();
    program.expressions = vec![MaterialExpression {
        id: leaf,
        kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.1)),
    }];
    program.outputs.color = leaf;
    program.outputs.alpha = leaf;
    for _ in 1..depth {
        let id = MaterialExpressionId::new();
        program.expressions.push(MaterialExpression {
            id,
            kind: MaterialExpressionKind::Add(program.outputs.color, leaf),
        });
        program.outputs.color = id;
    }
    program
}

#[test]
fn exact_size_expression_and_parameter_limits_are_inclusive() {
    let mut program = material();
    assert_eq!(
        render_material_asset_preview(&program, &MaterialFunctionLibrary::default(), 128, || false)
            .unwrap()
            .len(),
        128 * 128 * 4
    );
    for size in [0, 129] {
        assert!(
            render_material_asset_preview(
                &program,
                &MaterialFunctionLibrary::default(),
                size,
                || false
            )
            .unwrap_err()
            .contains("128 pixels")
        );
    }
    while program.expressions.len() < 256 {
        program.expressions.push(MaterialExpression {
            id: MaterialExpressionId::new(),
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
        });
    }
    assert!(render(&program).is_ok());
    program.expressions.push(MaterialExpression {
        id: MaterialExpressionId::new(),
        kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
    });
    assert!(render(&program).unwrap_err().contains("256 expressions"));
    let mut program = material();
    for index in 0..128 {
        program.parameters.push(MaterialParameter {
            id: MaterialParameterId::new(),
            name: format!("Parameter {index}"),
            value_type: MaterialValueType::Float,
            evaluation_domain: MaterialEvaluationDomain::Instance,
            default: Some(MaterialValue::Float(0.25)),
        });
    }
    assert!(render(&program).is_ok());
    let mut parameter = program.parameters[0].clone();
    parameter.id = MaterialParameterId::new();
    program.parameters.push(parameter);
    assert!(render(&program).unwrap_err().contains("128 parameters"));
}

#[test]
fn depth_limit_checks_exact_64_and_memoized_shared_subgraphs() {
    assert!(render(&chain(64)).is_ok());
    assert!(render(&chain(65)).unwrap_err().contains("64"));
    let mut program = chain(60);
    let shared = program.outputs.color;
    program.outputs.alpha = shared;
    for _ in 0..5 {
        let id = MaterialExpressionId::new();
        program.expressions.push(MaterialExpression {
            id,
            kind: MaterialExpressionKind::Add(program.outputs.alpha, shared),
        });
        program.outputs.alpha = id;
    }
    // The color traversal already memoized the shared 60-level branch. The second root
    // must still add its own depth, instead of accepting a short active recursion stack.
    assert_eq!(
        render(&program).unwrap_err(),
        "Preview limit: 64 expression levels"
    );
}

#[test]
fn vertex_displacement_requires_a_scene_on_every_material_domain() {
    for domain in [MaterialDomain::Sprite, MaterialDomain::Mesh] {
        let mut program = material();
        program.domain = domain;
        program.outputs.vertex_offset = Some(program.outputs.color);
        assert_eq!(
            render(&program).unwrap_err(),
            "Vertex displacement needs a scene preview"
        );
    }
}

#[test]
fn duplicate_identity_is_rejected_even_for_disconnected_expressions() {
    let mut program = material();
    let disconnected = MaterialExpression {
        id: MaterialExpressionId::new(),
        kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
    };
    program
        .expressions
        .extend([disconnected.clone(), disconnected]);
    assert_eq!(
        render(&program).unwrap_err(),
        "Duplicate material expression identity"
    );
}

#[test]
fn all_texture_derivative_and_raw_wesl_forms_fail_explicitly() {
    let base = material();
    let value = base.outputs.alpha;
    let cases = [
        (
            MaterialExpressionKind::SampleTexture {
                texture: value,
                uv: value,
            },
            "Texture sampling",
        ),
        (
            MaterialExpressionKind::SampleTextureLevel {
                texture: value,
                uv: value,
                level: value,
            },
            "Texture sampling",
        ),
        (
            MaterialExpressionKind::SampleTextureGradient {
                texture: value,
                uv: value,
                ddx: value,
                ddy: value,
            },
            "Texture sampling",
        ),
        (
            MaterialExpressionKind::DerivativeX { value },
            "Screen derivatives",
        ),
        (
            MaterialExpressionKind::DerivativeY { value },
            "Screen derivatives",
        ),
        (
            MaterialExpressionKind::FunctionInput(MaterialFunctionInputId::new()),
            "Custom WESL functions",
        ),
        (
            MaterialExpressionKind::CustomWeslCall {
                function: MaterialFunctionId::new(),
                entry_point: "preview".into(),
                source: "".into(),
                arguments: vec![],
                output_type: MaterialValueType::Float,
                evaluation_domain: aestra_core::material::MaterialExpressionDomain::Fragment,
            },
            "Custom WESL functions",
        ),
    ];
    for (kind, expected) in cases {
        let mut program = base.clone();
        replace_color(&mut program, kind);
        assert!(render(&program).unwrap_err().contains(expected));
    }
}

#[test]
fn an_unselected_unsupported_branch_is_validated_before_rasterization() {
    let mut program = material();
    let condition = MaterialExpressionId::new();
    let unsupported = MaterialExpressionId::new();
    program.expressions.extend([
        MaterialExpression {
            id: condition,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: unsupported,
            kind: MaterialExpressionKind::DerivativeX {
                value: program.outputs.alpha,
            },
        },
    ]);
    let good = program.outputs.alpha;
    replace_color(
        &mut program,
        MaterialExpressionKind::Select {
            condition,
            if_false: good,
            if_true: unsupported,
        },
    );
    assert!(render(&program).unwrap_err().contains("Screen derivatives"));
}

#[test]
fn disabled_node_bypass_ignores_an_unneeded_missing_texture_binding() {
    let mut program = material();
    let uv = program.outputs.alpha;
    replace_color(
        &mut program,
        MaterialExpressionKind::SampleTexture {
            texture: MaterialExpressionId::new(),
            uv,
        },
    );
    assert!(render(&program).unwrap_err().contains("Texture sampling"));
    program.disabled_expressions.push(program.outputs.color);
    let before = program.clone();
    let pixels = render(&program).unwrap();
    let mut bypassed = program.clone();
    bypassed.outputs.color = uv;
    assert_eq!(pixels, render(&bypassed).unwrap());
    assert_eq!(program, before);
}

#[test]
fn missing_saved_parameter_default_fails_and_a_real_default_recovers() {
    let mut program = material();
    let parameter = MaterialParameterId::new();
    program.parameters.push(MaterialParameter {
        id: parameter,
        name: "Saved value".into(),
        value_type: MaterialValueType::Float,
        evaluation_domain: MaterialEvaluationDomain::Instance,
        default: None,
    });
    replace_color(&mut program, MaterialExpressionKind::Parameter(parameter));
    assert!(render(&program).unwrap_err().contains("saved defaults"));
    program.parameters.last_mut().unwrap().default = Some(MaterialValue::Float(0.25));
    let before = program.clone();
    let pixels = render(&program).unwrap();
    let mut direct = program.clone();
    replace_color(
        &mut direct,
        MaterialExpressionKind::Constant(MaterialValue::Float(0.25)),
    );
    assert_eq!(pixels, render(&direct).unwrap());
    assert_eq!(program, before);
}

#[test]
fn project_graph_functions_use_the_supplied_library_and_saved_input_defaults() {
    let mut program = material();
    let input = MaterialFunctionInputId::new();
    let output = MaterialFunctionOutputId::new();
    let body = MaterialExpressionId::new();
    let function = MaterialFunction {
        id: MaterialFunctionId::new(),
        schema_version: MaterialSchemaVersion::CURRENT,
        name: "Saved passthrough".into(),
        inputs: vec![MaterialFunctionInput {
            id: input,
            name: "Value".into(),
            value_type: MaterialValueType::Float,
            default: Some(MaterialValue::Float(0.25)),
        }],
        outputs: vec![MaterialFunctionOutput {
            id: output,
            name: "Result".into(),
            value_type: MaterialValueType::Float,
            expression: body,
        }],
        expressions: vec![MaterialExpression {
            id: body,
            kind: MaterialExpressionKind::FunctionInput(input),
        }],
        custom_wesl: None,
    };
    replace_color(
        &mut program,
        MaterialExpressionKind::FunctionCall {
            function: MaterialFunctionRef::Project(function.id),
            arguments: BTreeMap::new(),
            output,
        },
    );
    assert!(render(&program).unwrap_err().contains("saved defaults"));
    let before = program.clone();
    let library = MaterialFunctionLibrary::new([function.clone()]);
    let pixels = render_material_asset_preview(&program, &library, 8, || false).unwrap();
    let mut direct = program.clone();
    replace_color(
        &mut direct,
        MaterialExpressionKind::Constant(MaterialValue::Float(0.25)),
    );
    assert_eq!(pixels, render(&direct).unwrap());
    assert_eq!(program, before);
    assert_eq!(
        library
            .get(MaterialFunctionRef::Project(function.id))
            .unwrap(),
        &function
    );
}

#[test]
fn a_nonfinite_saved_value_is_rejected_instead_of_a_checker_fallback() {
    let mut program = material();
    replace_color(
        &mut program,
        MaterialExpressionKind::Constant(MaterialValue::Float(f32::NAN)),
    );
    assert!(render(&program).unwrap_err().contains("saved defaults"));
}

#[test]
fn early_cancellation_precedes_invalid_size_and_graph_errors() {
    let mut program = material();
    program.outputs.color = MaterialExpressionId::new();
    let calls = std::cell::Cell::new(0);
    assert_eq!(
        render_material_asset_preview(&program, &MaterialFunctionLibrary::default(), 0, || {
            calls.set(calls.get() + 1);
            true
        })
        .unwrap_err(),
        "Cancelled"
    );
    assert_eq!(calls.get(), 1);
}
