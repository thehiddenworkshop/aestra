//! Production module registry and property value/label projection used by authoring panels.
use crate::localization::Localizer;
use aestra_compiler::ModuleRegistry;
use aestra_core::{ModuleInstance, Value};
use bevy::prelude::Resource;

pub(crate) fn localized_properties_input(
    localizer: &Localizer,
    input: &str,
    fallback: &str,
    description: bool,
) -> String {
    let message = match (input, description) {
        ("spawn_rate", false) => "properties-input-spawn-rate",
        ("spawn_rate", true) => "properties-input-spawn-rate-description",
        ("burst_count", false) => "properties-input-burst-count",
        ("burst_count", true) => "properties-input-burst-count-description",
        ("shape", false) => "properties-input-shape",
        ("shape", true) => "properties-input-shape-description",
        ("lifetime", false) => "properties-input-lifetime",
        ("lifetime", true) => "properties-input-lifetime-description",
        ("speed", false) => "properties-input-speed",
        ("speed", true) => "properties-input-speed-description",
        ("direction", false) => "properties-input-direction",
        ("direction", true) => "properties-input-direction-description",
        ("velocity_distribution", false) => "properties-input-velocity-distribution",
        ("velocity_distribution", true) => "properties-input-velocity-distribution-description",
        ("spread_degrees", false) => "properties-input-spread",
        ("spread_degrees", true) => "properties-input-spread-description",
        ("angular_velocity", false) => "properties-input-angular-velocity",
        ("angular_velocity", true) => "properties-input-angular-velocity-description",
        ("gravity", false) => "properties-input-gravity",
        ("gravity", true) => "properties-input-gravity-description",
        ("drag", false) => "properties-input-drag",
        ("drag", true) => "properties-input-drag-description",
        ("turbulence", false) => "properties-input-turbulence",
        ("turbulence", true) => "properties-input-turbulence-description",
        ("size", false) => "properties-input-size",
        ("size", true) => "properties-input-size-description",
        ("opacity", false) => "properties-input-opacity",
        ("opacity", true) => "properties-input-opacity-description",
        ("color", false) => "properties-input-color",
        ("color", true) => "properties-input-color-description",
        _ => return fallback.to_owned(),
    };
    localizer.text(message)
}

pub(crate) fn module_parameter(module: &ModuleInstance, name: &str) -> Option<Value> {
    module.active_parameter_value(name)
}

#[derive(Resource)]
pub(crate) struct EditorModuleRegistry(pub(crate) ModuleRegistry);

impl Default for EditorModuleRegistry {
    /// The built-in modules plus those of every linked extension (extensible-stages M10).
    fn default() -> Self {
        Self(aestra_compiler::ExtensionRegistry::linked().modules)
    }
}
