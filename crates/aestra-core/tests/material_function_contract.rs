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
    };

    let encoded = function.to_pretty_ron().unwrap();

    assert_eq!(MaterialFunction::from_ron(&encoded).unwrap(), function);
    assert!(function.validate_structure().is_valid());
}
