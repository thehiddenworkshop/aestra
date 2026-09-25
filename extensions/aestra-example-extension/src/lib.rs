//! The reference linked Aestra extension (extensible-stages M10, §23–25 Phase A).
//!
//! It is an ordinary crate that depends on the public SDK only — `aestra-compiler`'s
//! [`AestraExtension`] / [`ExtensionRegistry`] and `aestra-runtime`'s Execution IR — and adds, under its
//! own `org.example.aestra::` namespace:
//!
//! - a **capability** its stage provides and its module requires;
//! - a **domain** and a transient **resource type** (a 3-D force field);
//! - a **stage type**, *Field Forces*, lowered to a multi-pass [`ExecutionBlock`]: one field-building
//!   compute pass per module, a barrier, then one pass applying the field to the particles;
//! - a **module**, *Vortex* (strength / radius / axis), whose inputs use the same metadata as the
//!   built-ins, so the editor's schema-driven controls edit it with no plugin UI code;
//! - an **extension renderer**, *Debug Points*.
//!
//! No core enum or `match` changes for any of it. Call [`link`] once at startup.

use aestra_compiler::{
    AestraExtension, CapabilityExpression, CapabilitySet, DomainDescriptor, ExtensionManifest,
    ExtensionRegistry, InputControl, InputMetadata, ModuleLowerer, ModuleMetadata,
    PayloadMigration, RegistryConflict, RendererDescriptor, ResourceTypeDescriptor, StageLowerer,
    StageLoweringInput, StageTypeDescriptor,
};
use aestra_core::{
    CapabilityId, DomainTypeId, ModuleInstance, ModuleTypeId, PluginId, PropertyBag,
    PropertyControl, PropertyDescriptor, PropertySchema, PropertySource, RendererTypeId,
    ResourceTypeId, StageTypeId, Value, ValueType,
};
use aestra_runtime::{
    AESTRA_RESOURCE_PARTICLES, ComputeOp, ExecutionBlock, ExecutionOp, ExtensionModulePlan,
    ResourceAccess, ResourceDescriptor, ResourceLifetime, StagedDispatch,
};
use std::collections::BTreeMap;
use std::sync::Arc;

pub const PLUGIN_ID: &str = "org.example.aestra";
pub const CAPABILITY_FIELD_FORCES: &str = "org.example.aestra::capability/field_forces";
pub const DOMAIN_FIELD: &str = "org.example.aestra::domain/field";
pub const RESOURCE_FORCE_FIELD: &str = "org.example.aestra::resource/force_field";
pub const STAGE_FIELD_FORCES: &str = "org.example.aestra::stage/field_forces";
pub const MODULE_VORTEX: &str = "org.example.aestra::module/vortex";
pub const RENDERER_DEBUG_POINTS: &str = "org.example.aestra::renderer/debug_points";
/// The Vortex payload schema. v1 called the tangential acceleration `speed`; v2 renamed it to
/// `strength`, and [`VortexMigration`] upgrades v1 payloads.
pub const VORTEX_SCHEMA_VERSION: u32 = 2;

/// The force field's resolution per axis, and the workgroup edge its build pass uses.
const FIELD_RESOLUTION: u32 = 32;
const FIELD_WORKGROUP: u32 = 4;
const PARTICLE_WORKGROUP: u32 = 64;

/// The example extension. Stateless: everything it contributes is registered in [`register`].
///
/// [`register`]: AestraExtension::register
#[derive(Debug, Default, Clone, Copy)]
pub struct ExampleExtension;

/// Links the example extension into this process, so `EffectCompiler::default()` and
/// `ExtensionRegistry::linked()` include it. Idempotent.
pub fn link() {
    aestra_compiler::link_extension(Arc::new(ExampleExtension))
        .expect("the example extension registers only namespaced, unique ids");
}

impl AestraExtension for ExampleExtension {
    fn manifest(&self) -> ExtensionManifest {
        ExtensionManifest {
            plugin: PluginId::new(PLUGIN_ID),
            display_name: "Aestra Example Extension".into(),
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }

    fn register(&self, registry: &mut ExtensionRegistry) -> Result<(), RegistryConflict> {
        let field_forces = CapabilityId::new(CAPABILITY_FIELD_FORCES);
        registry.register_capability(field_forces.clone())?;
        registry.domains.register(DomainDescriptor {
            type_id: DomainTypeId::new(DOMAIN_FIELD),
            display_name: "Force Field".into(),
        })?;
        registry.resources.register(ResourceTypeDescriptor {
            type_id: ResourceTypeId::new(RESOURCE_FORCE_FIELD),
            display_name: "Force Field".into(),
            domain: DomainTypeId::new(DOMAIN_FIELD),
            lifetime: ResourceLifetime::Transient,
        })?;
        registry.register_stage(StageTypeDescriptor {
            type_id: StageTypeId::new(STAGE_FIELD_FORCES),
            display_name: "Field Forces".into(),
            role: None,
            provides: CapabilitySet::new([field_forces.clone()]),
        })?;
        registry.register_module(vortex_metadata(field_forces))?;
        registry.register_renderer(RendererDescriptor::extension(
            RendererTypeId::new(RENDERER_DEBUG_POINTS),
            "Debug Points",
            debug_points_schema(),
        ))?;
        registry.lowering.register_stage(
            StageTypeId::new(STAGE_FIELD_FORCES),
            Arc::new(FieldForcesLowerer),
        )?;
        registry
            .lowering
            .register_module(ModuleTypeId::new(MODULE_VORTEX), Arc::new(VortexLowerer))?;
        registry
            .migrations
            .register_module(ModuleTypeId::new(MODULE_VORTEX), Arc::new(VortexMigration))?;
        Ok(())
    }
}

fn vortex_metadata(field_forces: CapabilityId) -> ModuleMetadata {
    let number = |min: f32| InputControl::Number {
        step: 0.1,
        min: Some(min),
        max: None,
    };
    ModuleMetadata::extension(
        ModuleTypeId::new(MODULE_VORTEX),
        "Vortex",
        "Swirls particles around an axis through the emitter's force field.",
        "Forces",
        CapabilityExpression::AnyOf(CapabilitySet::new([field_forces])),
    )
    .with_inputs(vec![
        InputMetadata::new(
            "strength",
            "Strength",
            "Tangential acceleration at the vortex radius.",
            Value::Scalar(4.0),
            number(0.0),
        ),
        InputMetadata::new(
            "radius",
            "Radius",
            "Distance from the axis where the swirl peaks.",
            Value::Scalar(1.5),
            number(0.01),
        )
        .with_unit("m"),
        InputMetadata::new(
            "axis",
            "Axis",
            "Direction the particles swirl around.",
            Value::Vec3([0.0, 1.0, 0.0]),
            InputControl::Vector {
                step: 0.1,
                min: Some(-1.0),
                max: Some(1.0),
            },
        ),
    ])
    .with_tags(vec!["plugin", "force"])
    .with_cost(3)
    .with_schema_version(VORTEX_SCHEMA_VERSION)
}

/// Upgrades Vortex payloads one schema version at a time (extensible-stages M11, §35).
struct VortexMigration;

impl PayloadMigration for VortexMigration {
    fn migrate(&self, from: u32, payload: &mut BTreeMap<String, Value>) -> Result<(), String> {
        match from {
            // v1 → v2: `speed` was renamed `strength`.
            1 => {
                if let Some(speed) = payload.remove("speed") {
                    payload.insert("strength".into(), speed);
                }
                Ok(())
            }
            other => Err(format!("no Vortex migration from schema v{other}")),
        }
    }
}

fn debug_points_schema() -> PropertySchema {
    PropertySchema::new(
        1,
        vec![PropertyDescriptor {
            name: "point_size".into(),
            label: "Point Size".into(),
            description: "Screen-space size of each debug point.".into(),
            value_type: ValueType::Scalar,
            default: Value::Scalar(4.0),
            unit: Some("px".into()),
            control: PropertyControl::Number {
                step: 0.5,
                min: Some(1.0),
                max: Some(64.0),
            },
            sources: vec![PropertySource::Constant],
        }],
    )
}

/// Lowers a Vortex module: validates its resolved inputs and names the field-building kernel entry.
struct VortexLowerer;

impl ModuleLowerer for VortexLowerer {
    fn lower(
        &self,
        module: &ModuleInstance,
        payload: &PropertyBag,
    ) -> Result<ExtensionModulePlan, String> {
        let radius = payload
            .get_f32("radius")
            .ok_or("vortex radius must be a number")?;
        if radius <= 0.0 {
            return Err(format!("vortex radius must be positive, got {radius}"));
        }
        let axis = payload
            .get_vec3("axis")
            .ok_or("vortex axis must be a vector")?;
        if axis.iter().all(|component| component.abs() < f32::EPSILON) {
            return Err("vortex axis must not be zero".into());
        }
        Ok(ExtensionModulePlan {
            source: module.id,
            module_type: module.module_type.clone(),
            entry_point: "vortex".into(),
            parameters: payload.clone(),
        })
    }
}

/// Lowers a Field Forces stage: each module splats into the transient force field, a barrier makes
/// the field visible, then one pass applies it to the emitter's particles.
struct FieldForcesLowerer;

impl StageLowerer for FieldForcesLowerer {
    fn lower(&self, input: &StageLoweringInput<'_>) -> Result<ExecutionBlock, String> {
        if input.modules.is_empty() {
            return Err(format!("stage '{}' has no force modules", input.name));
        }
        let cells_per_axis = FIELD_RESOLUTION;
        let field_bytes = u64::from(cells_per_axis.pow(3)) * 16;
        let field_dispatch = StagedDispatch {
            x: cells_per_axis / FIELD_WORKGROUP,
            y: cells_per_axis / FIELD_WORKGROUP,
            z: cells_per_axis / FIELD_WORKGROUP,
        };
        let mut ops: Vec<ExecutionOp> = input
            .modules
            .iter()
            .map(|module| {
                ExecutionOp::Compute(ComputeOp {
                    name: format!("field_forces/{}", module.entry_point),
                    entry_point: module.entry_point.clone(),
                    accesses: vec![ResourceAccess::read_write(RESOURCE_FORCE_FIELD)],
                    dispatch: field_dispatch,
                })
            })
            .collect();
        ops.push(ExecutionOp::Barrier);
        ops.push(ExecutionOp::Compute(ComputeOp {
            name: "field_forces/apply".into(),
            entry_point: "apply_field".into(),
            accesses: vec![
                ResourceAccess::read(RESOURCE_FORCE_FIELD),
                ResourceAccess::read_write(AESTRA_RESOURCE_PARTICLES),
            ],
            dispatch: StagedDispatch {
                x: input.particle_capacity.div_ceil(PARTICLE_WORKGROUP).max(1),
                y: 1,
                z: 1,
            },
        }));
        Ok(ExecutionBlock {
            resources: vec![
                ResourceDescriptor {
                    id: ResourceTypeId::new(AESTRA_RESOURCE_PARTICLES),
                    bytes: 0,
                    lifetime: ResourceLifetime::Persistent,
                },
                ResourceDescriptor {
                    id: ResourceTypeId::new(RESOURCE_FORCE_FIELD),
                    bytes: field_bytes,
                    lifetime: ResourceLifetime::Transient,
                },
            ],
            ops,
        })
    }
}
