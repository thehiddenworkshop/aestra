//! Host bindings HB3: a moving target driven entirely through the portable runtime — no engine.
//! The host pushes snapshots; the instance keeps `Live` slots current, latches `SnapshotOnSpawn`
//! slots at start, reports missing bindings, and keys checkpoints by the input it observed.

use aestra_compiler::{EffectCompiler, ExtensionRegistry};
use aestra_core::{
    AESTRA_FIELD_LINEAR_VELOCITY, AESTRA_FIELD_POSITION, BindingFieldId, BindingUpdateMode,
    EffectAsset, EffectBinding, Emitter,
};
use aestra_runtime::{
    BindingError, BindingFrame, BindingSlot, BindingSnapshot, BindingState, CompiledBindingForward,
    EffectInstance, SpatialBindingSnapshot,
};
use std::sync::Arc;

const SOURCE: BindingSlot = BindingSlot(0);
const TARGET: BindingSlot = BindingSlot(1);

fn position() -> BindingFieldId {
    BindingFieldId::new(AESTRA_FIELD_POSITION)
}

fn velocity() -> BindingFieldId {
    BindingFieldId::new(AESTRA_FIELD_LINEAR_VELOCITY)
}

/// `Source` (SnapshotOnSpawn) and `Target` (Live, optional velocity), compiled to slots 0 and 1.
fn instance() -> EffectInstance {
    let mut effect = EffectAsset::new("Homing", 2.0);
    effect.emitters.push(Emitter::basic_sprite("Fireball", 2.0));
    let mut target = EffectBinding::spatial("Target", BindingUpdateMode::Live);
    target.optional_fields.insert(velocity());
    effect.bindings = vec![
        EffectBinding::spatial("Source", BindingUpdateMode::SnapshotOnSpawn),
        target,
    ];
    let compiled = EffectCompiler::with_extensions(ExtensionRegistry::builtin())
        .compile(&effect)
        .unwrap();
    EffectInstance::new(Arc::new(compiled))
}

fn frame(instance: &EffectInstance, source: [f32; 3], target: [f32; 3]) -> BindingFrame {
    let layout = |slot: BindingSlot| &instance.effect().bindings[slot.0].layout;
    BindingFrame {
        snapshots: vec![
            Some(SpatialBindingSnapshot::at(source).to_snapshot(layout(SOURCE))),
            Some(SpatialBindingSnapshot::at(target).to_snapshot(layout(TARGET))),
        ],
    }
}

/// The fake host trace from the roadmap: the target moves every tick, the caster walks too.
const TRACE: [([f32; 3], [f32; 3]); 3] = [
    ([0.0, 0.0, 0.0], [5.0, 0.0, 0.0]),
    ([1.0, 0.0, 0.0], [6.0, 0.0, 0.0]),
    ([2.0, 0.0, 0.0], [7.0, 0.0, 1.0]),
];

#[test]
fn a_moving_target_is_observed_live_while_the_source_stays_latched() {
    let mut instance = instance();
    for (source, target) in TRACE {
        let pushed = frame(&instance, source, target);
        instance.apply_binding_frame(&pushed).unwrap();
        instance.advance(1.0 / 60.0);
        assert_eq!(
            instance.binding_field(TARGET, &position()),
            Some(&target[..])
        );
        assert_eq!(
            instance.binding_field(SOURCE, &position()),
            Some(&[0.0, 0.0, 0.0][..]),
            "SnapshotOnSpawn keeps the value from instance start"
        );
    }
    // A restart is a new spawn: the source re-latches to where the caster is now.
    instance.restart();
    assert_eq!(
        instance.binding_field(SOURCE, &position()),
        Some(&[2.0, 0.0, 0.0][..])
    );
    // Bindings are not parameters.
    assert_eq!(instance.overridden_parameters().count(), 0);
}

#[test]
fn the_same_trace_yields_the_same_snapshots_on_every_instance() {
    let run = || {
        let mut instance = instance();
        let mut observed = Vec::new();
        for (source, target) in TRACE {
            let pushed = frame(&instance, source, target);
            instance.apply_binding_frame(&pushed).unwrap();
            observed.push((
                instance.binding(SOURCE).cloned(),
                instance.binding(TARGET).cloned(),
                instance.host_input_epoch(),
            ));
        }
        observed
    };
    assert_eq!(run(), run());
}

#[test]
fn optional_fields_are_absent_until_supplied() {
    let mut instance = instance();
    instance
        .set_spatial_binding("Target", SpatialBindingSnapshot::at([1.0, 2.0, 3.0]))
        .unwrap();
    assert_eq!(instance.binding_field(TARGET, &velocity()), None);
    let mut moving = SpatialBindingSnapshot::at([1.0, 2.0, 3.0]);
    moving.linear_velocity = Some([0.0, 0.0, -4.0]);
    instance.set_spatial_binding("Target", moving).unwrap();
    assert_eq!(
        instance.binding_field(TARGET, &velocity()),
        Some(&[0.0, 0.0, -4.0][..])
    );
}

#[test]
fn missing_bindings_are_reported_and_acquire_or_lose_changes_input_identity() {
    let mut instance = instance();
    let status = instance.binding_status();
    assert_eq!(status.missing_required().count(), 2);
    assert!(!status.is_satisfied());
    let start = instance.host_input_epoch();

    instance
        .apply_binding_frame(&frame(&instance, [0.0; 3], [5.0, 0.0, 0.0]))
        .unwrap();
    assert!(instance.binding_status().is_satisfied());
    let bound = instance.host_input_epoch();
    assert_ne!(bound, start, "acquiring objects changes input identity");

    // Moving a bound object is ordinary live input, not a new identity.
    instance
        .apply_binding_frame(&frame(&instance, [0.0; 3], [9.0, 0.0, 0.0]))
        .unwrap();
    assert_eq!(instance.host_input_epoch(), bound);

    // The target despawns: the slot becomes unbound and identity changes again.
    instance.set_binding(TARGET, None).unwrap();
    let lost = instance.binding_status();
    let missing: Vec<&str> = lost
        .missing_required()
        .map(|slot| slot.name.as_str())
        .collect();
    assert_eq!(missing, ["Target"]);
    assert_eq!(lost.slots[1].state, BindingState::Unbound);
    assert_ne!(instance.host_input_epoch(), bound);

    // Retargeting to a different object while still valid must be announced explicitly.
    let before = instance.host_input_epoch();
    instance.rebind(TARGET).unwrap();
    assert_ne!(instance.host_input_epoch(), before);
    // Declared bindings nothing reads do not affect the simulation, so seeking stays exact; reads
    // are covered in binding_input_contract.rs (host bindings HB4, roadmap §7.1).
    assert!(!instance.has_forward_only_inputs());
}

#[test]
fn invalid_updates_are_rejected_without_partial_effects() {
    let mut instance = instance();
    let good = frame(&instance, [0.0; 3], [5.0, 0.0, 0.0]);

    assert!(matches!(
        instance.apply_binding_frame(&BindingFrame {
            snapshots: vec![None]
        }),
        Err(BindingError::FrameSize {
            expected: 2,
            found: 1
        })
    ));

    // The second slot lacks its required position: nothing from the frame is applied.
    let target_layout = instance.effect().bindings[1].layout.clone();
    let mut partial = good.clone();
    partial.snapshots[1] = Some(BindingSnapshot::new(&target_layout));
    assert!(matches!(
        instance.apply_binding_frame(&partial),
        Err(BindingError::MissingRequiredField { slot: TARGET, .. })
    ));
    assert!(
        instance.binding(SOURCE).is_none(),
        "the valid first slot was not applied"
    );

    let mut wrong_stride = good.clone();
    wrong_stride.snapshots[0].as_mut().unwrap().values.push(0.0);
    assert!(matches!(
        instance.apply_binding_frame(&wrong_stride),
        Err(BindingError::Stride { .. })
    ));
    let mut not_finite = good;
    not_finite.snapshots[1].as_mut().unwrap().values[0] = f32::NAN;
    assert!(matches!(
        instance.apply_binding_frame(&not_finite),
        Err(BindingError::NonFinite(TARGET))
    ));
    assert!(matches!(
        instance.set_spatial_binding("Nobody", SpatialBindingSnapshot::at([0.0; 3])),
        Err(BindingError::UnknownName(_))
    ));
    assert!(matches!(
        instance.set_binding(BindingSlot(7), None),
        Err(BindingError::UnknownSlot(7))
    ));
}

#[test]
fn a_child_instance_reads_its_forwarded_slot_from_the_parent() {
    let mut parent = instance();
    parent
        .apply_binding_frame(&frame(&parent, [1.0, 1.0, 1.0], [4.0, 0.0, 0.0]))
        .unwrap();
    let mut child = instance();
    child.apply_binding_forwards(
        &parent,
        &[CompiledBindingForward {
            child: child.effect().bindings[1].source,
            child_slot: TARGET,
            parent_slot: TARGET,
        }],
    );
    assert_eq!(
        child.binding_field(TARGET, &position()),
        Some(&[4.0, 0.0, 0.0][..])
    );
}
