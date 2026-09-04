use aestra_core::{
    MaterialExpressionId, MaterialFunctionId, MaterialFunctionInputId, MaterialFunctionOutputId,
    material::{
        MaterialExpression, MaterialExpressionKind, MaterialFunction, MaterialFunctionInput,
        MaterialFunctionOutput, MaterialSchemaVersion, MaterialValueType,
    },
};

#[test]
fn typed_material_function_round_trips_with_stable_identity() {
    let input = MaterialFunctionInputId::from_u128(0xA101);
    let expression = MaterialExpressionId::from_u128(0xA102);
    let function = MaterialFunction {
        id: MaterialFunctionId::from_u128(0xA100),
        schema_version: MaterialSchemaVersion::CURRENT,
        name: "Identity".into(),
        inputs: vec![MaterialFunctionInput {
            id: input,
            name: "Value".into(),
            value_type: MaterialValueType::Vec2,
        }],
        outputs: vec![MaterialFunctionOutput {
            id: MaterialFunctionOutputId::from_u128(0xA103),
            name: "Value".into(),
            value_type: MaterialValueType::Vec2,
            expression,
        }],
        expressions: vec![MaterialExpression {
            id: expression,
            kind: MaterialExpressionKind::FunctionInput(input),
        }],
        custom_wesl: None,
    };

    let encoded = function.to_pretty_ron().unwrap();

    assert_eq!(MaterialFunction::from_ron(&encoded).unwrap(), function);
    assert!(function.validate_structure().is_valid());
}

#[test]
fn custom_wesl_function_round_trips_and_rejects_resource_declarations() {
    let function = MaterialFunction::from_ron(include_str!(
        "../../../assets/materials/pulse_wave.aestra.material-function.ron"
    ))
    .unwrap();
    let encoded = function.to_pretty_ron().unwrap();

    assert_eq!(MaterialFunction::from_ron(&encoded).unwrap(), function);
    assert!(function.custom_wesl.is_some());

    let mut unsafe_function = function;
    unsafe_function.custom_wesl.as_mut().unwrap().source =
        "@group(0) @binding(0) var<uniform> secret: f32;".into();
    assert!(!unsafe_function.validate_structure().is_valid());
}
