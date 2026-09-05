//! Typed material-function resolution and deterministic compiler inlining.

use crate::{MaterialCompileError, MaterialCompiler, MaterialIrProgram};
use aestra_core::{
    Diagnostic, DiagnosticCode, DiagnosticSeverity, MaterialExpressionId, MaterialFunctionId,
    MaterialFunctionInputId, MaterialFunctionOutputId, ValidationReport,
    material::{
        MaterialCustomWeslArgument, MaterialExpression, MaterialExpressionKind, MaterialFunction,
        MaterialFunctionInput, MaterialFunctionOutput, MaterialFunctionRef, MaterialProgram,
        MaterialSchemaVersion, MaterialValueType,
    },
};
use std::collections::{BTreeMap, BTreeSet};

/// Complete typed function environment supplied to material compilation.
#[derive(Debug, Clone)]
pub struct MaterialFunctionLibrary {
    built_ins: BTreeMap<MaterialFunctionId, MaterialFunction>,
    project: BTreeMap<MaterialFunctionId, MaterialFunction>,
}

impl Default for MaterialFunctionLibrary {
    fn default() -> Self {
        let mut library = Self {
            built_ins: BTreeMap::new(),
            project: BTreeMap::new(),
        };
        for function in built_in_material_functions() {
            library.register_builtin(function);
        }
        library
    }
}

impl MaterialFunctionLibrary {
    pub fn new(functions: impl IntoIterator<Item = MaterialFunction>) -> Self {
        let mut library = Self::default();
        for function in functions {
            library.register(function);
        }
        library
    }

    pub fn register(&mut self, function: MaterialFunction) -> Option<MaterialFunction> {
        self.register_project(function)
    }

    pub fn register_project(&mut self, function: MaterialFunction) -> Option<MaterialFunction> {
        self.project.insert(function.id, function)
    }

    pub fn register_builtin(&mut self, function: MaterialFunction) -> Option<MaterialFunction> {
        self.built_ins.insert(function.id, function)
    }

    pub fn get(&self, reference: MaterialFunctionRef) -> Option<&MaterialFunction> {
        match reference {
            MaterialFunctionRef::BuiltIn(id) => self.built_ins.get(&id),
            MaterialFunctionRef::Project(id) => self.project.get(&id),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &MaterialFunction> {
        self.built_ins.values().chain(self.project.values())
    }

    /// Iterates the complete catalog while preserving the reference namespace used by calls.
    pub fn iter_with_references(
        &self,
    ) -> impl Iterator<Item = (MaterialFunctionRef, &MaterialFunction)> {
        self.entries()
    }

    pub fn is_empty(&self) -> bool {
        self.built_ins.is_empty() && self.project.is_empty()
    }

    /// Validates every function's local graph plus all cross-function references and cycles.
    pub fn validation_report(&self) -> ValidationReport {
        let mut report = ValidationReport::default();
        for (reference, function) in self.entries() {
            append_report(
                &mut report,
                &format!("material_functions[{reference:?}]"),
                function.validate_structure(),
            );
            for (index, expression) in function.expressions.iter().enumerate() {
                let MaterialExpressionKind::FunctionCall {
                    function: reference,
                    arguments,
                    output,
                } = &expression.kind
                else {
                    continue;
                };
                let path = format!(
                    "material_functions[{}].expressions[{index}].kind",
                    function.id
                );
                validate_call_signature(self, *reference, arguments, *output, &path, &mut report);
            }
        }

        let mut state = BTreeMap::new();
        let mut stack = Vec::new();
        for (reference, _) in self.entries() {
            detect_function_cycle(reference, self, &mut state, &mut stack, &mut report);
        }
        report
    }

    fn entries(&self) -> impl Iterator<Item = (MaterialFunctionRef, &MaterialFunction)> {
        self.built_ins
            .iter()
            .map(|(&id, function)| (MaterialFunctionRef::BuiltIn(id), function))
            .chain(
                self.project
                    .iter()
                    .map(|(&id, function)| (MaterialFunctionRef::Project(id), function)),
            )
    }
}

/// Canonical reusable functions shipped by the semantic compiler.
pub fn built_in_material_functions() -> Vec<MaterialFunction> {
    let function_id = MaterialFunctionId::from_u128(0xa3574a0000004000800000000000f001);
    let normal_input = MaterialFunctionInputId::from_u128(0xa3574a0000004000800000000000f011);
    let view_input = MaterialFunctionInputId::from_u128(0xa3574a0000004000800000000000f012);
    let power_input = MaterialFunctionInputId::from_u128(0xa3574a0000004000800000000000f013);
    let normal_expression = MaterialExpressionId::from_u128(0xa3574a0000004000800000000000f021);
    let view_expression = MaterialExpressionId::from_u128(0xa3574a0000004000800000000000f022);
    let power_expression = MaterialExpressionId::from_u128(0xa3574a0000004000800000000000f023);
    let result_expression = MaterialExpressionId::from_u128(0xa3574a0000004000800000000000f024);
    let output_id = MaterialFunctionOutputId::from_u128(0xa3574a0000004000800000000000f031);

    vec![MaterialFunction {
        id: function_id,
        schema_version: MaterialSchemaVersion::CURRENT,
        name: "Fresnel Rim".to_owned(),
        inputs: vec![
            MaterialFunctionInput {
                id: normal_input,
                name: "Normal".to_owned(),
                value_type: MaterialValueType::Vec3,
            },
            MaterialFunctionInput {
                id: view_input,
                name: "View direction".to_owned(),
                value_type: MaterialValueType::Vec3,
            },
            MaterialFunctionInput {
                id: power_input,
                name: "Power".to_owned(),
                value_type: MaterialValueType::Float,
            },
        ],
        outputs: vec![MaterialFunctionOutput {
            id: output_id,
            name: "Factor".to_owned(),
            value_type: MaterialValueType::Float,
            expression: result_expression,
        }],
        expressions: vec![
            MaterialExpression {
                id: normal_expression,
                kind: MaterialExpressionKind::FunctionInput(normal_input),
            },
            MaterialExpression {
                id: view_expression,
                kind: MaterialExpressionKind::FunctionInput(view_input),
            },
            MaterialExpression {
                id: power_expression,
                kind: MaterialExpressionKind::FunctionInput(power_input),
            },
            MaterialExpression {
                id: result_expression,
                kind: MaterialExpressionKind::Fresnel {
                    normal: normal_expression,
                    view: view_expression,
                    power: power_expression,
                },
            },
        ],
        custom_wesl: None,
    }]
}

impl MaterialCompiler {
    /// Resolves authoring-level function calls into an ordinary semantic program.
    pub fn inline_functions(
        &self,
        program: &MaterialProgram,
        functions: &MaterialFunctionLibrary,
    ) -> Result<MaterialProgram, MaterialCompileError> {
        Ok(inline_material_functions(program, functions)?.program)
    }

    /// Compiles a semantic program after resolving and deterministically inlining typed functions.
    pub fn compile_with_functions(
        &self,
        program: &MaterialProgram,
        functions: &MaterialFunctionLibrary,
    ) -> Result<MaterialIrProgram, MaterialCompileError> {
        self.compile_function_expansion(&inline_material_functions(program, functions)?)
    }

    pub(crate) fn compile_function_expansion(
        &self,
        expansion: &FunctionExpansion,
    ) -> Result<MaterialIrProgram, MaterialCompileError> {
        let mut ir = self.compile_expanded(&expansion.program)?;
        let mut live_calls = BTreeSet::new();
        for (&call, &target) in &expansion.call_aliases {
            let Some(value) = ir.source_map.values.get(&target).copied() else {
                ir.source_map.eliminated.insert(call);
                continue;
            };
            live_calls.insert(expansion.call_instances[&call]);
            ir.source_map.values.insert(call, value);
            let sources = ir.source_map.expressions.entry(value).or_default();
            sources.push(call);
            sources.sort();
            sources.dedup();
        }
        ir.optimizations.function_calls_authored = expansion.call_aliases.len();
        ir.optimizations.function_calls_live = live_calls.len();
        ir.optimizations.function_calls_eliminated =
            expansion.call_aliases.len() - live_calls.len();
        Ok(ir)
    }
}

pub(crate) struct FunctionExpansion {
    pub(crate) program: MaterialProgram,
    call_aliases: BTreeMap<MaterialExpressionId, MaterialExpressionId>,
    call_instances: BTreeMap<MaterialExpressionId, (u128, MaterialFunctionOutputId)>,
}

pub(crate) fn inline_material_functions(
    program: &MaterialProgram,
    functions: &MaterialFunctionLibrary,
) -> Result<FunctionExpansion, MaterialCompileError> {
    let mut report = program.validate_structure();
    append_report(
        &mut report,
        "material_function_library",
        functions.validation_report(),
    );
    if !report.is_valid() {
        return Err(MaterialCompileError::Validation(report));
    }

    let expressions = program
        .expressions
        .iter()
        .map(|expression| (expression.id, expression))
        .collect::<BTreeMap<_, _>>();
    let mut expander = FunctionExpander {
        functions,
        output: Vec::new(),
        program_expressions: expressions,
        program_memo: BTreeMap::new(),
        function_memo: BTreeMap::new(),
        call_aliases: BTreeMap::new(),
        call_instances: BTreeMap::new(),
        invocation_namespaces: BTreeMap::new(),
        binding_checks: Vec::new(),
        report: ValidationReport::default(),
    };
    // Choose shared representatives by stable identity, independent of authored vector order.
    for id in expander
        .program_expressions
        .keys()
        .copied()
        .collect::<Vec<_>>()
    {
        expander.expand_program(id);
    }
    let color = expander.expand_program(program.outputs.color);
    let alpha = expander.expand_program(program.outputs.alpha);
    let vertex_offset = program
        .outputs
        .vertex_offset
        .and_then(|id| expander.expand_program(id));
    if !expander.report.is_valid() {
        return Err(MaterialCompileError::Validation(expander.report));
    }
    let (Some(color), Some(alpha)) = (color, alpha) else {
        return Err(MaterialCompileError::Validation(expander.report));
    };

    let mut expanded = program.clone();
    expanded.expressions = expander.output;
    expanded.outputs.color = color;
    expanded.outputs.alpha = alpha;
    expanded.outputs.vertex_offset = vertex_offset;
    expanded.disabled_expressions.retain(|id| {
        expanded
            .expressions
            .iter()
            .any(|expression| expression.id == *id)
    });
    expanded.inline_constants.retain(|id| {
        expanded
            .expressions
            .iter()
            .any(|expression| expression.id == *id)
    });

    let (analysis, expanded_report) = expanded.analyze_with_diagnostics();
    append_report(
        &mut expander.report,
        "expanded_material_program",
        expanded_report,
    );
    for check in &expander.binding_checks {
        let Some(info) = analysis.expressions.get(&check.expression) else {
            continue;
        };
        if info.value_type != check.expected {
            push_error(
                &mut expander.report,
                DiagnosticCode::MaterialTypeMismatch,
                &check.path,
                format!(
                    "material function {} expects {:?} but received {:?}",
                    check.subject, check.expected, info.value_type
                ),
            );
        }
    }
    if !expander.report.is_valid() {
        return Err(MaterialCompileError::Validation(expander.report));
    }
    Ok(FunctionExpansion {
        program: expanded,
        call_aliases: expander.call_aliases,
        call_instances: expander.call_instances,
    })
}

struct BindingCheck {
    path: String,
    expression: MaterialExpressionId,
    expected: MaterialValueType,
    subject: &'static str,
}

struct FunctionExpander<'a> {
    functions: &'a MaterialFunctionLibrary,
    output: Vec<MaterialExpression>,
    program_expressions: BTreeMap<MaterialExpressionId, &'a MaterialExpression>,
    program_memo: BTreeMap<MaterialExpressionId, MaterialExpressionId>,
    function_memo: BTreeMap<(u128, MaterialExpressionId), MaterialExpressionId>,
    call_aliases: BTreeMap<MaterialExpressionId, MaterialExpressionId>,
    call_instances: BTreeMap<MaterialExpressionId, (u128, MaterialFunctionOutputId)>,
    invocation_namespaces: BTreeMap<InvocationKey, u128>,
    binding_checks: Vec<BindingCheck>,
    report: ValidationReport,
}

/// Output is deliberately absent: outputs of one invocation share internal expressions.
type InvocationKey = (
    MaterialFunctionRef,
    BTreeMap<MaterialFunctionInputId, MaterialExpressionId>,
);

fn function_is_shareable(function: &MaterialFunction, library: &MaterialFunctionLibrary) -> bool {
    // The library has already been checked for recursion. Unknown/custom code has no purity
    // contract, so this exclusion also applies to ordinary functions wrapping custom calls.
    function.custom_wesl.is_none()
        && function
            .expressions
            .iter()
            .all(|expression| match &expression.kind {
                MaterialExpressionKind::CustomWeslCall { .. } => false,
                MaterialExpressionKind::FunctionCall { function, .. } => library
                    .get(*function)
                    .is_some_and(|function| function_is_shareable(function, library)),
                _ => true,
            })
}

impl FunctionExpander<'_> {
    fn expand_program(&mut self, id: MaterialExpressionId) -> Option<MaterialExpressionId> {
        if let Some(expanded) = self.program_memo.get(&id) {
            return Some(*expanded);
        }
        let expression = (*self.program_expressions.get(&id)?).clone();
        let expanded = match &expression.kind {
            MaterialExpressionKind::FunctionCall { .. } => {
                self.expand_call(&expression.kind, id, id.as_uuid().as_u128(), None)?
            }
            MaterialExpressionKind::FunctionInput(_) => return None,
            kind => {
                let kind = self.remap_program_kind(kind)?;
                self.output.push(MaterialExpression { id, kind });
                id
            }
        };
        self.program_memo.insert(id, expanded);
        Some(expanded)
    }

    fn expand_function(
        &mut self,
        function: &MaterialFunction,
        id: MaterialExpressionId,
        namespace: u128,
        bindings: &BTreeMap<MaterialFunctionInputId, MaterialExpressionId>,
    ) -> Option<MaterialExpressionId> {
        if let Some(expanded) = self.function_memo.get(&(namespace, id)) {
            return Some(*expanded);
        }
        let expression = function
            .expressions
            .iter()
            .find(|expression| expression.id == id)?;
        let expanded = match &expression.kind {
            MaterialExpressionKind::FunctionInput(input) => *bindings.get(input)?,
            MaterialExpressionKind::FunctionCall { .. } => {
                let call_id = derived_expression_id(namespace, expression.id);
                self.expand_call(
                    &expression.kind,
                    call_id,
                    call_id.as_uuid().as_u128(),
                    Some((function, namespace, bindings)),
                )?
            }
            kind => {
                let generated = derived_expression_id(namespace, expression.id);
                let kind = self.remap_function_kind(kind, function, namespace, bindings)?;
                self.output.push(MaterialExpression {
                    id: generated,
                    kind,
                });
                generated
            }
        };
        self.function_memo.insert((namespace, id), expanded);
        Some(expanded)
    }

    fn expand_call(
        &mut self,
        kind: &MaterialExpressionKind,
        call_id: MaterialExpressionId,
        namespace: u128,
        scope: Option<(
            &MaterialFunction,
            u128,
            &BTreeMap<MaterialFunctionInputId, MaterialExpressionId>,
        )>,
    ) -> Option<MaterialExpressionId> {
        let MaterialExpressionKind::FunctionCall {
            function: reference,
            arguments,
            output,
        } = kind
        else {
            return None;
        };
        let Some(function) = self.functions.get(*reference) else {
            push_error(
                &mut self.report,
                DiagnosticCode::InvalidReference,
                format!("material_function_call[{call_id}].function"),
                format!("material function {:?} is not available", reference),
            );
            return None;
        };
        let Some(function_output) = function
            .outputs
            .iter()
            .find(|candidate| candidate.id == *output)
        else {
            push_error(
                &mut self.report,
                DiagnosticCode::InvalidReference,
                format!("material_function_call[{call_id}].output"),
                format!(
                    "material function output {output} is not declared by {}",
                    function.name
                ),
            );
            return None;
        };

        let mut bindings = BTreeMap::new();
        let mut ordered_arguments = Vec::new();
        for input in &function.inputs {
            let Some(argument) = arguments.get(&input.id) else {
                push_error(
                    &mut self.report,
                    DiagnosticCode::InvalidReference,
                    format!("material_function_call[{call_id}].arguments[{}]", input.id),
                    format!("material function input '{}' has no argument", input.name),
                );
                continue;
            };
            let expanded = match scope {
                Some((owner, owner_namespace, owner_bindings)) => {
                    self.expand_function(owner, *argument, owner_namespace, owner_bindings)
                }
                None => self.expand_program(*argument),
            }?;
            bindings.insert(input.id, expanded);
            ordered_arguments.push(MaterialCustomWeslArgument {
                expression: expanded,
                value_type: input.value_type,
            });
            self.binding_checks.push(BindingCheck {
                path: format!("material_function_call[{call_id}].arguments[{}]", input.id),
                expression: expanded,
                expected: input.value_type,
                subject: "input",
            });
        }
        if let Some(custom_wesl) = &function.custom_wesl {
            let entry_point = custom_wesl.entry_points.get(output)?;
            self.output.push(MaterialExpression {
                id: call_id,
                kind: MaterialExpressionKind::CustomWeslCall {
                    function: function.id,
                    entry_point: entry_point.clone(),
                    source: custom_wesl.source.clone(),
                    arguments: ordered_arguments,
                    output_type: function_output.value_type,
                    evaluation_domain: custom_wesl.evaluation_domain,
                },
            });
            self.call_aliases.insert(call_id, call_id);
            self.call_instances.insert(call_id, (namespace, *output));
            return Some(call_id);
        }
        let namespace = if function_is_shareable(function, self.functions) {
            *self
                .invocation_namespaces
                .entry((*reference, bindings.clone()))
                .or_insert(namespace)
        } else {
            namespace
        };
        let target =
            self.expand_function(function, function_output.expression, namespace, &bindings)?;
        self.binding_checks.push(BindingCheck {
            path: format!("material_function_call[{call_id}].output"),
            expression: target,
            expected: function_output.value_type,
            subject: "output",
        });
        self.call_aliases.insert(call_id, target);
        self.call_instances.insert(call_id, (namespace, *output));
        Some(target)
    }

    fn remap_program_kind(
        &mut self,
        kind: &MaterialExpressionKind,
    ) -> Option<MaterialExpressionKind> {
        remap_kind(kind, |id| self.expand_program(id))
    }

    fn remap_function_kind(
        &mut self,
        kind: &MaterialExpressionKind,
        function: &MaterialFunction,
        namespace: u128,
        bindings: &BTreeMap<MaterialFunctionInputId, MaterialExpressionId>,
    ) -> Option<MaterialExpressionKind> {
        remap_kind(kind, |id| {
            self.expand_function(function, id, namespace, bindings)
        })
    }
}

fn remap_kind(
    kind: &MaterialExpressionKind,
    mut remap: impl FnMut(MaterialExpressionId) -> Option<MaterialExpressionId>,
) -> Option<MaterialExpressionKind> {
    use MaterialExpressionKind as E;
    Some(match kind {
        E::Constant(value) => E::Constant(value.clone()),
        E::Input(input) => E::Input(*input),
        E::Parameter(parameter) => E::Parameter(*parameter),
        E::FunctionInput(_) | E::FunctionCall { .. } | E::CustomWeslCall { .. } => return None,
        E::Add(a, b) => E::Add(remap(*a)?, remap(*b)?),
        E::Subtract(a, b) => E::Subtract(remap(*a)?, remap(*b)?),
        E::Multiply(a, b) => E::Multiply(remap(*a)?, remap(*b)?),
        E::Divide(a, b) => E::Divide(remap(*a)?, remap(*b)?),
        E::Lerp { start, end, factor } => E::Lerp {
            start: remap(*start)?,
            end: remap(*end)?,
            factor: remap(*factor)?,
        },
        E::Clamp { value, min, max } => E::Clamp {
            value: remap(*value)?,
            min: remap(*min)?,
            max: remap(*max)?,
        },
        E::Select {
            condition,
            if_false,
            if_true,
        } => E::Select {
            condition: remap(*condition)?,
            if_false: remap(*if_false)?,
            if_true: remap(*if_true)?,
        },
        E::Remap {
            value,
            input_min,
            input_max,
            output_min,
            output_max,
        } => E::Remap {
            value: remap(*value)?,
            input_min: remap(*input_min)?,
            input_max: remap(*input_max)?,
            output_min: remap(*output_min)?,
            output_max: remap(*output_max)?,
        },
        E::Smoothstep {
            edge_min,
            edge_max,
            value,
        } => E::Smoothstep {
            edge_min: remap(*edge_min)?,
            edge_max: remap(*edge_max)?,
            value: remap(*value)?,
        },
        E::NormalMap {
            sample,
            strength,
            flip_y,
            normal,
            tangent,
            bitangent,
        } => E::NormalMap {
            sample: remap(*sample)?,
            strength: remap(*strength)?,
            flip_y: remap(*flip_y)?,
            normal: remap(*normal)?,
            tangent: remap(*tangent)?,
            bitangent: remap(*bitangent)?,
        },
        E::Fresnel {
            normal,
            view,
            power,
        } => E::Fresnel {
            normal: remap(*normal)?,
            view: remap(*view)?,
            power: remap(*power)?,
        },
        E::RadialMask {
            uv,
            center,
            radius,
            softness,
            invert,
        } => E::RadialMask {
            uv: remap(*uv)?,
            center: remap(*center)?,
            radius: remap(*radius)?,
            softness: remap(*softness)?,
            invert: remap(*invert)?,
        },
        E::Dissolve {
            source,
            threshold,
            edge_width,
            invert,
        } => E::Dissolve {
            source: remap(*source)?,
            threshold: remap(*threshold)?,
            edge_width: remap(*edge_width)?,
            invert: remap(*invert)?,
        },
        E::DissolveEdge {
            source,
            threshold,
            edge_width,
            invert,
        } => E::DissolveEdge {
            source: remap(*source)?,
            threshold: remap(*threshold)?,
            edge_width: remap(*edge_width)?,
            invert: remap(*invert)?,
        },
        E::DepthFade {
            scene_depth,
            pixel_depth,
            fade_distance,
            invert,
        } => E::DepthFade {
            scene_depth: remap(*scene_depth)?,
            pixel_depth: remap(*pixel_depth)?,
            fade_distance: remap(*fade_distance)?,
            invert: remap(*invert)?,
        },
        E::SoftParticle {
            alpha,
            scene_depth,
            pixel_depth,
            fade_distance,
            invert,
        } => E::SoftParticle {
            alpha: remap(*alpha)?,
            scene_depth: remap(*scene_depth)?,
            pixel_depth: remap(*pixel_depth)?,
            fade_distance: remap(*fade_distance)?,
            invert: remap(*invert)?,
        },
        E::PanUv { uv, speed, time } => E::PanUv {
            uv: remap(*uv)?,
            speed: remap(*speed)?,
            time: remap(*time)?,
        },
        E::RotateUv { uv, center, angle } => E::RotateUv {
            uv: remap(*uv)?,
            center: remap(*center)?,
            angle: remap(*angle)?,
        },
        E::ScaleUv { uv, center, scale } => E::ScaleUv {
            uv: remap(*uv)?,
            center: remap(*center)?,
            scale: remap(*scale)?,
        },
        E::DerivativeX { value } => E::DerivativeX {
            value: remap(*value)?,
        },
        E::DerivativeY { value } => E::DerivativeY {
            value: remap(*value)?,
        },
        E::SampleTexture { texture, uv } => E::SampleTexture {
            texture: remap(*texture)?,
            uv: remap(*uv)?,
        },
        E::SampleTextureLevel { texture, uv, level } => E::SampleTextureLevel {
            texture: remap(*texture)?,
            uv: remap(*uv)?,
            level: remap(*level)?,
        },
        E::SampleTextureGradient {
            texture,
            uv,
            ddx,
            ddy,
        } => E::SampleTextureGradient {
            texture: remap(*texture)?,
            uv: remap(*uv)?,
            ddx: remap(*ddx)?,
            ddy: remap(*ddy)?,
        },
        E::ExtractComponent { value, component } => E::ExtractComponent {
            value: remap(*value)?,
            component: *component,
        },
    })
}

fn validate_call_signature(
    library: &MaterialFunctionLibrary,
    reference: MaterialFunctionRef,
    arguments: &BTreeMap<MaterialFunctionInputId, MaterialExpressionId>,
    output: aestra_core::MaterialFunctionOutputId,
    path: &str,
    report: &mut ValidationReport,
) {
    let Some(function) = library.get(reference) else {
        push_error(
            report,
            DiagnosticCode::InvalidReference,
            format!("{path}.function"),
            format!("material function {reference:?} is not available"),
        );
        return;
    };
    let declared = function
        .inputs
        .iter()
        .map(|input| input.id)
        .collect::<BTreeSet<_>>();
    let supplied = arguments.keys().copied().collect::<BTreeSet<_>>();
    for missing in declared.difference(&supplied) {
        push_error(
            report,
            DiagnosticCode::InvalidReference,
            format!("{path}.arguments[{missing}]"),
            "material function input has no argument",
        );
    }
    for unknown in supplied.difference(&declared) {
        push_error(
            report,
            DiagnosticCode::InvalidReference,
            format!("{path}.arguments[{unknown}]"),
            "argument targets an unknown material function input",
        );
    }
    if !function
        .outputs
        .iter()
        .any(|candidate| candidate.id == output)
    {
        push_error(
            report,
            DiagnosticCode::InvalidReference,
            format!("{path}.output"),
            "material function output is not declared",
        );
    }
}

fn detect_function_cycle(
    reference: MaterialFunctionRef,
    library: &MaterialFunctionLibrary,
    state: &mut BTreeMap<MaterialFunctionRef, u8>,
    stack: &mut Vec<MaterialFunctionRef>,
    report: &mut ValidationReport,
) {
    match state.get(&reference).copied() {
        Some(2) => return,
        Some(1) => {
            let start = stack
                .iter()
                .position(|candidate| *candidate == reference)
                .unwrap_or(0);
            let mut cycle = stack[start..].to_vec();
            cycle.push(reference);
            push_error(
                report,
                DiagnosticCode::ReferenceCycle,
                format!("material_functions[{reference:?}]"),
                format!("material function reference cycle detected: {cycle:?}"),
            );
            return;
        }
        _ => {}
    }
    state.insert(reference, 1);
    stack.push(reference);
    if let Some(function) = library.get(reference) {
        for dependency in function.expressions.iter().filter_map(|expression| {
            if let MaterialExpressionKind::FunctionCall { function, .. } = expression.kind {
                Some(function)
            } else {
                None
            }
        }) {
            if library.get(dependency).is_some() {
                detect_function_cycle(dependency, library, state, stack, report);
            }
        }
    }
    stack.pop();
    state.insert(reference, 2);
}

fn derived_expression_id(namespace: u128, source: MaterialExpressionId) -> MaterialExpressionId {
    let mut value = namespace ^ source.as_uuid().as_u128().rotate_left(47);
    value ^= value >> 33;
    value = value.wrapping_mul(0xff51afd7ed558ccd_ff51afd7ed558ccd_u128);
    value ^= value >> 29;
    value = value.wrapping_mul(0xc4ceb9fe1a85ec53_c4ceb9fe1a85ec53_u128);
    value ^= value >> 32;
    if value == 0 {
        value = 1;
    }
    MaterialExpressionId::from_u128(value)
}

fn append_report(target: &mut ValidationReport, prefix: &str, source: ValidationReport) {
    for mut diagnostic in source.diagnostics {
        diagnostic.path = format!("{prefix}.{}", diagnostic.path);
        target.push(diagnostic);
    }
}

fn push_error(
    report: &mut ValidationReport,
    code: DiagnosticCode,
    path: impl Into<String>,
    message: impl Into<String>,
) {
    report.push(Diagnostic {
        severity: DiagnosticSeverity::Error,
        code,
        path: path.into(),
        message: message.into(),
    });
}
