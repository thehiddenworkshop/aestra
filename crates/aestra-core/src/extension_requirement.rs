//! Plugin requirements recorded in authored effects (extensible-stages M11, plan §21).
//!
//! An effect names the plugins it depends on, with a version requirement, so a host that lacks one — or
//! has an incompatible version — can say exactly what is unavailable before any type lookup fails.
//! Core only stores and structurally validates these; matching them against installed extensions is
//! registry-aware and lives in the compiler (§20.1).

use crate::{EffectAsset, ExtensionId, SimulationDomain, StageKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// One plugin an effect depends on: its id and a semver requirement such as `^0.1.0` (§21).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ExtensionRequirement {
    pub plugin: ExtensionId,
    pub version: String,
}

impl ExtensionRequirement {
    pub fn new(plugin: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            plugin: ExtensionId::new(plugin),
            version: version.into(),
        }
    }
}

/// The plugin that owns a namespaced type id — the part before `::` in `{plugin_id}::…` — or `None` for
/// a core `aestra.*` id, which has no plugin namespace.
pub fn plugin_of(type_id: &str) -> Option<ExtensionId> {
    let (plugin, rest) = type_id.split_once("::")?;
    (!plugin.is_empty() && !rest.is_empty()).then(|| ExtensionId::new(plugin))
}

impl EffectAsset {
    /// Every plugin this effect references through a namespaced module, renderer, simulation-stage,
    /// domain, binding-kind or binding-field id. Structural — no registry needed — so it works while a plugin is missing.
    pub fn referenced_plugins(&self) -> BTreeSet<ExtensionId> {
        let mut plugins = BTreeSet::new();
        for binding in &self.bindings {
            plugins.extend(plugin_of(binding.kind.as_str()));
            for field in binding.fields() {
                plugins.extend(plugin_of(field.as_str()));
            }
        }
        for emitter in &self.emitters {
            for module in &emitter.modules {
                plugins.extend(plugin_of(&module.module_type.0));
                if let StageKind::Simulation(name) = &module.stage {
                    plugins.extend(plugin_of(emitter.simulation_stage_type(name).as_str()));
                }
            }
            for renderer in &emitter.renderers {
                plugins.extend(plugin_of(&renderer.renderer_type.0));
            }
            if let SimulationDomain::Custom(domain) = &emitter.simulation_domain {
                plugins.extend(plugin_of(domain));
            }
        }
        plugins
    }

    /// The recorded requirement for `plugin`, if the effect lists one.
    pub fn extension_requirement(&self, plugin: &ExtensionId) -> Option<&ExtensionRequirement> {
        self.extensions
            .iter()
            .find(|requirement| &requirement.plugin == plugin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Emitter, ModuleInstance, ModuleTypeId, StageTypeId};

    #[test]
    fn plugin_of_reads_the_namespace_prefix_only_for_namespaced_ids() {
        assert_eq!(
            plugin_of("org.example.aestra::module/vortex"),
            Some(ExtensionId::new("org.example.aestra"))
        );
        assert_eq!(plugin_of("aestra.module.motion"), None);
        assert_eq!(plugin_of("::module/x"), None);
        assert_eq!(plugin_of("org.x::"), None);
    }

    #[test]
    fn referenced_plugins_cover_modules_stages_and_renderers() {
        let mut effect = EffectAsset::new("Plugins", 1.0);
        let mut emitter = Emitter::basic_sprite("Emitter", 1.0);
        let mut module = ModuleInstance::motion([0.0; 3], 0.0, 0.0);
        module.module_type = ModuleTypeId::new("org.a::module/m");
        module.stage = StageKind::Simulation("Solve".into());
        emitter.modules.push(module);
        emitter
            .simulation_stage_types
            .insert("Solve".into(), StageTypeId::new("org.b::stage/s"));
        emitter.renderers[0].renderer_type = crate::RendererTypeId::new("org.c::renderer/r");
        effect.emitters.push(emitter);
        assert_eq!(
            effect.referenced_plugins(),
            ["org.a", "org.b", "org.c"]
                .into_iter()
                .map(ExtensionId::new)
                .collect()
        );
    }
}
