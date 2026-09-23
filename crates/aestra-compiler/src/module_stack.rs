//! The compact emitter module-stack projection (extensible-stages redesign M9, §28.1–28.3).
//!
//! The redesigned Properties panel shows an emitter's modules as a compact, stage-grouped **stack** of
//! short rows (a name plus a descriptor-driven one-line summary) with a focused inspector below, rather
//! than a scroll of fully-expanded parameter cards. This module is the engine-independent **data**
//! behind those rows — the projection the UI renders and the selection resolves against — mirroring the
//! material editor's `MaterialStackProjection`. The UI shell, splitter, drag-reorder, and the
//! selection-following inspector are built on top of this in the editor; the summaries and grouping are
//! computed here so they are testable without a running editor.

use crate::{EffectCompiler, InputControl};
use aestra_core::{
    Emitter, EmitterShape, ModuleId, ModuleInstance, ModuleParameters, ModuleTypeId, StageKind,
};

/// One module's row in the compact stack (§28.2): its identity, display name, enabled state, a
/// one-line summary of its key authored values, and whether its type is registered (unknown/missing
/// plugin modules still show, preserving their data — §20).
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleStackRow {
    pub module: ModuleId,
    pub module_type: ModuleTypeId,
    pub display_name: String,
    pub enabled: bool,
    pub summary: String,
    pub registered: bool,
}

/// One stage section of the stack (§28.3): its lifecycle/simulation stage, a display title, and the
/// module rows it contains in authored order.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleStackGroup {
    pub stage: StageKind,
    pub title: String,
    pub rows: Vec<ModuleStackRow>,
}

/// The compact stack projection of one emitter's modules (extensible-stages M9): stage groups in
/// canonical lifecycle order, then any authored simulation stages in first-appearance order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EmitterStackProjection {
    pub groups: Vec<ModuleStackGroup>,
}

impl EmitterStackProjection {
    /// Every row across all groups, for selection resolution and filtering.
    pub fn rows(&self) -> impl Iterator<Item = &ModuleStackRow> {
        self.groups.iter().flat_map(|group| group.rows.iter())
    }

    /// Finds the row for a module id, with its stage.
    pub fn row(&self, module: ModuleId) -> Option<(&StageKind, &ModuleStackRow)> {
        self.groups.iter().find_map(|group| {
            group
                .rows
                .iter()
                .find(|row| row.module == module)
                .map(|row| (&group.stage, row))
        })
    }
}

/// The stage title shown in the stack header for a stage (§28.1).
fn stage_title(stage: &StageKind) -> String {
    match stage {
        StageKind::EffectSpawn => "EFFECT SPAWN".to_string(),
        StageKind::EffectUpdate => "EFFECT UPDATE".to_string(),
        StageKind::EmitterSpawn => "EMITTER SPAWN".to_string(),
        StageKind::EmitterUpdate => "EMITTER UPDATE".to_string(),
        StageKind::ParticleSpawn => "PARTICLE SPAWN".to_string(),
        StageKind::ParticleUpdate => "PARTICLE UPDATE".to_string(),
        StageKind::Simulation(name) => format!("SIMULATION · {name}"),
    }
}

/// Concise number formatting for summaries: no trailing zeros, at most two decimals.
fn num(value: f32) -> String {
    if value == value.trunc() && value.abs() < 1.0e7 {
        format!("{}", value as i64)
    } else {
        let s = format!("{value:.2}");
        let s = s.trim_end_matches('0').trim_end_matches('.');
        s.to_string()
    }
}

fn shape_summary(shape: &EmitterShape) -> String {
    match shape {
        EmitterShape::Point => "Point".to_string(),
        EmitterShape::Circle { radius } => format!("Circle · R {}", num(*radius)),
        EmitterShape::Ring { radius } => format!("Ring · R {}", num(*radius)),
        EmitterShape::Sphere { radius } => format!("Sphere · R {}", num(*radius)),
        EmitterShape::Hemisphere { radius } => format!("Hemisphere · R {}", num(*radius)),
        EmitterShape::Box { half_extents } => format!(
            "Box · {}×{}×{}",
            num(half_extents[0] * 2.0),
            num(half_extents[1] * 2.0),
            num(half_extents[2] * 2.0)
        ),
        EmitterShape::Cylinder { radius, depth } => {
            format!("Cylinder · R {} · D {}", num(*radius), num(*depth))
        }
        EmitterShape::Cone { radius, depth } => {
            format!("Cone · R {} · D {}", num(*radius), num(*depth))
        }
    }
}

/// A one-line summary of a module's key authored values (§28.3), derived from its parameters.
pub fn module_summary(module: &ModuleInstance) -> String {
    match &module.parameters {
        ModuleParameters::Emission {
            spawn_rate,
            burst_count,
        } => {
            if *burst_count > 0 {
                format!("Rate {} · Burst {burst_count}", num(*spawn_rate))
            } else {
                format!("Rate {}", num(*spawn_rate))
            }
        }
        ModuleParameters::Shape { shape } => shape_summary(shape),
        ModuleParameters::Initialize {
            lifetime, speed, ..
        } => format!(
            "Speed {}–{} · Life {}–{}s",
            num(speed.min),
            num(speed.max),
            num(lifetime.min),
            num(lifetime.max)
        ),
        ModuleParameters::Motion {
            drag, turbulence, ..
        } => format!("Drag {} · Turb {}", num(*drag), num(*turbulence)),
        ModuleParameters::Appearance { color, .. } => {
            format!("{} color keys", color.keys.len())
        }
        ModuleParameters::Persistent {} => "Stateful".to_string(),
        ModuleParameters::Collision { colliders } => match colliders.len() {
            1 => "1 collider".to_string(),
            n => format!("{n} colliders"),
        },
        ModuleParameters::Custom(values) => match values.len() {
            0 => "No properties".to_string(),
            1 => "1 property".to_string(),
            n => format!("{n} properties"),
        },
    }
}

impl EffectCompiler {
    /// Projects one emitter's modules into the compact, stage-grouped stack (extensible-stages M9).
    /// Groups appear in canonical lifecycle order (spawn/update for emitter and particle), then any
    /// authored simulation stages in first-appearance order. Empty standard groups are included so the
    /// stack has stable sections; the editor may hide them in compact mode.
    pub fn project_emitter_stack(&self, emitter: &Emitter) -> EmitterStackProjection {
        let registry = self.registry();
        let row = |module: &ModuleInstance| {
            let metadata = registry.get(&module.module_type);
            ModuleStackRow {
                module: module.id,
                module_type: module.module_type.clone(),
                display_name: metadata
                    .map(|m| m.display_name.to_string())
                    .unwrap_or_else(|| module.module_type.0.clone()),
                enabled: module.enabled,
                summary: module_summary(module),
                registered: metadata.is_some(),
            }
        };

        let mut groups: Vec<ModuleStackGroup> = Vec::new();
        // Standard emitter lifecycle stages, always present for stable structure (§31.2).
        for stage in [
            StageKind::EmitterSpawn,
            StageKind::EmitterUpdate,
            StageKind::ParticleSpawn,
            StageKind::ParticleUpdate,
        ] {
            let rows = emitter
                .modules
                .iter()
                .filter(|module| module.stage == stage)
                .map(&row)
                .collect();
            groups.push(ModuleStackGroup {
                title: stage_title(&stage),
                stage,
                rows,
            });
        }
        // Authored simulation stages, in first-appearance order.
        for module in &emitter.modules {
            if let StageKind::Simulation(_) = &module.stage
                && !groups.iter().any(|group| group.stage == module.stage)
            {
                let stage = module.stage.clone();
                let rows = emitter
                    .modules
                    .iter()
                    .filter(|other| other.stage == stage)
                    .map(&row)
                    .collect();
                groups.push(ModuleStackGroup {
                    title: stage_title(&stage),
                    stage,
                    rows,
                });
            }
        }
        EmitterStackProjection { groups }
    }
}

/// Whether a module input's control should render inline in a compact stack row vs the inspector. Kept
/// here so the projection and the row renderer agree; today everything edits in the inspector, so this
/// simply reports whether the control is a trivial toggle worth surfacing inline.
pub fn control_is_inline_toggle(control: &InputControl) -> bool {
    matches!(control, InputControl::Toggle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_core::{Emitter, ScalarRange, Value};

    #[test]
    fn a_basic_sprite_projects_into_stage_grouped_rows_with_summaries() {
        let compiler = EffectCompiler::default();
        let emitter = Emitter::basic_sprite("Emitter", 2.0);
        let projection = compiler.project_emitter_stack(&emitter);

        // Four standard lifecycle groups, in canonical order.
        let titles: Vec<&str> = projection.groups.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(
            titles,
            vec![
                "EMITTER SPAWN",
                "EMITTER UPDATE",
                "PARTICLE SPAWN",
                "PARTICLE UPDATE"
            ]
        );

        // Emission lives in emitter update; shape + initialize in particle spawn; motion + appearance
        // in particle update.
        let update = &projection.groups[1];
        assert!(update.rows.iter().any(|r| r.display_name == "Emission"));
        let spawn = &projection.groups[2];
        assert_eq!(spawn.rows.len(), 2);
        let particle_update = &projection.groups[3];
        assert!(
            particle_update
                .rows
                .iter()
                .any(|r| r.display_name == "Motion")
        );

        // Every row is registered and carries a non-empty summary.
        for row in projection.rows() {
            assert!(row.registered, "{} is a built-in", row.display_name);
            assert!(
                !row.summary.is_empty(),
                "{} has a summary",
                row.display_name
            );
        }
    }

    #[test]
    fn summaries_are_descriptor_driven_and_concise() {
        assert_eq!(
            module_summary(&ModuleInstance::motion([0.0, -9.81, 0.0], 1.6, 5.0)),
            "Drag 1.6 · Turb 5"
        );
        assert_eq!(
            module_summary(&ModuleInstance::shape(EmitterShape::Sphere {
                radius: 14.0
            })),
            "Sphere · R 14"
        );
        assert_eq!(
            module_summary(&ModuleInstance::emission(22.0, 0)),
            "Rate 22"
        );
        assert_eq!(module_summary(&ModuleInstance::persistent()), "Stateful");
        assert_eq!(
            module_summary(&ModuleInstance::initialize(
                ScalarRange::new(0.7, 1.25),
                ScalarRange::new(3.0, 5.0),
                [0.0, 1.0, 0.0],
                30.0,
                ScalarRange::new(-1.0, 1.0),
            )),
            "Speed 3–5 · Life 0.7–1.25s"
        );
    }

    #[test]
    fn an_unregistered_custom_module_still_projects_a_row() {
        // A missing/plugin module (Custom) shows a row (its type id as the name) and is flagged
        // unregistered, so its data is preserved and surfaced rather than dropped (§20).
        let compiler = EffectCompiler::default();
        let mut emitter = Emitter::basic_sprite("Emitter", 2.0);
        let mut custom = ModuleInstance::motion([0.0, 0.0, 0.0], 0.0, 0.0);
        custom.module_type = ModuleTypeId::new("org.example.plugin::module/swirl");
        custom.parameters = ModuleParameters::Custom(
            [("strength".to_string(), Value::Scalar(2.0))]
                .into_iter()
                .collect(),
        );
        emitter.modules.push(custom);

        let projection = compiler.project_emitter_stack(&emitter);
        let row = projection
            .rows()
            .find(|row| row.module_type.0 == "org.example.plugin::module/swirl")
            .expect("the custom module projects a row");
        assert!(!row.registered, "the plugin module is not registered");
        assert_eq!(row.display_name, "org.example.plugin::module/swirl");
        assert_eq!(row.summary, "1 property");
    }
}
