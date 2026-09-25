//! Compiled host bindings (host bindings HB2).
//!
//! The compiler turns each authored [`aestra_core::EffectBinding`] into a [`CompiledBinding`] at a
//! dense [`BindingSlot`], exactly as effect parameters get a [`crate::ParameterSlot`]. Each binding
//! carries a packed [`BindingLayout`]: the fields the effect declared, in the kind's order, as `f32`
//! components at fixed offsets. Host snapshots (HB3), the GPU record (HB6) and the C ABI (HB13) are
//! all generated from this one layout, so a plugin binding kind needs no special casing downstream.

use aestra_core::{BindingFieldId, BindingId, BindingKindId, BindingUpdateMode, ValueType};

use std::collections::BTreeSet;

/// The most fields one binding may carry: a snapshot marks supplied optional fields in a `u32` mask.
pub const MAX_BINDING_FIELDS: usize = 32;

/// A dense index into an effect's compiled bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BindingSlot(pub usize);

/// The number of packed `f32` components a binding field of this type occupies, or `None` when the
/// type cannot be a binding field (curves, gradients, text, references…). Integers and booleans
/// travel as one component each.
pub fn binding_field_components(value_type: ValueType) -> Option<u32> {
    match value_type {
        ValueType::Bool | ValueType::U32 | ValueType::Scalar => Some(1),
        ValueType::Vec2 => Some(2),
        ValueType::Vec3 => Some(3),
        ValueType::Vec4 => Some(4),
        _ => None,
    }
}

/// One packed field of a [`BindingLayout`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledBindingField {
    pub field: BindingFieldId,
    pub value_type: ValueType,
    /// Offset of the first component, in `f32`s from the start of the binding's record.
    pub offset: u32,
}

/// The packed record layout of one binding: its declared fields, contiguous, in the kind's order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BindingLayout {
    pub fields: Vec<CompiledBindingField>,
    /// Total `f32` components per record.
    pub stride: u32,
}

impl BindingLayout {
    /// Packs `fields` contiguously in the given order. Errors on an unsupported value type or too
    /// many fields.
    pub fn pack(
        fields: impl IntoIterator<Item = (BindingFieldId, ValueType)>,
    ) -> Result<Self, String> {
        let mut layout = Self::default();
        for (field, value_type) in fields {
            let components = binding_field_components(value_type).ok_or_else(|| {
                format!(
                    "field '{}' has type {value_type:?}, which cannot be a binding field",
                    field.as_str()
                )
            })?;
            layout.fields.push(CompiledBindingField {
                field,
                value_type,
                offset: layout.stride,
            });
            layout.stride += components;
        }
        if layout.fields.len() > MAX_BINDING_FIELDS {
            return Err(format!(
                "a binding may carry at most {MAX_BINDING_FIELDS} fields, found {}",
                layout.fields.len()
            ));
        }
        Ok(layout)
    }

    /// The position (for the snapshot's presence mask) and packing of a field.
    pub fn field(&self, id: &BindingFieldId) -> Option<(usize, &CompiledBindingField)> {
        self.fields
            .iter()
            .enumerate()
            .find(|(_, field)| &field.field == id)
    }

    /// Checks the layout is well formed: supported types, unique fields, contiguous offsets, a
    /// matching stride, and the field limit. Used when reloading persisted artifacts.
    pub fn validate(&self) -> Result<(), String> {
        if self.fields.len() > MAX_BINDING_FIELDS {
            return Err(format!(
                "a binding may carry at most {MAX_BINDING_FIELDS} fields"
            ));
        }
        let mut expected = 0;
        let mut seen = BTreeSet::new();
        for field in &self.fields {
            if !seen.insert(&field.field) {
                return Err(format!("field '{}' appears twice", field.field.as_str()));
            }
            if field.offset != expected {
                return Err(format!(
                    "field '{}' is at offset {} but should be at {expected}",
                    field.field.as_str(),
                    field.offset
                ));
            }
            expected += binding_field_components(field.value_type).ok_or_else(|| {
                format!(
                    "field '{}' has unsupported type {:?}",
                    field.field.as_str(),
                    field.value_type
                )
            })?;
        }
        if self.stride != expected {
            return Err(format!(
                "stride is {} but the fields occupy {expected}",
                self.stride
            ));
        }
        Ok(())
    }
}

/// One compiled host binding: the authored declaration plus its packed layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledBinding {
    pub source: BindingId,
    pub name: String,
    pub kind: BindingKindId,
    pub update_mode: BindingUpdateMode,
    pub required: bool,
    pub required_fields: BTreeSet<BindingFieldId>,
    pub optional_fields: BTreeSet<BindingFieldId>,
    pub layout: BindingLayout,
}

impl CompiledBinding {
    /// Checks the layout carries exactly the declared fields. Used when reloading artifacts.
    pub fn validate(&self) -> Result<(), String> {
        self.layout.validate()?;
        let declared: BTreeSet<&BindingFieldId> = self
            .required_fields
            .iter()
            .chain(&self.optional_fields)
            .collect();
        let laid_out: BTreeSet<&BindingFieldId> = self
            .layout
            .fields
            .iter()
            .map(|field| &field.field)
            .collect();
        if declared != laid_out {
            return Err("the layout does not match the declared fields".into());
        }
        if !self.required_fields.is_disjoint(&self.optional_fields) {
            return Err("a field is both required and optional".into());
        }
        Ok(())
    }
}

/// A child effect clip's binding read through its parent (host bindings roadmap §9.6): the child's
/// slot is filled from the parent instance's slot, so the host binds only the root effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledBindingForward {
    pub child: BindingId,
    pub child_slot: BindingSlot,
    pub parent_slot: BindingSlot,
}

/// What a host must supply to run an effect (host bindings roadmap §2.9), kept separate from
/// renderer/GPU capability reporting. Derived from the compiled bindings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostRequirements {
    pub bindings: Vec<HostBindingRequirement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostBindingRequirement {
    pub slot: BindingSlot,
    pub name: String,
    pub kind: BindingKindId,
    pub update_mode: BindingUpdateMode,
    pub required: bool,
    pub required_fields: BTreeSet<BindingFieldId>,
    pub optional_fields: BTreeSet<BindingFieldId>,
}

impl HostRequirements {
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
}

impl crate::CompiledEffect {
    /// The host objects this effect expects, by slot.
    pub fn host_requirements(&self) -> HostRequirements {
        HostRequirements {
            bindings: self
                .bindings
                .iter()
                .enumerate()
                .map(|(index, binding)| HostBindingRequirement {
                    slot: BindingSlot(index),
                    name: binding.name.clone(),
                    kind: binding.kind.clone(),
                    update_mode: binding.update_mode,
                    required: binding.required,
                    required_fields: binding.required_fields.clone(),
                    optional_fields: binding.optional_fields.clone(),
                })
                .collect(),
        }
    }

    /// Looks a binding up by its public name.
    pub fn binding_named(&self, name: &str) -> Option<(BindingSlot, &CompiledBinding)> {
        self.bindings
            .iter()
            .enumerate()
            .find(|(_, binding)| binding.name == name)
            .map(|(index, binding)| (BindingSlot(index), binding))
    }
}

/// A host-field read's index in an instance's packed input table: the parameters come first, then
/// one entry per [`CompiledHostField`] (host bindings HB4). [`crate::Expression::HostField`] carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HostFieldSlot(pub usize);

/// Where a host field lives in its binding's record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledHostFieldRef {
    pub binding: BindingSlot,
    pub field: BindingFieldId,
    pub value_type: ValueType,
    pub offset: u32,
}

/// One module input read from a host binding field (host bindings HB4): its source and the authored
/// value used whenever the field is absent (binding unbound, lost, or optional field not supplied).
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledHostField {
    pub source: CompiledHostFieldRef,
    pub fallback: crate::RuntimeValue,
}

/// Converts packed components to a runtime value of the field's type.
pub fn host_field_value(value_type: ValueType, components: &[f32]) -> Option<crate::RuntimeValue> {
    use crate::RuntimeValue;
    Some(match (value_type, components) {
        (ValueType::Scalar, [x]) => RuntimeValue::Scalar(*x),
        (ValueType::Bool, [x]) => RuntimeValue::Bool(*x != 0.0),
        (ValueType::U32, [x]) => RuntimeValue::U32(x.max(0.0) as u32),
        (ValueType::Vec2, [x, y]) => RuntimeValue::Vec2([*x, *y]),
        (ValueType::Vec3, [x, y, z]) => RuntimeValue::Vec3([*x, *y, *z]),
        (ValueType::Vec4, [x, y, z, w]) => RuntimeValue::Vec4([*x, *y, *z, *w]),
        _ => return None,
    })
}

/// One binding's values for one tick, packed per its compiled [`BindingLayout`] (host bindings HB3).
///
/// `present` marks which layout fields the host supplied (bit `i` = `layout.fields[i]`); required
/// fields must always be present. An absent field keeps zeros and consumers use their authored
/// fallback.
#[derive(Debug, Clone, PartialEq)]
pub struct BindingSnapshot {
    pub present: u32,
    pub values: Vec<f32>,
}

impl BindingSnapshot {
    /// An empty record for `layout`: zeros, nothing present.
    pub fn new(layout: &BindingLayout) -> Self {
        Self {
            present: 0,
            values: vec![0.0; layout.stride as usize],
        }
    }

    /// Supplies one field.
    pub fn set(
        &mut self,
        layout: &BindingLayout,
        field: &BindingFieldId,
        values: &[f32],
    ) -> Result<(), BindingError> {
        let (index, packed) = layout
            .field(field)
            .ok_or_else(|| BindingError::UnknownField(field.clone()))?;
        let width = binding_field_components(packed.value_type).unwrap_or(0) as usize;
        if values.len() != width {
            return Err(BindingError::FieldWidth {
                field: field.clone(),
                expected: width,
                found: values.len(),
            });
        }
        let offset = packed.offset as usize;
        self.values[offset..offset + width].copy_from_slice(values);
        self.present |= 1 << index;
        Ok(())
    }

    /// The field's components, or `None` when the layout lacks it or the host did not supply it.
    pub fn field<'a>(
        &'a self,
        layout: &BindingLayout,
        field: &BindingFieldId,
    ) -> Option<&'a [f32]> {
        let (index, packed) = layout.field(field)?;
        if self.present & (1 << index) == 0 {
            return None;
        }
        let width = binding_field_components(packed.value_type)? as usize;
        let offset = packed.offset as usize;
        self.values.get(offset..offset + width)
    }

    fn validate(&self, slot: BindingSlot, binding: &CompiledBinding) -> Result<(), BindingError> {
        if self.values.len() != binding.layout.stride as usize {
            return Err(BindingError::Stride {
                slot,
                expected: binding.layout.stride as usize,
                found: self.values.len(),
            });
        }
        let known = if binding.layout.fields.len() >= 32 {
            u32::MAX
        } else {
            (1u32 << binding.layout.fields.len()) - 1
        };
        if self.present & !known != 0 {
            return Err(BindingError::UnknownPresence(slot));
        }
        for field in &binding.required_fields {
            if self.field(&binding.layout, field).is_none() {
                return Err(BindingError::MissingRequiredField {
                    slot,
                    field: field.clone(),
                });
            }
        }
        if self.values.iter().any(|value| !value.is_finite()) {
            return Err(BindingError::NonFinite(slot));
        }
        Ok(())
    }
}

/// The typed, host-facing snapshot of a spatial binding (`aestra.binding.spatial`). Adapters fill it
/// from their native world transform; [`Self::to_snapshot`] packs it for any spatial layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpatialBindingSnapshot {
    pub position: [f32; 3],
    /// Unit quaternion, `xyzw`.
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    /// `None` when the host cannot supply velocity; the field is then marked absent.
    pub linear_velocity: Option<[f32; 3]>,
}

impl SpatialBindingSnapshot {
    /// An object at `position` with identity rotation, unit scale and unknown velocity.
    pub fn at(position: [f32; 3]) -> Self {
        Self {
            position,
            rotation: [0.0, 0.0, 0.0, 1.0],
            scale: [1.0; 3],
            linear_velocity: None,
        }
    }

    /// Packs the fields `layout` contains.
    pub fn to_snapshot(&self, layout: &BindingLayout) -> BindingSnapshot {
        let mut snapshot = BindingSnapshot::new(layout);
        for field in &layout.fields {
            let values: Option<&[f32]> = match field.field.as_str() {
                aestra_core::AESTRA_FIELD_POSITION => Some(&self.position),
                aestra_core::AESTRA_FIELD_ROTATION => Some(&self.rotation),
                aestra_core::AESTRA_FIELD_SCALE => Some(&self.scale),
                aestra_core::AESTRA_FIELD_LINEAR_VELOCITY => {
                    self.linear_velocity.as_ref().map(|velocity| &velocity[..])
                }
                _ => None,
            };
            if let Some(values) = values {
                snapshot
                    .set(layout, &field.field, values)
                    .expect("spatial fields match their registered widths");
            }
        }
        snapshot
    }
}

/// Every binding slot's snapshot for one Aestra tick, pushed by the host in one call
/// (host bindings HB3). `None` means the slot's object is unbound or lost this tick.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BindingFrame {
    pub snapshots: Vec<Option<BindingSnapshot>>,
}

/// Why a binding update was rejected. Rejected updates change nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingError {
    UnknownSlot(usize),
    UnknownName(String),
    /// A frame must carry exactly one entry per slot.
    FrameSize {
        expected: usize,
        found: usize,
    },
    Stride {
        slot: BindingSlot,
        expected: usize,
        found: usize,
    },
    UnknownField(BindingFieldId),
    FieldWidth {
        field: BindingFieldId,
        expected: usize,
        found: usize,
    },
    MissingRequiredField {
        slot: BindingSlot,
        field: BindingFieldId,
    },
    /// The presence mask marks fields beyond the layout.
    UnknownPresence(BindingSlot),
    NonFinite(BindingSlot),
}

impl std::fmt::Display for BindingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownSlot(slot) => write!(f, "binding slot {slot} does not exist"),
            Self::UnknownName(name) => write!(f, "the effect declares no binding named '{name}'"),
            Self::FrameSize { expected, found } => {
                write!(f, "a binding frame needs {expected} slots, found {found}")
            }
            Self::Stride {
                slot,
                expected,
                found,
            } => write!(
                f,
                "slot {} expects {expected} packed values, found {found}",
                slot.0
            ),
            Self::UnknownField(field) => write!(f, "the layout has no field '{}'", field.as_str()),
            Self::FieldWidth {
                field,
                expected,
                found,
            } => write!(
                f,
                "field '{}' takes {expected} components, found {found}",
                field.as_str()
            ),
            Self::MissingRequiredField { slot, field } => write!(
                f,
                "slot {} is missing required field '{}'",
                slot.0,
                field.as_str()
            ),
            Self::UnknownPresence(slot) => {
                write!(f, "slot {} marks fields its layout does not have", slot.0)
            }
            Self::NonFinite(slot) => write!(f, "slot {} contains a non-finite value", slot.0),
        }
    }
}

impl std::error::Error for BindingError {}

/// What a binding slot currently holds, for host diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingState {
    /// The host supplies a value (for `Live`, this tick's; for `SnapshotOnSpawn`, the latched one).
    Bound,
    /// No value: never bound, or lost (despawned) and not re-bound.
    Unbound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingSlotStatus {
    pub slot: BindingSlot,
    pub name: String,
    pub required: bool,
    pub update_mode: BindingUpdateMode,
    pub state: BindingState,
}

/// Per-slot binding status of an instance (host bindings HB3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BindingStatus {
    pub slots: Vec<BindingSlotStatus>,
}

impl BindingStatus {
    /// Required bindings with no value: the effect cannot run as authored until the host binds them.
    pub fn missing_required(&self) -> impl Iterator<Item = &BindingSlotStatus> {
        self.slots
            .iter()
            .filter(|slot| slot.required && slot.state == BindingState::Unbound)
    }

    pub fn is_satisfied(&self) -> bool {
        self.missing_required().next().is_none()
    }
}

/// An instance's binding inputs: the latest host value per slot, and the value latched at instance
/// start for `SnapshotOnSpawn` slots.
#[derive(Debug, Clone, Default)]
pub(crate) struct BindingInputs {
    pub(crate) current: Vec<Option<BindingSnapshot>>,
    pub(crate) latched: Vec<Option<BindingSnapshot>>,
    /// Bumped whenever a slot is acquired, lost or rebound; part of checkpoint identity.
    pub(crate) epoch: u64,
}

impl BindingInputs {
    pub(crate) fn new(slots: usize) -> Self {
        Self {
            current: vec![None; slots],
            latched: vec![None; slots],
            epoch: 0,
        }
    }
}
impl crate::EffectInstance {
    /// Pushes every slot's snapshot for this tick at once (host bindings HB3). The frame is validated
    /// completely before anything changes, so a rejected frame leaves the instance untouched.
    pub fn apply_binding_frame(&mut self, frame: &BindingFrame) -> Result<(), BindingError> {
        let expected = self.effect.bindings.len();
        if frame.snapshots.len() != expected {
            return Err(BindingError::FrameSize {
                expected,
                found: frame.snapshots.len(),
            });
        }
        for (index, snapshot) in frame.snapshots.iter().enumerate() {
            if let Some(snapshot) = snapshot {
                snapshot.validate(BindingSlot(index), &self.effect.bindings[index])?;
            }
        }
        for (index, snapshot) in frame.snapshots.iter().enumerate() {
            self.store_binding(BindingSlot(index), snapshot.clone());
        }
        Ok(())
    }

    /// Sets one slot; `None` marks it unbound (the host lost or released the object).
    pub fn set_binding(
        &mut self,
        slot: BindingSlot,
        snapshot: Option<BindingSnapshot>,
    ) -> Result<(), BindingError> {
        let binding = self
            .effect
            .bindings
            .get(slot.0)
            .ok_or(BindingError::UnknownSlot(slot.0))?;
        if let Some(snapshot) = &snapshot {
            snapshot.validate(slot, binding)?;
        }
        self.store_binding(slot, snapshot);
        Ok(())
    }

    /// Convenience for spatial bindings addressed by their public name.
    pub fn set_spatial_binding(
        &mut self,
        name: &str,
        snapshot: SpatialBindingSnapshot,
    ) -> Result<(), BindingError> {
        let (slot, binding) = self
            .effect
            .binding_named(name)
            .ok_or_else(|| BindingError::UnknownName(name.to_string()))?;
        let packed = snapshot.to_snapshot(&binding.layout);
        self.set_binding(slot, Some(packed))
    }

    /// Tells the instance the host now binds a *different* object to `slot` (a new target), even if
    /// the slot stays valid throughout. Re-latches a `SnapshotOnSpawn` slot on its next value and
    /// changes checkpoint identity, so history recorded against the old object is not reused.
    pub fn rebind(&mut self, slot: BindingSlot) -> Result<(), BindingError> {
        if slot.0 >= self.effect.bindings.len() {
            return Err(BindingError::UnknownSlot(slot.0));
        }
        self.binding_inputs.latched[slot.0] = None;
        self.bump_binding_epoch();
        self.refresh_host_fields();
        Ok(())
    }

    /// The value readers see: this tick's for `Live`, the latched one for `SnapshotOnSpawn`.
    pub fn binding(&self, slot: BindingSlot) -> Option<&BindingSnapshot> {
        let binding = self.effect.bindings.get(slot.0)?;
        match binding.update_mode {
            BindingUpdateMode::Live => self.binding_inputs.current[slot.0].as_ref(),
            BindingUpdateMode::SnapshotOnSpawn => self.binding_inputs.latched[slot.0].as_ref(),
        }
    }

    /// One field of a slot's current value, if supplied.
    pub fn binding_field(&self, slot: BindingSlot, field: &BindingFieldId) -> Option<&[f32]> {
        let layout = &self.effect.bindings.get(slot.0)?.layout;
        self.binding(slot)?.field(layout, field)
    }

    /// Per-slot state, for host diagnostics: which required bindings are missing.
    pub fn binding_status(&self) -> BindingStatus {
        BindingStatus {
            slots: self
                .effect
                .bindings
                .iter()
                .enumerate()
                .map(|(index, binding)| BindingSlotStatus {
                    slot: BindingSlot(index),
                    name: binding.name.clone(),
                    required: binding.required,
                    update_mode: binding.update_mode,
                    state: if self.binding(BindingSlot(index)).is_some() {
                        BindingState::Bound
                    } else {
                        BindingState::Unbound
                    },
                })
                .collect(),
        }
    }

    /// Changes whenever a binding is acquired, lost or rebound. Part of checkpoint identity
    /// ([`crate::CheckpointContext::host_input`]).
    pub fn host_input_epoch(&self) -> u64 {
        self.binding_inputs.epoch
    }

    /// Whether the simulation reads host input that cannot be reconstructed for a backward seek
    /// (host bindings §7.1): a module input or plugin module reading a `Live` binding is forward-only
    /// unless the host records its stream. `SnapshotOnSpawn` values are latched and reproducible, and
    /// declared-but-unread bindings do not affect the simulation.
    pub fn has_forward_only_inputs(&self) -> bool {
        let live = |slot: BindingSlot| {
            self.effect
                .bindings
                .get(slot.0)
                .is_some_and(|binding| binding.update_mode == BindingUpdateMode::Live)
        };
        self.effect
            .host_fields
            .iter()
            .any(|field| live(field.source.binding))
            || self.effect.emitters.iter().any(|emitter| {
                emitter.extension_stages.iter().any(|stage| {
                    stage
                        .modules
                        .iter()
                        .flat_map(|module| module.host_fields.values())
                        .any(|field| live(field.binding))
                })
            })
    }

    /// Rewrites the host-field tail of the input table from the current binding values; absent
    /// fields take their fallback.
    fn refresh_host_fields(&mut self) {
        let start = self.effect.parameters.len();
        for (index, field) in self.effect.host_fields.iter().enumerate() {
            let value = self
                .binding(field.source.binding)
                .and_then(|snapshot| {
                    let layout = &self.effect.bindings[field.source.binding.0].layout;
                    snapshot.field(layout, &field.source.field)
                })
                .and_then(|components| host_field_value(field.source.value_type, components))
                .unwrap_or_else(|| field.fallback.clone());
            self.parameters[start + index] = value;
        }
    }

    /// Fills this (child) instance's forwarded slots from its parent's resolved values (host
    /// bindings roadmap §9.6).
    pub fn apply_binding_forwards(
        &mut self,
        parent: &crate::EffectInstance,
        forwards: &[CompiledBindingForward],
    ) {
        self.apply_forwarded_bindings(&parent.effect, &parent.resolved_bindings(), forwards);
    }

    /// Every slot's value as readers see it (`Live`: current; `SnapshotOnSpawn`: latched). Hosts
    /// pass this down a clip hierarchy with [`Self::apply_forwarded_bindings`].
    pub fn resolved_bindings(&self) -> Vec<Option<BindingSnapshot>> {
        (0..self.effect.bindings.len())
            .map(|index| self.binding(BindingSlot(index)).cloned())
            .collect()
    }

    /// Fills forwarded slots from a parent's [`Self::resolved_bindings`]. Parent and child may declare
    /// different field sets of the same kind, so each value is repacked field by field into the
    /// child's layout; if a field the child requires is missing, the slot stays unbound.
    pub fn apply_forwarded_bindings(
        &mut self,
        parent: &crate::CompiledEffect,
        parent_values: &[Option<BindingSnapshot>],
        forwards: &[CompiledBindingForward],
    ) {
        for forward in forwards {
            let (Some(binding), Some(parent_binding)) = (
                self.effect.bindings.get(forward.child_slot.0),
                parent.bindings.get(forward.parent_slot.0),
            ) else {
                continue;
            };
            let value = parent_values
                .get(forward.parent_slot.0)
                .and_then(Option::as_ref)
                .map(|source| {
                    let mut repacked = BindingSnapshot::new(&binding.layout);
                    for field in &binding.layout.fields {
                        if let Some(values) = source.field(&parent_binding.layout, &field.field) {
                            let _ = repacked.set(&binding.layout, &field.field, values);
                        }
                    }
                    repacked
                })
                .filter(|value| value.validate(forward.child_slot, binding).is_ok());
            self.store_binding(forward.child_slot, value);
        }
    }

    fn store_binding(&mut self, slot: BindingSlot, snapshot: Option<BindingSnapshot>) {
        let index = slot.0;
        let was_bound = self.binding_inputs.current[index].is_some();
        if snapshot.is_some() != was_bound {
            // Acquired or lost: the input history this instance observed changes character.
            self.bump_binding_epoch();
        }
        if self.binding_inputs.latched[index].is_none() && snapshot.is_some() {
            self.binding_inputs.latched[index] = snapshot.clone();
        }
        self.binding_inputs.current[index] = snapshot;
        self.refresh_host_fields();
    }

    fn bump_binding_epoch(&mut self) {
        self.binding_inputs.epoch = self.binding_inputs.epoch.wrapping_add(1);
        self.mark_history_discontinuity();
    }

    /// Clears `SnapshotOnSpawn` latches; each re-latches from its current value, if any.
    pub(crate) fn relatch_spawn_bindings(&mut self) {
        let inputs = &mut self.binding_inputs;
        for (latched, current) in inputs.latched.iter_mut().zip(&inputs.current) {
            *latched = current.clone();
        }
        self.refresh_host_fields();
    }
}
impl crate::CompiledEffectProject {
    /// Checks every clip's binding forwards against the effects actually present: the parent slot
    /// exists on the owner and the child slot on the child effect. Forwards are compiled together;
    /// this catches projects assembled from separately reloaded artifacts that disagree.
    pub fn validate_binding_forwards(&self) -> Result<(), String> {
        let effects = std::iter::once(&self.root).chain(self.dependencies.values());
        for owner in effects {
            for clip in &owner.effect_clips {
                let Some(child) = self.effect(clip.source.id) else {
                    continue;
                };
                for forward in &clip.binding_forwards {
                    if forward.parent_slot.0 >= owner.bindings.len() {
                        return Err(format!(
                            "effect '{}' clip {} forwards from missing slot {}",
                            owner.name, clip.source_clip, forward.parent_slot.0
                        ));
                    }
                    match child.bindings.get(forward.child_slot.0) {
                        Some(binding) if binding.source == forward.child => {}
                        _ => {
                            return Err(format!(
                                "effect '{}' clip {} forwards to slot {} of '{}', which is not binding {}",
                                owner.name,
                                clip.source_clip,
                                forward.child_slot.0,
                                child.name,
                                forward.child
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn field(id: &str) -> BindingFieldId {
        BindingFieldId::new(id)
    }

    #[test]
    fn fields_pack_contiguously_by_component_count() {
        let layout = BindingLayout::pack([
            (field("a.position"), ValueType::Vec3),
            (field("a.rotation"), ValueType::Vec4),
            (field("a.strength"), ValueType::Scalar),
        ])
        .unwrap();
        let offsets: Vec<u32> = layout.fields.iter().map(|f| f.offset).collect();
        assert_eq!(offsets, [0, 3, 7]);
        assert_eq!(layout.stride, 8);
        assert_eq!(layout.field(&field("a.strength")).unwrap().0, 2);
        layout.validate().unwrap();
    }

    #[test]
    fn non_numeric_types_and_malformed_layouts_are_rejected() {
        assert!(BindingLayout::pack([(field("a.curve"), ValueType::Curve)]).is_err());
        let mut layout = BindingLayout::pack([(field("a.p"), ValueType::Vec3)]).unwrap();
        layout.stride = 4;
        assert!(layout.validate().is_err());
        layout.stride = 3;
        layout.fields[0].offset = 1;
        assert!(layout.validate().is_err());
    }
}
