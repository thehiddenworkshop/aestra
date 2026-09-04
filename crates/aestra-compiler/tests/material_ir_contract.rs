use aestra_compiler::{
    MaterialCompileError, MaterialCompiler, MaterialIrConstant, MaterialIrInstruction,
    MaterialIrValueId, MaterialStackModifierKind, MaterialStackProjection,
    MaterialTextureSamplingMode,
};
use aestra_core::{
    AssetId, DiagnosticCode, MaterialExpressionId, MaterialParameterId, MaterialProgramId,
    material::{
        MaterialAddressMode, MaterialEvaluationDomain, MaterialExpression, MaterialExpressionKind,
        MaterialFilterMode, MaterialInput, MaterialMipFilterMode, MaterialParameter,
        MaterialProgram, MaterialSamplerDescriptor, MaterialTextureColorSpace,
        MaterialTextureDescriptor, MaterialValue, MaterialValueType, MaterialVectorComponent,
    },
};

fn texture_descriptor(color_space: MaterialTextureColorSpace) -> MaterialTextureDescriptor {
    MaterialTextureDescriptor {
        color_space,
        sampler: MaterialSamplerDescriptor {
            filter: MaterialFilterMode::Linear,
            mip_filter: MaterialMipFilterMode::Linear,
            address_u: MaterialAddressMode::Repeat,
            address_v: MaterialAddressMode::Repeat,
        },
    }
}

fn two_texture_flame_program() -> MaterialProgram {
    let main_parameter = MaterialParameterId::from_u128(0x1001);
    let noise_parameter = MaterialParameterId::from_u128(0x1002);
    let tint_parameter = MaterialParameterId::from_u128(0x1003);
    let intensity_parameter = MaterialParameterId::from_u128(0x1004);
    let main = MaterialExpressionId::from_u128(0x2001);
    let noise = MaterialExpressionId::from_u128(0x2002);
    let tint = MaterialExpressionId::from_u128(0x2003);
    let intensity = MaterialExpressionId::from_u128(0x2004);
    let uv = MaterialExpressionId::from_u128(0x2005);
    let main_sample = MaterialExpressionId::from_u128(0x2006);
    let noise_sample = MaterialExpressionId::from_u128(0x2007);
    let combined = MaterialExpressionId::from_u128(0x2008);
    let tinted = MaterialExpressionId::from_u128(0x2009);
    let bright = MaterialExpressionId::from_u128(0x200A);
    let particle_color = MaterialExpressionId::from_u128(0x200B);
    let color = MaterialExpressionId::from_u128(0x200C);
    let opacity = MaterialExpressionId::from_u128(0x200D);
    let alpha = MaterialExpressionId::from_u128(0x200E);
    let mut program = MaterialProgram::additive_sprite("Two Texture Flame");
    program.id = MaterialProgramId::from_u128(0x1000);
    program.parameters = vec![
        MaterialParameter {
            id: main_parameter,
            name: "main_texture".into(),
            value_type: MaterialValueType::Texture2D(texture_descriptor(
                MaterialTextureColorSpace::SrgbColor,
            )),
            evaluation_domain: MaterialEvaluationDomain::Instance,
            default: Some(MaterialValue::Texture2D(AssetId::from_u128(0x3001))),
        },
        MaterialParameter {
            id: noise_parameter,
            name: "noise_texture".into(),
            value_type: MaterialValueType::Texture2D(texture_descriptor(
                MaterialTextureColorSpace::LinearData,
            )),
            evaluation_domain: MaterialEvaluationDomain::Instance,
            default: Some(MaterialValue::Texture2D(AssetId::from_u128(0x3002))),
        },
        MaterialParameter {
            id: tint_parameter,
            name: "tint".into(),
            value_type: MaterialValueType::Color,
            evaluation_domain: MaterialEvaluationDomain::Instance,
            default: Some(MaterialValue::ColorSrgb([1.0, 0.5, 0.25, 1.0])),
        },
        MaterialParameter {
            id: intensity_parameter,
            name: "intensity".into(),
            value_type: MaterialValueType::Float,
            evaluation_domain: MaterialEvaluationDomain::Effect,
            default: Some(MaterialValue::Float(2.0)),
        },
    ];
    program.expressions = vec![
        MaterialExpression {
            id: main,
            kind: MaterialExpressionKind::Parameter(main_parameter),
        },
        MaterialExpression {
            id: noise,
            kind: MaterialExpressionKind::Parameter(noise_parameter),
        },
        MaterialExpression {
            id: tint,
            kind: MaterialExpressionKind::Parameter(tint_parameter),
        },
        MaterialExpression {
            id: intensity,
            kind: MaterialExpressionKind::Parameter(intensity_parameter),
        },
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: main_sample,
            kind: MaterialExpressionKind::SampleTexture { texture: main, uv },
        },
        MaterialExpression {
            id: noise_sample,
            kind: MaterialExpressionKind::SampleTexture { texture: noise, uv },
        },
        MaterialExpression {
            id: combined,
            kind: MaterialExpressionKind::Multiply(main_sample, noise_sample),
        },
        MaterialExpression {
            id: tinted,
            kind: MaterialExpressionKind::Multiply(combined, tint),
        },
        MaterialExpression {
            id: bright,
            kind: MaterialExpressionKind::Multiply(tinted, intensity),
        },
        MaterialExpression {
            id: particle_color,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleColor),
        },
        MaterialExpression {
            id: color,
            kind: MaterialExpressionKind::Multiply(bright, particle_color),
        },
        MaterialExpression {
            id: opacity,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleOpacity),
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::Multiply(opacity, intensity),
        },
    ];
    program.outputs.color = color;
    program.outputs.alpha = alpha;
    program
}

#[test]
fn valid_two_texture_flame_lowers_to_typed_backend_neutral_ir() {
    let program = two_texture_flame_program();

    let ir = MaterialCompiler.compile(&program).unwrap();

    assert_eq!(ir.source, program.id);
    assert_eq!(ir.parameters.len(), 4);
    assert_eq!(ir.optimizations.texture_samples_authored, 2);
    assert_eq!(ir.optimizations.texture_samples_eliminated, 0);
    assert_eq!(ir.optimizations.texture_samples_live, 2);
    assert_eq!(
        ir.values
            .iter()
            .filter(|value| matches!(
                value.instruction,
                MaterialIrInstruction::SampleTexture { .. }
            ))
            .count(),
        2
    );
    assert_eq!(
        ir.value(ir.outputs.color).unwrap().value_type,
        MaterialValueType::Color
    );
    assert_eq!(
        ir.value(ir.outputs.alpha).unwrap().value_type,
        MaterialValueType::Float
    );
    assert!(
        ir.values
            .iter()
            .enumerate()
            .all(|(index, value)| value.id == MaterialIrValueId(index as u32))
    );
    assert_eq!(ir.source_map.eliminated.len(), 0);
}

#[test]
fn uv_transforms_lower_as_semantic_instructions_with_source_mapping() {
    let uv = MaterialExpressionId::from_u128(0x3101);
    let speed = MaterialExpressionId::from_u128(0x3102);
    let time = MaterialExpressionId::from_u128(0x3103);
    let pan = MaterialExpressionId::from_u128(0x3104);
    let center = MaterialExpressionId::from_u128(0x3105);
    let angle = MaterialExpressionId::from_u128(0x3106);
    let rotate = MaterialExpressionId::from_u128(0x3107);
    let scale_value = MaterialExpressionId::from_u128(0x3108);
    let scale = MaterialExpressionId::from_u128(0x3109);
    let alpha = MaterialExpressionId::from_u128(0x310A);
    let mut program = MaterialProgram::additive_sprite("UV transform IR");
    program.expressions.extend([
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: speed,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.25, -0.5])),
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
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.75)),
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
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([1.5, 0.75])),
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
            id: alpha,
            kind: MaterialExpressionKind::ExtractComponent {
                value: scale,
                component: MaterialVectorComponent::X,
            },
        },
    ]);
    program.outputs.alpha = alpha;

    let ir = MaterialCompiler.compile(&program).unwrap();
    let pan_value = ir.source_map.values[&pan];
    let MaterialIrInstruction::PanUv {
        uv: ir_uv,
        speed: ir_speed,
        time: ir_time,
    } = ir.value(pan_value).unwrap().instruction
    else {
        panic!("PanUV must survive lowering as a semantic IR instruction");
    };

    assert_eq!(ir_uv, ir.source_map.values[&uv]);
    assert_eq!(ir_speed, ir.source_map.values[&speed]);
    assert_eq!(ir_time, ir.source_map.values[&time]);
    assert!(matches!(
        ir.value(ir_time).unwrap().instruction,
        MaterialIrInstruction::Input(MaterialInput::EffectTime)
    ));
    let rotate_value = ir.source_map.values[&rotate];
    let MaterialIrInstruction::RotateUv {
        uv: ir_rotate_uv,
        center: ir_center,
        angle: ir_angle,
    } = ir.value(rotate_value).unwrap().instruction
    else {
        panic!("RotateUV must survive lowering as a semantic IR instruction");
    };
    assert_eq!(ir_rotate_uv, pan_value);
    assert_eq!(ir_center, ir.source_map.values[&center]);
    assert_eq!(ir_angle, ir.source_map.values[&angle]);
    let scale_ir_value = ir.source_map.values[&scale];
    let MaterialIrInstruction::ScaleUv {
        uv: ir_scale_uv,
        center: ir_scale_center,
        scale: ir_scale,
    } = ir.value(scale_ir_value).unwrap().instruction
    else {
        panic!("ScaleUV must survive lowering as a semantic IR instruction");
    };
    assert_eq!(ir_scale_uv, rotate_value);
    assert_eq!(ir_scale_center, ir.source_map.values[&center]);
    assert_eq!(ir_scale, ir.source_map.values[&scale_value]);
}

#[test]
fn remap_lowers_with_promoted_bounds_and_source_mapping() {
    let value = MaterialExpressionId::from_u128(0x3201);
    let input_min = MaterialExpressionId::from_u128(0x3202);
    let input_max = MaterialExpressionId::from_u128(0x3203);
    let output_min = MaterialExpressionId::from_u128(0x3204);
    let output_max = MaterialExpressionId::from_u128(0x3205);
    let remap = MaterialExpressionId::from_u128(0x3206);
    let alpha = MaterialExpressionId::from_u128(0x3207);
    let mut program = MaterialProgram::additive_sprite("Remap IR");
    program.expressions.extend([
        MaterialExpression {
            id: value,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: input_min,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
        },
        MaterialExpression {
            id: input_max,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        },
        MaterialExpression {
            id: output_min,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([-1.0; 2])),
        },
        MaterialExpression {
            id: output_max,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([1.0; 2])),
        },
        MaterialExpression {
            id: remap,
            kind: MaterialExpressionKind::Remap {
                value,
                input_min,
                input_max,
                output_min,
                output_max,
            },
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::ExtractComponent {
                value: remap,
                component: MaterialVectorComponent::X,
            },
        },
    ]);
    program.outputs.alpha = alpha;

    let ir = MaterialCompiler.compile(&program).unwrap();
    let remap_value = ir.source_map.values[&remap];
    let MaterialIrInstruction::Remap {
        value: ir_value,
        input_min: ir_input_min,
        input_max: ir_input_max,
        output_min: ir_output_min,
        output_max: ir_output_max,
    } = ir.value(remap_value).unwrap().instruction
    else {
        panic!("Remap must survive lowering as a semantic IR instruction");
    };
    assert_eq!(
        ir.value(remap_value).unwrap().value_type,
        MaterialValueType::Vec2
    );
    assert_eq!(ir_value, ir.source_map.values[&value]);
    assert_eq!(ir_input_min, ir.source_map.values[&input_min]);
    assert_eq!(ir_input_max, ir.source_map.values[&input_max]);
    assert_eq!(ir_output_min, ir.source_map.values[&output_min]);
    assert_eq!(ir_output_max, ir.source_map.values[&output_max]);
}

#[test]
fn smoothstep_lowers_with_promoted_edges_and_source_mapping() {
    let edge_min = MaterialExpressionId::from_u128(0x3301);
    let edge_max = MaterialExpressionId::from_u128(0x3302);
    let value = MaterialExpressionId::from_u128(0x3303);
    let smoothstep = MaterialExpressionId::from_u128(0x3304);
    let alpha = MaterialExpressionId::from_u128(0x3305);
    let mut program = MaterialProgram::additive_sprite("Smoothstep IR");
    program.expressions.extend([
        MaterialExpression {
            id: edge_min,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.25)),
        },
        MaterialExpression {
            id: edge_max,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.75)),
        },
        MaterialExpression {
            id: value,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: smoothstep,
            kind: MaterialExpressionKind::Smoothstep {
                edge_min,
                edge_max,
                value,
            },
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::ExtractComponent {
                value: smoothstep,
                component: MaterialVectorComponent::X,
            },
        },
    ]);
    program.outputs.alpha = alpha;

    let ir = MaterialCompiler.compile(&program).unwrap();
    let smoothstep_value = ir.source_map.values[&smoothstep];
    let MaterialIrInstruction::Smoothstep {
        edge_min: ir_edge_min,
        edge_max: ir_edge_max,
        value: ir_value,
    } = ir.value(smoothstep_value).unwrap().instruction
    else {
        panic!("Smoothstep must survive lowering as a semantic IR instruction");
    };
    assert_eq!(
        ir.value(smoothstep_value).unwrap().value_type,
        MaterialValueType::Vec2
    );
    assert_eq!(ir_edge_min, ir.source_map.values[&edge_min]);
    assert_eq!(ir_edge_max, ir.source_map.values[&edge_max]);
    assert_eq!(ir_value, ir.source_map.values[&value]);
}

#[test]
fn fresnel_lowers_with_typed_sockets_and_source_mapping() {
    let normal = MaterialExpressionId::from_u128(0x3311);
    let view = MaterialExpressionId::from_u128(0x3312);
    let power = MaterialExpressionId::from_u128(0x3313);
    let fresnel = MaterialExpressionId::from_u128(0x3314);
    let mut program = MaterialProgram::additive_sprite("Fresnel IR");
    program.expressions.extend([
        MaterialExpression {
            id: normal,
            kind: MaterialExpressionKind::Input(MaterialInput::Normal),
        },
        MaterialExpression {
            id: view,
            kind: MaterialExpressionKind::Input(MaterialInput::ViewDirection),
        },
        MaterialExpression {
            id: power,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(3.0)),
        },
        MaterialExpression {
            id: fresnel,
            kind: MaterialExpressionKind::Fresnel {
                normal,
                view,
                power,
            },
        },
    ]);
    program.outputs.alpha = fresnel;

    let ir = MaterialCompiler.compile(&program).unwrap();
    let fresnel_value = ir.source_map.values[&fresnel];
    let MaterialIrInstruction::Fresnel {
        normal: ir_normal,
        view: ir_view,
        power: ir_power,
    } = ir.value(fresnel_value).unwrap().instruction
    else {
        panic!("Fresnel must survive lowering as a semantic IR instruction");
    };
    assert_eq!(
        ir.value(fresnel_value).unwrap().value_type,
        MaterialValueType::Float
    );
    assert_eq!(ir_normal, ir.source_map.values[&normal]);
    assert_eq!(ir_view, ir.source_map.values[&view]);
    assert_eq!(ir_power, ir.source_map.values[&power]);
    let MaterialStackProjection::Stack { entries } =
        MaterialCompiler.project_stack(&program).unwrap()
    else {
        panic!("a single Fresnel chain must have a stack projection");
    };
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].expression, fresnel);
    assert_eq!(entries[0].kind, MaterialStackModifierKind::Fresnel);
}

#[test]
fn radial_mask_lowers_with_typed_sockets_and_source_mapping() {
    let uv = MaterialExpressionId::from_u128(0x3401);
    let center = MaterialExpressionId::from_u128(0x3402);
    let radius = MaterialExpressionId::from_u128(0x3403);
    let softness = MaterialExpressionId::from_u128(0x3404);
    let invert = MaterialExpressionId::from_u128(0x3405);
    let radial_mask = MaterialExpressionId::from_u128(0x3406);
    let mut program = MaterialProgram::additive_sprite("Radial mask IR");
    program.expressions.extend([
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: center,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.5; 2])),
        },
        MaterialExpression {
            id: radius,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
        },
        MaterialExpression {
            id: softness,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.1)),
        },
        MaterialExpression {
            id: invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: radial_mask,
            kind: MaterialExpressionKind::RadialMask {
                uv,
                center,
                radius,
                softness,
                invert,
            },
        },
    ]);
    program.outputs.alpha = radial_mask;

    let ir = MaterialCompiler.compile(&program).unwrap();
    let radial_value = ir.source_map.values[&radial_mask];
    let MaterialIrInstruction::RadialMask {
        uv: ir_uv,
        center: ir_center,
        radius: ir_radius,
        softness: ir_softness,
        invert: ir_invert,
    } = ir.value(radial_value).unwrap().instruction
    else {
        panic!("RadialMask must survive lowering as a semantic IR instruction");
    };
    assert_eq!(
        ir.value(radial_value).unwrap().value_type,
        MaterialValueType::Float
    );
    assert_eq!(ir_uv, ir.source_map.values[&uv]);
    assert_eq!(ir_center, ir.source_map.values[&center]);
    assert_eq!(ir_radius, ir.source_map.values[&radius]);
    assert_eq!(ir_softness, ir.source_map.values[&softness]);
    assert_eq!(ir_invert, ir.source_map.values[&invert]);
}

#[test]
fn dissolve_lowers_with_typed_sockets_and_source_mapping() {
    let source = MaterialExpressionId::from_u128(0x3501);
    let threshold = MaterialExpressionId::from_u128(0x3502);
    let edge_width = MaterialExpressionId::from_u128(0x3503);
    let invert = MaterialExpressionId::from_u128(0x3504);
    let dissolve = MaterialExpressionId::from_u128(0x3505);
    let mut program = MaterialProgram::additive_sprite("Dissolve IR");
    program.expressions.extend([
        MaterialExpression {
            id: source,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleRandom),
        },
        MaterialExpression {
            id: threshold,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleNormalizedAge),
        },
        MaterialExpression {
            id: edge_width,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.08)),
        },
        MaterialExpression {
            id: invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: dissolve,
            kind: MaterialExpressionKind::Dissolve {
                source,
                threshold,
                edge_width,
                invert,
            },
        },
    ]);
    program.outputs.alpha = dissolve;

    let ir = MaterialCompiler.compile(&program).unwrap();
    let dissolve_value = ir.source_map.values[&dissolve];
    let MaterialIrInstruction::Dissolve {
        source: ir_source,
        threshold: ir_threshold,
        edge_width: ir_edge_width,
        invert: ir_invert,
    } = ir.value(dissolve_value).unwrap().instruction
    else {
        panic!("Dissolve must survive lowering as a semantic IR instruction");
    };
    assert_eq!(
        ir.value(dissolve_value).unwrap().value_type,
        MaterialValueType::Float
    );
    assert_eq!(ir_source, ir.source_map.values[&source]);
    assert_eq!(ir_threshold, ir.source_map.values[&threshold]);
    assert_eq!(ir_edge_width, ir.source_map.values[&edge_width]);
    assert_eq!(ir_invert, ir.source_map.values[&invert]);
}

#[test]
fn dissolve_edge_lowers_with_typed_sockets_and_source_mapping() {
    let source = MaterialExpressionId::from_u128(0x3601);
    let threshold = MaterialExpressionId::from_u128(0x3602);
    let edge_width = MaterialExpressionId::from_u128(0x3603);
    let invert = MaterialExpressionId::from_u128(0x3604);
    let dissolve_edge = MaterialExpressionId::from_u128(0x3605);
    let mut program = MaterialProgram::additive_sprite("Dissolve edge IR");
    program.expressions.extend([
        MaterialExpression {
            id: source,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleRandom),
        },
        MaterialExpression {
            id: threshold,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleNormalizedAge),
        },
        MaterialExpression {
            id: edge_width,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.08)),
        },
        MaterialExpression {
            id: invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: dissolve_edge,
            kind: MaterialExpressionKind::DissolveEdge {
                source,
                threshold,
                edge_width,
                invert,
            },
        },
    ]);
    program.outputs.alpha = dissolve_edge;

    let ir = MaterialCompiler.compile(&program).unwrap();
    let dissolve_edge_value = ir.source_map.values[&dissolve_edge];
    let MaterialIrInstruction::DissolveEdge {
        source: ir_source,
        threshold: ir_threshold,
        edge_width: ir_edge_width,
        invert: ir_invert,
    } = ir.value(dissolve_edge_value).unwrap().instruction
    else {
        panic!("DissolveEdge must survive lowering as a semantic IR instruction");
    };
    assert_eq!(
        ir.value(dissolve_edge_value).unwrap().value_type,
        MaterialValueType::Float
    );
    assert_eq!(ir_source, ir.source_map.values[&source]);
    assert_eq!(ir_threshold, ir.source_map.values[&threshold]);
    assert_eq!(ir_edge_width, ir.source_map.values[&edge_width]);
    assert_eq!(ir_invert, ir.source_map.values[&invert]);
}

#[test]
fn depth_fade_lowers_with_typed_sockets_and_source_mapping() {
    let scene_depth = MaterialExpressionId::from_u128(0x3701);
    let pixel_depth = MaterialExpressionId::from_u128(0x3702);
    let fade_distance = MaterialExpressionId::from_u128(0x3703);
    let invert = MaterialExpressionId::from_u128(0x3704);
    let depth_fade = MaterialExpressionId::from_u128(0x3705);
    let mut program = MaterialProgram::additive_sprite("Depth fade IR");
    program.expressions.extend([
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
            id: invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: depth_fade,
            kind: MaterialExpressionKind::DepthFade {
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            },
        },
    ]);
    program.outputs.alpha = depth_fade;

    let ir = MaterialCompiler.compile(&program).unwrap();
    let value = ir.source_map.values[&depth_fade];
    let MaterialIrInstruction::DepthFade {
        scene_depth: ir_scene_depth,
        pixel_depth: ir_pixel_depth,
        fade_distance: ir_fade_distance,
        invert: ir_invert,
    } = ir.value(value).unwrap().instruction
    else {
        panic!("DepthFade must survive lowering as a semantic IR instruction");
    };
    assert_eq!(
        ir.value(value).unwrap().value_type,
        MaterialValueType::Float
    );
    assert_eq!(ir_scene_depth, ir.source_map.values[&scene_depth]);
    assert_eq!(ir_pixel_depth, ir.source_map.values[&pixel_depth]);
    assert_eq!(ir_fade_distance, ir.source_map.values[&fade_distance]);
    assert_eq!(ir_invert, ir.source_map.values[&invert]);
}

#[test]
fn soft_particle_lowers_with_typed_sockets_and_source_mapping() {
    let alpha = MaterialExpressionId::from_u128(0x3801);
    let scene_depth = MaterialExpressionId::from_u128(0x3802);
    let pixel_depth = MaterialExpressionId::from_u128(0x3803);
    let fade_distance = MaterialExpressionId::from_u128(0x3804);
    let invert = MaterialExpressionId::from_u128(0x3805);
    let soft_particle = MaterialExpressionId::from_u128(0x3806);
    let mut program = MaterialProgram::additive_sprite("Soft particle IR");
    program.expressions.extend([
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleOpacity),
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
            id: invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: soft_particle,
            kind: MaterialExpressionKind::SoftParticle {
                alpha,
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            },
        },
    ]);
    program.outputs.alpha = soft_particle;

    let ir = MaterialCompiler.compile(&program).unwrap();
    let value = ir.source_map.values[&soft_particle];
    let MaterialIrInstruction::SoftParticle {
        alpha: ir_alpha,
        scene_depth: ir_scene_depth,
        pixel_depth: ir_pixel_depth,
        fade_distance: ir_fade_distance,
        invert: ir_invert,
    } = ir.value(value).unwrap().instruction
    else {
        panic!("SoftParticle must survive lowering as a semantic IR instruction");
    };
    assert_eq!(
        ir.value(value).unwrap().value_type,
        MaterialValueType::Float
    );
    assert_eq!(ir_alpha, ir.source_map.values[&alpha]);
    assert_eq!(ir_scene_depth, ir.source_map.values[&scene_depth]);
    assert_eq!(ir_pixel_depth, ir.source_map.values[&pixel_depth]);
    assert_eq!(ir_fade_distance, ir.source_map.values[&fade_distance]);
    assert_eq!(ir_invert, ir.source_map.values[&invert]);
}

#[test]
fn lowering_is_deterministic_independent_of_authored_vector_order() {
    let program = two_texture_flame_program();
    let mut reordered = program.clone();
    reordered.parameters.reverse();
    reordered.expressions.reverse();

    let first = MaterialCompiler.compile(&program).unwrap();
    let second = MaterialCompiler.compile(&reordered).unwrap();

    assert_eq!(first, second);
}

#[test]
fn folding_simplification_and_dead_elimination_preserve_source_mapping() {
    let color_a = MaterialExpressionId::from_u128(0x4001);
    let color_b = MaterialExpressionId::from_u128(0x4002);
    let color = MaterialExpressionId::from_u128(0x4003);
    let opacity = MaterialExpressionId::from_u128(0x4004);
    let zero = MaterialExpressionId::from_u128(0x4005);
    let alpha = MaterialExpressionId::from_u128(0x4006);
    let unreachable = MaterialExpressionId::from_u128(0x4007);
    let mut program = MaterialProgram::additive_sprite("Optimized");
    program.expressions = vec![
        MaterialExpression {
            id: color_a,
            kind: MaterialExpressionKind::Constant(MaterialValue::ColorSrgb([0.5, 0.25, 0.0, 1.0])),
        },
        MaterialExpression {
            id: color_b,
            kind: MaterialExpressionKind::Constant(MaterialValue::ColorSrgb([0.5, 1.0, 1.0, 1.0])),
        },
        MaterialExpression {
            id: color,
            kind: MaterialExpressionKind::Multiply(color_a, color_b),
        },
        MaterialExpression {
            id: opacity,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleOpacity),
        },
        MaterialExpression {
            id: zero,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::Add(opacity, zero),
        },
        MaterialExpression {
            id: unreachable,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(42.0)),
        },
    ];
    program.outputs.color = color;
    program.outputs.alpha = alpha;

    let ir = MaterialCompiler.compile(&program).unwrap();

    assert_eq!(ir.values.len(), 2);
    assert_eq!(ir.optimizations.constant_folds, 1);
    assert_eq!(ir.optimizations.trivial_simplifications, 1);
    assert!(ir.optimizations.eliminated_values >= 3);
    assert!(ir.source_map.eliminated.contains(&color_a));
    assert!(ir.source_map.eliminated.contains(&color_b));
    assert!(ir.source_map.eliminated.contains(&zero));
    assert!(ir.source_map.eliminated.contains(&unreachable));
    assert_eq!(ir.source_map.values[&opacity], ir.source_map.values[&alpha]);
    assert!(matches!(
        ir.value(ir.outputs.color).unwrap().instruction,
        MaterialIrInstruction::Constant(MaterialIrConstant::ColorLinear(_))
    ));
}

#[test]
fn common_subexpressions_merge_commutative_pure_operations_and_preserve_sources() {
    let particle_color = MaterialExpressionId::from_u128(0x4101);
    let tint = MaterialExpressionId::from_u128(0x4102);
    let first = MaterialExpressionId::from_u128(0x4103);
    let second = MaterialExpressionId::from_u128(0x4104);
    let color = MaterialExpressionId::from_u128(0x4105);
    let alpha = MaterialExpressionId::from_u128(0x4106);
    let mut program = MaterialProgram::additive_sprite("Common subexpressions");
    program.expressions = vec![
        MaterialExpression {
            id: particle_color,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleColor),
        },
        MaterialExpression {
            id: tint,
            kind: MaterialExpressionKind::Constant(MaterialValue::ColorSrgb([0.25, 0.5, 1.0, 1.0])),
        },
        MaterialExpression {
            id: first,
            kind: MaterialExpressionKind::Multiply(particle_color, tint),
        },
        MaterialExpression {
            id: second,
            kind: MaterialExpressionKind::Multiply(tint, particle_color),
        },
        MaterialExpression {
            id: color,
            kind: MaterialExpressionKind::Add(first, second),
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleOpacity),
        },
    ];
    program.outputs.color = color;
    program.outputs.alpha = alpha;

    let ir = MaterialCompiler.compile(&program).unwrap();

    assert_eq!(ir.optimizations.common_subexpressions, 1);
    assert_eq!(ir.source_map.values[&first], ir.source_map.values[&second]);
    assert_eq!(
        ir.source_map.expressions[&ir.source_map.values[&first]],
        [first, second]
    );
    assert!(!ir.source_map.eliminated.contains(&second));
    assert_eq!(ir.values.len(), 5);
}

#[test]
fn shader_static_parameters_specialize_early_and_enable_dependent_folding() {
    let parameter = MaterialParameterId::from_u128(0x4150);
    let parameter_read = MaterialExpressionId::from_u128(0x4151);
    let scale = MaterialExpressionId::from_u128(0x4152);
    let alpha = MaterialExpressionId::from_u128(0x4153);
    let mut program = MaterialProgram::additive_sprite("Shader-static specialization");
    program.parameters = vec![MaterialParameter {
        id: parameter,
        name: "alpha scale".into(),
        value_type: MaterialValueType::Float,
        evaluation_domain: MaterialEvaluationDomain::ShaderStatic,
        default: Some(MaterialValue::Float(2.0)),
    }];
    program.expressions.extend([
        MaterialExpression {
            id: parameter_read,
            kind: MaterialExpressionKind::Parameter(parameter),
        },
        MaterialExpression {
            id: scale,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(3.0)),
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::Multiply(parameter_read, scale),
        },
    ]);
    program.outputs.alpha = alpha;

    let specialized = MaterialCompiler.compile(&program).unwrap();

    assert_eq!(specialized.optimizations.specialized_parameter_reads, 1);
    assert_eq!(specialized.optimizations.constant_folds, 1);
    assert!(specialized.source_map.eliminated.contains(&parameter_read));
    assert!(specialized.parameters.iter().any(|item| {
        item.source == parameter && item.evaluation_domain == MaterialEvaluationDomain::ShaderStatic
    }));
    assert!(!specialized.values.iter().any(
        |value| matches!(value.instruction, MaterialIrInstruction::Parameter(id) if id == parameter)
    ));
    assert!(matches!(
        specialized.value(specialized.outputs.alpha).unwrap().instruction,
        MaterialIrInstruction::Constant(MaterialIrConstant::Float(value)) if value == 6.0
    ));

    let mut runtime_bound = program;
    runtime_bound.parameters[0].evaluation_domain = MaterialEvaluationDomain::Effect;
    let runtime_bound = MaterialCompiler.compile(&runtime_bound).unwrap();

    assert_eq!(runtime_bound.optimizations.specialized_parameter_reads, 0);
    assert!(runtime_bound.values.iter().any(
        |value| matches!(value.instruction, MaterialIrInstruction::Parameter(id) if id == parameter)
    ));
    assert!(matches!(
        runtime_bound
            .value(runtime_bound.outputs.alpha)
            .unwrap()
            .instruction,
        MaterialIrInstruction::Multiply(_, _)
    ));
}

#[test]
fn shader_static_select_prunes_the_unreachable_branch_and_its_features() {
    let parameter = MaterialParameterId::from_u128(0x4160);
    let condition = MaterialExpressionId::from_u128(0x4161);
    let fallback = MaterialExpressionId::from_u128(0x4162);
    let scene_depth = MaterialExpressionId::from_u128(0x4163);
    let select = MaterialExpressionId::from_u128(0x4164);
    let mut program = MaterialProgram::additive_sprite("Static feature pruning");
    program.parameters = vec![MaterialParameter {
        id: parameter,
        name: "use scene depth".into(),
        value_type: MaterialValueType::Bool,
        evaluation_domain: MaterialEvaluationDomain::ShaderStatic,
        default: Some(MaterialValue::Bool(false)),
    }];
    program.expressions.extend([
        MaterialExpression {
            id: condition,
            kind: MaterialExpressionKind::Parameter(parameter),
        },
        MaterialExpression {
            id: fallback,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        },
        MaterialExpression {
            id: scene_depth,
            kind: MaterialExpressionKind::Input(MaterialInput::SceneDepth),
        },
        MaterialExpression {
            id: select,
            kind: MaterialExpressionKind::Select {
                condition,
                if_false: fallback,
                if_true: scene_depth,
            },
        },
    ]);
    program.outputs.alpha = select;

    let ir = MaterialCompiler.compile(&program).unwrap();

    assert_eq!(ir.optimizations.specialized_parameter_reads, 1);
    assert_eq!(ir.optimizations.pruned_static_branches, 1);
    assert_eq!(ir.optimizations.pruned_features, 1);
    assert_eq!(ir.source_map.values[&select], ir.outputs.alpha);
    assert!(ir.source_map.eliminated.contains(&select));
    assert!(ir.source_map.eliminated.contains(&scene_depth));
    assert!(!ir.values.iter().any(|value| matches!(
        value.instruction,
        MaterialIrInstruction::Input(MaterialInput::SceneDepth)
            | MaterialIrInstruction::Select { .. }
    )));
}

#[test]
fn dynamic_select_keeps_both_branches_in_the_ir() {
    let parameter = MaterialParameterId::from_u128(0x4170);
    let condition = MaterialExpressionId::from_u128(0x4171);
    let fallback = MaterialExpressionId::from_u128(0x4172);
    let effect_time = MaterialExpressionId::from_u128(0x4173);
    let select = MaterialExpressionId::from_u128(0x4174);
    let mut program = MaterialProgram::additive_sprite("Dynamic select");
    program.parameters = vec![MaterialParameter {
        id: parameter,
        name: "use effect time".into(),
        value_type: MaterialValueType::Bool,
        evaluation_domain: MaterialEvaluationDomain::Effect,
        default: Some(MaterialValue::Bool(false)),
    }];
    program.expressions.extend([
        MaterialExpression {
            id: condition,
            kind: MaterialExpressionKind::Parameter(parameter),
        },
        MaterialExpression {
            id: fallback,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        },
        MaterialExpression {
            id: effect_time,
            kind: MaterialExpressionKind::Input(MaterialInput::EffectTime),
        },
        MaterialExpression {
            id: select,
            kind: MaterialExpressionKind::Select {
                condition,
                if_false: fallback,
                if_true: effect_time,
            },
        },
    ]);
    program.outputs.alpha = select;

    let ir = MaterialCompiler.compile(&program).unwrap();

    assert_eq!(ir.optimizations.pruned_static_branches, 0);
    assert_eq!(ir.optimizations.pruned_features, 0);
    assert!(matches!(
        ir.value(ir.outputs.alpha).unwrap().instruction,
        MaterialIrInstruction::Select { .. }
    ));
    assert!(ir.values.iter().any(|value| matches!(
        value.instruction,
        MaterialIrInstruction::Input(MaterialInput::EffectTime)
    )));
}

#[test]
fn identical_implicit_derivative_texture_samples_are_commoned_safely() {
    let texture_parameter = MaterialParameterId::from_u128(0x4201);
    let texture = MaterialExpressionId::from_u128(0x4202);
    let uv = MaterialExpressionId::from_u128(0x4203);
    let first = MaterialExpressionId::from_u128(0x4204);
    let second = MaterialExpressionId::from_u128(0x4205);
    let color = MaterialExpressionId::from_u128(0x4206);
    let alpha = MaterialExpressionId::from_u128(0x4207);
    let descriptor = texture_descriptor(MaterialTextureColorSpace::SrgbColor);
    let mut program = MaterialProgram::additive_sprite("Common texture samples");
    program.parameters = vec![MaterialParameter {
        id: texture_parameter,
        name: "texture".into(),
        value_type: MaterialValueType::Texture2D(descriptor),
        evaluation_domain: MaterialEvaluationDomain::Instance,
        default: Some(MaterialValue::Texture2D(AssetId::from_u128(0x4208))),
    }];
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
            id: color,
            kind: MaterialExpressionKind::Add(first, second),
        },
        MaterialExpression {
            id: alpha,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleOpacity),
        },
    ];
    program.outputs.color = color;
    program.outputs.alpha = alpha;

    let ir = MaterialCompiler.compile(&program).unwrap();

    assert_eq!(ir.optimizations.common_subexpressions, 1);
    assert_eq!(ir.optimizations.texture_samples_authored, 2);
    assert_eq!(ir.optimizations.texture_samples_eliminated, 1);
    assert_eq!(ir.optimizations.texture_samples_live, 1);
    assert_eq!(ir.source_map.values[&first], ir.source_map.values[&second]);
    assert_eq!(
        ir.source_map.expressions[&ir.source_map.values[&first]],
        [first, second]
    );
    assert_eq!(
        ir.values
            .iter()
            .filter(|value| matches!(
                value.instruction,
                MaterialIrInstruction::SampleTexture { .. }
            ))
            .count(),
        1
    );
    assert!(matches!(
        ir.value(ir.source_map.values[&first]).unwrap().instruction,
        MaterialIrInstruction::SampleTexture {
            sampling: MaterialTextureSamplingMode::ImplicitDerivatives,
            ..
        }
    ));
}

#[test]
fn explicit_lod_texture_samples_common_only_with_identical_levels() {
    let texture_parameter = MaterialParameterId::from_u128(0x4251);
    let texture = MaterialExpressionId::from_u128(0x4252);
    let uv = MaterialExpressionId::from_u128(0x4253);
    let first_level = MaterialExpressionId::from_u128(0x4254);
    let second_level = MaterialExpressionId::from_u128(0x4255);
    let first = MaterialExpressionId::from_u128(0x4256);
    let duplicate = MaterialExpressionId::from_u128(0x4257);
    let distinct = MaterialExpressionId::from_u128(0x4258);
    let combined = MaterialExpressionId::from_u128(0x4259);
    let color = MaterialExpressionId::from_u128(0x425A);
    let mut program = MaterialProgram::additive_sprite("Explicit texture sample levels");
    program.parameters.push(MaterialParameter {
        id: texture_parameter,
        name: "texture".into(),
        value_type: MaterialValueType::Texture2D(texture_descriptor(
            MaterialTextureColorSpace::SrgbColor,
        )),
        evaluation_domain: MaterialEvaluationDomain::Instance,
        default: Some(MaterialValue::Texture2D(AssetId::from_u128(0x425B))),
    });
    program.expressions.extend([
        MaterialExpression {
            id: texture,
            kind: MaterialExpressionKind::Parameter(texture_parameter),
        },
        MaterialExpression {
            id: uv,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: first_level,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        },
        MaterialExpression {
            id: second_level,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(2.0)),
        },
        MaterialExpression {
            id: first,
            kind: MaterialExpressionKind::SampleTextureLevel {
                texture,
                uv,
                level: first_level,
            },
        },
        MaterialExpression {
            id: duplicate,
            kind: MaterialExpressionKind::SampleTextureLevel {
                texture,
                uv,
                level: first_level,
            },
        },
        MaterialExpression {
            id: distinct,
            kind: MaterialExpressionKind::SampleTextureLevel {
                texture,
                uv,
                level: second_level,
            },
        },
        MaterialExpression {
            id: combined,
            kind: MaterialExpressionKind::Add(first, duplicate),
        },
        MaterialExpression {
            id: color,
            kind: MaterialExpressionKind::Add(combined, distinct),
        },
    ]);
    program.outputs.color = color;

    let ir = MaterialCompiler.compile(&program).unwrap();

    assert_eq!(
        ir.source_map.values[&first],
        ir.source_map.values[&duplicate]
    );
    assert_ne!(
        ir.source_map.values[&first],
        ir.source_map.values[&distinct]
    );
    assert_eq!(ir.optimizations.texture_samples_authored, 3);
    assert_eq!(ir.optimizations.texture_samples_eliminated, 1);
    assert_eq!(ir.optimizations.texture_samples_live, 2);
    assert!(matches!(
        ir.value(ir.source_map.values[&first]).unwrap().instruction,
        MaterialIrInstruction::SampleTexture {
            sampling: MaterialTextureSamplingMode::ExplicitLod { .. },
            ..
        }
    ));
}

#[test]
fn texture_samples_with_distinct_uv_operands_remain_separate() {
    let texture_parameter = MaterialParameterId::from_u128(0x4211);
    let texture = MaterialExpressionId::from_u128(0x4212);
    let first_uv = MaterialExpressionId::from_u128(0x4213);
    let second_uv = MaterialExpressionId::from_u128(0x4214);
    let first = MaterialExpressionId::from_u128(0x4215);
    let second = MaterialExpressionId::from_u128(0x4216);
    let color = MaterialExpressionId::from_u128(0x4217);
    let unused = MaterialExpressionId::from_u128(0x4219);
    let descriptor = texture_descriptor(MaterialTextureColorSpace::SrgbColor);
    let mut program = MaterialProgram::additive_sprite("Distinct texture coordinates");
    program.parameters = vec![MaterialParameter {
        id: texture_parameter,
        name: "texture".into(),
        value_type: MaterialValueType::Texture2D(descriptor),
        evaluation_domain: MaterialEvaluationDomain::Instance,
        default: Some(MaterialValue::Texture2D(AssetId::from_u128(0x4218))),
    }];
    program.expressions.extend([
        MaterialExpression {
            id: texture,
            kind: MaterialExpressionKind::Parameter(texture_parameter),
        },
        MaterialExpression {
            id: first_uv,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: second_uv,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.25, 0.75])),
        },
        MaterialExpression {
            id: first,
            kind: MaterialExpressionKind::SampleTexture {
                texture,
                uv: first_uv,
            },
        },
        MaterialExpression {
            id: second,
            kind: MaterialExpressionKind::SampleTexture {
                texture,
                uv: second_uv,
            },
        },
        MaterialExpression {
            id: color,
            kind: MaterialExpressionKind::Add(first, second),
        },
        MaterialExpression {
            id: unused,
            kind: MaterialExpressionKind::SampleTexture {
                texture,
                uv: first_uv,
            },
        },
    ]);
    program.outputs.color = color;

    let ir = MaterialCompiler.compile(&program).unwrap();

    assert_ne!(ir.source_map.values[&first], ir.source_map.values[&second]);
    assert_eq!(ir.optimizations.texture_samples_authored, 3);
    assert_eq!(ir.optimizations.texture_samples_eliminated, 1);
    assert_eq!(ir.optimizations.texture_samples_live, 2);
    assert!(ir.source_map.eliminated.contains(&unused));
}

#[test]
fn authored_srgb_constants_are_lowered_to_linear_color() {
    let mut program = MaterialProgram::additive_sprite("Linearized");
    program.expressions[0].kind =
        MaterialExpressionKind::Constant(MaterialValue::ColorSrgb([0.5, 0.5, 0.5, 0.25]));

    let ir = MaterialCompiler.compile(&program).unwrap();
    let MaterialIrInstruction::Constant(MaterialIrConstant::ColorLinear(color)) =
        ir.value(ir.outputs.color).unwrap().instruction
    else {
        panic!("color output should be a linear IR constant");
    };

    assert!((color[0] - 0.214_041_14).abs() < 0.000_001);
    assert_eq!(color[3], 0.25);
}

#[test]
fn invalid_materials_never_reach_ir_lowering() {
    let mut program = MaterialProgram::additive_sprite("Invalid output");
    program.outputs.alpha = program.outputs.color;

    let error = MaterialCompiler.compile(&program).unwrap_err();

    assert!(matches!(error, MaterialCompileError::Validation(_)));
    assert!(
        error
            .report()
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::MaterialTypeMismatch)
    );
}
