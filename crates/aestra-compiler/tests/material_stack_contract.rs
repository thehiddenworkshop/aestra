use aestra_compiler::{
    MATERIAL_PRESET_DISSOLVE, MATERIAL_PRESET_SOFT_DISSOLVE, MATERIAL_PRESET_UV_DRIFT,
    MaterialCompiler, MaterialPresetCatalog, MaterialPresetCategory, MaterialPresetDefault,
    MaterialPresetDescriptor, MaterialPresetRecipe, MaterialStackEditError,
    MaterialStackFallbackReason, MaterialStackInsertTarget, MaterialStackModifierKind,
    MaterialStackMoveError, MaterialStackMoveTarget, MaterialStackPresetTarget,
    MaterialStackProjection, MaterialStackProperty,
};
use aestra_core::material::{
    MaterialEvaluationDomain, MaterialExpression, MaterialExpressionKind, MaterialInput,
    MaterialParameter, MaterialProgram, MaterialSamplerDescriptor, MaterialTextureColorSpace,
    MaterialTextureDescriptor, MaterialValue, MaterialValueType, MaterialVectorComponent,
};

fn texture_type() -> MaterialValueType {
    MaterialValueType::Texture2D(MaterialTextureDescriptor {
        color_space: MaterialTextureColorSpace::SrgbColor,
        sampler: MaterialSamplerDescriptor::default(),
    })
}
use aestra_core::{AssetId, MaterialExpressionId, MaterialParameterId, MaterialPresetId};

fn linear_stack_program() -> MaterialProgram {
    let uv = MaterialExpressionId::from_u128(0x5101);
    let speed = MaterialExpressionId::from_u128(0x5102);
    let time = MaterialExpressionId::from_u128(0x5103);
    let pan = MaterialExpressionId::from_u128(0x5104);
    let texture_parameter = MaterialParameterId::from_u128(0x5110);
    let texture = MaterialExpressionId::from_u128(0x5105);
    let sample = MaterialExpressionId::from_u128(0x5106);
    let sampled_alpha = MaterialExpressionId::from_u128(0x5107);
    let threshold = MaterialExpressionId::from_u128(0x5108);
    let edge_width = MaterialExpressionId::from_u128(0x5109);
    let dissolve_invert = MaterialExpressionId::from_u128(0x510a);
    let dissolve = MaterialExpressionId::from_u128(0x510b);
    let scene_depth = MaterialExpressionId::from_u128(0x510c);
    let pixel_depth = MaterialExpressionId::from_u128(0x510d);
    let fade_distance = MaterialExpressionId::from_u128(0x510e);
    let fade_invert = MaterialExpressionId::from_u128(0x510f);
    let soft_particle = MaterialExpressionId::from_u128(0x5111);
    let texture_type = texture_type();
    let mut program = MaterialProgram::additive_sprite("Linear stack");
    program.parameters.push(MaterialParameter {
        id: texture_parameter,
        name: "Texture".into(),
        value_type: texture_type,
        evaluation_domain: MaterialEvaluationDomain::Instance,
        default: Some(MaterialValue::Texture2D(AssetId::from_u128(0x5112))),
    });
    program.expressions = vec![
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: speed,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.1, 0.0])),
        },
        MaterialExpression {
            id: time,
            kind: MaterialExpressionKind::Input(MaterialInput::EffectTime),
        },
        MaterialExpression {
            id: pan,
            kind: MaterialExpressionKind::PanUv { uv, speed, time },
        },
        MaterialExpression {
            id: texture,
            kind: MaterialExpressionKind::Parameter(texture_parameter),
        },
        MaterialExpression {
            id: sample,
            kind: MaterialExpressionKind::SampleTexture { texture, uv: pan },
        },
        MaterialExpression {
            id: sampled_alpha,
            kind: MaterialExpressionKind::ExtractComponent {
                value: sample,
                component: MaterialVectorComponent::W,
            },
        },
        MaterialExpression {
            id: threshold,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.4)),
        },
        MaterialExpression {
            id: edge_width,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.1)),
        },
        MaterialExpression {
            id: dissolve_invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: dissolve,
            kind: MaterialExpressionKind::Dissolve {
                source: sampled_alpha,
                threshold,
                edge_width,
                invert: dissolve_invert,
            },
        },
        MaterialExpression {
            id: scene_depth,
            kind: MaterialExpressionKind::Input(MaterialInput::SceneDepth),
        },
        MaterialExpression {
            id: pixel_depth,
            kind: MaterialExpressionKind::Input(MaterialInput::PixelDepth),
        },
        MaterialExpression {
            id: fade_distance,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
        },
        MaterialExpression {
            id: fade_invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: soft_particle,
            kind: MaterialExpressionKind::SoftParticle {
                alpha: dissolve,
                scene_depth,
                pixel_depth,
                fade_distance,
                invert: fade_invert,
            },
        },
    ];
    program.outputs.color = sample;
    program.outputs.alpha = soft_particle;
    program
}

fn reorderable_uv_stack_program() -> MaterialProgram {
    let uv = MaterialExpressionId::from_u128(0x5301);
    let speed = MaterialExpressionId::from_u128(0x5302);
    let time = MaterialExpressionId::from_u128(0x5303);
    let pan = MaterialExpressionId::from_u128(0x5304);
    let center = MaterialExpressionId::from_u128(0x5305);
    let angle = MaterialExpressionId::from_u128(0x5306);
    let rotate = MaterialExpressionId::from_u128(0x5307);
    let scale_value = MaterialExpressionId::from_u128(0x5308);
    let scale = MaterialExpressionId::from_u128(0x5309);
    let texture_parameter = MaterialParameterId::from_u128(0x530a);
    let texture = MaterialExpressionId::from_u128(0x530b);
    let sample = MaterialExpressionId::from_u128(0x530c);
    let alpha = MaterialExpressionId::from_u128(0x530d);
    let texture_type = texture_type();
    let mut program = MaterialProgram::additive_sprite("Reorderable UV stack");
    program.parameters.push(MaterialParameter {
        id: texture_parameter,
        name: "Texture".into(),
        value_type: texture_type,
        evaluation_domain: MaterialEvaluationDomain::Instance,
        default: Some(MaterialValue::Texture2D(AssetId::from_u128(0x530e))),
    });
    program.expressions = vec![
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: speed,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.1, 0.0])),
        },
        MaterialExpression {
            id: time,
            kind: MaterialExpressionKind::Input(MaterialInput::EffectTime),
        },
        MaterialExpression {
            id: pan,
            kind: MaterialExpressionKind::PanUv { uv, speed, time },
        },
        MaterialExpression {
            id: center,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.5, 0.5])),
        },
        MaterialExpression {
            id: angle,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
        },
        MaterialExpression {
            id: rotate,
            kind: MaterialExpressionKind::RotateUv {
                uv: pan,
                center,
                angle,
            },
        },
        MaterialExpression {
            id: scale_value,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([2.0, 2.0])),
        },
        MaterialExpression {
            id: scale,
            kind: MaterialExpressionKind::ScaleUv {
                uv: rotate,
                center,
                scale: scale_value,
            },
        },
        MaterialExpression {
            id: texture,
            kind: MaterialExpressionKind::Parameter(texture_parameter),
        },
        MaterialExpression {
            id: sample,
            kind: MaterialExpressionKind::SampleTexture { texture, uv: scale },
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::ExtractComponent {
                value: sample,
                component: MaterialVectorComponent::W,
            },
        },
    ];
    program.outputs.color = sample;
    program.outputs.alpha = alpha;
    program
}

#[test]
fn linear_semantic_program_projects_in_source_to_output_order() {
    let program = linear_stack_program();

    let MaterialStackProjection::Stack { entries } =
        MaterialCompiler.project_stack(&program).unwrap()
    else {
        panic!("linear semantic program must project as a stack");
    };
    assert_eq!(
        entries.iter().map(|entry| entry.kind).collect::<Vec<_>>(),
        vec![
            MaterialStackModifierKind::PanUv,
            MaterialStackModifierKind::BaseTexture,
            MaterialStackModifierKind::Dissolve,
            MaterialStackModifierKind::SoftParticle,
        ]
    );
    assert_eq!(
        entries[0].expression,
        MaterialExpressionId::from_u128(0x5104)
    );
    assert_eq!(
        entries[3].expression,
        MaterialExpressionId::from_u128(0x5111)
    );
}

#[test]
fn projection_is_independent_of_authored_expression_order() {
    let program = linear_stack_program();
    let mut reordered = program.clone();
    reordered.expressions.reverse();

    assert_eq!(
        MaterialCompiler.project_stack(&program).unwrap(),
        MaterialCompiler.project_stack(&reordered).unwrap()
    );
}

#[test]
fn shared_modifier_fan_out_requires_the_advanced_representation() {
    let mut program = linear_stack_program();
    let pan = MaterialExpressionId::from_u128(0x5104);
    let texture = MaterialExpressionId::from_u128(0x5105);
    let alternate_sample = MaterialExpressionId::from_u128(0x5113);
    program.expressions.push(MaterialExpression {
        id: alternate_sample,
        kind: MaterialExpressionKind::SampleTexture { texture, uv: pan },
    });
    program.outputs.color = alternate_sample;

    let MaterialStackProjection::Advanced { reason } =
        MaterialCompiler.project_stack(&program).unwrap()
    else {
        panic!("one modifier feeding separate chains must require the advanced representation");
    };
    assert_eq!(
        reason,
        MaterialStackFallbackReason::Branched { expression: pan }
    );
}

#[test]
fn independent_texture_chains_require_the_advanced_representation() {
    let texture_parameter = MaterialParameterId::from_u128(0x5201);
    let texture = MaterialExpressionId::from_u128(0x5202);
    let uv = MaterialExpressionId::from_u128(0x5203);
    let first = MaterialExpressionId::from_u128(0x5204);
    let second = MaterialExpressionId::from_u128(0x5205);
    let alpha = MaterialExpressionId::from_u128(0x5206);
    let texture_type = texture_type();
    let mut program = MaterialProgram::additive_sprite("Branched textures");
    program.parameters.push(MaterialParameter {
        id: texture_parameter,
        name: "Texture".into(),
        value_type: texture_type,
        evaluation_domain: MaterialEvaluationDomain::Instance,
        default: Some(MaterialValue::Texture2D(AssetId::from_u128(0x5207))),
    });
    program.expressions = vec![
        MaterialExpression {
            id: texture,
            kind: MaterialExpressionKind::Parameter(texture_parameter),
        },
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: first,
            kind: MaterialExpressionKind::SampleTexture { texture, uv },
        },
        MaterialExpression {
            id: second,
            kind: MaterialExpressionKind::SampleTexture { texture, uv },
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::ExtractComponent {
                value: second,
                component: MaterialVectorComponent::W,
            },
        },
    ];
    program.outputs.color = first;
    program.outputs.alpha = alpha;

    let MaterialStackProjection::Advanced { reason } =
        MaterialCompiler.project_stack(&program).unwrap()
    else {
        panic!("independent texture chains must not be represented as one stack");
    };
    assert_eq!(
        reason,
        MaterialStackFallbackReason::MultipleRoots {
            expressions: vec![first, second],
        }
    );
}

#[test]
fn move_targets_only_include_valid_positions_in_a_direct_typed_chain() {
    let program = reorderable_uv_stack_program();
    let rotate = MaterialExpressionId::from_u128(0x5307);
    let sample = MaterialExpressionId::from_u128(0x530c);

    assert_eq!(
        MaterialCompiler
            .stack_move_targets(&program, rotate)
            .unwrap(),
        vec![
            MaterialStackMoveTarget { index: 0 },
            MaterialStackMoveTarget { index: 2 },
        ]
    );
    assert!(
        MaterialCompiler
            .stack_move_targets(&program, sample)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn stack_move_preserves_ids_and_rewires_the_terminal_consumer() {
    let program = reorderable_uv_stack_program();
    let pan = MaterialExpressionId::from_u128(0x5304);
    let rotate = MaterialExpressionId::from_u128(0x5307);
    let scale = MaterialExpressionId::from_u128(0x5309);
    let sample = MaterialExpressionId::from_u128(0x530c);
    let original_ids = program
        .expressions
        .iter()
        .map(|expression| expression.id)
        .collect::<Vec<_>>();

    let plan = MaterialCompiler.plan_stack_move(&program, pan, 2).unwrap();
    assert_eq!(plan.from_index, 0);
    assert_eq!(plan.to_index, 2);
    assert_eq!(
        plan.replacement
            .expressions
            .iter()
            .map(|expression| expression.id)
            .collect::<Vec<_>>(),
        original_ids
    );
    let MaterialStackProjection::Stack { entries } =
        MaterialCompiler.project_stack(&plan.replacement).unwrap()
    else {
        panic!("moved program must remain a stack");
    };
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.expression)
            .collect::<Vec<_>>(),
        vec![rotate, scale, pan, sample]
    );
    assert!(matches!(
        plan.replacement
            .expressions
            .iter()
            .find(|expression| expression.id == sample)
            .unwrap()
            .kind,
        MaterialExpressionKind::SampleTexture { uv, .. } if uv == pan
    ));
}

#[test]
fn incompatible_and_advanced_moves_are_rejected_without_a_replacement() {
    let program = reorderable_uv_stack_program();
    let pan = MaterialExpressionId::from_u128(0x5304);
    assert!(matches!(
        MaterialCompiler.plan_stack_move(&program, pan, 3),
        Err(MaterialStackMoveError::IncompatibleTarget { index: 3 })
    ));

    let mut advanced = linear_stack_program();
    let source = MaterialExpressionId::from_u128(0x5104);
    let texture = MaterialExpressionId::from_u128(0x5105);
    let alternate = MaterialExpressionId::from_u128(0x5310);
    advanced.expressions.push(MaterialExpression {
        id: alternate,
        kind: MaterialExpressionKind::SampleTexture {
            texture,
            uv: source,
        },
    });
    advanced.outputs.color = alternate;
    assert!(matches!(
        MaterialCompiler.plan_stack_move(&advanced, source, 1),
        Err(MaterialStackMoveError::Advanced)
    ));
}

#[test]
fn compatible_modifier_insertion_uses_defaults_and_preserves_the_linear_stack() {
    let program = reorderable_uv_stack_program();
    let rotate = MaterialExpressionId::from_u128(0x5307);
    let targets = MaterialCompiler.stack_insert_targets(&program).unwrap();
    assert!(targets.contains(&MaterialStackInsertTarget {
        index: 1,
        kind: MaterialStackModifierKind::ScaleUv,
    }));

    let plan = MaterialCompiler
        .plan_stack_insert(&program, MaterialStackModifierKind::ScaleUv, 1)
        .unwrap();
    assert_eq!(plan.index, 1);
    assert_eq!(plan.kind, MaterialStackModifierKind::ScaleUv);
    let MaterialStackProjection::Stack { entries } =
        MaterialCompiler.project_stack(&plan.replacement).unwrap()
    else {
        panic!("inserted modifier must remain a linear stack");
    };
    assert_eq!(entries[1].expression, plan.expression);
    assert_eq!(entries[2].expression, rotate);
    assert!(entries[1].enabled);
}

#[test]
fn compatible_preset_insertion_is_one_configured_stack_replacement() {
    let program = reorderable_uv_stack_program();
    let original_entries = match MaterialCompiler.project_stack(&program).unwrap() {
        MaterialStackProjection::Stack { entries } => entries,
        MaterialStackProjection::Advanced { .. } => panic!("fixture must be a stack"),
    };
    let targets = MaterialCompiler.stack_preset_targets(&program).unwrap();
    assert!(targets.contains(&MaterialStackPresetTarget {
        index: 0,
        preset: MATERIAL_PRESET_UV_DRIFT,
    }));

    let plan = MaterialCompiler
        .plan_stack_insert_preset(&program, MATERIAL_PRESET_UV_DRIFT, 0)
        .unwrap();
    assert_eq!(plan.expressions.len(), 2);
    let MaterialStackProjection::Stack { entries } =
        MaterialCompiler.project_stack(&plan.replacement).unwrap()
    else {
        panic!("preset replacement must remain a stack");
    };
    assert_eq!(entries[0].expression, plan.expressions[0]);
    assert_eq!(entries[0].kind, MaterialStackModifierKind::PanUv);
    assert_eq!(entries[1].expression, plan.expressions[1]);
    assert_eq!(entries[1].kind, MaterialStackModifierKind::ScaleUv);
    assert_eq!(
        entries[2..]
            .iter()
            .map(|entry| entry.expression)
            .collect::<Vec<_>>(),
        original_entries
            .iter()
            .map(|entry| entry.expression)
            .collect::<Vec<_>>()
    );
    let pan_settings = MaterialCompiler
        .stack_modifier_properties(&plan.replacement, plan.expressions[0])
        .unwrap();
    assert_eq!(
        pan_settings
            .iter()
            .find(|property| property.property == MaterialStackProperty::Speed)
            .unwrap()
            .value,
        MaterialValue::Vec2([0.15, 0.05])
    );
    let scale_settings = MaterialCompiler
        .stack_modifier_properties(&plan.replacement, plan.expressions[1])
        .unwrap();
    assert_eq!(
        scale_settings
            .iter()
            .find(|property| property.property == MaterialStackProperty::Scale)
            .unwrap()
            .value,
        MaterialValue::Vec2([1.1, 1.1])
    );
}

#[test]
fn builtin_preset_catalog_exposes_stable_searchable_semantic_recipes() {
    let catalog = MaterialCompiler.material_preset_catalog();
    assert_eq!(catalog.iter().len(), 4);
    let dissolve = catalog.get(MATERIAL_PRESET_DISSOLVE).unwrap();
    assert_eq!(dissolve.display_name, "Dissolve");
    assert_eq!(dissolve.category, MaterialPresetCategory::Masking);
    assert!(dissolve.tags.iter().any(|tag| tag == "threshold"));
    let MaterialPresetRecipe::Stack {
        modifiers,
        defaults,
    } = &dissolve.recipe
    else {
        panic!("built-in dissolve should remain a stack recipe")
    };
    assert_eq!(modifiers, &[MaterialStackModifierKind::Dissolve]);
    assert!(defaults.iter().any(|default| {
        default.property == MaterialStackProperty::Threshold
            && default.value == MaterialValue::Float(0.5)
    }));
}

#[test]
fn dissolve_preset_is_compatibility_filtered_and_configured_by_the_catalog_recipe() {
    let program = linear_stack_program();
    let target = MaterialCompiler
        .stack_preset_targets(&program)
        .unwrap()
        .into_iter()
        .find(|target| target.preset == MATERIAL_PRESET_DISSOLVE)
        .expect("the scalar mask chain accepts the dissolve preset");
    let plan = MaterialCompiler
        .plan_stack_insert_preset(&program, MATERIAL_PRESET_DISSOLVE, target.index)
        .unwrap();
    assert_eq!(plan.expressions.len(), 1);
    let properties = MaterialCompiler
        .stack_modifier_properties(&plan.replacement, plan.expressions[0])
        .unwrap();
    assert!(properties.iter().any(|property| {
        property.property == MaterialStackProperty::Threshold
            && property.value == MaterialValue::Float(0.5)
    }));
    assert!(properties.iter().any(|property| {
        property.property == MaterialStackProperty::EdgeWidth
            && property.value == MaterialValue::Float(0.06)
    }));
}

#[test]
fn explicit_preset_catalogs_accept_new_semantic_recipes_without_compiler_changes() {
    let preset = MaterialPresetId::from_u128(0xA357_1000);
    let mut catalog = MaterialPresetCatalog::default();
    catalog.register(MaterialPresetDescriptor {
        schema_version: aestra_core::material::MaterialPresetSchemaVersion::CURRENT,
        id: preset,
        display_name: "Wide UV".into(),
        description: "Scales UV coordinates horizontally.".into(),
        category: MaterialPresetCategory::Shaping,
        tags: vec!["uv".into(), "wide".into()],
        recipe: MaterialPresetRecipe::Stack {
            modifiers: vec![MaterialStackModifierKind::ScaleUv],
            defaults: vec![MaterialPresetDefault {
                step: 0,
                property: MaterialStackProperty::Scale,
                value: MaterialValue::Vec2([2.0, 1.0]),
            }],
        },
    });
    let program = reorderable_uv_stack_program();
    let target = MaterialCompiler
        .stack_preset_targets_with_catalog(&program, &catalog)
        .unwrap()
        .into_iter()
        .next()
        .expect("the custom UV recipe should expose a compatible insertion edge");
    assert_eq!(target.preset, preset);
    let plan = MaterialCompiler
        .plan_stack_insert_preset_with_catalog(&program, &catalog, preset, target.index)
        .unwrap();
    let properties = MaterialCompiler
        .stack_modifier_properties(&plan.replacement, plan.expressions[0])
        .unwrap();
    assert!(properties.iter().any(|property| {
        property.property == MaterialStackProperty::Scale
            && property.value == MaterialValue::Vec2([2.0, 1.0])
    }));
}

#[test]
fn project_graph_recipe_materializes_a_branched_hologram_atomically() {
    let preset = MaterialPresetDescriptor::from_ron(include_str!(
        "../../../assets/materials/hologram.aestra.material-preset.ron"
    ))
    .unwrap();
    let preset_id = preset.id;
    let mut catalog = MaterialPresetCatalog::default();
    catalog.register(preset);
    let program = linear_stack_program();
    let original_expression_count = program.expressions.len();
    let target = MaterialCompiler
        .stack_preset_targets_with_catalog(&program, &catalog)
        .unwrap()
        .into_iter()
        .find(|target| target.preset == preset_id)
        .expect("the hologram graph should be compatible with the terminal alpha edge");

    let plan = MaterialCompiler
        .plan_stack_insert_preset_with_catalog(&program, &catalog, preset_id, target.index)
        .unwrap();

    assert_eq!(plan.expressions.len(), 12);
    assert_eq!(
        plan.replacement.expressions.len(),
        original_expression_count + plan.expressions.len()
    );
    assert_ne!(plan.replacement.outputs.color, program.outputs.color);
    assert_ne!(plan.replacement.outputs.alpha, program.outputs.alpha);
    assert!(plan.replacement.analyze().is_ok());
    assert!(matches!(
        MaterialCompiler.project_stack(&plan.replacement).unwrap(),
        MaterialStackProjection::Advanced { .. }
    ));
    assert!(plan.expressions.iter().all(|id| {
        plan.replacement
            .expressions
            .iter()
            .any(|expression| expression.id == *id)
    }));
}

#[test]
fn incompatible_preset_is_rejected_without_a_partial_replacement() {
    let program = reorderable_uv_stack_program();
    assert!(matches!(
        MaterialCompiler.plan_stack_insert_preset(&program, MATERIAL_PRESET_SOFT_DISSOLVE, 0,),
        Err(MaterialStackEditError::IncompatiblePreset {
            preset: MATERIAL_PRESET_SOFT_DISSOLVE,
            index: 0,
        })
    ));
}

#[test]
fn modifier_removal_reconnects_the_primary_chain_and_rejects_unsafe_nodes() {
    let program = reorderable_uv_stack_program();
    let pan = MaterialExpressionId::from_u128(0x5304);
    let angle = MaterialExpressionId::from_u128(0x5306);
    let rotate = MaterialExpressionId::from_u128(0x5307);
    let scale = MaterialExpressionId::from_u128(0x5309);
    let sample = MaterialExpressionId::from_u128(0x530c);

    let plan = MaterialCompiler
        .plan_stack_remove(&program, rotate)
        .unwrap();
    assert!(
        !plan
            .replacement
            .expressions
            .iter()
            .any(|expression| expression.id == rotate)
    );
    assert!(
        !plan
            .replacement
            .expressions
            .iter()
            .any(|expression| expression.id == angle)
    );
    assert!(matches!(
        plan.replacement
            .expressions
            .iter()
            .find(|expression| expression.id == scale)
            .unwrap()
            .kind,
        MaterialExpressionKind::ScaleUv { uv, .. } if uv == pan
    ));
    assert!(matches!(
        MaterialCompiler.plan_stack_remove(&program, sample),
        Err(MaterialStackEditError::IncompatibleRemoval { expression }) if expression == sample
    ));
}

#[test]
fn disabled_modifier_is_a_lossless_typed_bypass() {
    let program = reorderable_uv_stack_program();
    let rotate = MaterialExpressionId::from_u128(0x5307);
    let original_kind = program
        .expressions
        .iter()
        .find(|expression| expression.id == rotate)
        .unwrap()
        .kind
        .clone();

    let disabled = MaterialCompiler
        .plan_stack_set_enabled(&program, rotate, false)
        .unwrap();
    assert_eq!(disabled.replacement.disabled_expressions, vec![rotate]);
    assert_eq!(
        disabled
            .replacement
            .expressions
            .iter()
            .find(|expression| expression.id == rotate)
            .unwrap()
            .kind,
        original_kind
    );
    let MaterialStackProjection::Stack { entries } = MaterialCompiler
        .project_stack(&disabled.replacement)
        .unwrap()
    else {
        panic!("disabled modifier must remain visible in the stack");
    };
    assert!(
        !entries
            .iter()
            .find(|entry| entry.expression == rotate)
            .unwrap()
            .enabled
    );
    let ir = MaterialCompiler.compile(&disabled.replacement).unwrap();
    assert!(ir.source_map.eliminated.contains(&rotate));
    let serialized = disabled.replacement.to_pretty_ron().unwrap();
    assert_eq!(
        MaterialProgram::from_ron(&serialized).unwrap(),
        disabled.replacement.normalized()
    );

    let enabled = MaterialCompiler
        .plan_stack_set_enabled(&disabled.replacement, rotate, true)
        .unwrap();
    assert!(enabled.replacement.disabled_expressions.is_empty());
    assert_eq!(enabled.replacement, program);
}

#[test]
fn modifier_inspector_reflects_only_owned_constant_settings() {
    let program = reorderable_uv_stack_program();
    let pan = MaterialExpressionId::from_u128(0x5304);
    let properties = MaterialCompiler
        .stack_modifier_properties(&program, pan)
        .unwrap();

    assert_eq!(properties.len(), 1);
    assert_eq!(properties[0].property, MaterialStackProperty::Speed);
    assert_eq!(properties[0].name, "Speed");
    assert_eq!(properties[0].value, MaterialValue::Vec2([0.1, 0.0]));

    let sample = MaterialExpressionId::from_u128(0x530c);
    assert!(
        MaterialCompiler
            .stack_modifier_properties(&program, sample)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn modifier_property_edit_preserves_identity_and_validates_the_replacement() {
    let program = reorderable_uv_stack_program();
    let pan = MaterialExpressionId::from_u128(0x5304);
    let speed = MaterialExpressionId::from_u128(0x5302);
    let original_ids = program
        .expressions
        .iter()
        .map(|expression| expression.id)
        .collect::<Vec<_>>();

    let plan = MaterialCompiler
        .plan_stack_set_property(
            &program,
            pan,
            MaterialStackProperty::Speed,
            MaterialValue::Vec2([0.75, -0.25]),
        )
        .unwrap();
    assert_eq!(
        plan.replacement
            .expressions
            .iter()
            .map(|expression| expression.id)
            .collect::<Vec<_>>(),
        original_ids
    );
    assert!(matches!(
        &plan
            .replacement
            .expressions
            .iter()
            .find(|expression| expression.id == speed)
            .unwrap()
            .kind,
        MaterialExpressionKind::Constant(MaterialValue::Vec2(value))
            if *value == [0.75, -0.25]
    ));
    plan.replacement.analyze().unwrap();
    assert!(matches!(
        MaterialCompiler.plan_stack_set_property(
            &program,
            pan,
            MaterialStackProperty::Speed,
            MaterialValue::Float(1.0),
        ),
        Err(MaterialStackEditError::PropertyTypeMismatch {
            property: MaterialStackProperty::Speed
        })
    ));
}
