//! Host bindings HB4: module inputs read binding fields. A built-in input that opts in
//! (`aestra.spawn.initialize` `direction`) lowers to `Expression::HostField`, the CPU reference reads
//! the live value through the instance's input table, and the authored constant is the fallback.

use aestra_compiler::{EffectCompiler, ExtensionRegistry};
use aestra_core::{
    AESTRA_FIELD_LINEAR_VELOCITY, AESTRA_FIELD_POSITION, AESTRA_FIELD_ROTATION, BindingFieldId,
    BindingUpdateMode, DiagnosticCode, EffectAsset, EffectBinding, Emitter, EmitterShape,
    HostFieldRef, MODULE_INITIALIZE, MODULE_MOTION, ModuleInstance, ModuleParameters,
    PropertySource, ScalarRange,
};
use aestra_runtime::{
    EffectInstance, Expression, Instruction, ParticleSample, RuntimeValue, SpatialBindingSnapshot,
};
use std::sync::Arc;

fn velocity() -> BindingFieldId {
    BindingFieldId::new(AESTRA_FIELD_LINEAR_VELOCITY)
}

/// A point emitter with no gravity or spread whose Initialize `direction` reads `Target`'s
/// velocity when `bind` is set.
fn effect(update_mode: BindingUpdateMode, bind: bool) -> EffectAsset {
    let mut effect = EffectAsset::new("Along", 2.0);
    let mut emitter = Emitter::basic_sprite("Sparks", 2.0);
    let mut target = EffectBinding::spatial("Target", update_mode);
    target.optional_fields.insert(velocity());
    for module in &mut emitter.modules {
        match &mut module.parameters {
            ModuleParameters::Shape { shape } => *shape = EmitterShape::Point,
            ModuleParameters::Initialize {
                speed,
                direction,
                spread_degrees,
                ..
            } => {
                *speed = ScalarRange::new(10.0, 10.0);
                *direction = [0.0, 1.0, 0.0];
                *spread_degrees = 0.0;
            }
            ModuleParameters::Motion {
                gravity,
                drag,
                turbulence,
            } => {
                *gravity = [0.0; 3];
                *drag = 0.0;
                *turbulence = 0.0;
            }
            _ => {}
        }
        if bind && module.module_type.0 == MODULE_INITIALIZE {
            module
                .property_sources
                .insert("direction".into(), PropertySource::HostBinding);
            module.host_bindings.insert(
                "direction".into(),
                HostFieldRef::new(target.id, AESTRA_FIELD_LINEAR_VELOCITY),
            );
        }
    }
    effect.emitters.push(emitter);
    effect.bindings = vec![target];
    effect
}

fn compile(effect: &EffectAsset) -> Result<aestra_runtime::CompiledEffect, Vec<DiagnosticCode>> {
    EffectCompiler::with_extensions(ExtensionRegistry::builtin())
        .compile(effect)
        .map_err(|error| error.report().diagnostics.iter().map(|d| d.code).collect())
}

fn particles(instance: &EffectInstance) -> Vec<ParticleSample> {
    let mut output = Vec::new();
    instance.evaluate(&mut output);
    output
}

#[test]
fn a_bound_input_lowers_to_a_host_field_after_the_parameters() {
    let compiled = compile(&effect(BindingUpdateMode::Live, true)).unwrap();
    assert_eq!(compiled.host_fields.len(), 1);
    assert_eq!(
        compiled.host_fields[0].fallback,
        RuntimeValue::Vec3([0.0, 1.0, 0.0])
    );
    let direction = compiled.emitters[0]
        .execution
        .particle_spawn
        .iter()
        .find_map(|instruction| match instruction {
            Instruction::Initialize { direction, .. } => Some(direction.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        direction,
        Expression::HostField(aestra_runtime::HostFieldSlot(compiled.parameters.len()))
    );
}

#[test]
fn the_cpu_reference_reads_the_live_field_and_falls_back_when_absent() {
    let compiled = Arc::new(compile(&effect(BindingUpdateMode::Live, true)).unwrap());
    let mut instance = EffectInstance::new(compiled);
    instance.seek(0.5);

    // Unbound: the authored direction (+Y) applies.
    let unbound = particles(&instance);
    assert!(!unbound.is_empty());
    assert!(
        unbound
            .iter()
            .all(|p| p.position[1] > 0.0 && p.position[2].abs() < 1e-3)
    );

    // The target moves along -Z: particles now launch along its velocity.
    let mut moving = SpatialBindingSnapshot::at([3.0, 0.0, 0.0]);
    moving.linear_velocity = Some([0.0, 0.0, -1.0]);
    instance.set_spatial_binding("Target", moving).unwrap();
    assert_eq!(
        instance.parameter_values().last(),
        Some(&RuntimeValue::Vec3([0.0, 0.0, -1.0]))
    );
    let bound = particles(&instance);
    assert!(
        bound
            .iter()
            .all(|p| p.position[2] < 0.0 && p.position[1].abs() < 1e-3)
    );

    // Velocity no longer supplied (optional field absent): back to the fallback.
    instance
        .set_spatial_binding("Target", SpatialBindingSnapshot::at([3.0, 0.0, 0.0]))
        .unwrap();
    assert_eq!(
        instance.parameter_values().last(),
        Some(&RuntimeValue::Vec3([0.0, 1.0, 0.0]))
    );
    assert!(
        instance.has_forward_only_inputs(),
        "a Live read is forward-only"
    );
}

#[test]
fn only_live_reads_make_seeking_forward_only() {
    let unread = EffectInstance::new(Arc::new(
        compile(&effect(BindingUpdateMode::Live, false)).unwrap(),
    ));
    assert!(
        !unread.has_forward_only_inputs(),
        "a declared but unread binding"
    );
    let latched = EffectInstance::new(Arc::new(
        compile(&effect(BindingUpdateMode::SnapshotOnSpawn, true)).unwrap(),
    ));
    assert!(
        !latched.has_forward_only_inputs(),
        "SnapshotOnSpawn is reproducible"
    );
}

fn initialize(effect: &mut EffectAsset) -> &mut ModuleInstance {
    effect.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_INITIALIZE)
        .unwrap()
}

#[test]
fn invalid_host_bindings_are_diagnosed() {
    // The field's type must match the input's (rotation is Vec4, direction is Vec3).
    let mut asset = effect(BindingUpdateMode::Live, true);
    let target = asset.bindings[0].id;
    asset.bindings[0]
        .optional_fields
        .insert(BindingFieldId::new(AESTRA_FIELD_ROTATION));
    initialize(&mut asset).host_bindings.insert(
        "direction".into(),
        HostFieldRef::new(target, AESTRA_FIELD_ROTATION),
    );
    assert!(
        compile(&asset)
            .unwrap_err()
            .contains(&DiagnosticCode::ParameterTypeMismatch)
    );

    // The binding must declare the field it is read through.
    let mut asset = effect(BindingUpdateMode::Live, true);
    asset.bindings[0].optional_fields.clear();
    assert!(
        compile(&asset)
            .unwrap_err()
            .contains(&DiagnosticCode::InvalidReference)
    );

    // The binding must exist.
    let mut asset = effect(BindingUpdateMode::Live, true);
    asset.bindings.clear();
    assert!(
        compile(&asset)
            .unwrap_err()
            .contains(&DiagnosticCode::InvalidReference)
    );

    // An input that does not opt in cannot be host-bound (Motion's gravity).
    let mut asset = effect(BindingUpdateMode::Live, false);
    let target = asset.bindings[0].id;
    let motion = asset.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_MOTION)
        .unwrap();
    motion.host_bindings.insert(
        "gravity".into(),
        HostFieldRef::new(target, AESTRA_FIELD_POSITION),
    );
    assert!(
        compile(&asset)
            .unwrap_err()
            .contains(&DiagnosticCode::InvalidValue)
    );

    // A HostBinding source must name a field.
    let mut asset = effect(BindingUpdateMode::Live, true);
    initialize(&mut asset).host_bindings.clear();
    assert!(
        compile(&asset)
            .unwrap_err()
            .contains(&DiagnosticCode::InvalidValue)
    );
}

#[test]
fn host_bound_inputs_round_trip_through_the_authored_format() {
    let asset = effect(BindingUpdateMode::Live, true);
    let saved = asset.to_pretty_ron().unwrap();
    assert!(saved.contains("host_bindings"));
    assert!(saved.contains("HostBinding"));
    assert_eq!(EffectAsset::from_ron(&saved).unwrap(), asset);
}
