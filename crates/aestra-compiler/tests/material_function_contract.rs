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
        custom_wesl: None,
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

fn custom_wesl_calling_program(function: &MaterialFunction) -> MaterialProgram {
    let color = MaterialExpressionId::from_u128(0xFA01);
    let phase = MaterialExpressionId::from_u128(0xFA02);
    let width = MaterialExpressionId::from_u128(0xFA03);
    let call = MaterialExpressionId::from_u128(0xFA04);
    let mut program = MaterialProgram::additive_sprite("Custom WESL caller");
    program.expressions = vec![
        MaterialExpression {
            id: color,
            kind: MaterialExpressionKind::Constant(MaterialValue::ColorSrgb([1.0; 4])),
        },
        MaterialExpression {
            id: phase,
            kind: MaterialExpressionKind::Input(aestra_core::material::MaterialInput::EffectTime),
        },
        MaterialExpression {
            id: width,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.35)),
        },
        MaterialExpression {
            id: call,
            kind: MaterialExpressionKind::FunctionCall {
                function: MaterialFunctionRef::Project(function.id),
                arguments: BTreeMap::from([
                    (function.inputs[0].id, phase),
                    (function.inputs[1].id, width),
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
fn typed_custom_wesl_calls_lower_without_erasing_the_validated_source() {
    let function = MaterialFunction::from_ron(include_str!(
        "../../../assets/materials/pulse_wave.aestra.material-function.ron"
    ))
    .unwrap();
    let program = custom_wesl_calling_program(&function);
    let ir = MaterialCompiler
        .compile_with_functions(&program, &MaterialFunctionLibrary::new([function.clone()]))
        .unwrap();

    assert!(ir.values.iter().any(|value| matches!(
        &value.instruction,
        MaterialIrInstruction::CustomWeslCall {
            function: id,
            entry_point,
            arguments,
            ..
        } if *id == function.id && entry_point == "pulse_wave" && arguments.len() == 2
    )));
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
    duplicate_alpha_call(&mut program);
    program.expressions[1].kind =
        MaterialExpressionKind::Input(aestra_core::material::MaterialInput::ParticleOpacity);
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
    assert_eq!(
        compiled.root.optimizations.material_function_calls_authored,
        2
    );
    assert_eq!(
        compiled
            .root
            .optimizations
            .material_function_calls_eliminated,
        1
    );
    assert_eq!(compiled.root.optimizations.material_function_calls_live, 1);
}

fn duplicate_alpha_call(program: &mut MaterialProgram) -> MaterialExpressionId {
    let original = program.outputs.alpha;
    let duplicate = MaterialExpressionId::from_u128(0xFB01);
    let sum = MaterialExpressionId::from_u128(0xFB02);
    let kind = program
        .expressions
        .iter()
        .find(|expression| expression.id == original)
        .unwrap()
        .kind
        .clone();
    program.expressions.extend([
        MaterialExpression {
            id: duplicate,
            kind,
        },
        MaterialExpression {
            id: sum,
            kind: MaterialExpressionKind::Add(original, duplicate),
        },
    ]);
    program.outputs.alpha = sum;
    duplicate
}

#[test]
fn identical_calls_share_expansion_and_preserve_every_call_source_independent_of_order() {
    let library = MaterialFunctionLibrary::new([scale_function()]);
    let mut program = calling_program(MaterialValue::Float(0.8));
    program.expressions[1].kind =
        MaterialExpressionKind::Input(aestra_core::material::MaterialInput::ParticleOpacity);
    let first = program.outputs.alpha;
    let second = duplicate_alpha_call(&mut program);
    let expanded = MaterialCompiler
        .inline_functions(&program, &library)
        .unwrap();
    assert_eq!(
        expanded
            .expressions
            .iter()
            .filter(|expression| matches!(expression.kind, MaterialExpressionKind::Multiply(..)))
            .count(),
        1
    );
    let ir = MaterialCompiler
        .compile_with_functions(&program, &library)
        .unwrap();
    let value = ir.source_map.values[&first];
    assert_eq!(value, ir.source_map.values[&second]);
    assert!(ir.source_map.expressions[&value].contains(&first));
    assert!(ir.source_map.expressions[&value].contains(&second));
    assert_eq!(ir.optimizations.function_calls_authored, 2);
    assert_eq!(ir.optimizations.function_calls_eliminated, 1);
    assert_eq!(ir.optimizations.function_calls_live, 1);

    program.expressions.reverse();
    assert_eq!(
        expanded,
        MaterialCompiler
            .inline_functions(&program, &library)
            .unwrap()
    );
    assert_eq!(
        ir,
        MaterialCompiler
            .compile_with_functions(&program, &library)
            .unwrap()
    );
}

#[test]
fn different_arguments_and_function_namespaces_keep_separate_invocations() {
    let mut program = calling_program(MaterialValue::Float(0.8));
    program.expressions[2].kind =
        MaterialExpressionKind::Input(aestra_core::material::MaterialInput::ParticleOpacity);
    let first = program.outputs.alpha;
    let second = duplicate_alpha_call(&mut program);
    let function = scale_function();
    let reference = MaterialFunctionRef::BuiltIn(function.id);
    let library = MaterialFunctionLibrary::new([function.clone()]);
    let argument = MaterialExpressionId::from_u128(0xFB03);
    program.expressions.push(MaterialExpression {
        id: argument,
        kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.6)),
    });
    let MaterialExpressionKind::FunctionCall { arguments, .. } = &mut program
        .expressions
        .iter_mut()
        .find(|expression| expression.id == second)
        .unwrap()
        .kind
    else {
        unreachable!()
    };
    arguments.insert(function.inputs[0].id, argument);
    let ir = MaterialCompiler
        .compile_with_functions(&program, &library)
        .unwrap();
    assert_ne!(ir.source_map.values[&first], ir.source_map.values[&second]);
    assert_eq!(ir.optimizations.function_calls_live, 2);
    assert_eq!(ir.optimizations.function_calls_eliminated, 0);

    let mut library = library;
    library.register_builtin(function.clone());
    let MaterialExpressionKind::FunctionCall {
        function: call_function,
        arguments,
        ..
    } = &mut program
        .expressions
        .iter_mut()
        .find(|expression| expression.id == second)
        .unwrap()
        .kind
    else {
        unreachable!()
    };
    *call_function = reference;
    arguments.insert(
        function.inputs[0].id,
        MaterialExpressionId::from_u128(0xF102),
    );
    let expanded = MaterialCompiler
        .inline_functions(&program, &library)
        .unwrap();
    assert_eq!(
        expanded
            .expressions
            .iter()
            .filter(|expression| matches!(expression.kind, MaterialExpressionKind::Multiply(..)))
            .count(),
        2
    );
}

#[test]
fn multiple_outputs_of_nested_functions_share_internal_work() {
    let leaf = scale_function();
    let mut wrapper = leaf.clone();
    wrapper.id = MaterialFunctionId::from_u128(0xFC01);
    wrapper.expressions[2].kind = MaterialExpressionKind::FunctionCall {
        function: MaterialFunctionRef::Project(leaf.id),
        arguments: BTreeMap::from([
            (leaf.inputs[0].id, wrapper.expressions[0].id),
            (leaf.inputs[1].id, wrapper.expressions[1].id),
        ]),
        output: leaf.outputs[0].id,
    };
    let extra = MaterialExpressionId::from_u128(0xFC02);
    wrapper.expressions.push(MaterialExpression {
        id: extra,
        kind: MaterialExpressionKind::Add(wrapper.expressions[2].id, wrapper.expressions[0].id),
    });
    wrapper.outputs.push(MaterialFunctionOutput {
        id: MaterialFunctionOutputId::from_u128(0xFC03),
        name: "Plus input".into(),
        value_type: MaterialValueType::Float,
        expression: extra,
    });
    let mut program = calling_program(MaterialValue::Float(0.8));
    let second = duplicate_alpha_call(&mut program);
    program.expressions[1].kind =
        MaterialExpressionKind::Input(aestra_core::material::MaterialInput::ParticleOpacity);
    for expression in &mut program.expressions {
        if let MaterialExpressionKind::FunctionCall {
            function, output, ..
        } = &mut expression.kind
        {
            *function = MaterialFunctionRef::Project(wrapper.id);
            if expression.id == second {
                *output = wrapper.outputs[1].id;
            }
        }
    }
    let library = MaterialFunctionLibrary::new([leaf, wrapper]);
    let expanded = MaterialCompiler
        .inline_functions(&program, &library)
        .unwrap();
    assert_eq!(
        expanded
            .expressions
            .iter()
            .filter(|expression| matches!(expression.kind, MaterialExpressionKind::Multiply(..)))
            .count(),
        1
    );
    let ir = MaterialCompiler
        .compile_with_functions(&program, &library)
        .unwrap();
    assert_eq!(ir.optimizations.function_calls_authored, 3); // Two outputs and one nested site.
    assert_eq!(ir.optimizations.function_calls_live, 3);
    assert_eq!(ir.optimizations.function_calls_eliminated, 0);
}

#[test]
fn custom_wesl_calls_and_their_wrappers_are_never_shared() {
    let custom = MaterialFunction::from_ron(include_str!(
        "../../../assets/materials/pulse_wave.aestra.material-function.ron"
    ))
    .unwrap();
    for wrapped in [false, true] {
        let mut program = custom_wesl_calling_program(&custom);
        let mut functions = vec![custom.clone()];
        if wrapped {
            let mut wrapper = custom.clone();
            wrapper.id = MaterialFunctionId::from_u128(0xFD01);
            wrapper.custom_wesl = None;
            wrapper.expressions = wrapper
                .inputs
                .iter()
                .enumerate()
                .map(|(index, input)| MaterialExpression {
                    id: MaterialExpressionId::from_u128(0xFD10 + index as u128),
                    kind: MaterialExpressionKind::FunctionInput(input.id),
                })
                .collect();
            let nested = MaterialExpressionId::from_u128(0xFD20);
            wrapper.expressions.push(MaterialExpression {
                id: nested,
                kind: MaterialExpressionKind::FunctionCall {
                    function: MaterialFunctionRef::Project(custom.id),
                    arguments: wrapper
                        .inputs
                        .iter()
                        .zip(&wrapper.expressions)
                        .map(|(input, expression)| (input.id, expression.id))
                        .collect(),
                    output: custom.outputs[0].id,
                },
            });
            wrapper.outputs[0].expression = nested;
            for expression in &mut program.expressions {
                if let MaterialExpressionKind::FunctionCall { function, .. } = &mut expression.kind
                {
                    *function = MaterialFunctionRef::Project(wrapper.id);
                }
            }
            functions.push(wrapper);
        }
        duplicate_alpha_call(&mut program);
        let ir = MaterialCompiler
            .compile_with_functions(&program, &MaterialFunctionLibrary::new(functions))
            .unwrap();
        assert_eq!(
            ir.values
                .iter()
                .filter(|value| matches!(
                    value.instruction,
                    MaterialIrInstruction::CustomWeslCall { .. }
                ))
                .count(),
            2
        );
        assert_eq!(ir.optimizations.function_calls_eliminated, 0);
    }
}

#[test]
fn dead_function_calls_are_recorded_as_eliminated_sources() {
    let mut program = calling_program(MaterialValue::Float(0.8));
    let call = program.outputs.alpha;
    program.outputs.alpha = MaterialExpressionId::from_u128(0xF102);
    let ir = MaterialCompiler
        .compile_with_functions(&program, &MaterialFunctionLibrary::new([scale_function()]))
        .unwrap();
    assert!(ir.source_map.eliminated.contains(&call));
    assert!(!ir.source_map.values.contains_key(&call));
    assert_eq!(ir.optimizations.function_calls_authored, 1);
    assert_eq!(ir.optimizations.function_calls_eliminated, 1);
    assert_eq!(ir.optimizations.function_calls_live, 0);
}
