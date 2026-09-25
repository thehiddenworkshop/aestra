//! Host bindings HB1: binding kinds are registry descriptors, governed like every other extension id.

use aestra_core::{
    AESTRA_BINDING_SPATIAL, AESTRA_FIELD_LINEAR_VELOCITY, AESTRA_FIELD_POSITION,
    AESTRA_FIELD_ROTATION, AESTRA_FIELD_SCALE, BindingFieldId, BindingKindId, ExtensionId,
    ValueType,
};
use aestra_extension::{
    AestraExtension, BindingFieldDescriptor, BindingKindDescriptor, ExtensionManifest,
    ExtensionRegistry, RegistryConflict,
};

#[test]
fn the_builtin_registry_has_the_spatial_kind_with_four_typed_fields() {
    let registry = ExtensionRegistry::builtin();
    let spatial = registry
        .bindings
        .get(&BindingKindId::new(AESTRA_BINDING_SPATIAL))
        .expect("the spatial kind is built in");
    let fields: Vec<(&str, ValueType)> = spatial
        .fields
        .iter()
        .map(|field| (field.id.as_str(), field.value_type))
        .collect();
    assert_eq!(
        fields,
        [
            (AESTRA_FIELD_POSITION, ValueType::Vec3),
            (AESTRA_FIELD_ROTATION, ValueType::Vec4),
            (AESTRA_FIELD_SCALE, ValueType::Vec3),
            (AESTRA_FIELD_LINEAR_VELOCITY, ValueType::Vec3),
        ]
    );
}

/// An extension registering one binding kind with the given fields.
struct KindExtension {
    plugin: &'static str,
    kind: &'static str,
    fields: Vec<(&'static str, ValueType)>,
}

impl AestraExtension for KindExtension {
    fn manifest(&self) -> ExtensionManifest {
        ExtensionManifest {
            plugin: ExtensionId::new(self.plugin),
            display_name: self.plugin.into(),
            version: "1.0.0".into(),
        }
    }

    fn register(&self, registry: &mut ExtensionRegistry) -> Result<(), RegistryConflict> {
        registry.register_binding_kind(BindingKindDescriptor {
            type_id: BindingKindId::new(self.kind),
            display_name: "Kind".into(),
            fields: self
                .fields
                .iter()
                .map(|(id, value_type)| BindingFieldDescriptor::new(id, id, *value_type))
                .collect(),
        })
    }
}

#[test]
fn a_plugin_kind_may_reuse_core_fields_and_add_its_own() {
    let mut registry = ExtensionRegistry::builtin();
    registry
        .install(&KindExtension {
            plugin: "org.x.fluid",
            kind: "org.x.fluid::binding/source",
            fields: vec![
                (AESTRA_FIELD_POSITION, ValueType::Vec3),
                ("org.x.fluid::field/flow_rate", ValueType::Scalar),
            ],
        })
        .expect("core fields are reusable; own fields are namespaced");
    assert_eq!(
        registry
            .bindings
            .field_type(&BindingFieldId::new("org.x.fluid::field/flow_rate")),
        Some(ValueType::Scalar)
    );
}

#[test]
fn binding_kinds_and_new_fields_must_live_in_the_plugin_namespace() {
    let mut registry = ExtensionRegistry::builtin();
    assert!(matches!(
        registry.install(&KindExtension {
            plugin: "org.x.squat",
            kind: "aestra.binding.squatted",
            fields: vec![],
        }),
        Err(RegistryConflict::OutsideNamespace { .. })
    ));
    assert!(matches!(
        registry.install(&KindExtension {
            plugin: "org.x.squat",
            kind: "org.x.squat::binding/k",
            fields: vec![("org.y::field/stolen", ValueType::Scalar)],
        }),
        Err(RegistryConflict::OutsideNamespace { id, .. }) if id == "org.y::field/stolen"
    ));
    assert!(
        registry.installed().is_empty(),
        "rejected installs leave nothing behind"
    );
}

#[test]
fn a_field_keeps_one_value_type_across_kinds() {
    let mut registry = ExtensionRegistry::builtin();
    assert!(matches!(
        registry.install(&KindExtension {
            plugin: "org.x.flat",
            kind: "org.x.flat::binding/k",
            fields: vec![(AESTRA_FIELD_POSITION, ValueType::Scalar)],
        }),
        Err(RegistryConflict::BindingFieldTypeMismatch { .. })
    ));
    assert!(matches!(
        registry.install(&KindExtension {
            plugin: "org.x.twice",
            kind: "org.x.twice::binding/k",
            fields: vec![
                ("org.x.twice::field/a", ValueType::Scalar),
                ("org.x.twice::field/a", ValueType::Scalar),
            ],
        }),
        Err(RegistryConflict::DuplicateBindingField { .. })
    ));
}
