use aestra_compiler::MaterialCompiler;
use aestra_core::{
    AssetId, MaterialExpressionId, MaterialParameterId, MaterialProgramId,
    material::{
        MaterialAddressMode, MaterialCullMode, MaterialDepthTest, MaterialEvaluationDomain,
        MaterialExpression, MaterialExpressionKind, MaterialFilterMode, MaterialInput,
        MaterialMipFilterMode, MaterialParameter, MaterialRenderState, MaterialSamplerDescriptor,
        MaterialTextureColorSpace, MaterialTextureDescriptor, MaterialValue, MaterialValueType,
    },
};
use aestra_gpu::material::{
    MATERIAL_BIND_GROUP, MISSING_TEXTURE_FALLBACK_RGBA, MaterialBackendCapabilities,
    MaterialCapabilityIssueCode, MaterialColorTargetFormat, MaterialGpuError,
    MaterialParameterBinding, MaterialPipelineVariant, MaterialShaderCompiler,
};
use naga::{
    back::{hlsl, spv},
    valid::{Capabilities, ValidationFlags, Validator},
};

fn assert_portable_shader_targets(wgsl: &str) {
    let module = naga::front::wgsl::parse_str(wgsl).unwrap();
    let info = Validator::new(ValidationFlags::all(), Capabilities::all())
        .validate(&module)
        .unwrap();
    let spirv = spv::write_vec(&module, &info, &spv::Options::default(), None).unwrap();
    assert_eq!(spirv.first(), Some(&0x0723_0203));

    let mut output = String::new();
    let reflection = hlsl::Writer::new(
        &mut output,
        &hlsl::Options::default(),
        &hlsl::PipelineOptions::default(),
    )
    .write(&module, &info, None)
    .unwrap();
    assert!(!output.is_empty());
    assert!(reflection.entry_point_names.iter().all(Result::is_ok));
}

fn sampler(address_u: MaterialAddressMode) -> MaterialSamplerDescriptor {
    MaterialSamplerDescriptor {
        filter: MaterialFilterMode::Linear,
        mip_filter: MaterialMipFilterMode::Linear,
        address_u,
        address_v: MaterialAddressMode::Repeat,
    }
}

fn texture_descriptor(
    color_space: MaterialTextureColorSpace,
    address_u: MaterialAddressMode,
) -> MaterialTextureDescriptor {
    MaterialTextureDescriptor {
        color_space,
        sampler: sampler(address_u),
    }
}

fn two_texture_flame_program() -> aestra_core::material::MaterialProgram {
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
    let bright = MaterialExpressionId::from_u128(0x200a);
    let particle_color = MaterialExpressionId::from_u128(0x200b);
    let color = MaterialExpressionId::from_u128(0x200c);
    let opacity = MaterialExpressionId::from_u128(0x200d);
    let alpha = MaterialExpressionId::from_u128(0x200e);
    let mut program = aestra_core::material::MaterialProgram::additive_sprite("Two Texture Flame");
    program.id = MaterialProgramId::from_u128(0x1000);
    program.parameters = vec![
        MaterialParameter {
            id: main_parameter,
            name: "main_texture".into(),
            value_type: MaterialValueType::Texture2D(texture_descriptor(
                MaterialTextureColorSpace::SrgbColor,
                MaterialAddressMode::Repeat,
            )),
            evaluation_domain: MaterialEvaluationDomain::Instance,
            default: Some(MaterialValue::Texture2D(AssetId::from_u128(0x3001))),
        },
        MaterialParameter {
            id: noise_parameter,
            name: "noise_texture".into(),
            value_type: MaterialValueType::Texture2D(texture_descriptor(
                MaterialTextureColorSpace::LinearData,
                MaterialAddressMode::ClampToEdge,
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

fn compile(
    program: &aestra_core::material::MaterialProgram,
) -> aestra_gpu::material::CompiledMaterialProgram {
    let ir = MaterialCompiler.compile(program).unwrap();
    MaterialShaderCompiler
        .compile(&ir, &MaterialBackendCapabilities::portable_minimum())
        .unwrap()
}

#[test]
fn additive_flame_generates_valid_wesl_and_deterministic_resource_reflection() {
    let compiled = compile(&two_texture_flame_program());

    assert_eq!(compiled.resource_layout.group, MATERIAL_BIND_GROUP);
    assert_eq!(compiled.resource_layout.uniforms.binding, Some(0));
    assert_eq!(compiled.resource_layout.uniforms.size, 32);
    assert_eq!(compiled.resource_layout.textures.len(), 2);
    assert_eq!(compiled.resource_layout.samplers.len(), 2);
    assert_eq!(compiled.resource_layout.textures[0].binding, 1);
    assert_eq!(compiled.resource_layout.textures[1].binding, 2);
    assert_eq!(compiled.resource_layout.samplers[0].binding, 3);
    assert_eq!(compiled.resource_layout.samplers[1].binding, 4);
    assert_eq!(MISSING_TEXTURE_FALLBACK_RGBA, [255, 0, 255, 255]);
    assert!(compiled.shader.wesl.contains("@group(2) @binding(1)"));
    assert!(compiled.shader.wesl.contains("textureSample"));
    assert!(compiled.shader.wesl.contains("@location(6) uv0"));
    assert!(
        compiled
            .shader
            .wesl
            .contains("@location(13) @interpolate(flat) visible")
    );
    assert!(compiled.shader.wesl.contains("output.a, 0.0, 1.0"));
    assert!(compiled.shader.wgsl.contains("fn fragment_material"));
    assert_portable_shader_targets(&compiled.shader.wgsl);
    let texture_line = compiled
        .shader
        .wesl
        .lines()
        .position(|line| line.contains("textureSample"))
        .unwrap() as u32
        + 1;
    let texture_value = compiled.source_map.wesl_lines[&texture_line];
    assert!(
        !compiled.source_map.ir.expressions[&texture_value].is_empty(),
        "generated shader diagnostics must resolve back to semantic expressions"
    );
    assert_eq!(
        compiled.program_fingerprint.to_string(),
        "dd1b055ee8e8f793d1ffa72a1053e26ee607fe782d2b4d52ed9109a370a2c87e"
    );
    assert_eq!(
        compiled.reflection.required_vertex_inputs,
        vec![MaterialInput::Uv0]
    );
    assert_eq!(
        compiled.reflection.required_particle_inputs,
        vec![MaterialInput::ParticleColor, MaterialInput::ParticleOpacity]
    );
}

#[test]
fn authored_order_does_not_change_layout_shader_or_fingerprint() {
    let program = two_texture_flame_program();
    let mut reordered = program.clone();
    reordered.parameters.reverse();
    reordered.expressions.reverse();

    assert_eq!(compile(&program), compile(&reordered));
}

#[test]
fn equal_sampler_descriptors_share_one_stable_binding() {
    let mut program = two_texture_flame_program();
    let main_descriptor = match program.parameters[0].value_type {
        MaterialValueType::Texture2D(descriptor) => descriptor,
        _ => unreachable!(),
    };
    program.parameters[1].value_type = MaterialValueType::Texture2D(MaterialTextureDescriptor {
        color_space: MaterialTextureColorSpace::LinearData,
        sampler: main_descriptor.sampler,
    });

    let compiled = compile(&program);

    assert_eq!(compiled.resource_layout.samplers.len(), 1);
    assert_eq!(
        compiled.resource_layout.textures[0].sampler_binding,
        compiled.resource_layout.textures[1].sampler_binding
    );
}

#[test]
fn ordinary_instance_defaults_and_texture_assets_do_not_rebuild_shader_or_pipeline() {
    let program = two_texture_flame_program();
    let mut edited = program.clone();
    edited.parameters[0].default = Some(MaterialValue::Texture2D(AssetId::from_u128(0x9999)));
    edited.parameters[2].default = Some(MaterialValue::ColorSrgb([0.1, 0.2, 0.3, 1.0]));
    edited.parameters[3].default = Some(MaterialValue::Float(9.0));

    let first = compile(&program);
    let second = compile(&edited);
    let variant = MaterialPipelineVariant {
        target_format: MaterialColorTargetFormat::Bgra8UnormSrgb,
        sample_count: 4,
        feature_bits: 3,
    };

    assert_eq!(first.program_fingerprint, second.program_fingerprint);
    assert_eq!(first.shader.wesl, second.shader.wesl);
    assert_eq!(
        first
            .pipeline_key(program.render_state_policy.default, variant)
            .unwrap(),
        second
            .pipeline_key(edited.render_state_policy.default, variant)
            .unwrap()
    );
}

#[test]
fn shader_static_values_rebuild_shader_but_render_state_only_changes_pipeline_key() {
    let mut program = two_texture_flame_program();
    program.parameters[3].evaluation_domain = MaterialEvaluationDomain::ShaderStatic;
    let mut specialized = program.clone();
    specialized.parameters[3].default = Some(MaterialValue::Float(4.0));
    let first = compile(&program);
    let second = compile(&specialized);

    assert_ne!(first.program_fingerprint, second.program_fingerprint);
    assert_ne!(first.shader.wesl, second.shader.wesl);

    let alpha_state = MaterialRenderState {
        blend: aestra_core::BlendMode::Alpha,
        depth_test: MaterialDepthTest::LessEqual,
        depth_write: false,
        cull_mode: MaterialCullMode::None,
    };
    let mut flexible = two_texture_flame_program();
    flexible.render_state_policy.allowed.push(alpha_state);
    let compiled = compile(&flexible);
    let variant = MaterialPipelineVariant {
        target_format: MaterialColorTargetFormat::Rgba16Float,
        sample_count: 1,
        feature_bits: 0,
    };
    let additive = compiled
        .pipeline_key(flexible.render_state_policy.default, variant)
        .unwrap();
    let alpha = compiled.pipeline_key(alpha_state, variant).unwrap();

    assert_ne!(additive.digest(), alpha.digest());
    assert_eq!(additive.program, alpha.program);
}

#[test]
fn backend_limits_fail_with_structured_capability_issues() {
    let ir = MaterialCompiler
        .compile(&two_texture_flame_program())
        .unwrap();
    let capabilities = MaterialBackendCapabilities {
        max_bind_groups: 2,
        max_bindings_per_bind_group: 3,
        max_sampled_textures_per_shader_stage: 1,
        max_samplers_per_shader_stage: 1,
        max_uniform_buffer_binding_size: 16,
    };

    let error = MaterialShaderCompiler
        .compile(&ir, &capabilities)
        .unwrap_err();
    let MaterialGpuError::Capabilities(report) = error else {
        panic!("expected a capability report");
    };
    for code in [
        MaterialCapabilityIssueCode::BindGroupUnavailable,
        MaterialCapabilityIssueCode::BindingLimitExceeded,
        MaterialCapabilityIssueCode::TextureLimitExceeded,
        MaterialCapabilityIssueCode::SamplerLimitExceeded,
        MaterialCapabilityIssueCode::UniformLimitExceeded,
    ] {
        assert!(report.issues.iter().any(|issue| issue.code == code));
    }
}

#[test]
fn unsupported_inputs_report_their_semantic_expression() {
    let mut program = two_texture_flame_program();
    let opacity = program.outputs.alpha;
    program
        .expressions
        .iter_mut()
        .find(|expression| expression.id == opacity)
        .unwrap()
        .kind = MaterialExpressionKind::Input(MaterialInput::ParticleAge);
    let ir = MaterialCompiler.compile(&program).unwrap();

    let error = MaterialShaderCompiler
        .compile(&ir, &MaterialBackendCapabilities::portable_minimum())
        .unwrap_err();

    assert!(matches!(
        error,
        MaterialGpuError::UnsupportedInput {
            input: MaterialInput::ParticleAge,
            expressions,
        } if expressions == vec![opacity]
    ));
}

#[test]
fn effect_time_is_reflected_as_a_scene_input() {
    let mut program = two_texture_flame_program();
    let alpha = program.outputs.alpha;
    program
        .expressions
        .iter_mut()
        .find(|expression| expression.id == alpha)
        .unwrap()
        .kind = MaterialExpressionKind::Input(MaterialInput::EffectTime);

    let compiled = compile(&program);

    assert_eq!(
        compiled.reflection.required_scene_inputs,
        vec![MaterialInput::EffectTime]
    );
    assert!(compiled.shader.wesl.contains("input.effect_time"));
}

#[test]
fn semantic_uv_transforms_generate_portable_texture_coordinates() {
    let mut program = two_texture_flame_program();
    let original_uv = MaterialExpressionId::from_u128(0x2005);
    let speed = MaterialExpressionId::from_u128(0x2010);
    let time = MaterialExpressionId::from_u128(0x2011);
    let pan = MaterialExpressionId::from_u128(0x2012);
    let center = MaterialExpressionId::from_u128(0x2013);
    let angle = MaterialExpressionId::from_u128(0x2014);
    let rotate = MaterialExpressionId::from_u128(0x2015);
    let scale_value = MaterialExpressionId::from_u128(0x2016);
    let scale = MaterialExpressionId::from_u128(0x2017);
    let input_min = MaterialExpressionId::from_u128(0x2018);
    let input_max = MaterialExpressionId::from_u128(0x2019);
    let output_min = MaterialExpressionId::from_u128(0x201A);
    let output_max = MaterialExpressionId::from_u128(0x201B);
    let remap = MaterialExpressionId::from_u128(0x201C);
    let edge_min = MaterialExpressionId::from_u128(0x201D);
    let edge_max = MaterialExpressionId::from_u128(0x201E);
    let smoothstep = MaterialExpressionId::from_u128(0x201F);
    let mask_radius = MaterialExpressionId::from_u128(0x2020);
    let mask_softness = MaterialExpressionId::from_u128(0x2021);
    let mask_invert = MaterialExpressionId::from_u128(0x2022);
    let radial_mask = MaterialExpressionId::from_u128(0x2023);
    let dissolve_threshold = MaterialExpressionId::from_u128(0x2024);
    let dissolve_edge_width = MaterialExpressionId::from_u128(0x2025);
    let dissolve_invert = MaterialExpressionId::from_u128(0x2026);
    let dissolve = MaterialExpressionId::from_u128(0x2027);
    let dissolve_edge = MaterialExpressionId::from_u128(0x2028);
    let combined_mask = MaterialExpressionId::from_u128(0x2029);
    program.expressions.extend([
        MaterialExpression {
            id: speed,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([0.2, -0.1])),
        },
        MaterialExpression {
            id: time,
            kind: MaterialExpressionKind::Input(MaterialInput::EffectTime),
        },
        MaterialExpression {
            id: pan,
            kind: MaterialExpressionKind::PanUv {
                uv: original_uv,
                speed,
                time,
            },
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
            id: input_min,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
        },
        MaterialExpression {
            id: input_max,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        },
        MaterialExpression {
            id: output_min,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(-1.0)),
        },
        MaterialExpression {
            id: output_max,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        },
        MaterialExpression {
            id: remap,
            kind: MaterialExpressionKind::Remap {
                value: scale,
                input_min,
                input_max,
                output_min,
                output_max,
            },
        },
        MaterialExpression {
            id: edge_min,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.75)),
        },
        MaterialExpression {
            id: edge_max,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.25)),
        },
        MaterialExpression {
            id: smoothstep,
            kind: MaterialExpressionKind::Smoothstep {
                edge_min,
                edge_max,
                value: remap,
            },
        },
        MaterialExpression {
            id: mask_radius,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.45)),
        },
        MaterialExpression {
            id: mask_softness,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
        },
        MaterialExpression {
            id: mask_invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: radial_mask,
            kind: MaterialExpressionKind::RadialMask {
                uv: original_uv,
                center,
                radius: mask_radius,
                softness: mask_softness,
                invert: mask_invert,
            },
        },
        MaterialExpression {
            id: dissolve_threshold,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
        },
        MaterialExpression {
            id: dissolve_edge_width,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.0)),
        },
        MaterialExpression {
            id: dissolve_invert,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: dissolve,
            kind: MaterialExpressionKind::Dissolve {
                source: radial_mask,
                threshold: dissolve_threshold,
                edge_width: dissolve_edge_width,
                invert: dissolve_invert,
            },
        },
        MaterialExpression {
            id: dissolve_edge,
            kind: MaterialExpressionKind::DissolveEdge {
                source: radial_mask,
                threshold: dissolve_threshold,
                edge_width: dissolve_edge_width,
                invert: dissolve_invert,
            },
        },
        MaterialExpression {
            id: combined_mask,
            kind: MaterialExpressionKind::Add(dissolve, dissolve_edge),
        },
    ]);
    for expression in &mut program.expressions {
        if let MaterialExpressionKind::SampleTexture { uv, .. } = &mut expression.kind {
            *uv = smoothstep;
        }
    }
    program.outputs.alpha = combined_mask;

    let compiled = compile(&program);

    assert!(compiled.shader.wesl.contains("input.effect_time"));
    assert!(
        compiled
            .reflection
            .required_scene_inputs
            .contains(&MaterialInput::EffectTime)
    );
    let pan_line = compiled
        .source_map
        .wesl_lines
        .iter()
        .find_map(|(line, value)| {
            compiled
                .source_map
                .ir
                .expressions
                .get(value)
                .is_some_and(|expressions| expressions.contains(&pan))
                .then_some(*line)
        })
        .expect("PanUV must map to a generated shader line");
    let pan_expression = compiled
        .shader
        .wesl
        .lines()
        .nth(pan_line as usize - 1)
        .unwrap();
    assert!(pan_expression.contains('+'));
    assert!(pan_expression.contains('*'));
    let rotate_line = compiled
        .source_map
        .wesl_lines
        .iter()
        .find_map(|(line, value)| {
            compiled
                .source_map
                .ir
                .expressions
                .get(value)
                .is_some_and(|expressions| expressions.contains(&rotate))
                .then_some(*line)
        })
        .expect("RotateUV must map to a generated shader line");
    let rotate_expression = compiled
        .shader
        .wesl
        .lines()
        .nth(rotate_line as usize - 1)
        .unwrap();
    assert!(rotate_expression.contains("mat2x2<f32>"));
    assert!(rotate_expression.contains("cos"));
    assert!(rotate_expression.contains("sin"));
    let scale_line = compiled
        .source_map
        .wesl_lines
        .iter()
        .find_map(|(line, value)| {
            compiled
                .source_map
                .ir
                .expressions
                .get(value)
                .is_some_and(|expressions| expressions.contains(&scale))
                .then_some(*line)
        })
        .expect("ScaleUV must map to a generated shader line");
    let scale_expression = compiled
        .shader
        .wesl
        .lines()
        .nth(scale_line as usize - 1)
        .unwrap();
    assert!(scale_expression.contains('-'));
    assert!(scale_expression.contains('*'));
    let remap_line = compiled
        .source_map
        .wesl_lines
        .iter()
        .find_map(|(line, value)| {
            compiled
                .source_map
                .ir
                .expressions
                .get(value)
                .is_some_and(|expressions| expressions.contains(&remap))
                .then_some(*line)
        })
        .expect("Remap must map to a generated shader line");
    let remap_expression = compiled
        .shader
        .wesl
        .lines()
        .nth(remap_line as usize - 1)
        .unwrap();
    assert!(remap_expression.contains("select"));
    assert!(remap_expression.contains("abs"));
    assert!(remap_expression.contains("0.000001"));
    let smoothstep_line = compiled
        .source_map
        .wesl_lines
        .iter()
        .find_map(|(line, value)| {
            compiled
                .source_map
                .ir
                .expressions
                .get(value)
                .is_some_and(|expressions| expressions.contains(&smoothstep))
                .then_some(*line)
        })
        .expect("Smoothstep must map to a generated shader line");
    let smoothstep_expression = compiled
        .shader
        .wesl
        .lines()
        .nth(smoothstep_line as usize - 1)
        .unwrap();
    assert!(smoothstep_expression.contains("clamp"));
    assert!(smoothstep_expression.contains("select"));
    assert!(smoothstep_expression.contains("abs"));
    assert!(smoothstep_expression.contains(">="));
    assert!(smoothstep_expression.contains("vec2<f32>"));
    assert!(smoothstep_expression.contains("0.000001"));
    let radial_mask_line = compiled
        .source_map
        .wesl_lines
        .iter()
        .find_map(|(line, value)| {
            compiled
                .source_map
                .ir
                .expressions
                .get(value)
                .is_some_and(|expressions| expressions.contains(&radial_mask))
                .then_some(*line)
        })
        .expect("RadialMask must map to a generated shader line");
    let radial_mask_expression = compiled
        .shader
        .wesl
        .lines()
        .nth(radial_mask_line as usize - 1)
        .unwrap();
    assert!(radial_mask_expression.contains("length"));
    assert!(radial_mask_expression.contains("max"));
    assert!(radial_mask_expression.contains("clamp"));
    assert!(radial_mask_expression.contains("select"));
    assert!(radial_mask_expression.contains("<="));
    assert!(radial_mask_expression.contains("0.000001"));
    let dissolve_line = compiled
        .source_map
        .wesl_lines
        .iter()
        .find_map(|(line, value)| {
            compiled
                .source_map
                .ir
                .expressions
                .get(value)
                .is_some_and(|expressions| expressions.contains(&dissolve))
                .then_some(*line)
        })
        .expect("Dissolve must map to a generated shader line");
    let dissolve_expression = compiled
        .shader
        .wesl
        .lines()
        .nth(dissolve_line as usize - 1)
        .unwrap();
    assert!(dissolve_expression.contains("max"));
    assert!(dissolve_expression.contains("clamp"));
    assert!(dissolve_expression.contains("select"));
    assert!(dissolve_expression.contains(">="));
    assert!(dissolve_expression.contains("0.000001"));
    let dissolve_edge_line = compiled
        .source_map
        .wesl_lines
        .iter()
        .find_map(|(line, value)| {
            compiled
                .source_map
                .ir
                .expressions
                .get(value)
                .is_some_and(|expressions| expressions.contains(&dissolve_edge))
                .then_some(*line)
        })
        .expect("DissolveEdge must map to a generated shader line");
    let dissolve_edge_expression = compiled
        .shader
        .wesl
        .lines()
        .nth(dissolve_edge_line as usize - 1)
        .unwrap();
    assert!(dissolve_edge_expression.contains("max"));
    assert!(dissolve_edge_expression.contains("clamp"));
    assert!(dissolve_edge_expression.contains("select"));
    assert!(dissolve_edge_expression.contains("&&"));
    assert!(dissolve_edge_expression.contains(">="));
    assert!(dissolve_edge_expression.contains("0.000001"));
    assert_portable_shader_targets(&compiled.shader.wgsl);
}

#[test]
fn reflection_links_each_parameter_to_its_portable_binding() {
    let compiled = compile(&two_texture_flame_program());

    assert!(matches!(
        compiled.reflection.parameters[0].binding,
        MaterialParameterBinding::Texture {
            binding: 1,
            sampler_binding: 4,
        }
    ));
    assert!(matches!(
        compiled.reflection.parameters[2].binding,
        MaterialParameterBinding::Uniform {
            binding: 0,
            offset: 0,
        }
    ));
    assert!(matches!(
        compiled.reflection.parameters[3].binding,
        MaterialParameterBinding::Uniform {
            binding: 0,
            offset: 16,
        }
    ));
}

#[test]
fn depth_fade_compiles_single_and_multisampled_scene_depth_variants() {
    let scene_depth = MaterialExpressionId::from_u128(0xd001);
    let pixel_depth = MaterialExpressionId::from_u128(0xd002);
    let fade_distance = MaterialExpressionId::from_u128(0xd003);
    let invert = MaterialExpressionId::from_u128(0xd004);
    let fade = MaterialExpressionId::from_u128(0xd005);
    let mut program = aestra_core::material::MaterialProgram::additive_sprite("Depth fade");
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
            id: fade,
            kind: MaterialExpressionKind::DepthFade {
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            },
        },
    ]);
    program.outputs.alpha = fade;

    let compiled = compile(&program);

    assert_eq!(
        compiled.reflection.required_scene_inputs,
        vec![MaterialInput::SceneDepth, MaterialInput::PixelDepth]
    );
    assert!(compiled.shader.wesl.contains("@group(3) @binding(0)"));
    assert!(compiled.shader.wesl.contains("texture_depth_2d"));
    assert!(compiled.shader.wesl.contains("aestra_linear_view_depth"));
    assert!(compiled.shader.wesl.contains("scene_depth"));
    assert!(compiled.shader.wesl.contains("pixel_depth"));
    assert!(compiled.shader.wesl.contains("0.5"));
    assert!(compiled.shader.wesl.contains("select"));
    assert!(
        compiled
            .multisampled_shader
            .wesl
            .contains("texture_depth_multisampled_2d")
    );
    assert!(
        compiled
            .multisampled_shader
            .wesl
            .contains("@builtin(sample_index)")
    );
    assert_portable_shader_targets(&compiled.shader.wgsl);
    assert_portable_shader_targets(&compiled.multisampled_shader.wgsl);
}

#[test]
fn soft_particle_multiplies_alpha_by_depth_fade_for_both_scene_depth_variants() {
    let alpha = MaterialExpressionId::from_u128(0xd201);
    let scene_depth = MaterialExpressionId::from_u128(0xd202);
    let pixel_depth = MaterialExpressionId::from_u128(0xd203);
    let fade_distance = MaterialExpressionId::from_u128(0xd204);
    let invert = MaterialExpressionId::from_u128(0xd205);
    let soft_particle = MaterialExpressionId::from_u128(0xd206);
    let mut program = aestra_core::material::MaterialProgram::additive_sprite("Soft particle");
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

    let compiled = compile(&program);

    assert_eq!(
        compiled.reflection.required_scene_inputs,
        vec![MaterialInput::SceneDepth, MaterialInput::PixelDepth]
    );
    assert!(
        compiled
            .reflection
            .required_particle_inputs
            .contains(&MaterialInput::ParticleOpacity)
    );
    let expression_line = compiled
        .source_map
        .wesl_lines
        .iter()
        .find_map(|(line, value)| {
            compiled
                .source_map
                .ir
                .expressions
                .get(value)
                .is_some_and(|expressions| expressions.contains(&soft_particle))
                .then_some(*line)
        })
        .expect("SoftParticle must map to a generated shader line");
    let expression = compiled
        .shader
        .wesl
        .lines()
        .nth(expression_line as usize - 1)
        .unwrap();
    assert!(expression.contains('*'));
    assert!(expression.contains("clamp"));
    assert!(expression.contains("select"));
    assert!(compiled.shader.wesl.contains("texture_depth_2d"));
    assert!(
        compiled
            .multisampled_shader
            .wesl
            .contains("texture_depth_multisampled_2d")
    );
    assert_portable_shader_targets(&compiled.shader.wgsl);
    assert_portable_shader_targets(&compiled.multisampled_shader.wgsl);
}

#[test]
fn depth_inputs_report_the_scene_bind_group_capability() {
    let scene_depth = MaterialExpressionId::from_u128(0xd101);
    let mut program = aestra_core::material::MaterialProgram::additive_sprite("Scene depth");
    program.expressions.push(MaterialExpression {
        id: scene_depth,
        kind: MaterialExpressionKind::Input(MaterialInput::SceneDepth),
    });
    program.outputs.alpha = scene_depth;
    let ir = MaterialCompiler.compile(&program).unwrap();
    let mut capabilities = MaterialBackendCapabilities::portable_minimum();
    capabilities.max_bind_groups = 3;

    let error = MaterialShaderCompiler
        .compile(&ir, &capabilities)
        .unwrap_err();
    let MaterialGpuError::Capabilities(report) = error else {
        panic!("depth input must be rejected when group 3 is unavailable");
    };
    assert!(report.issues.iter().any(|issue| {
        issue.code == MaterialCapabilityIssueCode::BindGroupUnavailable
            && issue.message.contains("scene depth group 3")
    }));
}
