//! Extensible-stages M13: compute programs are registered like every other plugin contribution —
//! unique, non-empty, and under the registering extension's namespace.

use aestra_core::{ComputeProgramId, ExtensionId};
use aestra_extension::{
    AestraExtension, ComputeProgram, ExtensionManifest, ExtensionRegistry, RegistryConflict,
};

fn program(id: &str, entries: &[&str]) -> ComputeProgram {
    ComputeProgram {
        id: ComputeProgramId::new(id),
        wgsl: "@compute @workgroup_size(1) fn main() {}".into(),
        entry_points: entries.iter().map(|entry| entry.to_string()).collect(),
    }
}

struct ProgramExtension(&'static str);

impl AestraExtension for ProgramExtension {
    fn manifest(&self) -> ExtensionManifest {
        ExtensionManifest {
            plugin: ExtensionId::new("org.example.programs"),
            display_name: "Programs".into(),
            version: "0.1.0".into(),
        }
    }

    fn register(&self, registry: &mut ExtensionRegistry) -> Result<(), RegistryConflict> {
        registry.register_program(program(self.0, &["main"]))
    }
}

#[test]
fn programs_are_unique_and_declare_entry_points() {
    let mut registry = ExtensionRegistry::builtin();
    registry
        .register_program(program("org.x::program/a", &["main"]))
        .unwrap();
    assert_eq!(
        registry.register_program(program("org.x::program/a", &["main"])),
        Err(RegistryConflict::DuplicateProgram(ComputeProgramId::new(
            "org.x::program/a"
        )))
    );
    assert_eq!(
        registry.register_program(program("org.x::program/b", &[])),
        Err(RegistryConflict::EmptyProgram(ComputeProgramId::new(
            "org.x::program/b"
        )))
    );
}

#[test]
fn a_plugin_program_must_live_under_the_plugin_namespace() {
    let mut registry = ExtensionRegistry::builtin();
    registry
        .install(&ProgramExtension("org.example.programs::program/solver"))
        .expect("a namespaced program installs");
    assert!(
        registry
            .programs
            .get(&ComputeProgramId::new(
                "org.example.programs::program/solver"
            ))
            .is_some()
    );

    let mut registry = ExtensionRegistry::builtin();
    let error = registry
        .install(&ProgramExtension("aestra.program.solver"))
        .unwrap_err();
    assert!(matches!(error, RegistryConflict::OutsideNamespace { .. }));
    assert_eq!(registry.programs.iter().count(), 0, "registry unchanged");
}
