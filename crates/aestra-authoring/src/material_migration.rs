use crate::{
    MaterialAuthoringDocument, MaterialCommand, MaterialCommandError, MaterialCommandExecutor,
    MaterialTransaction, MaterialTransactionOutcome,
};
use aestra_core::{
    AssetId, BlendMode, EffectId, MaterialDefinition, MaterialExpressionId, MaterialId,
    MaterialInput as LegacyMaterialInput, MaterialParameterId, MaterialProgramId,
    MaterialProperties, ParameterId, RendererId, RendererProperties, SpriteColorSource, UvRect,
    Value,
    material::{
        LEGACY_SPRITE_SOFTNESS_PARAMETER, MaterialDepthTest, MaterialDomain,
        MaterialEvaluationDomain, MaterialExpression, MaterialExpressionKind, MaterialInstance,
        MaterialParameter, MaterialParameterValue, MaterialProgram, MaterialProgramRef,
        MaterialRenderState, MaterialRenderStatePolicy, MaterialSamplerDescriptor,
        MaterialSchemaVersion, MaterialTextureColorSpace, MaterialTextureDescriptor, MaterialValue,
        MaterialValueType, MaterialVectorComponent,
    },
};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

const PROGRAM_SALT: u128 = 0x8a40_2df5_9045_4b7b_a822_56d3_eaf0_1001;
const INSTANCE_SALT: u128 = 0x8a40_2df5_9045_4b7b_a822_56d3_eaf0_1002;
const SOFTNESS_SALT: u128 = 0x8a40_2df5_9045_4b7b_a822_56d3_eaf0_1003;
const TINT_SALT: u128 = 0x8a40_2df5_9045_4b7b_a822_56d3_eaf0_1004;
const TEXTURE_SALT: u128 = 0x8a40_2df5_9045_4b7b_a822_56d3_eaf0_1005;

/// One legacy material/renderer presentation pair replaced by a semantic program instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyMaterialMigrationMapping {
    pub legacy_material: MaterialId,
    pub semantic_instance: MaterialId,
    pub program: MaterialProgramId,
    pub renderers: Vec<RendererId>,
}

/// A deterministic, non-destructive Material 5 transaction.
///
/// Legacy definitions remain in the effect as a compatibility/recovery source. Renderers are
/// reassigned to newly-created semantic instances and all mutations are expressed through the
/// baseline material command API.
#[derive(Debug, Clone, PartialEq)]
pub struct LegacyMaterialMigrationPlan {
    pub transaction: MaterialTransaction,
    pub mappings: Vec<LegacyMaterialMigrationMapping>,
}

impl LegacyMaterialMigrationPlan {
    pub fn is_empty(&self) -> bool {
        self.transaction.commands.is_empty()
    }
}

#[derive(Debug, Error)]
pub enum LegacyMaterialMigrationError {
    #[error("legacy material {material} references missing effect parameter {parameter}")]
    MissingEffectParameter {
        material: MaterialId,
        parameter: ParameterId,
    },
    #[error(
        "legacy material {material} parameter {parameter} has incompatible value {actual:?}; expected {expected}"
    )]
    IncompatibleEffectParameter {
        material: MaterialId,
        parameter: ParameterId,
        actual: aestra_core::ValueType,
        expected: &'static str,
    },
    #[error("flipbook renderer {renderer} references missing flipbook {flipbook}")]
    MissingFlipbook {
        renderer: RendererId,
        flipbook: AssetId,
    },
    #[error("deterministic migration ID {id} collides with an existing {kind}")]
    IdCollision { kind: &'static str, id: String },
    #[error(transparent)]
    Command(#[from] MaterialCommandError),
}

/// Plans migration of every renderer that still references a legacy sprite material.
pub fn plan_legacy_sprite_material_migration(
    document: &MaterialAuthoringDocument,
) -> Result<LegacyMaterialMigrationPlan, LegacyMaterialMigrationError> {
    let legacy = document
        .effect
        .materials
        .iter()
        .map(|material| (material.id, material))
        .collect::<BTreeMap<_, _>>();
    let mut groups = BTreeMap::<MigrationKey, MigrationGroup>::new();

    for emitter in &document.effect.emitters {
        for renderer in &emitter.renderers {
            let Some(material) = legacy.get(&renderer.material).copied() else {
                continue;
            };
            let (texture, apply_legacy_uv) = match renderer.properties {
                RendererProperties::Sprite => (sprite_texture(material), true),
                RendererProperties::Flipbook { flipbook, .. } => {
                    let texture = document
                        .effect
                        .flipbooks
                        .iter()
                        .find(|candidate| candidate.id == flipbook)
                        .map(|flipbook| flipbook.texture)
                        .ok_or(LegacyMaterialMigrationError::MissingFlipbook {
                            renderer: renderer.id,
                            flipbook,
                        })?;
                    (Some(texture), false)
                }
                _ => continue,
            };
            let key = MigrationKey {
                material: material.id,
                texture,
                apply_legacy_uv,
            };
            groups
                .entry(key)
                .or_insert_with(|| MigrationGroup {
                    material,
                    renderers: Vec::new(),
                })
                .renderers
                .push((emitter.id, renderer.id));
        }
    }

    let mut commands = Vec::new();
    let mut mappings = Vec::new();
    let program_index = document.programs.len();
    let instance_index = document.effect.material_instances.len();
    let mut reserved_programs = document
        .programs
        .iter()
        .map(|program| program.id)
        .collect::<BTreeSet<_>>();
    let mut reserved_instances = document
        .effect
        .material_instances
        .iter()
        .map(|instance| instance.id)
        .chain(document.effect.materials.iter().map(|material| material.id))
        .collect::<BTreeSet<_>>();

    for (offset, (key, group)) in groups.into_iter().enumerate() {
        let seed = migration_seed(document.effect.id, key);
        let program_id = MaterialProgramId::from_u128(derive_id(seed, PROGRAM_SALT));
        let instance_id = MaterialId::from_u128(derive_id(seed, INSTANCE_SALT));
        if !reserved_programs.insert(program_id) {
            return Err(LegacyMaterialMigrationError::IdCollision {
                kind: "material program",
                id: program_id.to_string(),
            });
        }
        if !reserved_instances.insert(instance_id) {
            return Err(LegacyMaterialMigrationError::IdCollision {
                kind: "material instance",
                id: instance_id.to_string(),
            });
        }

        let (program, instance) = migrate_group(
            &document.effect,
            key,
            group.material,
            program_id,
            instance_id,
            seed,
        )?;
        commands.push(MaterialCommand::AddMaterialProgram {
            program,
            index: program_index + offset,
        });
        commands.push(MaterialCommand::AddMaterialInstance {
            instance,
            index: instance_index + offset,
        });
        for &(emitter, renderer) in &group.renderers {
            commands.push(MaterialCommand::AssignRendererMaterial {
                emitter,
                renderer,
                material: instance_id,
            });
        }
        mappings.push(LegacyMaterialMigrationMapping {
            legacy_material: key.material,
            semantic_instance: instance_id,
            program: program_id,
            renderers: group
                .renderers
                .iter()
                .map(|(_, renderer)| *renderer)
                .collect(),
        });
    }

    Ok(LegacyMaterialMigrationPlan {
        transaction: MaterialTransaction::new("Migrate legacy sprite materials", commands),
        mappings,
    })
}

/// Executes the deterministic migration transaction atomically.
pub fn migrate_legacy_sprite_materials(
    document: &mut MaterialAuthoringDocument,
) -> Result<(LegacyMaterialMigrationPlan, MaterialTransactionOutcome), LegacyMaterialMigrationError>
{
    let plan = plan_legacy_sprite_material_migration(document)?;
    let outcome = MaterialCommandExecutor::execute(document, &plan.transaction)?;
    Ok((plan, outcome))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct MigrationKey {
    material: MaterialId,
    texture: Option<AssetId>,
    apply_legacy_uv: bool,
}

struct MigrationGroup<'a> {
    material: &'a MaterialDefinition,
    renderers: Vec<(aestra_core::EmitterId, RendererId)>,
}

fn sprite_texture(material: &MaterialDefinition) -> Option<AssetId> {
    let MaterialProperties::Sprite { texture, .. } = material.properties;
    texture
}

fn migrate_group(
    effect: &aestra_core::EffectAsset,
    key: MigrationKey,
    material: &MaterialDefinition,
    program_id: MaterialProgramId,
    instance_id: MaterialId,
    seed: u128,
) -> Result<(MaterialProgram, MaterialInstance), LegacyMaterialMigrationError> {
    let MaterialProperties::Sprite {
        softness,
        color,
        uv,
        ..
    } = &material.properties;
    let render_state = legacy_render_state(material.blend);
    let softness_id = MaterialParameterId::from_u128(derive_id(seed, SOFTNESS_SALT));
    let softness_default = legacy_scalar(effect, material.id, softness)?;
    let mut parameters = vec![MaterialParameter {
        id: softness_id,
        name: LEGACY_SPRITE_SOFTNESS_PARAMETER.into(),
        value_type: MaterialValueType::Float,
        evaluation_domain: legacy_evaluation_domain(softness),
        default: Some(MaterialValue::Float(1.0)),
    }];
    let mut values = BTreeMap::from([(
        softness_id,
        legacy_parameter_value(softness, MaterialValue::Float(softness_default)),
    )]);
    let mut expressions = Vec::new();
    let mut expression_ids = ExpressionIds::new(seed);
    // Keep softness reflected for the compatibility presentation wrapper. It is intentionally
    // unreachable from color/alpha until coverage becomes a first-class semantic primitive.
    expressions.push(MaterialExpression {
        id: expression_ids.next(),
        kind: MaterialExpressionKind::Parameter(softness_id),
    });

    let (mut color_expression, mut alpha_expression) = match color {
        SpriteColorSource::ParticleColor => {
            let color = expression_ids.next();
            let alpha = expression_ids.next();
            expressions.extend([
                MaterialExpression {
                    id: color,
                    kind: MaterialExpressionKind::Input(
                        aestra_core::material::MaterialInput::ParticleColor,
                    ),
                },
                MaterialExpression {
                    id: alpha,
                    kind: MaterialExpressionKind::Input(
                        aestra_core::material::MaterialInput::ParticleOpacity,
                    ),
                },
            ]);
            (color, alpha)
        }
        SpriteColorSource::Value(input) => {
            let tint_id = MaterialParameterId::from_u128(derive_id(seed, TINT_SALT));
            let tint_default = legacy_color(effect, material.id, input)?;
            parameters.push(MaterialParameter {
                id: tint_id,
                name: "Tint".into(),
                value_type: MaterialValueType::Color,
                evaluation_domain: legacy_evaluation_domain(input),
                default: Some(MaterialValue::ColorSrgb([1.0; 4])),
            });
            values.insert(
                tint_id,
                legacy_parameter_value(input, MaterialValue::ColorSrgb(tint_default)),
            );
            let tint = expression_ids.next();
            let alpha = expression_ids.next();
            expressions.extend([
                MaterialExpression {
                    id: tint,
                    kind: MaterialExpressionKind::Parameter(tint_id),
                },
                MaterialExpression {
                    id: alpha,
                    kind: MaterialExpressionKind::ExtractComponent {
                        value: tint,
                        component: MaterialVectorComponent::W,
                    },
                },
            ]);
            (tint, alpha)
        }
    };

    if let Some(texture) = key.texture {
        let texture_id = MaterialParameterId::from_u128(derive_id(seed, TEXTURE_SALT));
        parameters.push(MaterialParameter {
            id: texture_id,
            name: "Texture".into(),
            value_type: MaterialValueType::Texture2D(MaterialTextureDescriptor {
                color_space: MaterialTextureColorSpace::SrgbColor,
                sampler: MaterialSamplerDescriptor::default(),
            }),
            evaluation_domain: MaterialEvaluationDomain::Instance,
            default: Some(MaterialValue::Texture2D(texture)),
        });
        values.insert(
            texture_id,
            MaterialParameterValue::Constant(MaterialValue::Texture2D(texture)),
        );
        let texture_expression = expression_ids.next();
        let uv_expression = build_uv_expression(
            &mut expressions,
            &mut expression_ids,
            if key.apply_legacy_uv {
                *uv
            } else {
                UvRect::FULL
            },
        );
        let sample = expression_ids.next();
        let sampled_alpha = expression_ids.next();
        let combined_color = expression_ids.next();
        let combined_alpha = expression_ids.next();
        expressions.extend([
            MaterialExpression {
                id: texture_expression,
                kind: MaterialExpressionKind::Parameter(texture_id),
            },
            MaterialExpression {
                id: sample,
                kind: MaterialExpressionKind::SampleTexture {
                    texture: texture_expression,
                    uv: uv_expression,
                },
            },
            MaterialExpression {
                id: sampled_alpha,
                kind: MaterialExpressionKind::ExtractComponent {
                    value: sample,
                    component: MaterialVectorComponent::W,
                },
            },
            MaterialExpression {
                id: combined_color,
                kind: MaterialExpressionKind::Multiply(color_expression, sample),
            },
            MaterialExpression {
                id: combined_alpha,
                kind: MaterialExpressionKind::Multiply(alpha_expression, sampled_alpha),
            },
        ]);
        color_expression = combined_color;
        alpha_expression = combined_alpha;
    }

    let program = MaterialProgram {
        id: program_id,
        schema_version: MaterialSchemaVersion::CURRENT,
        name: format!("{} Semantic", material.name),
        domain: MaterialDomain::Sprite,
        render_state_policy: MaterialRenderStatePolicy::fixed(render_state),
        parameters,
        expressions,
        disabled_expressions: Vec::new(),
        outputs: aestra_core::material::MaterialOutputs {
            color: color_expression,
            alpha: alpha_expression,
        },
    };
    let instance = MaterialInstance {
        id: instance_id,
        program: MaterialProgramRef::Project(program_id),
        values,
        render_state,
    };
    Ok((program, instance))
}

fn build_uv_expression(
    expressions: &mut Vec<MaterialExpression>,
    ids: &mut ExpressionIds,
    uv: UvRect,
) -> MaterialExpressionId {
    let source = ids.next();
    expressions.push(MaterialExpression {
        id: source,
        kind: MaterialExpressionKind::Input(aestra_core::material::MaterialInput::Uv0),
    });
    if uv == UvRect::FULL {
        return source;
    }
    let scale = ids.next();
    let scaled = ids.next();
    let offset = ids.next();
    let transformed = ids.next();
    expressions.extend([
        MaterialExpression {
            id: scale,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2([
                uv.max[0] - uv.min[0],
                uv.max[1] - uv.min[1],
            ])),
        },
        MaterialExpression {
            id: scaled,
            kind: MaterialExpressionKind::Multiply(source, scale),
        },
        MaterialExpression {
            id: offset,
            kind: MaterialExpressionKind::Constant(MaterialValue::Vec2(uv.min)),
        },
        MaterialExpression {
            id: transformed,
            kind: MaterialExpressionKind::Add(scaled, offset),
        },
    ]);
    transformed
}

fn legacy_render_state(blend: BlendMode) -> MaterialRenderState {
    MaterialRenderState {
        blend,
        depth_test: MaterialDepthTest::LessEqual,
        depth_write: false,
        cull_mode: aestra_core::material::MaterialCullMode::None,
    }
}

fn legacy_scalar(
    effect: &aestra_core::EffectAsset,
    material: MaterialId,
    input: &LegacyMaterialInput<f32>,
) -> Result<f32, LegacyMaterialMigrationError> {
    match input {
        LegacyMaterialInput::Constant(value) => Ok(*value),
        LegacyMaterialInput::Parameter(parameter) => {
            match effect_parameter(effect, material, *parameter)? {
                Value::Scalar(value) => Ok(*value),
                value => Err(LegacyMaterialMigrationError::IncompatibleEffectParameter {
                    material,
                    parameter: *parameter,
                    actual: value.value_type(),
                    expected: "Scalar",
                }),
            }
        }
    }
}

fn legacy_color(
    effect: &aestra_core::EffectAsset,
    material: MaterialId,
    input: &LegacyMaterialInput<[f32; 4]>,
) -> Result<[f32; 4], LegacyMaterialMigrationError> {
    match input {
        LegacyMaterialInput::Constant(value) => Ok(*value),
        LegacyMaterialInput::Parameter(parameter) => {
            match effect_parameter(effect, material, *parameter)? {
                Value::Vec4(value) => Ok(*value),
                value => Err(LegacyMaterialMigrationError::IncompatibleEffectParameter {
                    material,
                    parameter: *parameter,
                    actual: value.value_type(),
                    expected: "Vec4",
                }),
            }
        }
    }
}

fn effect_parameter(
    effect: &aestra_core::EffectAsset,
    material: MaterialId,
    parameter: ParameterId,
) -> Result<&Value, LegacyMaterialMigrationError> {
    effect
        .parameters
        .iter()
        .find(|candidate| candidate.id == parameter)
        .map(|parameter| &parameter.default)
        .ok_or(LegacyMaterialMigrationError::MissingEffectParameter {
            material,
            parameter,
        })
}

fn legacy_parameter_value<T>(
    input: &LegacyMaterialInput<T>,
    constant: MaterialValue,
) -> MaterialParameterValue {
    match input {
        LegacyMaterialInput::Constant(_) => MaterialParameterValue::Constant(constant),
        LegacyMaterialInput::Parameter(parameter) => {
            MaterialParameterValue::EffectParameter(*parameter)
        }
    }
}

fn legacy_evaluation_domain<T>(input: &LegacyMaterialInput<T>) -> MaterialEvaluationDomain {
    match input {
        LegacyMaterialInput::Constant(_) => MaterialEvaluationDomain::Instance,
        LegacyMaterialInput::Parameter(_) => MaterialEvaluationDomain::Effect,
    }
}

fn migration_seed(effect: EffectId, key: MigrationKey) -> u128 {
    let mut value = effect.as_uuid().as_u128();
    value = derive_id(value, key.material.as_uuid().as_u128());
    value = derive_id(
        value,
        key.texture.map_or(0, |texture| texture.as_uuid().as_u128()),
    );
    derive_id(value, u128::from(key.apply_legacy_uv))
}

fn derive_id(mut value: u128, salt: u128) -> u128 {
    value ^= salt;
    value ^= value >> 31;
    value = value.wrapping_mul(0x9e37_79b9_7f4a_7c15_6eed_0e9d_a4d9_4a4f);
    value ^= value >> 47;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9_94d0_49bb_1331_11eb);
    let value = value ^ (value >> 53);
    if value == 0 { 1 } else { value }
}

struct ExpressionIds {
    seed: u128,
    next: u128,
}

impl ExpressionIds {
    fn new(seed: u128) -> Self {
        Self { seed, next: 1 }
    }

    fn next(&mut self) -> MaterialExpressionId {
        let id = MaterialExpressionId::from_u128(derive_id(self.seed, self.next));
        self.next += 1;
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_compiler::MaterialCompiler;
    use aestra_core::{
        AssetDefinition, EffectAsset, EffectParameter, Emitter, FlipbookDefinition,
        FlipbookPlaybackMode, FlipbookTimeSource, RendererInstance,
    };
    use aestra_gpu::material::{MaterialBackendCapabilities, MaterialShaderCompiler};

    fn textured_document() -> MaterialAuthoringDocument {
        let mut effect = EffectAsset::new("Legacy migration", 2.0);
        let texture = AssetDefinition::texture("Smoke", "textures/smoke.png");
        let mut material = MaterialDefinition::sprite("Smoke Alpha", BlendMode::Alpha, 0.2);
        material.properties = MaterialProperties::Sprite {
            softness: LegacyMaterialInput::Constant(0.2),
            color: SpriteColorSource::ParticleColor,
            texture: Some(texture.id),
            uv: UvRect {
                min: [0.25, 0.5],
                max: [0.75, 1.0],
            },
        };
        let mut emitter = Emitter::basic_sprite("Smoke", effect.duration);
        emitter.renderers[0].material = material.id;
        effect.assets.push(texture);
        effect.materials = vec![material];
        effect.emitters.push(emitter);
        MaterialAuthoringDocument::new(effect, Vec::new())
    }

    #[test]
    fn migration_is_deterministic_transactional_and_non_destructive() {
        let original = textured_document();
        let first = plan_legacy_sprite_material_migration(&original).unwrap();
        let second = plan_legacy_sprite_material_migration(&original).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.mappings.len(), 1);

        let mut document = original.clone();
        let (plan, outcome) = migrate_legacy_sprite_materials(&mut document).unwrap();
        assert_eq!(document.effect.materials, original.effect.materials);
        assert_eq!(document.programs.len(), 1);
        assert_eq!(document.effect.material_instances.len(), 1);
        assert_eq!(
            document.effect.emitters[0].renderers[0].material,
            plan.mappings[0].semantic_instance
        );
        assert!(document.validate().is_ok());
        assert!(
            document.programs[0]
                .expressions
                .iter()
                .any(|expression| matches!(
                    expression.kind,
                    MaterialExpressionKind::ExtractComponent {
                        component: MaterialVectorComponent::W,
                        ..
                    }
                ))
        );
        assert!(
            document.programs[0]
                .expressions
                .iter()
                .any(|expression| { matches!(expression.kind, MaterialExpressionKind::Add(_, _)) })
        );
        let ir = MaterialCompiler.compile(&document.programs[0]).unwrap();
        let compiled = MaterialShaderCompiler
            .compile(&ir, &MaterialBackendCapabilities::portable_minimum())
            .unwrap();
        assert_eq!(compiled.resource_layout.textures.len(), 1);
        assert!(compiled.shader.wesl.contains(".w"));

        MaterialCommandExecutor::execute(&mut document, &outcome.inverse).unwrap();
        assert_eq!(document, original);
    }

    #[test]
    fn legacy_effect_parameter_bindings_become_semantic_instance_bindings() {
        let mut effect = EffectAsset::new("Parameterized", 2.0);
        let softness = ParameterId::from_u128(0x100);
        let tint = ParameterId::from_u128(0x101);
        effect.parameters = vec![
            EffectParameter {
                id: softness,
                name: "Softness".into(),
                default: Value::Scalar(0.35),
                exposed: true,
            },
            EffectParameter {
                id: tint,
                name: "Tint".into(),
                default: Value::Vec4([0.8, 0.4, 0.2, 0.6]),
                exposed: true,
            },
        ];
        let mut material = MaterialDefinition::sprite("Parameterized", BlendMode::Additive, 1.0);
        material.properties = MaterialProperties::Sprite {
            softness: LegacyMaterialInput::Parameter(softness),
            color: SpriteColorSource::Value(LegacyMaterialInput::Parameter(tint)),
            texture: None,
            uv: UvRect::FULL,
        };
        let mut emitter = Emitter::basic_sprite("Emitter", effect.duration);
        emitter.renderers[0].material = material.id;
        effect.materials = vec![material];
        effect.emitters.push(emitter);
        let mut document = MaterialAuthoringDocument::new(effect, Vec::new());

        migrate_legacy_sprite_materials(&mut document).unwrap();
        let instance = &document.effect.material_instances[0];
        assert!(
            instance
                .values
                .values()
                .any(|value| { *value == MaterialParameterValue::EffectParameter(softness) })
        );
        assert!(
            instance
                .values
                .values()
                .any(|value| { *value == MaterialParameterValue::EffectParameter(tint) })
        );
        assert!(document.validate().is_ok());
    }

    #[test]
    fn flipbook_migration_samples_the_flipbook_texture_and_is_idempotent() {
        let mut effect = EffectAsset::new("Flipbook", 2.0);
        let texture = AssetDefinition::texture("Atlas", "textures/atlas.png");
        let flipbook = FlipbookDefinition::grid("Atlas", texture.id, 2, 2, 12.0);
        let material = MaterialDefinition::sprite("Flipbook Additive", BlendMode::Additive, 0.1);
        let mut emitter = Emitter::basic_sprite("Flipbook", effect.duration);
        emitter.renderers[0] = RendererInstance::flipbook(material.id, flipbook.id);
        if let RendererProperties::Flipbook {
            time_source,
            playback,
            random_start,
            ..
        } = &mut emitter.renderers[0].properties
        {
            *time_source = FlipbookTimeSource::EffectTime;
            *playback = FlipbookPlaybackMode::PingPong;
            *random_start = true;
        }
        effect.assets.push(texture.clone());
        effect.flipbooks.push(flipbook);
        effect.materials = vec![material];
        effect.emitters.push(emitter);
        let mut document = MaterialAuthoringDocument::new(effect, Vec::new());

        migrate_legacy_sprite_materials(&mut document).unwrap();
        assert!(
            document.effect.material_instances[0]
                .values
                .values()
                .any(|value| {
                    *value == MaterialParameterValue::Constant(MaterialValue::Texture2D(texture.id))
                })
        );
        assert!(
            document.programs[0]
                .expressions
                .iter()
                .any(|expression| matches!(
                    expression.kind,
                    MaterialExpressionKind::SampleTexture { .. }
                ))
        );
        assert!(
            plan_legacy_sprite_material_migration(&document)
                .unwrap()
                .is_empty()
        );
    }
}
