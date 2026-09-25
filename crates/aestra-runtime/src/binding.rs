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
