//! Interface checks for plugin compute programs (extensible-stages M13, plan §13.2).
//!
//! A plugin stage lowers to an [`ExecutionBlock`] whose compute ops name entry points of registered
//! [`ComputeProgram`]s. The Execution IR binding convention puts the block's `i`-th declared resource at
//! `@group(0) @binding(i)`, and an op's `accesses` must name exactly the resources its entry point uses.
//! [`check_program_block`] proves that from the WGSL itself — with naga, no GPU — so a lowering whose
//! declared accesses disagree with its shaders (a missing resource, an extra one, or a write to a
//! resource declared read-only) is rejected before any backend binds it. The declared accesses are what
//! barrier/hazard reasoning trusts, so they must be truthful.

use aestra_compiler::{ComputeProgram, ComputeProgramRegistry};
use aestra_core::ComputeProgramId;
use aestra_runtime::{ExecutionBlock, ExecutionOp, ResourceAccessMode};
use std::collections::BTreeMap;

/// How one entry point uses one binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BindingUse {
    pub read: bool,
    pub write: bool,
}

/// The `@group(0)` bindings each declared entry point of a program statically uses.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProgramInterface {
    pub entries: BTreeMap<String, BTreeMap<u32, BindingUse>>,
}

/// Parses and validates a program, and records the bindings each of its declared entry points uses.
/// Errors when the WGSL does not validate, a declared entry is missing or not a compute entry, or a
/// resource binding lives outside `@group(0)`.
pub fn program_interface(program: &ComputeProgram) -> Result<ProgramInterface, String> {
    let id = program.id.as_str();
    let module = naga::front::wgsl::parse_str(&program.wgsl)
        .map_err(|error| format!("program '{id}': {}", error.emit_to_string(&program.wgsl)))?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .map_err(|error| format!("program '{id}' does not validate: {error:?}"))?;
    for (_, global) in module.global_variables.iter() {
        if let Some(binding) = &global.binding
            && binding.group != 0
        {
            return Err(format!(
                "program '{id}' binds a resource in @group({}); the Execution IR uses @group(0)",
                binding.group
            ));
        }
    }
    let mut interface = ProgramInterface::default();
    for entry in &program.entry_points {
        let Some((index, entry_point)) = module
            .entry_points
            .iter()
            .enumerate()
            .find(|(_, candidate)| &candidate.name == entry)
        else {
            return Err(format!("program '{id}' has no entry point '{entry}'"));
        };
        if entry_point.stage != naga::ShaderStage::Compute {
            return Err(format!(
                "program '{id}' entry '{entry}' is not a compute entry"
            ));
        }
        let function = info.get_entry_point(index);
        let mut uses = BTreeMap::new();
        for (handle, global) in module.global_variables.iter() {
            let (Some(binding), usage) = (&global.binding, function[handle]) else {
                continue;
            };
            if usage.is_empty() {
                continue;
            }
            uses.insert(
                binding.binding,
                BindingUse {
                    read: usage
                        .intersects(naga::valid::GlobalUse::READ | naga::valid::GlobalUse::QUERY),
                    write: usage
                        .intersects(naga::valid::GlobalUse::WRITE | naga::valid::GlobalUse::ATOMIC),
                },
            );
        }
        interface.entries.insert(entry.clone(), uses);
    }
    Ok(interface)
}

/// Checks every compute op of `block` against its program (see the module docs): the program is
/// registered, it declares the op's entry point, the entry uses exactly the op's declared resources at
/// their convention bindings, and it writes only resources the op declares writable. Ops without a
/// program are rejected — a program-driven backend has nothing to run for them.
pub fn check_program_block(
    block: &ExecutionBlock,
    programs: &ComputeProgramRegistry,
) -> Result<(), String> {
    let mut interfaces: BTreeMap<ComputeProgramId, ProgramInterface> = BTreeMap::new();
    check_ops(block, &block.ops, programs, &mut interfaces)
}

fn check_ops(
    block: &ExecutionBlock,
    ops: &[ExecutionOp],
    programs: &ComputeProgramRegistry,
    interfaces: &mut BTreeMap<ComputeProgramId, ProgramInterface>,
) -> Result<(), String> {
    for op in ops {
        let compute = match op {
            ExecutionOp::Compute(compute) => compute,
            ExecutionOp::Repeat { body, .. } => {
                check_ops(block, body, programs, interfaces)?;
                continue;
            }
            ExecutionOp::Barrier | ExecutionOp::Copy(_) => continue,
        };
        let name = &compute.name;
        let Some(program_id) = &compute.program else {
            return Err(format!("compute op '{name}' names no program"));
        };
        if !interfaces.contains_key(program_id) {
            let program = programs.get(program_id).ok_or_else(|| {
                format!(
                    "compute op '{name}' names unregistered program '{}'",
                    program_id.as_str()
                )
            })?;
            interfaces.insert(program_id.clone(), program_interface(program)?);
        }
        let uses = interfaces[program_id]
            .entries
            .get(&compute.entry_point)
            .ok_or_else(|| {
                format!(
                    "compute op '{name}': program '{}' declares no entry '{}'",
                    program_id.as_str(),
                    compute.entry_point
                )
            })?;
        let mut declared = BTreeMap::new();
        for access in &compute.accesses {
            let binding = block.binding_of(&access.resource).ok_or_else(|| {
                format!(
                    "compute op '{name}' accesses undeclared resource '{}'",
                    access.resource.as_str()
                )
            })?;
            declared.insert(binding, access);
        }
        for (binding, usage) in uses {
            let Some(access) = declared.get(binding) else {
                return Err(format!(
                    "compute op '{name}': entry '{}' uses @binding({binding}) ('{}'), which the op \
                     does not declare",
                    compute.entry_point,
                    block
                        .resources
                        .get(*binding as usize)
                        .map_or("no declared resource", |resource| resource.id.as_str())
                ));
            };
            if usage.write && access.mode == ResourceAccessMode::Read {
                return Err(format!(
                    "compute op '{name}': entry '{}' writes '{}', declared read-only",
                    compute.entry_point,
                    access.resource.as_str()
                ));
            }
        }
        if let Some((_, unused)) = declared
            .iter()
            .find(|(binding, _)| !uses.contains_key(binding))
        {
            return Err(format!(
                "compute op '{name}' declares '{}', which entry '{}' never uses",
                unused.resource.as_str(),
                compute.entry_point
            ));
        }
    }
    Ok(())
}
