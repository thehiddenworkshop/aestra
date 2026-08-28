//! Typed, explicit migrations from legacy effect formats into the current semantic model.

use crate::{
    AssetDefinition, AssetError, AssetId, EffectAsset, EffectId, EffectParameter, Emitter,
    EmitterId, EmitterShape, EmitterTransform, EventLink, FlipbookDefinition, Gradient,
    MaterialDefinition, ModuleId, ModuleParameters, ModuleTypeId, ParameterId, RendererInstance,
    ScalarRange, SimulationDomain, StageKind, Value,
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq)]
pub struct EffectAssetMigration {
    pub source_version: u32,
    pub target_version: u32,
    pub asset: EffectAsset,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EffectAssetLoad {
    Current(EffectAsset),
    MigrationRequired(EffectAssetMigration),
}

impl EffectAssetLoad {
    pub fn asset(&self) -> &EffectAsset {
        match self {
            Self::Current(asset) => asset,
            Self::MigrationRequired(migration) => &migration.asset,
        }
    }
}

#[derive(Deserialize)]
struct EffectFormatHeader {
    format_version: u32,
}

pub fn detect_effect_format(source: &str) -> Result<u32, AssetError> {
    Ok(ron::from_str::<EffectFormatHeader>(source)?.format_version)
}

pub fn prepare_effect_asset(source: &str) -> Result<EffectAssetLoad, AssetError> {
    let source_version = detect_effect_format(source)?;
    if source_version == crate::CURRENT_FORMAT_VERSION {
        return Ok(EffectAssetLoad::Current(EffectAsset::from_ron(source)?));
    }

    let Some(step) = MIGRATIONS.iter().find(|step| {
        step.source_version == source_version
            && step.target_version == crate::CURRENT_FORMAT_VERSION
    }) else {
        return Err(AssetError::UnsupportedFormat {
            found: source_version,
            current: crate::CURRENT_FORMAT_VERSION,
        });
    };
    let asset = (step.migrate)(source)?;
    asset.validate()?;
    Ok(EffectAssetLoad::MigrationRequired(EffectAssetMigration {
        source_version,
        target_version: crate::CURRENT_FORMAT_VERSION,
        asset,
    }))
}

struct MigrationStep {
    source_version: u32,
    target_version: u32,
    migrate: fn(&str) -> Result<EffectAsset, AssetError>,
}

const MIGRATIONS: &[MigrationStep] = &[MigrationStep {
    source_version: 2,
    target_version: 3,
    migrate: migrate_v2_to_v3,
}];

fn migrate_v2_to_v3(source: &str) -> Result<EffectAsset, AssetError> {
    let legacy: v2::EffectAsset = ron::from_str(source)?;
    if legacy.format_version != 2 {
        return Err(AssetError::Migration {
            from: legacy.format_version,
            to: 3,
            message: "typed v2 migration received a different format version".into(),
        });
    }

    let mut direction_parameters = BTreeSet::new();
    let mut gravity_parameters = BTreeSet::new();
    let emitters = legacy
        .emitters
        .into_iter()
        .map(|emitter| {
            migrate_v2_emitter(emitter, &mut direction_parameters, &mut gravity_parameters)
        })
        .collect();
    if direction_parameters
        .intersection(&gravity_parameters)
        .next()
        .is_some()
    {
        return Err(AssetError::Migration {
            from: 2,
            to: 3,
            message:
                "one parameter drives both 2D direction and gravity and cannot be migrated safely"
                    .into(),
        });
    }
    let parameters = legacy
        .parameters
        .into_iter()
        .map(|mut parameter| {
            if direction_parameters.contains(&parameter.id) {
                let Value::Scalar(direction) = parameter.default else {
                    return Err(AssetError::Migration {
                        from: 2,
                        to: 3,
                        message: format!(
                            "direction parameter '{}' must contain a scalar angle",
                            parameter.name
                        ),
                    });
                };
                parameter.default = Value::Vec3(direction_vector(direction));
            } else if gravity_parameters.contains(&parameter.id) {
                let Value::Vec2(gravity) = parameter.default else {
                    return Err(AssetError::Migration {
                        from: 2,
                        to: 3,
                        message: format!(
                            "gravity parameter '{}' must contain a 2D vector",
                            parameter.name
                        ),
                    });
                };
                parameter.default = Value::Vec3([gravity[0], gravity[1], 0.0]);
            }
            Ok(parameter)
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(EffectAsset {
        format_version: crate::CURRENT_FORMAT_VERSION,
        id: legacy.id,
        name: legacy.name,
        duration: legacy.duration,
        looping: legacy.looping,
        assets: legacy.assets,
        flipbooks: legacy.flipbooks,
        materials: legacy.materials,
        parameters,
        emitters,
        events: legacy.events,
        dependencies: legacy.dependencies,
        metadata: legacy.metadata,
    })
}

fn migrate_v2_emitter(
    emitter: v2::Emitter,
    direction_parameters: &mut BTreeSet<ParameterId>,
    gravity_parameters: &mut BTreeSet<ParameterId>,
) -> Emitter {
    Emitter {
        id: emitter.id,
        name: emitter.name,
        enabled: emitter.enabled,
        transform: EmitterTransform::default(),
        start_time: emitter.start_time,
        duration: emitter.duration,
        max_particles: emitter.max_particles,
        simulation_domain: emitter.simulation_domain,
        modules: emitter
            .modules
            .into_iter()
            .map(|module| migrate_v2_module(module, direction_parameters, gravity_parameters))
            .collect(),
        renderers: emitter.renderers,
    }
}

fn migrate_v2_module(
    mut module: v2::ModuleInstance,
    direction_parameters: &mut BTreeSet<ParameterId>,
    gravity_parameters: &mut BTreeSet<ParameterId>,
) -> crate::ModuleInstance {
    let parameters = match module.parameters {
        v2::ModuleParameters::Emission {
            spawn_rate,
            burst_count,
        } => ModuleParameters::Emission {
            spawn_rate,
            burst_count,
        },
        v2::ModuleParameters::Shape { shape } => ModuleParameters::Shape { shape },
        v2::ModuleParameters::Initialize {
            lifetime,
            speed,
            direction_degrees,
            spread_degrees,
            angular_velocity,
        } => {
            if let Some(parameter) = module.bindings.remove("direction_degrees") {
                direction_parameters.insert(parameter);
                module.bindings.insert("direction".into(), parameter);
            }
            ModuleParameters::Initialize {
                lifetime,
                speed,
                direction: direction_vector(direction_degrees),
                spread_degrees,
                angular_velocity,
            }
        }
        v2::ModuleParameters::Motion {
            gravity,
            drag,
            turbulence,
        } => {
            if let Some(parameter) = module.bindings.get("gravity") {
                gravity_parameters.insert(*parameter);
            }
            ModuleParameters::Motion {
                gravity: [gravity[0], gravity[1], 0.0],
                drag,
                turbulence,
            }
        }
        v2::ModuleParameters::Appearance {
            size,
            opacity,
            color,
        } => ModuleParameters::Appearance {
            size,
            opacity,
            color,
        },
        v2::ModuleParameters::Custom(values) => ModuleParameters::Custom(values),
    };
    crate::ModuleInstance {
        id: module.id,
        module_type: module.module_type,
        stage: module.stage,
        enabled: module.enabled,
        parameters,
        bindings: module.bindings,
    }
}

fn direction_vector(degrees: f32) -> [f32; 3] {
    let radians = degrees.to_radians();
    [radians.cos(), radians.sin(), 0.0]
}

mod v2 {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    pub(super) struct EffectAsset {
        pub format_version: u32,
        pub id: EffectId,
        pub name: String,
        pub duration: f32,
        pub looping: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub assets: Vec<AssetDefinition>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub flipbooks: Vec<FlipbookDefinition>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub materials: Vec<MaterialDefinition>,
        #[serde(default)]
        pub parameters: Vec<EffectParameter>,
        #[serde(default)]
        pub emitters: Vec<Emitter>,
        #[serde(default)]
        pub events: Vec<EventLink>,
        #[serde(default)]
        pub dependencies: Vec<AssetId>,
        #[serde(default)]
        pub metadata: BTreeMap<String, String>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    pub(super) struct Emitter {
        pub id: EmitterId,
        pub name: String,
        pub enabled: bool,
        pub start_time: f32,
        pub duration: f32,
        pub max_particles: u32,
        pub simulation_domain: SimulationDomain,
        pub modules: Vec<ModuleInstance>,
        pub renderers: Vec<RendererInstance>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    pub(super) struct ModuleInstance {
        pub id: ModuleId,
        pub module_type: ModuleTypeId,
        pub stage: StageKind,
        pub enabled: bool,
        pub parameters: ModuleParameters,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        pub bindings: BTreeMap<String, ParameterId>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    pub(super) enum ModuleParameters {
        Emission {
            spawn_rate: f32,
            burst_count: u32,
        },
        Shape {
            shape: EmitterShape,
        },
        Initialize {
            lifetime: ScalarRange,
            speed: ScalarRange,
            direction_degrees: f32,
            spread_degrees: f32,
            angular_velocity: ScalarRange,
        },
        Motion {
            gravity: [f32; 2],
            drag: f32,
            turbulence: f32,
        },
        Appearance {
            size: crate::Curve,
            opacity: crate::Curve,
            color: Gradient,
        },
        Custom(BTreeMap<String, Value>),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DEFAULT_SPRITE_MATERIAL_ID, MODULE_APPEARANCE, MODULE_EMISSION, MODULE_INITIALIZE,
        MODULE_MOTION, MODULE_SHAPE, MaterialDefinition, ModuleTypeId, RendererInstance,
    };

    fn legacy_v2_asset() -> v2::EffectAsset {
        let direction_parameter = EffectParameter {
            id: ParameterId::new(),
            name: "Launch angle".into(),
            default: Value::Scalar(90.0),
            exposed: true,
        };
        let gravity_parameter = EffectParameter {
            id: ParameterId::new(),
            name: "Gravity".into(),
            default: Value::Vec2([2.0, -18.0]),
            exposed: true,
        };
        let mut initialize_bindings = BTreeMap::new();
        initialize_bindings.insert("direction_degrees".into(), direction_parameter.id);
        let mut motion_bindings = BTreeMap::new();
        motion_bindings.insert("gravity".into(), gravity_parameter.id);
        v2::EffectAsset {
            format_version: 2,
            id: EffectId::new(),
            name: "Legacy".into(),
            duration: 2.0,
            looping: true,
            assets: Vec::new(),
            flipbooks: Vec::new(),
            materials: vec![MaterialDefinition::default_sprite()],
            parameters: vec![direction_parameter, gravity_parameter],
            emitters: vec![v2::Emitter {
                id: EmitterId::new(),
                name: "Emitter".into(),
                enabled: true,
                start_time: 0.0,
                duration: 2.0,
                max_particles: 64,
                simulation_domain: SimulationDomain::Particle,
                modules: vec![
                    v2_module(
                        MODULE_EMISSION,
                        StageKind::EmitterUpdate,
                        v2::ModuleParameters::Emission {
                            spawn_rate: 12.0,
                            burst_count: 4,
                        },
                    ),
                    v2_module(
                        MODULE_SHAPE,
                        StageKind::ParticleSpawn,
                        v2::ModuleParameters::Shape {
                            shape: EmitterShape::Circle { radius: 2.0 },
                        },
                    ),
                    v2::ModuleInstance {
                        bindings: initialize_bindings,
                        ..v2_module(
                            MODULE_INITIALIZE,
                            StageKind::ParticleSpawn,
                            v2::ModuleParameters::Initialize {
                                lifetime: ScalarRange::new(0.5, 1.0),
                                speed: ScalarRange::new(3.0, 5.0),
                                direction_degrees: 90.0,
                                spread_degrees: 30.0,
                                angular_velocity: ScalarRange::new(-1.0, 1.0),
                            },
                        )
                    },
                    v2::ModuleInstance {
                        bindings: motion_bindings,
                        ..v2_module(
                            MODULE_MOTION,
                            StageKind::ParticleUpdate,
                            v2::ModuleParameters::Motion {
                                gravity: [2.0, -18.0],
                                drag: 0.5,
                                turbulence: 1.0,
                            },
                        )
                    },
                    v2_module(
                        MODULE_APPEARANCE,
                        StageKind::ParticleUpdate,
                        v2::ModuleParameters::Appearance {
                            size: crate::Curve::new(vec![crate::CurveKey::new(0.0, 1.0)]),
                            opacity: crate::Curve::new(vec![crate::CurveKey::new(0.0, 1.0)]),
                            color: Gradient::new(vec![crate::ColorKey::new(0.0, [1.0; 4])]),
                        },
                    ),
                ],
                renderers: vec![RendererInstance::sprite(DEFAULT_SPRITE_MATERIAL_ID)],
            }],
            events: Vec::new(),
            dependencies: Vec::new(),
            metadata: BTreeMap::new(),
        }
    }

    fn v2_module(
        module_type: &str,
        stage: StageKind,
        parameters: v2::ModuleParameters,
    ) -> v2::ModuleInstance {
        v2::ModuleInstance {
            id: ModuleId::new(),
            module_type: ModuleTypeId::new(module_type),
            stage,
            enabled: true,
            parameters,
            bindings: BTreeMap::new(),
        }
    }

    #[test]
    fn detects_current_and_legacy_versions_without_full_deserialization() {
        assert_eq!(detect_effect_format("(format_version: 2)").unwrap(), 2);
        assert_eq!(
            detect_effect_format("(format_version: 3, extra: 1)").unwrap(),
            3
        );
    }

    #[test]
    fn typed_v2_migration_preserves_ids_and_converts_3d_inputs() {
        let legacy = legacy_v2_asset();
        let effect_id = legacy.id;
        let emitter_id = legacy.emitters[0].id;
        let source = ron::ser::to_string(&legacy).unwrap();

        let EffectAssetLoad::MigrationRequired(migration) = prepare_effect_asset(&source).unwrap()
        else {
            panic!("v2 must require migration");
        };

        assert_eq!(migration.source_version, 2);
        assert_eq!(migration.target_version, 3);
        assert_eq!(migration.asset.id, effect_id);
        assert_eq!(migration.asset.emitters[0].id, emitter_id);
        assert_eq!(
            migration.asset.emitters[0].transform,
            EmitterTransform::default()
        );
        let direction = migration.asset.emitters[0].direction();
        assert!(direction[0].abs() < 1.0e-6);
        assert!((direction[1] - 1.0).abs() < 1.0e-6);
        assert_eq!(direction[2], 0.0);
        assert_eq!(migration.asset.emitters[0].gravity(), [2.0, -18.0, 0.0]);
        let Value::Vec3(parameter_direction) = migration.asset.parameters[0].default else {
            panic!("direction parameter must become Vec3");
        };
        assert!(parameter_direction[0].abs() < 1.0e-6);
        assert!((parameter_direction[1] - 1.0).abs() < 1.0e-6);
        assert_eq!(parameter_direction[2], 0.0);
        assert_eq!(
            migration.asset.parameters[1].default,
            Value::Vec3([2.0, -18.0, 0.0])
        );
        assert_eq!(
            migration.asset.emitters[0].modules[2]
                .bindings
                .get("direction"),
            Some(&migration.asset.parameters[0].id)
        );
        assert!(
            !migration.asset.emitters[0].modules[2]
                .bindings
                .contains_key("direction_degrees")
        );
    }

    #[test]
    fn future_versions_are_rejected_without_mutation() {
        let error = prepare_effect_asset("(format_version: 99)").unwrap_err();
        assert!(matches!(
            error,
            AssetError::UnsupportedFormat {
                found: 99,
                current: crate::CURRENT_FORMAT_VERSION
            }
        ));
    }
}
