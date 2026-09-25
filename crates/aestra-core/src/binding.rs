//! Host binding declarations (host bindings HB1).
//!
//! An effect declares the live host objects it expects — `Source`, `Target`, `SwordTip` — as named
//! binding slots of a registered **kind** (`aestra.binding.spatial`) with the **fields** it needs
//! (`aestra.field.position`). The host adapter (Bevy, Godot, Unity, …) decides which native object
//! fills each slot and pushes snapshots of its field values; nothing engine-specific is ever stored
//! here.
//!
//! Core validates declarations structurally only. Whether a kind is registered and provides the
//! declared fields is registry-aware and checked by the compiler (extensible plan §20.1).

use crate::diagnostic::{Diagnostic, DiagnosticCode, ValidationReport};
use crate::model::register_id;
use crate::{BindingFieldId, BindingId, BindingKindId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The binding name reserved for the effect's own placement, readable like a binding but never
/// declared (host bindings roadmap §2.12).
pub const RESERVED_SELF_BINDING: &str = "Self";

/// When the host's value is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum BindingUpdateMode {
    /// Captured once, when a particle or emitter spawns, and kept in simulation state.
    SnapshotOnSpawn,
    /// Read every Aestra tick.
    Live,
}

/// A named slot the host fills with a live object (host bindings HB1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectBinding {
    pub id: BindingId,
    pub name: String,
    pub kind: BindingKindId,
    pub update_mode: BindingUpdateMode,
    /// Whether the effect cannot run meaningfully without the host binding this slot.
    #[serde(default = "default_true")]
    pub required: bool,
    /// Fields the host must supply.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub required_fields: BTreeSet<BindingFieldId>,
    /// Fields used when the host supplies them; consumers fall back to authored values otherwise.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub optional_fields: BTreeSet<BindingFieldId>,
}

fn default_true() -> bool {
    true
}

impl EffectBinding {
    /// A required spatial binding needing the host object's position.
    pub fn spatial(name: impl Into<String>, update_mode: BindingUpdateMode) -> Self {
        Self {
            id: BindingId::new(),
            name: name.into(),
            kind: BindingKindId::new(crate::AESTRA_BINDING_SPATIAL),
            update_mode,
            required: true,
            required_fields: [BindingFieldId::new(crate::AESTRA_FIELD_POSITION)].into(),
            optional_fields: BTreeSet::new(),
        }
    }

    /// Every field this binding declares, required first.
    pub fn fields(&self) -> impl Iterator<Item = &BindingFieldId> {
        self.required_fields.iter().chain(&self.optional_fields)
    }
}

/// Structural validation of an effect's binding declarations: stable ids, names, the reserved
/// `Self` name, and coherent field sets.
pub(crate) fn validate_bindings(
    bindings: &[EffectBinding],
    report: &mut ValidationReport,
    semantic_ids: &mut BTreeMap<u128, String>,
) {
    let mut names: BTreeMap<&str, usize> = BTreeMap::new();
    for (index, binding) in bindings.iter().enumerate() {
        let path = format!("effect.bindings[{index}]");
        register_id(
            report,
            semantic_ids,
            binding.id.as_uuid().as_u128(),
            format!("{path}.id"),
        );
        let name = binding.name.trim();
        if name.is_empty() {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidValue,
                format!("{path}.name"),
                "binding name cannot be empty",
            ));
        } else if name != binding.name {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidValue,
                format!("{path}.name"),
                "binding name cannot have surrounding whitespace",
            ));
        } else if name == RESERVED_SELF_BINDING {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidValue,
                format!("{path}.name"),
                "'Self' is reserved for the effect's own placement and cannot be declared",
            ));
        } else if let Some(first) = names.insert(name, index) {
            report.push(Diagnostic::error(
                DiagnosticCode::DuplicateId,
                format!("{path}.name"),
                format!("binding name '{name}' is already used by effect.bindings[{first}]"),
            ));
        }
        if binding.kind.as_str().trim().is_empty() {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidValue,
                format!("{path}.kind"),
                "binding kind cannot be empty",
            ));
        }
        for field in binding.fields() {
            if field.as_str().trim().is_empty() {
                report.push(Diagnostic::error(
                    DiagnosticCode::InvalidValue,
                    format!("{path}.fields"),
                    "binding field ids cannot be empty",
                ));
            }
        }
        for field in binding
            .required_fields
            .intersection(&binding.optional_fields)
        {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidValue,
                format!("{path}.optional_fields"),
                format!(
                    "field '{}' cannot be both required and optional",
                    field.as_str()
                ),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EffectAsset, Emitter};

    fn effect_with(bindings: Vec<EffectBinding>) -> EffectAsset {
        let mut effect = EffectAsset::new("Bound", 1.0);
        effect.emitters.push(Emitter::basic_sprite("Emitter", 1.0));
        effect.bindings = bindings;
        effect
    }

    fn codes_at(effect: &EffectAsset, path_prefix: &str) -> Vec<DiagnosticCode> {
        effect
            .validation_report()
            .diagnostics
            .into_iter()
            .filter(|diagnostic| diagnostic.path.starts_with(path_prefix))
            .map(|diagnostic| diagnostic.code)
            .collect()
    }

    #[test]
    fn source_and_target_bindings_validate_and_round_trip_through_v4() {
        let mut target = EffectBinding::spatial("Target", BindingUpdateMode::Live);
        target
            .optional_fields
            .insert(BindingFieldId::new(crate::AESTRA_FIELD_LINEAR_VELOCITY));
        let effect = effect_with(vec![
            EffectBinding::spatial("Source", BindingUpdateMode::SnapshotOnSpawn),
            target,
        ]);
        assert!(
            effect.validation_report().is_valid(),
            "{:?}",
            effect.validation_report()
        );
        let saved = effect.to_pretty_ron().unwrap();
        assert!(saved.contains("bindings"));
        assert!(saved.contains("aestra.binding.spatial"));
        assert_eq!(EffectAsset::from_ron(&saved).unwrap(), effect);
    }

    #[test]
    fn an_effect_without_bindings_writes_no_bindings_key() {
        let effect = effect_with(Vec::new());
        let saved = effect.to_pretty_ron().unwrap();
        assert!(
            !saved.contains("bindings"),
            "the field is omitted when empty"
        );
        assert_eq!(EffectAsset::from_ron(&saved).unwrap(), effect);
    }

    #[test]
    fn names_ids_self_and_field_sets_are_validated() {
        let first = EffectBinding::spatial("Target", BindingUpdateMode::Live);
        let mut duplicate_id = EffectBinding::spatial("Other", BindingUpdateMode::Live);
        duplicate_id.id = first.id;
        let duplicate_name = EffectBinding::spatial("Target", BindingUpdateMode::Live);
        let reserved = EffectBinding::spatial("Self", BindingUpdateMode::Live);
        let empty = EffectBinding::spatial("  ", BindingUpdateMode::Live);
        let mut overlapping = EffectBinding::spatial("Overlap", BindingUpdateMode::Live);
        overlapping
            .optional_fields
            .insert(BindingFieldId::new(crate::AESTRA_FIELD_POSITION));
        let effect = effect_with(vec![
            first,
            duplicate_id,
            duplicate_name,
            reserved,
            empty,
            overlapping,
        ]);
        assert_eq!(
            codes_at(&effect, "effect.bindings[1].id"),
            [DiagnosticCode::DuplicateId]
        );
        assert_eq!(
            codes_at(&effect, "effect.bindings[2].name"),
            [DiagnosticCode::DuplicateId]
        );
        assert_eq!(
            codes_at(&effect, "effect.bindings[3].name"),
            [DiagnosticCode::InvalidValue]
        );
        assert_eq!(
            codes_at(&effect, "effect.bindings[4].name"),
            [DiagnosticCode::InvalidValue]
        );
        assert_eq!(
            codes_at(&effect, "effect.bindings[5].optional_fields"),
            [DiagnosticCode::InvalidValue]
        );
    }

    #[test]
    fn plugin_binding_kinds_count_as_referenced_extensions() {
        let mut source = EffectBinding::spatial("Source", BindingUpdateMode::Live);
        source.kind = BindingKindId::new("org.example.fluid::binding/source");
        source
            .optional_fields
            .insert(BindingFieldId::new("org.example.flow::field/rate"));
        let effect = effect_with(vec![source]);
        let plugins = effect.referenced_plugins();
        assert!(plugins.contains(&crate::ExtensionId::new("org.example.fluid")));
        assert!(plugins.contains(&crate::ExtensionId::new("org.example.flow")));
    }
}
