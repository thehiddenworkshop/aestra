//! Schema-versioned generic property payloads and their describing schemas (extensibility redesign M2).
//!
//! A plugin-defined module/stage/renderer must remain **serializable and editable without the plugin's
//! Rust code**, so authored plugin data is stored as an [`ExtensionPayload`] — a schema version plus a
//! self-describing [`PropertyBag`] over Aestra's existing semantic [`Value`] model — never as an opaque
//! plugin struct. When the plugin *is* installed it also publishes a [`PropertySchema`] describing each
//! property (label, type, control, constraints, default), which drives validation and the schema-driven
//! inspector. `PropertySchema` is the **generalization** of the compiler's `ModuleMetadata.inputs`
//! (§19.1): built-ins express their inputs as a `PropertySchema` so built-ins and plugins share one
//! control-rendering and validation path. Unknown bag entries — data from a newer plugin version, or
//! from a plugin that is currently missing — are always preserved, never dropped.

use crate::{Value, ValueType};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A schema-versioned bag of authored plugin data (extensibility redesign M2, §18). Serializable and
/// deserializable **without the plugin's code**: `schema_version` lets the plugin migrate its own data
/// across versions, and `values` is a self-describing [`PropertyBag`]. Nothing here disappears when the
/// plugin is unavailable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtensionPayload {
    pub schema_version: u32,
    #[serde(default)]
    pub values: PropertyBag,
}

impl ExtensionPayload {
    pub fn new(schema_version: u32) -> Self {
        Self {
            schema_version,
            values: PropertyBag::default(),
        }
    }

    pub fn with_values(schema_version: u32, values: PropertyBag) -> Self {
        Self {
            schema_version,
            values,
        }
    }
}

/// A self-describing map of property name → [`Value`], the deterministic (sorted) storage behind an
/// [`ExtensionPayload`] and the generic escape hatch built-ins can also read through typed accessors.
/// It is just data: an entry whose describing property is unknown (a newer plugin, or a missing one) is
/// preserved on round trip, so opening an effect never destroys plugin data.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PropertyBag {
    values: BTreeMap<String, Value>,
}

impl PropertyBag {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.values.get(name)
    }

    pub fn set(&mut self, name: impl Into<String>, value: Value) {
        self.values.insert(name.into(), value);
    }

    pub fn remove(&mut self, name: &str) -> Option<Value> {
        self.values.remove(name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.values.iter()
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.values.keys()
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    // Typed helper accessors (§18): built-ins read their payload with the right type without matching
    // on `Value`. Each returns `None` when absent or of a different type.
    pub fn get_bool(&self, name: &str) -> Option<bool> {
        match self.get(name)? {
            Value::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn get_u32(&self, name: &str) -> Option<u32> {
        match self.get(name)? {
            Value::U32(value) => Some(*value),
            _ => None,
        }
    }

    pub fn get_f32(&self, name: &str) -> Option<f32> {
        match self.get(name)? {
            Value::Scalar(value) => Some(*value),
            _ => None,
        }
    }

    pub fn get_vec3(&self, name: &str) -> Option<[f32; 3]> {
        match self.get(name)? {
            Value::Vec3(value) => Some(*value),
            _ => None,
        }
    }

    pub fn get_str(&self, name: &str) -> Option<&str> {
        match self.get(name)? {
            Value::Text(value) => Some(value.as_str()),
            _ => None,
        }
    }
}

impl FromIterator<(String, Value)> for PropertyBag {
    fn from_iter<T: IntoIterator<Item = (String, Value)>>(iter: T) -> Self {
        Self {
            values: iter.into_iter().collect(),
        }
    }
}

/// How a property is edited — the generalization of the compiler's `InputControl` (§19.1), owned and
/// serializable so a plugin can publish it. Numeric bounds also drive validation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PropertyControl {
    /// A boolean toggle.
    Toggle,
    /// A single scalar with a step and optional inclusive bounds.
    Number {
        step: f32,
        min: Option<f32>,
        max: Option<f32>,
    },
    /// A vector whose components share a step and optional inclusive bounds.
    Vector {
        step: f32,
        min: Option<f32>,
        max: Option<f32>,
    },
    /// A `[min, max]` scalar range with a step and optional inclusive bounds on both ends.
    Range {
        step: f32,
        min: Option<f32>,
        max: Option<f32>,
    },
    /// A choice from named options (empty means the host supplies the options, as built-ins do).
    Choice { options: Vec<String> },
    /// An animation curve editor with a value range.
    Curve { step: f32, min: f32, max: f32 },
    /// A color gradient editor.
    Gradient,
    /// A reference to a resource (asset, material, parameter, …).
    Reference,
}

/// One property of a [`PropertySchema`]: its identity, type, editing control, default, and the
/// authoring sources it accepts. The owned/serializable generalization of a single `InputMetadata`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropertyDescriptor {
    pub name: String,
    pub label: String,
    #[serde(default)]
    pub description: String,
    pub value_type: ValueType,
    pub default: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    pub control: PropertyControl,
    /// The authoring sources this property accepts (constant, random range, curve, gradient). Empty
    /// means constant only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<crate::PropertySource>,
}

impl PropertyDescriptor {
    /// Validates one value against this descriptor: its type must match, and numeric values must fall
    /// within the control's bounds. Returns `None` when the value is acceptable.
    fn check(&self, value: &Value) -> Option<PropertyInvalid> {
        if value.value_type() != self.value_type {
            return Some(PropertyInvalid::TypeMismatch {
                expected: self.value_type,
                found: value.value_type(),
            });
        }
        let out_of_range = |v: f32, min: Option<f32>, max: Option<f32>| {
            min.is_some_and(|lo| v < lo) || max.is_some_and(|hi| v > hi)
        };
        let violation = match (&self.control, value) {
            (PropertyControl::Number { min, max, .. }, Value::Scalar(v)) => {
                out_of_range(*v, *min, *max)
            }
            (PropertyControl::Vector { min, max, .. }, Value::Vec3(components)) => {
                components.iter().any(|v| out_of_range(*v, *min, *max))
            }
            (PropertyControl::Range { min, max, .. }, Value::Range(range)) => {
                out_of_range(range.min, *min, *max)
                    || out_of_range(range.max, *min, *max)
                    || range.min > range.max
            }
            _ => false,
        };
        violation.then_some(PropertyInvalid::OutOfRange)
    }
}

/// Why a property value is invalid against its descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyInvalid {
    /// The value's type does not match the descriptor's declared type.
    TypeMismatch {
        expected: ValueType,
        found: ValueType,
    },
    /// A numeric value (or range endpoint) falls outside the control's bounds, or a range is inverted.
    OutOfRange,
}

/// A validation finding for one property of a [`PropertyBag`] against a [`PropertySchema`].
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyIssue {
    pub property: String,
    pub problem: PropertyInvalid,
}

/// The describing schema for an extension's properties (extensibility redesign M2, §19) — the
/// generalization of the compiler's `ModuleMetadata.inputs`. Drives validation and the schema-driven
/// inspector; built-ins express their inputs as one of these so built-ins and plugins share a path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropertySchema {
    pub schema_version: u32,
    pub properties: Vec<PropertyDescriptor>,
}

impl PropertySchema {
    pub fn new(schema_version: u32, properties: Vec<PropertyDescriptor>) -> Self {
        Self {
            schema_version,
            properties,
        }
    }

    pub fn descriptor(&self, name: &str) -> Option<&PropertyDescriptor> {
        self.properties.iter().find(|p| p.name == name)
    }

    /// A fresh [`PropertyBag`] holding every property's default — a plugin's initial authored data.
    pub fn default_bag(&self) -> PropertyBag {
        self.properties
            .iter()
            .map(|descriptor| (descriptor.name.clone(), descriptor.default.clone()))
            .collect()
    }

    /// Fills in any property missing from `bag` with its default, leaving present (including unknown)
    /// entries untouched.
    pub fn apply_defaults(&self, bag: &mut PropertyBag) {
        for descriptor in &self.properties {
            if !bag.contains(&descriptor.name) {
                bag.set(descriptor.name.clone(), descriptor.default.clone());
            }
        }
    }

    /// Bag keys that no property describes — data from a newer plugin version or an unrelated source.
    /// These are **preserved**, never dropped; this is only for reporting.
    pub fn unknown_keys<'a>(&self, bag: &'a PropertyBag) -> Vec<&'a str> {
        bag.keys()
            .filter(|key| self.descriptor(key).is_none())
            .map(String::as_str)
            .collect()
    }

    /// Validates every *described* property present in the bag (type + numeric bounds). Unknown keys
    /// are not reported (they are preserved), and a property absent from the bag is not an error (its
    /// default applies). Returns the findings, empty when the bag is valid.
    pub fn validate(&self, bag: &PropertyBag) -> Vec<PropertyIssue> {
        let mut issues = Vec::new();
        for descriptor in &self.properties {
            if let Some(value) = bag.get(&descriptor.name)
                && let Some(problem) = descriptor.check(value)
            {
                issues.push(PropertyIssue {
                    property: descriptor.name.clone(),
                    problem,
                });
            }
        }
        issues
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScalarRange;

    /// A fake plugin's "Test Module" schema — `strength: float`, `mode: enum` (§M2 acceptance).
    fn test_module_schema() -> PropertySchema {
        PropertySchema::new(
            1,
            vec![
                PropertyDescriptor {
                    name: "strength".to_string(),
                    label: "Strength".to_string(),
                    description: "How strong the effect is".to_string(),
                    value_type: ValueType::Scalar,
                    default: Value::Scalar(1.0),
                    unit: None,
                    control: PropertyControl::Number {
                        step: 0.1,
                        min: Some(0.0),
                        max: Some(10.0),
                    },
                    sources: vec![crate::PropertySource::Constant],
                },
                PropertyDescriptor {
                    name: "mode".to_string(),
                    label: "Mode".to_string(),
                    description: String::new(),
                    value_type: ValueType::Text,
                    default: Value::Text("PIC".to_string()),
                    unit: None,
                    control: PropertyControl::Choice {
                        options: vec!["PIC".to_string(), "FLIP".to_string(), "APIC".to_string()],
                    },
                    sources: Vec::new(),
                },
            ],
        )
    }

    #[test]
    fn a_fake_plugin_payload_serializes_and_deserializes_without_plugin_code() {
        // The acceptance workload: a plugin defines Test Module { strength: float, mode: enum }; its
        // authored data serializes and deserializes with no plugin Rust struct — the bag is just data.
        let mut bag = PropertyBag::new();
        bag.set("strength", Value::Scalar(2.5));
        bag.set("mode", Value::Text("FLIP".to_string()));
        let payload = ExtensionPayload::with_values(1, bag);

        let ron = ron::to_string(&payload).expect("serialize");
        let restored: ExtensionPayload = ron::from_str(&ron).expect("deserialize without plugin code");
        assert_eq!(payload, restored, "payload round trips losslessly");
        assert_eq!(restored.schema_version, 1);
        assert_eq!(restored.values.get_f32("strength"), Some(2.5));
        assert_eq!(restored.values.get_str("mode"), Some("FLIP"));
    }

    #[test]
    fn validation_accepts_valid_data_and_reports_type_and_range_problems() {
        let schema = test_module_schema();

        let mut ok = PropertyBag::new();
        ok.set("strength", Value::Scalar(3.0));
        ok.set("mode", Value::Text("APIC".to_string()));
        assert!(schema.validate(&ok).is_empty(), "valid data has no issues");

        let mut wrong_type = PropertyBag::new();
        wrong_type.set("strength", Value::Bool(true)); // should be a scalar
        let issues = schema.validate(&wrong_type);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].property, "strength");
        assert!(matches!(
            issues[0].problem,
            PropertyInvalid::TypeMismatch { .. }
        ));

        let mut out_of_range = PropertyBag::new();
        out_of_range.set("strength", Value::Scalar(50.0)); // max is 10
        let issues = schema.validate(&out_of_range);
        assert_eq!(issues, vec![PropertyIssue {
            property: "strength".to_string(),
            problem: PropertyInvalid::OutOfRange,
        }]);
    }

    #[test]
    fn unknown_properties_are_preserved_when_the_plugin_is_missing() {
        // A bag written by a newer plugin (or opened without the plugin) carries a key the current
        // schema does not describe. Validation ignores it, it survives a round trip, and it is
        // reported as unknown rather than dropped.
        let schema = test_module_schema();
        let mut bag = schema.default_bag();
        bag.set("future_option", Value::U32(7)); // not in the schema

        assert!(
            schema.validate(&bag).is_empty(),
            "an unknown key is not a validation error"
        );
        assert_eq!(schema.unknown_keys(&bag), vec!["future_option"]);

        let payload = ExtensionPayload::with_values(schema.schema_version, bag);
        let ron = ron::to_string(&payload).expect("serialize");
        let restored: ExtensionPayload = ron::from_str(&ron).expect("deserialize");
        assert_eq!(
            restored.values.get_u32("future_option"),
            Some(7),
            "the unknown property is preserved across a round trip"
        );
    }

    #[test]
    fn apply_defaults_fills_missing_but_keeps_present_and_unknown() {
        let schema = test_module_schema();
        let mut bag = PropertyBag::new();
        bag.set("strength", Value::Scalar(4.0)); // present — keep
        bag.set("extra", Value::Bool(false)); // unknown — keep
        schema.apply_defaults(&mut bag);

        assert_eq!(bag.get_f32("strength"), Some(4.0), "present value kept");
        assert_eq!(bag.get_str("mode"), Some("PIC"), "missing value defaulted");
        assert_eq!(bag.get_bool("extra"), Some(false), "unknown value kept");
    }

    #[test]
    fn range_control_validates_endpoints_and_ordering() {
        let schema = PropertySchema::new(
            1,
            vec![PropertyDescriptor {
                name: "span".to_string(),
                label: "Span".to_string(),
                description: String::new(),
                value_type: ValueType::Range,
                default: Value::Range(ScalarRange::new(0.0, 1.0)),
                unit: None,
                control: PropertyControl::Range {
                    step: 0.1,
                    min: Some(0.0),
                    max: Some(1.0),
                },
                sources: Vec::new(),
            }],
        );
        let mut inverted = PropertyBag::new();
        inverted.set("span", Value::Range(ScalarRange::new(0.8, 0.2)));
        assert_eq!(
            schema.validate(&inverted),
            vec![PropertyIssue {
                property: "span".to_string(),
                problem: PropertyInvalid::OutOfRange,
            }],
            "an inverted range is rejected"
        );
    }
}
