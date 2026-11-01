//! Separate F8 smoke art candidates; runtime motion, births and lighting stay authored.
use aestra_core::{material::*, *};
use std::path::{Path, PathBuf};

const BASE: u128 = 0xa3574a00_0000_4000_8000_000000f8d000;
const FLOW_PARAMETER: u128 = BASE + 0x810;
const FLOW_VELOCITY: [f32; 3] = [0.6, 0.12, 0.08];
const FEATHER_PARAMETER: u128 = BASE + 0x811;
const FEATHER: f32 = 0.75;
const BREAKUP_PARAMETER: u128 = BASE + 0x813;
const BREAKUP: f32 = 0.65;
const DEPTH_PARAMETER: u128 = BASE + 0x814;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Family {
    Gold,
    Blue,
}
impl Family {
    fn source(self) -> &'static str {
        match self {
            Self::Gold => "effects/fireworks_gold_tail_tuned_smoke.aestra.ron",
            Self::Blue => "effects/fireworks_point_star_tuned_smoke.aestra.ron",
        }
    }
    fn file(self) -> &'static str {
        match self {
            Self::Gold => "fireworks_gold_tail_reference_smoke.aestra.ron",
            Self::Blue => "fireworks_point_star_reference_smoke.aestra.ron",
        }
    }
    fn id(self) -> EffectId {
        EffectId::from_u128(
            BASE + match self {
                Self::Gold => 0x200,
                Self::Blue => 0x100,
            },
        )
    }
    fn source_id(self) -> EffectId {
        EffectId::from_u128(
            0xa3574a00_0000_4000_8000_000000f8b000
                + match self {
                    Self::Gold => 0x200,
                    Self::Blue => 0x100,
                },
        )
    }
}
type KeySamples = &'static [(f32, f32)];
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/test")
}
fn read(root: &Path, path: &str) -> Result<String, String> {
    std::fs::read_to_string(root.join(path)).map_err(|e| e.to_string())
}

fn author(
    root: &Path,
    family: Family,
) -> Result<(EffectAsset, MaterialProgram, MaterialFunction), String> {
    let mut asset =
        EffectAsset::from_ron(&read(root, family.source())?).map_err(|e| e.to_string())?;
    asset.id = family.id();
    asset.name = match family {
        Family::Gold => "Gold Tail — Filament Smoke Reference Candidate",
        Family::Blue => "Point Stars — Filament Smoke Reference Candidate",
    }
    .into();
    let mut program = MaterialProgram::from_ron(
        &read(
            root,
            "materials/fireworks_eroding_smoke.aestra.material.ron",
        )?
        .replace("000000f8b7", "000000f8d7")
        .replace("000000f8b6", "000000f8d6"),
    )
    .map_err(|e| e.to_string())?;
    program.name = "Fireworks Filament Lit Smoke".into();
    let mut function = MaterialFunction::from_ron(
        &read(
            root,
            "materials/eroding_smoke_wisp.aestra.material-function.ron",
        )?
        .replace("000000f8b6", "000000f8d6"),
    )
    .map_err(|e| e.to_string())?;
    function.name = "Fireworks Curled Filament Smoke".into();
    let world_input = MaterialFunctionInputId::from_u128(BASE + 0x606);
    let world_expression = MaterialExpressionId::from_u128(BASE + 0x727);
    let core_input = MaterialFunctionInputId::from_u128(BASE + 0x607);
    let core_parameter = MaterialParameterId::from_u128(BASE + 0x704);
    let core_expression = MaterialExpressionId::from_u128(BASE + 0x728);
    let time_input = MaterialFunctionInputId::from_u128(BASE + 0x608);
    let flow_input = MaterialFunctionInputId::from_u128(BASE + 0x609);
    let time_expression = MaterialExpressionId::from_u128(BASE + 0x729);
    let flow_expression = MaterialExpressionId::from_u128(BASE + 0x72a);
    let flow_parameter = MaterialParameterId::from_u128(BASE + 0x705);
    let feather_input = MaterialFunctionInputId::from_u128(BASE + 0x60a);
    let feather_expression = MaterialExpressionId::from_u128(BASE + 0x72b);
    let feather_parameter = MaterialParameterId::from_u128(BASE + 0x706);
    let breakup_input = MaterialFunctionInputId::from_u128(BASE + 0x60b);
    let breakup_expression = MaterialExpressionId::from_u128(BASE + 0x72c);
    let breakup_parameter = MaterialParameterId::from_u128(BASE + 0x707);
    let depth_input = MaterialFunctionInputId::from_u128(BASE + 0x60c);
    let depth_expression = MaterialExpressionId::from_u128(BASE + 0x72d);
    let depth_parameter = MaterialParameterId::from_u128(BASE + 0x708);
    function.inputs.push(MaterialFunctionInput {
        id: world_input,
        name: "World position".into(),
        value_type: MaterialValueType::Vec3,
        default: None,
    });
    program.expressions.push(MaterialExpression {
        id: world_expression,
        kind: MaterialExpressionKind::Input(aestra_core::material::MaterialInput::WorldPosition),
    });
    function.inputs.push(MaterialFunctionInput {
        id: core_input,
        name: "Fresh core".into(),
        value_type: MaterialValueType::Float,
        default: None,
    });
    program.parameters.push(MaterialParameter {
        id: core_parameter,
        name: "Fresh core".into(),
        value_type: MaterialValueType::Float,
        evaluation_domain: MaterialEvaluationDomain::Instance,
        default: Some(MaterialValue::Float(0.0)),
    });
    program.expressions.push(MaterialExpression {
        id: core_expression,
        kind: MaterialExpressionKind::Parameter(core_parameter),
    });
    function.inputs.extend([
        MaterialFunctionInput {
            id: time_input,
            name: "Effect time".into(),
            value_type: MaterialValueType::Float,
            default: None,
        },
        MaterialFunctionInput {
            id: flow_input,
            name: "Pattern velocity".into(),
            value_type: MaterialValueType::Vec3,
            default: None,
        },
    ]);
    program.parameters.push(MaterialParameter {
        id: flow_parameter,
        name: "Pattern velocity".into(),
        value_type: MaterialValueType::Vec3,
        evaluation_domain: MaterialEvaluationDomain::Effect,
        default: Some(MaterialValue::Vec3(FLOW_VELOCITY)),
    });
    program.expressions.extend([
        MaterialExpression {
            id: time_expression,
            kind: MaterialExpressionKind::Input(aestra_core::material::MaterialInput::EffectTime),
        },
        MaterialExpression {
            id: flow_expression,
            kind: MaterialExpressionKind::Parameter(flow_parameter),
        },
    ]);
    asset.parameters.push(EffectParameter {
        id: ParameterId::from_u128(FLOW_PARAMETER),
        name: "Smoke pattern velocity".into(),
        default: Value::Vec3(FLOW_VELOCITY),
        exposed: true,
    });
    function.inputs.push(MaterialFunctionInput {
        id: feather_input,
        name: "Edge feather".into(),
        value_type: MaterialValueType::Float,
        default: Some(MaterialValue::Float(0.0)),
    });
    program.parameters.push(MaterialParameter {
        id: feather_parameter,
        name: "Edge feather".into(),
        value_type: MaterialValueType::Float,
        evaluation_domain: MaterialEvaluationDomain::Effect,
        default: Some(MaterialValue::Float(FEATHER)),
    });
    program.expressions.push(MaterialExpression {
        id: feather_expression,
        kind: MaterialExpressionKind::Parameter(feather_parameter),
    });
    asset.parameters.push(EffectParameter {
        id: ParameterId::from_u128(FEATHER_PARAMETER),
        name: "Smoke edge feather".into(),
        default: Value::Scalar(FEATHER),
        exposed: true,
    });
    function.inputs.push(MaterialFunctionInput {
        id: breakup_input,
        name: "Billow breakup".into(),
        value_type: MaterialValueType::Float,
        default: Some(MaterialValue::Float(0.0)),
    });
    program.parameters.push(MaterialParameter {
        id: breakup_parameter,
        name: "Billow breakup".into(),
        value_type: MaterialValueType::Float,
        evaluation_domain: MaterialEvaluationDomain::Effect,
        default: Some(MaterialValue::Float(BREAKUP)),
    });
    program.expressions.push(MaterialExpression {
        id: breakup_expression,
        kind: MaterialExpressionKind::Parameter(breakup_parameter),
    });
    asset.parameters.push(EffectParameter {
        id: ParameterId::from_u128(BREAKUP_PARAMETER),
        name: "Smoke billow breakup".into(),
        default: Value::Scalar(BREAKUP),
        exposed: true,
    });
    function.inputs.push(MaterialFunctionInput {
        id: depth_input,
        name: "Depth variation".into(),
        value_type: MaterialValueType::Bool,
        default: Some(MaterialValue::Bool(false)),
    });
    program.parameters.push(MaterialParameter {
        id: depth_parameter,
        name: "Depth variation".into(),
        value_type: MaterialValueType::Bool,
        evaluation_domain: MaterialEvaluationDomain::Effect,
        default: Some(MaterialValue::Bool(true)),
    });
    program.expressions.push(MaterialExpression {
        id: depth_expression,
        kind: MaterialExpressionKind::Parameter(depth_parameter),
    });
    asset.parameters.push(EffectParameter {
        id: ParameterId::from_u128(DEPTH_PARAMETER),
        name: "Smoke depth variation".into(),
        default: Value::Bool(true),
        exposed: true,
    });
    for expression in &mut program.expressions {
        if let MaterialExpressionKind::FunctionCall {
            function: called,
            arguments,
            ..
        } = &mut expression.kind
            && *called == MaterialFunctionRef::Project(function.id)
        {
            arguments.insert(world_input, world_expression);
            arguments.insert(core_input, core_expression);
            arguments.insert(time_input, time_expression);
            arguments.insert(flow_input, flow_expression);
            arguments.insert(feather_input, feather_expression);
            arguments.insert(breakup_input, breakup_expression);
            arguments.insert(depth_input, depth_expression);
        }
    }
    function
        .custom_wesl
        .as_mut()
        .ok_or("Smoke function source missing")?
        .source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../crates/aestra-gpu/src/shaders/advected_smoke.wesl"
    ))
    .into();
    let wake_materials: Vec<_> = asset
        .emitters
        .iter()
        .filter(|e| e.name == "Launch smoke")
        .flat_map(|e| e.renderers.iter().map(|r| r.material))
        .collect();
    for instance in &mut asset.material_instances {
        if instance.program
            != MaterialProgramRef::Project(MaterialProgramId::from_u128(
                0xa3574a00_0000_4000_8000_000000f8b700,
            ))
        {
            continue;
        }
        instance.program = MaterialProgramRef::Project(program.id);
        instance.values.insert(
            flow_parameter,
            MaterialParameterValue::EffectParameter(ParameterId::from_u128(FLOW_PARAMETER)),
        );
        instance.values.insert(
            feather_parameter,
            MaterialParameterValue::EffectParameter(ParameterId::from_u128(FEATHER_PARAMETER)),
        );
        instance.values.insert(
            breakup_parameter,
            MaterialParameterValue::EffectParameter(ParameterId::from_u128(BREAKUP_PARAMETER)),
        );
        instance.values.insert(
            depth_parameter,
            MaterialParameterValue::EffectParameter(ParameterId::from_u128(DEPTH_PARAMETER)),
        );
        if wake_materials.contains(&instance.id) {
            // Launch and burst puffs share this authored instance. Dense star
            // paths use their own instance and retain erosion without the core.
            instance.values.insert(
                core_parameter,
                MaterialParameterValue::Constant(MaterialValue::Float(1.0)),
            );
        }
        for n in 1..=3 {
            if let Some(value) = instance.values.remove(&MaterialParameterId::from_u128(
                0xa3574a00_0000_4000_8000_000000f8b700 + n,
            )) {
                instance
                    .values
                    .insert(MaterialParameterId::from_u128(BASE + 0x700 + n), value);
            }
        }
    }
    for emitter in &mut asset.emitters {
        for module in &mut emitter.modules {
            let ModuleParameters::Appearance { size, opacity, .. } = &mut module.parameters else {
                continue;
            };
            let (sizes, alphas): (KeySamples, KeySamples) = match emitter.name.as_str() {
                "Launch smoke" => (
                    &[(0.0, 2.1), (0.08, 2.3), (0.4, 3.0), (1.0, 5.0)],
                    &[
                        (0.0, 0.0),
                        (0.012, 0.27),
                        (0.12, 0.18),
                        (0.55, 0.06),
                        (0.85, 0.01),
                        (1.0, 0.0),
                    ],
                ),
                "Burst smoke" => (
                    &[(0.0, 0.4), (0.12, 1.6), (0.45, 4.0), (1.0, 6.0)],
                    &[
                        (0.0, 0.0),
                        (0.015, 0.07),
                        (0.12, 0.055),
                        (0.55, 0.021),
                        (0.85, 0.0035),
                        (1.0, 0.0),
                    ],
                ),
                "Star path smoke" => (
                    if family == Family::Blue {
                        // Point-star contrails remain narrower than the gold
                        // ember haze, especially when several shells overlap.
                        &[(0.0, 0.65), (0.25, 1.2), (0.65, 2.4), (1.0, 3.2)]
                    } else {
                        &[(0.0, 0.8), (0.25, 1.6), (0.65, 3.1), (1.0, 4.0)]
                    },
                    &[
                        (0.0, 0.0),
                        (0.012, 0.0325),
                        (0.2, 0.021),
                        (0.55, 0.008),
                        (0.85, 0.0011),
                        (1.0, 0.0),
                    ],
                ),
                _ => continue,
            };
            size.keys = sizes.iter().map(|&(t, v)| CurveKey::new(t, v)).collect();
            // Point stars leave stronger smoke than the gold ember family,
            // preserving the authored family distinction without more births.
            let density = if family == Family::Blue && emitter.name == "Star path smoke" {
                2.0
            } else {
                1.0
            };
            opacity.keys = alphas
                .iter()
                .map(|&(t, v)| CurveKey::new(t, v * density))
                .collect();
        }
    }
    asset.metadata.insert("reference_smoke".into(), "Separate F8 draft v3: per-instance effect-clock advection of coarse shared world-space density, soft fresh wake and finite eroding tails. Smoke pattern velocity affects procedural density only; particle motion, births, lifetimes, lighting and storage ceilings remain unchanged. Video is qualitative, not photometric calibration; no default adoption.".into());
    Ok((asset, program, function))
}

fn pair(root: &Path) -> Result<EffectAsset, String> {
    let mut asset = EffectAsset::from_ron(&read(
        root,
        "effects/fireworks_shell_tuned_smoke.aestra.ron",
    )?)
    .map_err(|e| e.to_string())?;
    asset.id = EffectId::from_u128(BASE);
    asset.name = "Gold and Blue — Filament Smoke Reference Pair".into();
    for clip in &mut asset.effect_clips {
        clip.source.id = [Family::Gold, Family::Blue]
            .into_iter()
            .find(|f| f.source_id() == clip.source.id)
            .ok_or("Unexpected reference pair source")?
            .id();
    }
    Ok(asset)
}

fn editor_sample(
    gold: &EffectAsset,
    program: &MaterialProgram,
    function: &MaterialFunction,
) -> (EffectAsset, MaterialProgram, MaterialFunction) {
    let base = 0xa3574a00_0000_4000_8000_00000000d900;
    let mut program = program.clone();
    program.name = "Advected Smoke".into();
    for p in &mut program.parameters {
        if p.name == "Ambient" {
            p.default = Some(MaterialValue::Vec3([0.35, 0.4, 0.45]));
        }
        if p.name == "Density" {
            p.evaluation_domain = MaterialEvaluationDomain::Effect;
        }
    }
    let mut function = function.clone();
    function.name = "Advected Smoke Density".into();
    let mut effect = EffectAsset::new("Smoke Material Lab", 14.0);
    effect.id = EffectId::from_u128(base + 1);
    effect.materials.clear();
    effect.parameters = gold
        .parameters
        .iter()
        .filter(|p| {
            p.id == ParameterId::from_u128(FLOW_PARAMETER)
                || p.id == ParameterId::from_u128(FEATHER_PARAMETER)
                || p.id == ParameterId::from_u128(BREAKUP_PARAMETER)
                || p.id == ParameterId::from_u128(DEPTH_PARAMETER)
        })
        .cloned()
        .collect();
    let density = ParameterId::from_u128(BASE + 0x812);
    effect.parameters.push(EffectParameter {
        id: density,
        name: "Smoke density".into(),
        default: Value::Scalar(0.75),
        exposed: true,
    });
    let mut emitter = gold
        .emitters
        .iter()
        .find(|e| e.name == "Launch smoke")
        .unwrap()
        .clone();
    let mut material = gold
        .material_instances
        .iter()
        .find(|m| m.id == emitter.renderers[0].material)
        .unwrap()
        .clone();
    material.id = MaterialId::from_u128(base + 3);
    material.values.insert(
        MaterialParameterId::from_u128(BASE + 0x702),
        MaterialParameterValue::EffectParameter(density),
    );
    emitter.id = EmitterId::from_u128(base + 2);
    emitter.name = "Smoke Plume".into();
    emitter.duration = 6.0;
    emitter.max_particles = 256;
    emitter.scene_outputs.clear();
    for (i, module) in emitter.modules.iter_mut().enumerate() {
        module.id = ModuleId::from_u128(base + 0x10 + i as u128);
        module.bindings.clear();
        match &mut module.parameters {
            ModuleParameters::Emission {
                spawn_rate,
                burst_count,
            } => {
                *spawn_rate = 24.0;
                *burst_count = 8;
            }
            ModuleParameters::Initialize {
                lifetime,
                speed,
                direction,
                spread_degrees,
                ..
            } => {
                *lifetime = ScalarRange::new(5.0, 8.0);
                *speed = ScalarRange::new(5.0, 9.0);
                *direction = [0.0, 1.0, 0.0];
                *spread_degrees = 20.0;
            }
            ModuleParameters::Motion {
                gravity,
                drag,
                turbulence,
            } => {
                *gravity = [0.6, 0.18, 0.1];
                *drag = 0.35;
                *turbulence = 0.6;
            }
            ModuleParameters::Appearance {
                size,
                opacity,
                color,
            } => {
                size.id = CurveId::from_u128(base + 0x31);
                size.keys = [(0.0, 8.0), (0.3, 14.0), (1.0, 24.0)]
                    .into_iter()
                    .map(|(t, v)| CurveKey::new(t, v))
                    .collect();
                opacity.id = CurveId::from_u128(base + 0x32);
                opacity.keys = [(0.0, 0.0), (0.06, 0.4), (0.55, 0.2), (1.0, 0.0)]
                    .into_iter()
                    .map(|(t, v)| CurveKey::new(t, v))
                    .collect();
                color.id = GradientId::from_u128(base + 0x33);
            }
            _ => {}
        }
    }
    for (i, renderer) in emitter.renderers.iter_mut().enumerate() {
        renderer.id = RendererId::from_u128(base + 0x40 + i as u128);
        renderer.material = material.id;
    }
    effect.emitters.push(emitter);
    effect.material_instances.push(material);
    effect.metadata.insert("notes".into(), "Editor-ready reusable smoke material: open the Material Graph, edit Smoke edge feather / Smoke pattern velocity / Smoke density, or drag Advected Smoke Density into another graph. No fireworks, host outputs or demo runtime required. Six seconds of emission, then a finite tail before the 14-second restart.".into());
    (effect, program, function)
}

fn copy_project(source: &Path, destination: &Path) -> Result<(), String> {
    for entry in std::fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            return Err("Review project does not follow symlinks".into());
        }
        let target = destination.join(entry.file_name());
        if kind.is_dir() {
            std::fs::create_dir(&target).map_err(|e| e.to_string())?;
            copy_project(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// A plain editor effect: an invisible moving source deposits a finite wake.
/// The same distance-event and zero-inheritance authoring applies to projectiles.
fn editor_wake_sample(gold: &EffectAsset, lab: &EffectAsset) -> EffectAsset {
    let mut effect = lab.clone();
    effect.id = EffectId::from_u128(0xa3574a00_0000_4000_8000_00000000da01);
    effect.name = "Smoke Wake Lab".into();
    effect.emitters = gold
        .emitters
        .iter()
        .filter(|e| matches!(e.name.as_str(), "Launch shell" | "Launch smoke"))
        .cloned()
        .collect();
    effect.events = gold
        .events
        .iter()
        .filter(|link| {
            link.trigger.distance_settings().is_some()
                && effect.emitters.iter().any(|e| e.id == link.target)
        })
        .cloned()
        .collect();
    for emitter in &mut effect.emitters {
        emitter.scene_outputs.clear();
        emitter.duration = emitter.duration.min(effect.duration);
        if emitter.name == "Launch shell" {
            emitter.name = "Moving source".into();
            emitter
                .renderers
                .retain(|r| matches!(r.properties, RendererProperties::Sprite));
            for module in &mut emitter.modules {
                if let ModuleParameters::Initialize {
                    speed, direction, ..
                } = &mut module.parameters
                {
                    *speed = ScalarRange::new(30.0, 30.0);
                    *direction = [0.5, 0.8660254, 0.0];
                }
                if let ModuleParameters::Appearance { opacity, .. } = &mut module.parameters {
                    opacity.keys = vec![CurveKey::new(0.0, 0.0), CurveKey::new(1.0, 0.0)];
                }
            }
        } else {
            emitter.name = "Deposited smoke".into();
            for module in &mut emitter.modules {
                if let ModuleParameters::Appearance { size, .. } = &mut module.parameters {
                    size.keys = [(0.0, 2.8), (0.08, 4.0), (0.4, 6.5), (1.0, 9.0)]
                        .into_iter()
                        .map(|(t, v)| CurveKey::new(t, v))
                        .collect();
                }
            }
        }
        for renderer in &mut emitter.renderers {
            renderer.material = lab.material_instances[0].id;
        }
        for module in &mut emitter.modules {
            // Curves remain editable constants in this self-contained lab.
            module.bindings.clear();
        }
    }
    effect.metadata.insert("notes".into(), "Moving source deposits smoke every world-space unit; zero inherited velocity, no independent smoke emission. Source dies at 1.3s, smoke drifts and disperses for 8–10s, then the 14s loop restarts. Uses the shared Advected Smoke material; no fireworks host, sound or scene-light dependency.".into());
    effect
}

fn main() -> Result<(), String> {
    let root = root().canonicalize().map_err(|e| e.to_string())?;
    let (asset, program, function) = author(&root, Family::Gold)?;
    let (blue, _, _) = author(&root, Family::Blue)?;
    let pair = pair(&root)?;
    let (lab, lab_program, lab_function) = editor_sample(&asset, &program, &function);
    let wake_lab = editor_wake_sample(&asset, &lab);
    let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sample-project");
    for (path, text) in [
        (
            "effects/smoke_material_lab.aestra.ron",
            lab.to_pretty_ron().map_err(|e| e.to_string())?,
        ),
        (
            "effects/smoke_wake_lab.aestra.ron",
            wake_lab.to_pretty_ron().map_err(|e| e.to_string())?,
        ),
        (
            "materials/advected_smoke.aestra.material.ron",
            lab_program.to_pretty_ron().map_err(|e| e.to_string())?,
        ),
        (
            "materials/advected_smoke_density.aestra.material-function.ron",
            lab_function.to_pretty_ron().map_err(|e| e.to_string())?,
        ),
    ] {
        std::fs::write(sample.join(path), text).map_err(|e| e.to_string())?;
    }
    for (path, text) in [
        (
            format!("effects/{}", Family::Gold.file()),
            asset.to_pretty_ron().map_err(|e| e.to_string())?,
        ),
        (
            format!("effects/{}", Family::Blue.file()),
            blue.to_pretty_ron().map_err(|e| e.to_string())?,
        ),
        (
            "effects/fireworks_shell_reference_smoke.aestra.ron".into(),
            pair.to_pretty_ron().map_err(|e| e.to_string())?,
        ),
        (
            "materials/fireworks_filament_smoke.aestra.material.ron".into(),
            program.to_pretty_ron().map_err(|e| e.to_string())?,
        ),
        (
            "materials/filament_smoke_wisp.aestra.material-function.ron".into(),
            function.to_pretty_ron().map_err(|e| e.to_string())?,
        ),
    ] {
        std::fs::write(root.join(path), text).map_err(|e| e.to_string())?;
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        if args.len() != 2 || args[0] != "--review-project" {
            return Err("Use --review-project FRESH_DIRECTORY, or no arguments".into());
        }
        let requested = PathBuf::from(&args[1]);
        let destination = requested
            .parent()
            .ok_or("Review parent missing")?
            .canonicalize()
            .map_err(|e| e.to_string())?
            .join(requested.file_name().ok_or("Review directory missing")?);
        if destination.starts_with(&root) || destination.exists() {
            return Err("Use a fresh directory outside the source project".into());
        }
        std::fs::create_dir(&destination).map_err(|e| e.to_string())?;
        copy_project(&root, &destination)?;
        // Preserve existing clip/host references inside this fresh overlay.
        for (family, mut host_asset) in [(Family::Gold, asset), (Family::Blue, blue)] {
            host_asset.id = family.source_id();
            std::fs::write(
                destination.join(family.source()),
                host_asset.to_pretty_ron().map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        }
        println!("Host review project: {}", destination.display());
    }
    println!("Authored gold, blue and two-shell reference candidates");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn candidate_preserves_all_nonappearance_work_and_regenerates_saved_art() {
        for family in [Family::Gold, Family::Blue] {
            check_family(family);
        }
    }

    fn check_family(family: Family) {
        let root = root();
        let (candidate, program, function) = author(&root, family).unwrap();
        let control = EffectAsset::from_ron(&read(&root, family.source()).unwrap()).unwrap();
        assert_eq!(candidate.events, control.events);
        assert_eq!(candidate.particle_outputs, control.particle_outputs);
        assert_eq!(candidate.point_lights, control.point_lights);
        assert_eq!(candidate.particle_budgets, control.particle_budgets);
        assert_eq!(
            &candidate.parameters[..control.parameters.len()],
            &control.parameters
        );
        assert_eq!(candidate.parameters.len(), control.parameters.len() + 4);
        assert_eq!(
            candidate.parameters[control.parameters.len()].default,
            Value::Vec3(FLOW_VELOCITY)
        );
        assert_eq!(
            candidate.material_instances.len(),
            control.material_instances.len()
        );
        for (a, b) in candidate
            .material_instances
            .iter()
            .zip(&control.material_instances)
        {
            let mut expected = b.clone();
            if b.program
                == MaterialProgramRef::Project(MaterialProgramId::from_u128(
                    0xa3574a00_0000_4000_8000_000000f8b700,
                ))
            {
                expected.program = MaterialProgramRef::Project(program.id);
                expected.values.insert(
                    MaterialParameterId::from_u128(BASE + 0x705),
                    MaterialParameterValue::EffectParameter(ParameterId::from_u128(FLOW_PARAMETER)),
                );
                expected.values.insert(
                    MaterialParameterId::from_u128(BASE + 0x706),
                    MaterialParameterValue::EffectParameter(ParameterId::from_u128(
                        FEATHER_PARAMETER,
                    )),
                );
                expected.values.insert(
                    MaterialParameterId::from_u128(BASE + 0x707),
                    MaterialParameterValue::EffectParameter(ParameterId::from_u128(
                        BREAKUP_PARAMETER,
                    )),
                );
                expected.values.insert(
                    MaterialParameterId::from_u128(BASE + 0x708),
                    MaterialParameterValue::EffectParameter(ParameterId::from_u128(
                        DEPTH_PARAMETER,
                    )),
                );
                for n in 1..=3 {
                    if let Some(value) = expected.values.remove(&MaterialParameterId::from_u128(
                        0xa3574a00_0000_4000_8000_000000f8b700 + n,
                    )) {
                        expected
                            .values
                            .insert(MaterialParameterId::from_u128(BASE + 0x700 + n), value);
                    }
                }
                if control
                    .emitters
                    .iter()
                    .filter(|e| e.name == "Launch smoke")
                    .flat_map(|e| &e.renderers)
                    .any(|r| r.material == b.id)
                {
                    expected.values.insert(
                        MaterialParameterId::from_u128(BASE + 0x704),
                        MaterialParameterValue::Constant(MaterialValue::Float(1.0)),
                    );
                }
            }
            assert_eq!(
                a, &expected,
                "Only smoke program remapping, pattern velocity and wake core may change material instances"
            );
        }
        assert_eq!(candidate.emitters.len(), control.emitters.len());
        for (a, b) in candidate.emitters.iter().zip(&control.emitters) {
            assert_eq!(a.max_particles, b.max_particles);
            assert_eq!(a.renderers, b.renderers);
            assert_eq!(a.scene_outputs, b.scene_outputs);
            assert_eq!(a.modules.len(), b.modules.len());
            for (am, bm) in a.modules.iter().zip(&b.modules) {
                let mut expected = bm.clone();
                if a.name.to_lowercase().contains("smoke")
                    && let (
                        ModuleParameters::Appearance { size, opacity, .. },
                        ModuleParameters::Appearance {
                            size: target_size,
                            opacity: target_opacity,
                            ..
                        },
                    ) = (&am.parameters, &mut expected.parameters)
                {
                    *target_size = size.clone();
                    *target_opacity = opacity.clone();
                }
                assert_eq!(am, &expected);
            }
        }
        assert_eq!(
            candidate,
            EffectAsset::from_ron(&read(&root, &format!("effects/{}", family.file())).unwrap())
                .unwrap()
        );
        assert_eq!(
            program,
            MaterialProgram::from_ron(
                &read(
                    &root,
                    "materials/fireworks_filament_smoke.aestra.material.ron"
                )
                .unwrap()
            )
            .unwrap()
        );
        assert_eq!(
            function,
            MaterialFunction::from_ron(
                &read(
                    &root,
                    "materials/filament_smoke_wisp.aestra.material-function.ron"
                )
                .unwrap()
            )
            .unwrap()
        );
        let index = aestra_project::ProjectAssetIndex::scan(&root);
        for tier in ["high", "medium", "low"] {
            let resolved = index.resolve_effect_project(&candidate).unwrap();
            let compiled = aestra_compiler::EffectCompiler::default()
                .with_tier(aestra_runtime::QualityTier::preset(tier).unwrap())
                .compile_resolved_project(&resolved)
                .unwrap();
            // Core compilation alone does not prove GPU material support.
            // Exercise the public host's shader lowering/binding path as well.
            let mut presented = aestra_bevy::PresentedEffect::new(compiled.root.clone());
            for emitter in &compiled.root.emitters {
                for renderer in &emitter.renderers {
                    assert!(
                        presented
                            .material_binding_for_emitter(renderer.material, emitter.source)
                            .is_some(),
                        "{}: native material binding missing for {}",
                        tier,
                        emitter.name
                    );
                    if emitter.name.to_lowercase().contains("smoke") {
                        assert_eq!(
                            presented
                                .material_binding_for_emitter(renderer.material, emitter.source)
                                .unwrap()
                                .values()
                                .get(&MaterialParameterId::from_u128(BASE + 0x708)),
                            Some(&MaterialValue::Bool(true))
                        );
                    }
                }
            }
            // Exercise the existing typed host parameter route: selecting zero
            // flow at spawn restores an anchored field without a different shader.
            let programs: Vec<_> = compiled
                .root
                .emitters
                .iter()
                .flat_map(|e| e.renderers.iter().map(move |r| (e.source, r.material)))
                .map(|(e, m)| {
                    (
                        e,
                        m,
                        presented
                            .material_binding_for_emitter(m, e)
                            .unwrap()
                            .program()
                            .clone(),
                    )
                })
                .collect();
            presented
                .instance
                .set_parameter(
                    ParameterId::from_u128(FLOW_PARAMETER),
                    Value::Vec3([0.0; 3]),
                )
                .unwrap();
            presented
                .instance
                .set_parameter(
                    ParameterId::from_u128(FEATHER_PARAMETER),
                    Value::Scalar(0.0),
                )
                .unwrap();
            presented
                .instance
                .set_parameter(
                    ParameterId::from_u128(BREAKUP_PARAMETER),
                    Value::Scalar(0.0),
                )
                .unwrap();
            presented
                .instance
                .set_parameter(ParameterId::from_u128(DEPTH_PARAMETER), Value::Bool(false))
                .unwrap();
            presented.refresh_automatic_material_bindings();
            for emitter in &compiled.root.emitters {
                if !emitter.name.to_lowercase().contains("smoke") {
                    continue;
                }
                for renderer in &emitter.renderers {
                    let binding = presented
                        .material_binding_for_emitter(renderer.material, emitter.source)
                        .unwrap();
                    assert!(
                        binding
                            .program()
                            .reflection
                            .required_scene_inputs
                            .contains(&aestra_core::material::MaterialInput::EffectTime)
                    );
                    assert_eq!(
                        binding
                            .values()
                            .get(&MaterialParameterId::from_u128(BASE + 0x705)),
                        Some(&MaterialValue::Vec3([0.0; 3]))
                    );
                    assert_eq!(
                        binding
                            .values()
                            .get(&MaterialParameterId::from_u128(BASE + 0x706)),
                        Some(&MaterialValue::Float(0.0))
                    );
                    assert_eq!(
                        binding
                            .values()
                            .get(&MaterialParameterId::from_u128(BASE + 0x707)),
                        Some(&MaterialValue::Float(0.0))
                    );
                    assert_eq!(
                        binding
                            .values()
                            .get(&MaterialParameterId::from_u128(BASE + 0x708)),
                        Some(&MaterialValue::Bool(false))
                    );
                }
            }
            for (emitter, material, program) in &programs {
                assert!(std::sync::Arc::ptr_eq(
                    program,
                    presented
                        .material_binding_for_emitter(*material, *emitter)
                        .unwrap()
                        .program()
                ));
            }
            presented
                .instance
                .set_parameter(ParameterId::from_u128(DEPTH_PARAMETER), Value::Bool(true))
                .unwrap();
            presented.refresh_automatic_material_bindings();
            for (emitter, material, program) in &programs {
                let binding = presented
                    .material_binding_for_emitter(*material, *emitter)
                    .unwrap();
                assert!(std::sync::Arc::ptr_eq(program, binding.program()));
                if binding
                    .values()
                    .contains_key(&MaterialParameterId::from_u128(BASE + 0x708))
                {
                    assert_eq!(
                        binding
                            .values()
                            .get(&MaterialParameterId::from_u128(BASE + 0x708)),
                        Some(&MaterialValue::Bool(true))
                    );
                }
            }
        }
    }

    #[test]
    fn editor_smoke_sample_regenerates_with_the_shared_kernel_and_compiles_all_tiers() {
        let (gold, program, function) = author(&root(), Family::Gold).unwrap();
        let (effect, program, function) = editor_sample(&gold, &program, &function);
        let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sample-project");
        assert_eq!(
            effect,
            EffectAsset::from_ron(&read(&sample, "effects/smoke_material_lab.aestra.ron").unwrap())
                .unwrap()
        );
        assert_eq!(
            program,
            MaterialProgram::from_ron(
                &read(&sample, "materials/advected_smoke.aestra.material.ron").unwrap()
            )
            .unwrap()
        );
        assert_eq!(
            function,
            MaterialFunction::from_ron(
                &read(
                    &sample,
                    "materials/advected_smoke_density.aestra.material-function.ron"
                )
                .unwrap()
            )
            .unwrap()
        );
        assert!(
            effect.events.is_empty()
                && effect.point_lights.is_empty()
                && effect.particle_outputs.is_empty()
        );
        let index = aestra_project::ProjectAssetIndex::scan(&sample);
        for tier in ["high", "medium", "low"] {
            let resolved = index.resolve_effect_project(&effect).unwrap();
            let compiled = aestra_compiler::EffectCompiler::default()
                .with_tier(aestra_runtime::QualityTier::preset(tier).unwrap())
                .compile_resolved_project(&resolved)
                .unwrap();
            let presented = aestra_bevy::PresentedEffect::new(compiled.root.clone());
            for e in &compiled.root.emitters {
                for r in &e.renderers {
                    assert!(
                        presented
                            .material_binding_for_emitter(r.material, e.source)
                            .is_some()
                    );
                }
            }
        }
    }

    #[test]
    fn editor_wake_deposits_without_origin_emission_or_launch_velocity_and_drains() {
        let (gold, program, function) = author(&root(), Family::Gold).unwrap();
        let (lab, _, _) = editor_sample(&gold, &program, &function);
        let wake = editor_wake_sample(&gold, &lab);
        let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sample-project");
        assert_eq!(
            wake,
            EffectAsset::load_ron(sample.join("effects/smoke_wake_lab.aestra.ron")).unwrap()
        );
        assert!(wake.point_lights.is_empty() && wake.particle_outputs.is_empty());
        assert_eq!(wake.events.len(), 1);
        let link = &wake.events[0];
        assert!(link.trigger.distance_settings().is_some());
        assert_eq!((link.count, link.inherit_velocity), (1, 0.0));
        let smoke = wake.emitters.iter().find(|e| e.id == link.target).unwrap();
        let source = wake.emitters.iter().find(|e| e.id == link.source).unwrap();
        for module in &smoke.modules {
            match &module.parameters {
                ModuleParameters::Emission {
                    spawn_rate,
                    burst_count,
                } => assert_eq!((*spawn_rate, *burst_count), (0.0, 0)),
                ModuleParameters::Initialize {
                    speed, lifetime, ..
                } => {
                    assert!(speed.max <= 0.15);
                    assert!(lifetime.min >= 8.0 && lifetime.max < wake.duration - 1.3);
                }
                _ => {}
            }
        }
        assert!(source.modules.iter().any(|m| matches!(&m.parameters,
            ModuleParameters::Initialize { speed, lifetime, .. } if speed.min == 30.0 && lifetime.max == 1.3)));
        let index = aestra_project::ProjectAssetIndex::scan(&sample);
        for tier in ["high", "medium", "low"] {
            let resolved = index.resolve_effect_project(&wake).unwrap();
            let compiled = aestra_compiler::EffectCompiler::default()
                .with_tier(aestra_runtime::QualityTier::preset(tier).unwrap())
                .compile_resolved_project(&resolved)
                .unwrap();
            let presented = aestra_bevy::PresentedEffect::new(compiled.root.clone());
            for emitter in &compiled.root.emitters {
                for renderer in &emitter.renderers {
                    assert!(
                        presented
                            .material_binding_for_emitter(renderer.material, emitter.source)
                            .is_some()
                    );
                }
            }
        }
    }

    #[test]
    fn pair_preserves_choreography_and_resolves_both_native_families_at_every_tier() {
        let root = root();
        let asset = pair(&root).unwrap();
        let control = EffectAsset::from_ron(
            &read(&root, "effects/fireworks_shell_tuned_smoke.aestra.ron").unwrap(),
        )
        .unwrap();
        assert_eq!(asset.duration, control.duration);
        assert_eq!(asset.playback_mode, control.playback_mode);
        assert_eq!(asset.effect_clips.len(), control.effect_clips.len());
        for (a, b) in asset.effect_clips.iter().zip(&control.effect_clips) {
            let mut expected = b.clone();
            expected.source.id = [Family::Gold, Family::Blue]
                .into_iter()
                .find(|f| f.source_id() == b.source.id)
                .unwrap()
                .id();
            assert_eq!(a, &expected);
        }
        assert_eq!(
            asset,
            EffectAsset::from_ron(
                &read(&root, "effects/fireworks_shell_reference_smoke.aestra.ron").unwrap()
            )
            .unwrap()
        );
        let index = aestra_project::ProjectAssetIndex::scan(&root);
        for tier in ["high", "medium", "low"] {
            let resolved = index.resolve_effect_project(&asset).unwrap();
            let compiled = aestra_compiler::EffectCompiler::default()
                .with_tier(aestra_runtime::QualityTier::preset(tier).unwrap())
                .compile_resolved_project(&resolved)
                .unwrap();
            assert_eq!(compiled.dependencies.len(), 2);
            for family in [Family::Gold, Family::Blue] {
                let effect = compiled.dependencies.get(&family.id()).unwrap();
                let presented = aestra_bevy::PresentedEffect::new(effect.clone());
                for emitter in &effect.emitters {
                    for renderer in &emitter.renderers {
                        assert!(
                            presented
                                .material_binding_for_emitter(renderer.material, emitter.source)
                                .is_some()
                        );
                    }
                }
            }
        }
    }
}
