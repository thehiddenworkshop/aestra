use aestra_compiler::{
    EffectCompiler, MaterialCompiler, MaterialFunctionLibrary, MaterialIrInstruction,
};
use aestra_core::{
    DiagnosticCode, EffectAsset, MaterialExpressionId, MaterialFunctionId, MaterialFunctionInputId,
    MaterialFunctionOutputId, MaterialId,
    material::{
        MaterialExpression, MaterialExpressionKind, MaterialFunction, MaterialFunctionInput,
        MaterialFunctionOutput, MaterialFunctionRef, MaterialInstance, MaterialProgram,
        MaterialProgramRef, MaterialRenderState, MaterialSchemaVersion, MaterialValue,
        MaterialValueType,
    },
};
use aestra_project::ProjectAssetIndex;
use std::collections::BTreeMap;

fn scale_function() -> MaterialFunction {
    let function = MaterialFunctionId::from_u128(0xF001);
    let value = MaterialFunctionInputId::from_u128(0xF002);
    let scale = MaterialFunctionInputId::from_u128(0xF003);
    let value_expression = MaterialExpressionId::from_u128(0xF004);
    let scale_expression = MaterialExpressionId::from_u128(0xF005);
    let product = MaterialExpressionId::from_u128(0xF006);
    MaterialFunction {
        id: function,
        schema_version: MaterialSchemaVersion::CURRENT,
        name: "Scale Float".into(),
        inputs: vec![
            MaterialFunctionInput {
                id: value,
                name: "Value".into(),
                value_type: MaterialValueType::Float,
            },
            MaterialFunctionInput {
                id: scale,
                name: "Scale".into(),
                value_type: MaterialValueType::Float,
            },
        ],
        outputs: vec![MaterialFunctionOutput {
            id: MaterialFunctionOutputId::from_u128(0xF007),
            name: "Scaled".into(),
            value_type: MaterialValueType::Float,
            expression: product,
        }],
        expressions: vec![
            MaterialExpression {
                id: value_expression,
                kind: MaterialExpressionKind::FunctionInput(value),
            },
            MaterialExpression {
                id: scale_expression,
                kind: MaterialExpressionKind::FunctionInput(scale),
            },
            MaterialExpression {
                id: product,
                kind: MaterialExpressionKind::Multiply(value_expression, scale_expression),
            },
        ],
    }
}

fn calling_program(argument: MaterialValue) -> MaterialProgram {
    let function = scale_function();
    let color = MaterialExpressionId::from_u128(0xF101);
    let value = MaterialExpressionId::from_u128(0xF102);
    let scale = MaterialExpressionId::from_u128(0xF103);
    let call = MaterialExpressionId::from_u128(0xF104);
    let mut program = MaterialProgram::additive_sprite("Function caller");
    program.expressions = vec![
        MaterialExpression {
            id: color,
            kind: MaterialExpressionKind::Constant(MaterialValue::ColorSrgb([1.0; 4])),
        },
        MaterialExpression {
            id: value,
            kind: MaterialExpressionKind::Constant(argument),
        },
        MaterialExpression {
            id: scale,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
        },
        MaterialExpression {
            id: call,
            kind: MaterialExpressionKind::FunctionCall {
                function: MaterialFunctionRef::Project(function.id),
                arguments: BTreeMap::from([
                    (function.inputs[0].id, value),
                    (function.inputs[1].id, scale),
                ]),
                output: function.outputs[0].id,
            },
        },
    ];
    program.outputs.color = color;
    program.outputs.alpha = call;
    program
}

#[test]
fn typed_function_calls_are_deterministically_inlined() {
    let function = scale_function();
    let library = MaterialFunctionLibrary::new([function]);
    let program = calling_program(MaterialValue::Float(0.8));

    let first = MaterialCompiler
        .compile_with_functions(&program, &library)
        .unwrap();
    let second = MaterialCompiler
        .compile_with_functions(&program, &library)
        .unwrap();

    assert_eq!(first, second);
    assert!(first.values.iter().any(|value| matches!(
        value.instruction,
        MaterialIrInstruction::Constant(aestra_compiler::MaterialIrConstant::Float(value))
            if value == 0.4
    )));
    assert!(
        first
            .source_map
            .values
            .contains_key(&MaterialExpressionId::from_u128(0xF104))
    );
}

#[test]
fn function_calls_reject_argument_type_mismatches() {
    let function = scale_function();
    let library = MaterialFunctionLibrary::new([function]);
    let program = calling_program(MaterialValue::Vec2([0.8, 0.4]));

    let error = MaterialCompiler
        .compile_with_functions(&program, &library)
        .unwrap_err();

    assert!(error.report().diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::MaterialTypeMismatch
            && diagnostic.message.contains("function input")
    }));
}

#[test]
fn function_library_rejects_recursive_calls() {
    let mut function = scale_function();
    let call = function.expressions[2].id;
    function.expressions[2].kind = MaterialExpressionKind::FunctionCall {
        function: MaterialFunctionRef::Project(function.id),
        arguments: BTreeMap::from([
            (function.inputs[0].id, function.expressions[0].id),
            (function.inputs[1].id, function.expressions[1].id),
        ]),
        output: function.outputs[0].id,
    };
    function.outputs[0].expression = call;
    let report = MaterialFunctionLibrary::new([function]).validation_report();

    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::ReferenceCycle)
    );
}

#[test]
fn project_compilation_resolves_and_erases_project_local_function_calls() {
    let temporary = tempfile::tempdir().unwrap();
    let function = scale_function();
    function
        .save_ron(temporary.path().join("scale.aestra.material-function.ron"))
        .unwrap();
    let mut program = calling_program(MaterialValue::Float(0.8));
    program.id = aestra_core::MaterialProgramId::from_u128(0xF201);
    program
        .save_ron(temporary.path().join("caller.aestra.material.ron"))
        .unwrap();
    let mut effect = EffectAsset::new("Function project", 1.0);
    effect.material_instances.push(MaterialInstance {
        id: MaterialId::from_u128(0xF202),
        program: MaterialProgramRef::Project(program.id),
        values: BTreeMap::new(),
        render_state: MaterialRenderState::additive_sprite(),
    });
    let index = ProjectAssetIndex::scan(temporary.path());

    let compiled = EffectCompiler::default()
        .compile_project(&effect, &index)
        .unwrap();

    let expanded = &compiled.root.material_programs[0];
    assert!(expanded.expressions.iter().all(|expression| !matches!(
        expression.kind,
        MaterialExpressionKind::FunctionCall { .. } | MaterialExpressionKind::FunctionInput(_)
    )));
    MaterialCompiler.compile(expanded).unwrap();
}
