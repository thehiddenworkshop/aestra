//! Backend-neutral, read-only projection of a semantic material program as a graph.

use crate::material_stack::ExpressionSink;
use crate::{
    MaterialCompileError, MaterialCompiler, MaterialFunctionLibrary, MaterialIrProgram,
    MaterialIrValueId, MaterialStackModifierKind, material_stack::append_default_modifier,
};
pub use aestra_core::material::MaterialGraphFunction;
use aestra_core::{
    MaterialExpressionId, MaterialFunctionInputId, MaterialFunctionOutputId, MaterialParameterId,
    MaterialProgramId, ValidationReport,
    material::{
        MaterialDomain, MaterialEvaluationDomain, MaterialExpression, MaterialExpressionDomain,
        MaterialExpressionKind, MaterialFilterMode, MaterialFunctionRef, MaterialInput,
        MaterialMipFilterMode, MaterialParameter, MaterialProgram, MaterialSamplerDescriptor,
        MaterialTextureColorSpace, MaterialTextureDescriptor, MaterialValue, MaterialValueType,
        MaterialVectorComponent,
    },
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

trait GraphConstruction: ExpressionSink {
    fn uses_local_normal_basis(&self) -> bool {
        true
    }
    fn source_type(
        &self,
        source: MaterialExpressionId,
        functions: &MaterialFunctionLibrary,
    ) -> Option<MaterialValueType>;
    fn texture_reference(&self) -> Option<MaterialExpressionKind>;
    fn palette_reference(&mut self) -> Option<MaterialExpressionKind>;
    fn palette_swap_references(&mut self) -> Option<[MaterialExpressionKind; 2]>;
    fn pattern_reference(&mut self) -> Option<MaterialExpressionKind> {
        self.texture_reference()
    }
    fn outline_texture_reference(&mut self) -> Option<MaterialExpressionKind> {
        self.texture_reference()
    }
}

fn is_srgb_texture(value_type: MaterialValueType) -> bool {
    matches!(value_type, MaterialValueType::Texture2D(texture)
        if texture.color_space == MaterialTextureColorSpace::SrgbColor)
}

impl GraphConstruction for MaterialProgram {
    fn uses_local_normal_basis(&self) -> bool {
        self.domain == MaterialDomain::Sprite
    }

    fn source_type(
        &self,
        source: MaterialExpressionId,
        functions: &MaterialFunctionLibrary,
    ) -> Option<MaterialValueType> {
        if let Ok(analysis) = self.analyze()
            && let Some(info) = analysis.expressions.get(&source)
        {
            return Some(info.value_type);
        }
        let ir = MaterialCompiler
            .compile_with_functions(self, functions)
            .ok()?;
        ir.value(*ir.source_map.values.get(&source)?)
            .map(|value| value.value_type)
    }
    fn texture_reference(&self) -> Option<MaterialExpressionKind> {
        self.parameters
            .iter()
            .find(|p| matches!(p.value_type, MaterialValueType::Texture2D(_)))
            .map(|p| MaterialExpressionKind::Parameter(p.id))
    }

    fn palette_reference(&mut self) -> Option<MaterialExpressionKind> {
        Some(append_texture_parameter(
            self,
            "Palette",
            MaterialTextureColorSpace::SrgbColor,
        ))
    }

    fn palette_swap_references(&mut self) -> Option<[MaterialExpressionKind; 2]> {
        Some(["Source Palette", "Target Palette"].map(|label| {
            append_texture_parameter(self, label, MaterialTextureColorSpace::SrgbColor)
        }))
    }

    fn pattern_reference(&mut self) -> Option<MaterialExpressionKind> {
        Some(append_texture_parameter(
            self,
            "Dither Pattern",
            MaterialTextureColorSpace::LinearData,
        ))
    }
    fn outline_texture_reference(&mut self) -> Option<MaterialExpressionKind> {
        self.texture_reference().or_else(|| {
            Some(append_texture_parameter(
                self,
                "Outline Texture",
                MaterialTextureColorSpace::SrgbColor,
            ))
        })
    }
}

fn append_texture_parameter(
    program: &mut MaterialProgram,
    label: &str,
    color_space: MaterialTextureColorSpace,
) -> MaterialExpressionKind {
    let names = program
        .parameters
        .iter()
        .map(|parameter| parameter.name.as_str())
        .collect::<BTreeSet<_>>();
    let name = std::iter::once(label.to_owned())
        .chain((2..).map(|index| format!("{label} {index}")))
        .find(|name| !names.contains(name.as_str()))
        .expect("one texture parameter name is available");
    let id = MaterialParameterId::new();
    program.parameters.push(MaterialParameter {
        id,
        name,
        value_type: MaterialValueType::Texture2D(MaterialTextureDescriptor {
            color_space,
            sampler: MaterialSamplerDescriptor {
                filter: MaterialFilterMode::Nearest,
                mip_filter: MaterialMipFilterMode::None,
                ..Default::default()
            },
        }),
        evaluation_domain: MaterialEvaluationDomain::Instance,
        default: None,
    });
    MaterialExpressionKind::Parameter(id)
}
struct FunctionConstruction {
    expressions: Vec<MaterialExpression>,
    inputs: Vec<aestra_core::material::MaterialFunctionInput>,
}
impl ExpressionSink for FunctionConstruction {
    fn expressions_mut(&mut self) -> &mut Vec<MaterialExpression> {
        &mut self.expressions
    }
}
impl GraphConstruction for FunctionConstruction {
    fn source_type(
        &self,
        _: MaterialExpressionId,
        _: &MaterialFunctionLibrary,
    ) -> Option<MaterialValueType> {
        None
    }
    fn texture_reference(&self) -> Option<MaterialExpressionKind> {
        self.inputs
            .iter()
            .find(|input| matches!(input.value_type, MaterialValueType::Texture2D(_)))
            .map(|input| MaterialExpressionKind::FunctionInput(input.id))
    }
    fn palette_reference(&mut self) -> Option<MaterialExpressionKind> {
        self.inputs
            .iter()
            .find(|input| is_srgb_texture(input.value_type))
            .map(|input| MaterialExpressionKind::FunctionInput(input.id))
    }
    fn palette_swap_references(&mut self) -> Option<[MaterialExpressionKind; 2]> {
        let mut palettes = self
            .inputs
            .iter()
            .filter(|input| is_srgb_texture(input.value_type));
        Some([
            MaterialExpressionKind::FunctionInput(palettes.next()?.id),
            MaterialExpressionKind::FunctionInput(palettes.next()?.id),
        ])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialGraphNodeKind {
    Constant,
    Input(MaterialInput),
    Parameter(MaterialParameterId),
    FunctionInput(MaterialFunctionInputId),
    FunctionCall(MaterialFunctionRef),
    Function(MaterialGraphFunction),
    ExtractComponent(MaterialVectorComponent),
}

/// A compiler-owned node kind that can be created by graph authoring clients.
///
/// Keeping this catalog outside the editor gives UI, CLI, and AI clients the same list of
/// constructible nodes and the same typed defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialGraphCreateKind {
    Constant(MaterialValueType),
    Input(MaterialInput),
    Parameter(MaterialParameterId),
    FunctionCall {
        function: MaterialFunctionRef,
        output: MaterialFunctionOutputId,
    },
    Function(MaterialGraphFunction),
    ExtractComponent(MaterialVectorComponent),
}

impl MaterialGraphCreateKind {
    pub const fn consumes_source(self) -> bool {
        matches!(
            self,
            Self::FunctionCall { .. } | Self::Function(_) | Self::ExtractComponent(_)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialGraphNodeDescriptor {
    pub kind: MaterialGraphCreateKind,
    pub label: String,
    pub category: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialGraphNodeCreationPlan {
    pub expression: MaterialExpressionId,
    pub created_expressions: Vec<MaterialExpressionId>,
    pub replacement: MaterialProgram,
}

#[derive(Debug, Error)]
pub enum MaterialGraphNodeCreationError {
    #[error("this node cannot be created in a graph function")]
    FunctionContextUnsupported,
    #[error(transparent)]
    Compile(#[from] MaterialCompileError),
    #[error("source material expression {expression} is unavailable")]
    SourceMissing { expression: MaterialExpressionId },
    #[error("{kind:?} cannot consume material expression {expression}")]
    IncompatibleSource {
        kind: MaterialGraphCreateKind,
        expression: MaterialExpressionId,
    },
    #[error("Sample Texture requires a declared Texture2D material parameter")]
    TextureParameterMissing,
    #[error("Nearest Palette Color requires a declared sRGB Texture2D material parameter")]
    PaletteParameterMissing,
    #[error("texture constants are resources and cannot be created as literal graph nodes")]
    TextureConstantUnsupported,
    #[error("material function {function:?} is unavailable")]
    FunctionUnavailable { function: MaterialFunctionRef },
    #[error("material function {function:?} has no output {output}")]
    FunctionOutputUnavailable {
        function: MaterialFunctionRef,
        output: MaterialFunctionOutputId,
    },
    #[error("material function input '{input}' requires a resource connection")]
    FunctionInputDefaultUnavailable { input: String },
}

const MATERIAL_INPUTS: [MaterialInput; 35] = [
    MaterialInput::ScenePointIrradiance,
    MaterialInput::RibbonUv,
    MaterialInput::RibbonDirection,
    MaterialInput::Uv0,
    MaterialInput::Uv1,
    MaterialInput::LocalPosition,
    MaterialInput::WorldPosition,
    MaterialInput::EffectPositionXY,
    MaterialInput::WorldPositionXY,
    MaterialInput::ScreenPixel,
    MaterialInput::ParticlePositionXY,
    MaterialInput::Normal,
    MaterialInput::Tangent,
    MaterialInput::Bitangent,
    MaterialInput::ViewDirection,
    MaterialInput::ScreenUv,
    MaterialInput::ParticleColor,
    MaterialInput::ParticleOpacity,
    MaterialInput::ParticleAge,
    MaterialInput::ParticleNormalizedAge,
    MaterialInput::ParticleLifetime,
    MaterialInput::ParticleVelocity,
    MaterialInput::ParticleSpeed,
    MaterialInput::ParticleRandom,
    MaterialInput::ParticleId,
    MaterialInput::ParticleSize,
    MaterialInput::ParticleRotation,
    MaterialInput::EffectTime,
    MaterialInput::EmitterTime,
    MaterialInput::EffectNormalizedTime,
    MaterialInput::EmitterNormalizedTime,
    MaterialInput::SceneDepth,
    MaterialInput::CameraPosition,
    MaterialInput::CameraDirection,
    MaterialInput::PixelDepth,
];

const MATERIAL_FUNCTIONS: &[MaterialGraphFunction] = &[
    MaterialGraphFunction::Add,
    MaterialGraphFunction::Subtract,
    MaterialGraphFunction::Multiply,
    MaterialGraphFunction::Divide,
    MaterialGraphFunction::Lerp,
    MaterialGraphFunction::Clamp,
    MaterialGraphFunction::Select,
    MaterialGraphFunction::Remap,
    MaterialGraphFunction::Smoothstep,
    MaterialGraphFunction::Fresnel,
    MaterialGraphFunction::NormalMap,
    MaterialGraphFunction::SpritePointIrradiance,
    MaterialGraphFunction::RadialMask,
    MaterialGraphFunction::Dissolve,
    MaterialGraphFunction::DissolveEdge,
    MaterialGraphFunction::DepthFade,
    MaterialGraphFunction::SoftParticle,
    MaterialGraphFunction::PanUv,
    MaterialGraphFunction::RotateUv,
    MaterialGraphFunction::ScaleUv,
    MaterialGraphFunction::DerivativeX,
    MaterialGraphFunction::DerivativeY,
    MaterialGraphFunction::SampleTexture,
    MaterialGraphFunction::SampleTextureLevel,
    MaterialGraphFunction::SampleTextureGradient,
    MaterialGraphFunction::NearestPaletteColor,
    MaterialGraphFunction::PaletteRamp,
    MaterialGraphFunction::PaletteShift,
    MaterialGraphFunction::PaletteSwap,
    MaterialGraphFunction::BayerDither2x2,
    MaterialGraphFunction::BayerDither4x4,
    MaterialGraphFunction::BayerDither8x8,
    MaterialGraphFunction::BlueNoiseDither,
    MaterialGraphFunction::PatternDither,
    MaterialGraphFunction::PixelSnap,
    MaterialGraphFunction::Posterize,
    MaterialGraphFunction::PixelateUv,
    MaterialGraphFunction::Quantize,
    MaterialGraphFunction::Threshold,
    MaterialGraphFunction::PixelOutline,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialGraphPort {
    /// Stable semantic socket name within the node.
    pub name: String,
    /// Stable function signature identity when this is a function-call input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function_input: Option<MaterialFunctionInputId>,
    pub source: MaterialExpressionId,
    /// The connected value's analyzed type. Absent when validation failed.
    pub value_type: Option<MaterialValueType>,
    /// The connected value's analyzed evaluation domain. Absent when validation failed.
    pub evaluation_domain: Option<MaterialExpressionDomain>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialGraphNode {
    pub expression: MaterialExpressionId,
    pub kind: MaterialGraphNodeKind,
    pub label: String,
    pub inputs: Vec<MaterialGraphPort>,
    pub value_type: Option<MaterialValueType>,
    pub evaluation_domain: Option<MaterialExpressionDomain>,
    pub disabled: bool,
    pub reachable: bool,
    /// Function-resolution error associated with this node, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_message: Option<String>,
    /// Optional link into optimized compiler IR. Multiple expressions can alias one IR value.
    pub ir_value: Option<MaterialIrValueId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MaterialGraphOutputKind {
    Color,
    Alpha,
    VertexOffset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialGraphOutput {
    pub kind: MaterialGraphOutputKind,
    /// Nil only for an unconnected optional output; no edge is emitted for that socket.
    pub source: MaterialExpressionId,
    pub value_type: Option<MaterialValueType>,
    pub evaluation_domain: Option<MaterialExpressionDomain>,
    pub ir_value: Option<MaterialIrValueId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialGraphEdgeTarget {
    Input {
        expression: MaterialExpressionId,
        port: String,
    },
    FunctionInput {
        expression: MaterialExpressionId,
        input: MaterialFunctionInputId,
    },
    Output(MaterialGraphOutputKind),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialGraphEdge {
    pub source: MaterialExpressionId,
    pub target: MaterialGraphEdgeTarget,
    pub value_type: Option<MaterialValueType>,
    pub evaluation_domain: Option<MaterialExpressionDomain>,
}

/// Deterministic graph data suitable for editors, CLI tools, and AI inspection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialGraphProjection {
    pub program: MaterialProgramId,
    pub nodes: Vec<MaterialGraphNode>,
    pub edges: Vec<MaterialGraphEdge>,
    pub outputs: Vec<MaterialGraphOutput>,
    pub diagnostics: ValidationReport,
}

impl MaterialCompiler {
    /// Returns the canonical graph-node catalog for one program.
    pub fn graph_node_catalog(
        &self,
        program: &MaterialProgram,
    ) -> Vec<MaterialGraphNodeDescriptor> {
        self.graph_node_catalog_with_functions(program, &MaterialFunctionLibrary::default())
    }

    /// Returns the canonical graph-node catalog, including reusable built-in and project
    /// material functions from the supplied library.
    pub fn graph_node_catalog_with_functions(
        &self,
        program: &MaterialProgram,
        functions: &MaterialFunctionLibrary,
    ) -> Vec<MaterialGraphNodeDescriptor> {
        let mut nodes = self.graph_catalog(&program.parameters, functions);
        if program.domain != MaterialDomain::Sprite {
            nodes.retain(|node| {
                node.kind
                    != MaterialGraphCreateKind::Function(
                        MaterialGraphFunction::SpritePointIrradiance,
                    )
            });
        }
        nodes
    }

    /// Shared catalog for function bodies. Signature inputs are supplied by the function editor.
    pub fn function_graph_node_catalog(
        &self,
        function: &aestra_core::material::MaterialFunction,
        functions: &MaterialFunctionLibrary,
    ) -> Vec<MaterialGraphNodeDescriptor> {
        if function.custom_wesl.is_some() {
            return Vec::new();
        }
        self.graph_catalog(&[], functions).into_iter().filter(|node| {
            !matches!(node.kind, MaterialGraphCreateKind::FunctionCall { function: MaterialFunctionRef::Project(id), .. } if id == function.id)
                && self.function_graph_node_expressions(function, node.kind, functions).is_ok()
        }).collect()
    }

    /// Constructs function-native expressions using the same defaults as material nodes.
    /// The authoring transaction must validate the replacement and its dependent callers.
    pub fn function_graph_node_expressions(
        &self,
        function: &aestra_core::material::MaterialFunction,
        kind: MaterialGraphCreateKind,
        functions: &MaterialFunctionLibrary,
    ) -> Result<Vec<MaterialExpression>, MaterialGraphNodeCreationError> {
        if function.custom_wesl.is_some() {
            return Err(MaterialGraphNodeCreationError::FunctionContextUnsupported);
        }
        let mut builder = FunctionConstruction {
            expressions: function.expressions.clone(),
            inputs: function.inputs.clone(),
        };
        let first = builder.expressions.len();
        match kind {
            MaterialGraphCreateKind::Constant(ty) => {
                let value = default_value(ty, false)
                    .ok_or(MaterialGraphNodeCreationError::TextureConstantUnsupported)?;
                append_graph_constant(&mut builder, value);
            }
            MaterialGraphCreateKind::Input(input) => {
                append_graph_expression(&mut builder, MaterialExpressionKind::Input(input));
            }
            MaterialGraphCreateKind::Function(operation) => {
                append_graph_function(&mut builder, operation, None, functions)?;
            }
            MaterialGraphCreateKind::ExtractComponent(component) => {
                let value =
                    append_graph_constant(&mut builder, MaterialValue::Vec4([0.0, 0.0, 0.0, 1.0]));
                append_graph_expression(
                    &mut builder,
                    MaterialExpressionKind::ExtractComponent { value, component },
                );
            }
            MaterialGraphCreateKind::FunctionCall {
                function: reference,
                output,
            } if reference != MaterialFunctionRef::Project(function.id) => {
                append_graph_function_call(&mut builder, functions, reference, output, None)?;
            }
            _ => return Err(MaterialGraphNodeCreationError::FunctionContextUnsupported),
        }
        Ok(builder.expressions.split_off(first))
    }

    fn graph_catalog(
        &self,
        parameters: &[aestra_core::material::MaterialParameter],
        functions: &MaterialFunctionLibrary,
    ) -> Vec<MaterialGraphNodeDescriptor> {
        let mut nodes = [
            MaterialValueType::Float,
            MaterialValueType::Vec2,
            MaterialValueType::Vec3,
            MaterialValueType::Vec4,
            MaterialValueType::Color,
            MaterialValueType::Bool,
        ]
        .into_iter()
        .map(|value_type| MaterialGraphNodeDescriptor {
            kind: MaterialGraphCreateKind::Constant(value_type),
            label: constant_label(value_type).to_owned(),
            category: "Constants".to_owned(),
        })
        .collect::<Vec<_>>();
        nodes.extend(
            MATERIAL_INPUTS
                .into_iter()
                .map(|input| MaterialGraphNodeDescriptor {
                    kind: MaterialGraphCreateKind::Input(input),
                    label: format!("{input:?}"),
                    category: "Inputs".to_owned(),
                }),
        );
        nodes.extend(
            parameters
                .iter()
                .map(|parameter| MaterialGraphNodeDescriptor {
                    kind: MaterialGraphCreateKind::Parameter(parameter.id),
                    label: parameter.name.clone(),
                    category: "Parameters".to_owned(),
                }),
        );
        nodes.extend(MATERIAL_FUNCTIONS.iter().copied().map(|function| {
            MaterialGraphNodeDescriptor {
                kind: MaterialGraphCreateKind::Function(function),
                label: function.display_name().to_owned(),
                category: function.category().to_owned(),
            }
        }));
        nodes.extend(
            functions
                .iter_with_references()
                .flat_map(|(reference, function)| {
                    function
                        .outputs
                        .iter()
                        .map(move |output| MaterialGraphNodeDescriptor {
                            kind: MaterialGraphCreateKind::FunctionCall {
                                function: reference,
                                output: output.id,
                            },
                            label: if function.outputs.len() == 1 {
                                function.name.clone()
                            } else {
                                format!("{} · {}", function.name, output.name)
                            },
                            category: match reference {
                                MaterialFunctionRef::BuiltIn(_)
                                    if functions.builtin_noise(reference).is_some() =>
                                {
                                    "Noise".to_owned()
                                }
                                MaterialFunctionRef::BuiltIn(_) => "Built-in Functions".to_owned(),
                                MaterialFunctionRef::Project(_) => "Project Functions".to_owned(),
                            },
                        })
                }),
        );
        nodes.extend(
            [
                MaterialVectorComponent::X,
                MaterialVectorComponent::Y,
                MaterialVectorComponent::Z,
                MaterialVectorComponent::W,
            ]
            .into_iter()
            .map(|component| MaterialGraphNodeDescriptor {
                kind: MaterialGraphCreateKind::ExtractComponent(component),
                label: format!("Extract {component:?}"),
                category: "Math".to_owned(),
            }),
        );
        nodes
    }

    /// Builds one graph node and any typed default expressions it requires without rewiring a
    /// consumer. The returned replacement is fully compiler-validated.
    pub fn plan_graph_node_creation(
        &self,
        program: &MaterialProgram,
        kind: MaterialGraphCreateKind,
        source: Option<MaterialExpressionId>,
    ) -> Result<MaterialGraphNodeCreationPlan, MaterialGraphNodeCreationError> {
        self.plan_graph_node_creation_with_functions(
            program,
            kind,
            source,
            &MaterialFunctionLibrary::default(),
        )
    }

    /// Builds a graph node using the complete reusable function environment.
    pub fn plan_graph_node_creation_with_functions(
        &self,
        program: &MaterialProgram,
        kind: MaterialGraphCreateKind,
        source: Option<MaterialExpressionId>,
        functions: &MaterialFunctionLibrary,
    ) -> Result<MaterialGraphNodeCreationPlan, MaterialGraphNodeCreationError> {
        if let Some(source) = source
            && !program
                .expressions
                .iter()
                .any(|expression| expression.id == source)
        {
            return Err(MaterialGraphNodeCreationError::SourceMissing { expression: source });
        }
        let mut replacement = program.clone();
        let first_created = replacement.expressions.len();
        let expression = match kind {
            MaterialGraphCreateKind::Constant(value_type) => {
                let value = default_value(value_type, false)
                    .ok_or(MaterialGraphNodeCreationError::TextureConstantUnsupported)?;
                append_graph_expression(&mut replacement, MaterialExpressionKind::Constant(value))
            }
            MaterialGraphCreateKind::Input(input) => {
                append_graph_expression(&mut replacement, MaterialExpressionKind::Input(input))
            }
            MaterialGraphCreateKind::Parameter(parameter) => append_graph_expression(
                &mut replacement,
                MaterialExpressionKind::Parameter(parameter),
            ),
            MaterialGraphCreateKind::FunctionCall { function, output } => {
                append_graph_function_call(&mut replacement, functions, function, output, source)?
            }
            MaterialGraphCreateKind::Function(function) => {
                append_graph_function(&mut replacement, function, source, functions)?
            }
            MaterialGraphCreateKind::ExtractComponent(component) => {
                let value = source.unwrap_or_else(|| {
                    append_graph_constant(
                        &mut replacement,
                        MaterialValue::Vec4([0.0, 0.0, 0.0, 1.0]),
                    )
                });
                append_graph_expression(
                    &mut replacement,
                    MaterialExpressionKind::ExtractComponent { value, component },
                )
            }
        };
        // Default-input constants are single-use, so they inline onto their consuming socket
        // automatically (see `MaterialProgram::inline_constants`); nothing is recorded here.
        self.compile_with_functions(&replacement, functions)?;
        let created_expressions = replacement.expressions[first_created..]
            .iter()
            .map(|expression| expression.id)
            .collect();
        Ok(MaterialGraphNodeCreationPlan {
            expression,
            created_expressions,
            replacement,
        })
    }

    /// Projects every authored expression, including disabled and unreachable expressions.
    ///
    /// Projection never fails: invalid programs remain inspectable, while ports whose types could
    /// not be proven carry `None`. Passing compiled IR adds source-map links without making the
    /// graph dependent on a renderer backend.
    pub fn project_graph(
        &self,
        program: &MaterialProgram,
        ir: Option<&MaterialIrProgram>,
    ) -> MaterialGraphProjection {
        self.project_graph_with_functions(program, ir, &MaterialFunctionLibrary::default())
    }

    /// Projects a program with signature-aware material-function nodes and ports.
    pub fn project_graph_with_functions(
        &self,
        program: &MaterialProgram,
        ir: Option<&MaterialIrProgram>,
        functions: &MaterialFunctionLibrary,
    ) -> MaterialGraphProjection {
        let program = program.normalized();
        let mut diagnostics = program.validation_report();
        diagnostics
            .diagnostics
            .extend(functions.validation_report().diagnostics);
        if let Err(MaterialCompileError::Validation(report)) =
            self.compile_with_functions(&program, functions)
        {
            diagnostics.diagnostics.extend(report.diagnostics);
        }
        diagnostics.diagnostics.sort();
        diagnostics.diagnostics.dedup();
        let analysis = program.analyze().ok();
        let info = analysis
            .as_ref()
            .map(|analysis| &analysis.expressions)
            .cloned()
            .unwrap_or_default();
        let reachable = reachable_expressions(&program);
        let disabled = program
            .disabled_expressions
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let ir_values = ir
            .filter(|ir| ir.source == program.id)
            .map(|ir| &ir.source_map.values);

        let inline_constants = program.inline_constants();
        let nodes = program
            .expressions
            .iter()
            .filter(|expression| !inline_constants.contains(&expression.id))
            .map(|expression| {
                let inputs = expression_ports(&expression.kind, functions)
                    .into_iter()
                    .map(|(name, source, function_input)| {
                        let source_info = info.get(&source);
                        let ir_info = ir_values
                            .and_then(|values| values.get(&source))
                            .and_then(|value| ir.and_then(|ir| ir.value(*value)));
                        let signature_type = function_input
                            .and_then(|input| {
                                let MaterialExpressionKind::FunctionCall { function, .. } =
                                    &expression.kind
                                else {
                                    return None;
                                };
                                functions
                                    .get(*function)
                                    .and_then(|function| {
                                        function
                                            .inputs
                                            .iter()
                                            .find(|candidate| candidate.id == input)
                                    })
                                    .map(|input| input.value_type)
                            })
                            .or_else(|| {
                                (name == "frame_bounds").then_some(MaterialValueType::Vec4)
                            });
                        MaterialGraphPort {
                            name,
                            function_input,
                            source,
                            value_type: signature_type.or_else(|| {
                                source_info
                                    .map(|info| info.value_type)
                                    .or_else(|| ir_info.map(|info| info.value_type))
                            }),
                            evaluation_domain: source_info
                                .map(|info| info.evaluation_domain)
                                .or_else(|| ir_info.map(|info| info.evaluation_domain)),
                        }
                    })
                    .collect();
                let expression_info = info.get(&expression.id);
                let ir_info = ir_values
                    .and_then(|values| values.get(&expression.id))
                    .and_then(|value| ir.and_then(|ir| ir.value(*value)));
                let signature_type = match &expression.kind {
                    MaterialExpressionKind::FunctionCall {
                        function, output, ..
                    } => functions
                        .get(*function)
                        .and_then(|function| {
                            function
                                .outputs
                                .iter()
                                .find(|candidate| candidate.id == *output)
                        })
                        .map(|output| output.value_type),
                    _ => None,
                };
                let validation_message = match &expression.kind {
                    MaterialExpressionKind::FunctionCall { function, .. } => diagnostics
                        .diagnostics
                        .iter()
                        .find(|diagnostic| {
                            let id = function.id().to_string();
                            diagnostic.path.contains(&expression.id.to_string())
                                || diagnostic.path.contains(&id)
                                || diagnostic.message.contains(&id)
                        })
                        .map(|diagnostic| diagnostic.message.clone()),
                    _ => None,
                };
                MaterialGraphNode {
                    expression: expression.id,
                    kind: node_kind(&expression.kind),
                    label: node_label(&expression.kind, &program, functions),
                    inputs,
                    value_type: signature_type.or_else(|| {
                        expression_info
                            .map(|info| info.value_type)
                            .or_else(|| ir_info.map(|info| info.value_type))
                    }),
                    evaluation_domain: expression_info
                        .map(|info| info.evaluation_domain)
                        .or_else(|| ir_info.map(|info| info.evaluation_domain)),
                    disabled: disabled.contains(&expression.id),
                    reachable: reachable.contains(&expression.id),
                    validation_message,
                    ir_value: ir_values
                        .and_then(|values| values.get(&expression.id))
                        .copied(),
                }
            })
            .collect::<Vec<_>>();

        let mut edges = nodes
            .iter()
            .flat_map(|node| {
                node.inputs
                    .iter()
                    .filter(|port| {
                        !port.source.is_nil() && !inline_constants.contains(&port.source)
                    })
                    .map(|port| MaterialGraphEdge {
                        source: port.source,
                        target: port.function_input.map_or_else(
                            || MaterialGraphEdgeTarget::Input {
                                expression: node.expression,
                                port: port.name.clone(),
                            },
                            |input| MaterialGraphEdgeTarget::FunctionInput {
                                expression: node.expression,
                                input,
                            },
                        ),
                        value_type: port.value_type,
                        evaluation_domain: port.evaluation_domain,
                    })
            })
            .collect::<Vec<_>>();
        let outputs = [
            (MaterialGraphOutputKind::Color, program.outputs.color),
            (MaterialGraphOutputKind::Alpha, program.outputs.alpha),
        ]
        .into_iter()
        .chain(
            (program.domain == aestra_core::material::MaterialDomain::Mesh).then_some((
                MaterialGraphOutputKind::VertexOffset,
                program
                    .outputs
                    .vertex_offset
                    .unwrap_or(MaterialExpressionId::from_u128(0)),
            )),
        )
        .map(|(kind, source)| {
            let source_info = info.get(&source);
            let ir_info = ir_values
                .and_then(|values| values.get(&source))
                .and_then(|value| ir.and_then(|ir| ir.value(*value)));
            MaterialGraphOutput {
                kind,
                source,
                value_type: source_info
                    .map(|info| info.value_type)
                    .or_else(|| ir_info.map(|info| info.value_type))
                    .or_else(|| {
                        (kind == MaterialGraphOutputKind::VertexOffset)
                            .then_some(MaterialValueType::Vec3)
                    }),
                evaluation_domain: source_info
                    .map(|info| info.evaluation_domain)
                    .or_else(|| ir_info.map(|info| info.evaluation_domain)),
                ir_value: ir_values.and_then(|values| values.get(&source)).copied(),
            }
        })
        .collect::<Vec<_>>();
        edges.extend(
            outputs
                .iter()
                .filter(|output| !output.source.is_nil())
                .map(|output| MaterialGraphEdge {
                    source: output.source,
                    target: MaterialGraphEdgeTarget::Output(output.kind),
                    value_type: output.value_type,
                    evaluation_domain: output.evaluation_domain,
                }),
        );

        MaterialGraphProjection {
            program: program.id,
            nodes,
            edges,
            outputs,
            diagnostics,
        }
    }
}

fn constant_label(value_type: MaterialValueType) -> &'static str {
    match value_type {
        MaterialValueType::Float => "Float",
        MaterialValueType::Vec2 => "Vector 2",
        MaterialValueType::Vec3 => "Vector 3",
        MaterialValueType::Vec4 => "Vector 4",
        MaterialValueType::Color => "Color",
        MaterialValueType::Bool => "Boolean",
        MaterialValueType::Texture2D(_) => "Texture",
    }
}

fn append_graph_function_call(
    program: &mut impl GraphConstruction,
    functions: &MaterialFunctionLibrary,
    reference: MaterialFunctionRef,
    output: MaterialFunctionOutputId,
    source: Option<MaterialExpressionId>,
) -> Result<MaterialExpressionId, MaterialGraphNodeCreationError> {
    let function =
        functions
            .get(reference)
            .ok_or(MaterialGraphNodeCreationError::FunctionUnavailable {
                function: reference,
            })?;
    if !function
        .outputs
        .iter()
        .any(|candidate| candidate.id == output)
    {
        return Err(MaterialGraphNodeCreationError::FunctionOutputUnavailable {
            function: reference,
            output,
        });
    }
    let inputs = function.inputs.clone();
    let source_type = source.and_then(|source| program.source_type(source, functions));
    let source_input = source.and_then(|_| {
        source_type.and_then(|source_type| {
            inputs
                .iter()
                .find(|input| input.value_type == source_type)
                .map(|input| input.id)
        })
    });
    if let Some(source) = source
        && source_input.is_none()
    {
        return Err(MaterialGraphNodeCreationError::IncompatibleSource {
            kind: MaterialGraphCreateKind::FunctionCall {
                function: reference,
                output,
            },
            expression: source,
        });
    }

    let mut arguments = BTreeMap::new();
    for input in inputs {
        let expression = if Some(input.id) == source_input {
            source.expect("a selected function input requires a source")
        } else if input.id == aestra_core::noise::POSITION
            && functions.builtin_noise(reference).is_some()
        {
            let noise = functions.builtin_noise(reference).unwrap();
            append_graph_expression(
                program,
                MaterialExpressionKind::Input(if noise.dimensions == 2 {
                    MaterialInput::Uv0
                } else {
                    MaterialInput::WorldPosition
                }),
            )
        } else {
            let value = input
                .default
                .clone()
                .or_else(|| default_value(input.value_type, false))
                .ok_or_else(
                    || MaterialGraphNodeCreationError::FunctionInputDefaultUnavailable {
                        input: input.name.clone(),
                    },
                )?;
            append_graph_constant(program, value)
        };
        arguments.insert(input.id, expression);
    }
    Ok(append_graph_expression(
        program,
        MaterialExpressionKind::FunctionCall {
            function: reference,
            arguments,
            output,
        },
    ))
}

fn append_graph_function<P: GraphConstruction>(
    program: &mut P,
    function: MaterialGraphFunction,
    source: Option<MaterialExpressionId>,
    functions: &MaterialFunctionLibrary,
) -> Result<MaterialExpressionId, MaterialGraphNodeCreationError> {
    let source_type = source.and_then(|source| program.source_type(source, functions));
    let numeric_source = |program: &mut P| {
        source.unwrap_or_else(|| append_graph_constant(program, MaterialValue::Float(0.0)))
    };
    let expression = match function {
        MaterialGraphFunction::Add | MaterialGraphFunction::Subtract => {
            let left = numeric_source(program);
            let right = append_graph_constant(
                program,
                default_value(source_type.unwrap_or(MaterialValueType::Float), false)
                    .unwrap_or(MaterialValue::Float(0.0)),
            );
            if function == MaterialGraphFunction::Add {
                MaterialExpressionKind::Add(left, right)
            } else {
                MaterialExpressionKind::Subtract(left, right)
            }
        }
        MaterialGraphFunction::Multiply | MaterialGraphFunction::Divide => {
            let left = numeric_source(program);
            let right = append_graph_constant(program, MaterialValue::Float(1.0));
            if function == MaterialGraphFunction::Multiply {
                MaterialExpressionKind::Multiply(left, right)
            } else {
                MaterialExpressionKind::Divide(left, right)
            }
        }
        MaterialGraphFunction::Lerp => {
            let start = numeric_source(program);
            let end = append_graph_constant(
                program,
                default_value(source_type.unwrap_or(MaterialValueType::Float), true)
                    .unwrap_or(MaterialValue::Float(1.0)),
            );
            let factor = append_graph_constant(program, MaterialValue::Float(0.5));
            MaterialExpressionKind::Lerp { start, end, factor }
        }
        MaterialGraphFunction::Clamp => {
            let value = numeric_source(program);
            let value_type = source_type.unwrap_or(MaterialValueType::Float);
            let min = append_graph_constant(
                program,
                default_value(value_type, false).unwrap_or(MaterialValue::Float(0.0)),
            );
            let max = append_graph_constant(
                program,
                default_value(value_type, true).unwrap_or(MaterialValue::Float(1.0)),
            );
            MaterialExpressionKind::Clamp { value, min, max }
        }
        MaterialGraphFunction::Select => {
            if matches!(source_type, Some(MaterialValueType::Texture2D(_))) {
                return Err(MaterialGraphNodeCreationError::IncompatibleSource {
                    kind: MaterialGraphCreateKind::Function(function),
                    expression: source.expect("source type requires a source"),
                });
            }
            let value_type = source_type.unwrap_or(MaterialValueType::Float);
            let if_false = source.unwrap_or_else(|| {
                append_graph_constant(
                    program,
                    default_value(value_type, false).unwrap_or(MaterialValue::Float(0.0)),
                )
            });
            let if_true = append_graph_constant(
                program,
                default_value(value_type, true).unwrap_or(MaterialValue::Float(1.0)),
            );
            let condition = append_graph_constant(program, MaterialValue::Bool(false));
            MaterialExpressionKind::Select {
                condition,
                if_false,
                if_true,
            }
        }
        MaterialGraphFunction::NormalMap => {
            let sample = source.unwrap_or_else(|| {
                append_graph_constant(program, MaterialValue::Vec3([0.5, 0.5, 1.0]))
            });
            let strength = append_graph_constant(program, MaterialValue::Float(1.0));
            let flip_y = append_graph_constant(program, MaterialValue::Bool(false));
            let local_basis = program.uses_local_normal_basis();
            let [normal, tangent, bitangent] = [
                (MaterialInput::Normal, [0.0, 0.0, 1.0]),
                (MaterialInput::Tangent, [1.0, 0.0, 0.0]),
                (MaterialInput::Bitangent, [0.0, 1.0, 0.0]),
            ]
            .map(|(input, axis)| {
                append_graph_expression(
                    program,
                    if local_basis {
                        MaterialExpressionKind::Constant(MaterialValue::Vec3(axis))
                    } else {
                        MaterialExpressionKind::Input(input)
                    },
                )
            });
            MaterialExpressionKind::NormalMap {
                sample,
                strength,
                flip_y,
                normal,
                tangent,
                bitangent,
            }
        }
        MaterialGraphFunction::SpritePointIrradiance => {
            if let Some(expression) = source
                && source_type != Some(MaterialValueType::Vec3)
            {
                return Err(MaterialGraphNodeCreationError::IncompatibleSource {
                    kind: MaterialGraphCreateKind::Function(function),
                    expression,
                });
            }
            MaterialExpressionKind::SpritePointIrradiance {
                normal: source.unwrap_or_else(|| {
                    append_graph_constant(program, MaterialValue::Vec3([0.0, 0.0, 1.0]))
                }),
            }
        }
        MaterialGraphFunction::Fresnel => {
            let normal = source.unwrap_or_else(|| {
                append_graph_expression(
                    program,
                    MaterialExpressionKind::Input(MaterialInput::Normal),
                )
            });
            let view = append_graph_expression(
                program,
                MaterialExpressionKind::Input(MaterialInput::ViewDirection),
            );
            let power = append_graph_constant(program, MaterialValue::Float(5.0));
            MaterialExpressionKind::Fresnel {
                normal,
                view,
                power,
            }
        }
        MaterialGraphFunction::DepthFade => {
            let scene_depth = source.unwrap_or_else(|| {
                append_graph_expression(
                    program,
                    MaterialExpressionKind::Input(MaterialInput::SceneDepth),
                )
            });
            let pixel_depth = append_graph_expression(
                program,
                MaterialExpressionKind::Input(MaterialInput::PixelDepth),
            );
            let fade_distance = append_graph_constant(program, MaterialValue::Float(0.5));
            let invert = append_graph_constant(program, MaterialValue::Bool(false));
            MaterialExpressionKind::DepthFade {
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            }
        }
        MaterialGraphFunction::DerivativeX | MaterialGraphFunction::DerivativeY => {
            let value = source.unwrap_or_else(|| {
                append_graph_expression(program, MaterialExpressionKind::Input(MaterialInput::Uv0))
            });
            if function == MaterialGraphFunction::DerivativeX {
                MaterialExpressionKind::DerivativeX { value }
            } else {
                MaterialExpressionKind::DerivativeY { value }
            }
        }
        MaterialGraphFunction::SampleTexture => {
            let texture_reference = program
                .texture_reference()
                .ok_or(MaterialGraphNodeCreationError::TextureParameterMissing)?;
            let default_texture = || texture_reference.clone();
            let (texture, uv) = match source_type {
                Some(MaterialValueType::Texture2D(_)) => (
                    source.expect("source type requires a source"),
                    append_graph_expression(
                        program,
                        MaterialExpressionKind::Input(MaterialInput::Uv0),
                    ),
                ),
                _ => (
                    append_graph_expression(program, default_texture()),
                    source.unwrap_or_else(|| {
                        append_graph_expression(
                            program,
                            MaterialExpressionKind::Input(MaterialInput::Uv0),
                        )
                    }),
                ),
            };
            MaterialExpressionKind::SampleTexture { texture, uv }
        }
        MaterialGraphFunction::SampleTextureLevel => {
            let texture_reference = program
                .texture_reference()
                .ok_or(MaterialGraphNodeCreationError::TextureParameterMissing)?;
            let (texture, uv) = match source_type {
                Some(MaterialValueType::Texture2D(_)) => (
                    source.expect("source type requires a source"),
                    append_graph_expression(
                        program,
                        MaterialExpressionKind::Input(MaterialInput::Uv0),
                    ),
                ),
                _ => (
                    append_graph_expression(program, texture_reference.clone()),
                    source.unwrap_or_else(|| {
                        append_graph_expression(
                            program,
                            MaterialExpressionKind::Input(MaterialInput::Uv0),
                        )
                    }),
                ),
            };
            let level = append_graph_constant(program, MaterialValue::Float(0.0));
            MaterialExpressionKind::SampleTextureLevel { texture, uv, level }
        }
        MaterialGraphFunction::SampleTextureGradient => {
            let texture_reference = program
                .texture_reference()
                .ok_or(MaterialGraphNodeCreationError::TextureParameterMissing)?;
            let (texture, uv) = match source_type {
                Some(MaterialValueType::Texture2D(_)) => (
                    source.expect("source type requires a source"),
                    append_graph_expression(
                        program,
                        MaterialExpressionKind::Input(MaterialInput::Uv0),
                    ),
                ),
                Some(MaterialValueType::Vec2) => (
                    append_graph_expression(program, texture_reference.clone()),
                    source.expect("source type requires a source"),
                ),
                Some(_) => {
                    return Err(MaterialGraphNodeCreationError::IncompatibleSource {
                        kind: MaterialGraphCreateKind::Function(function),
                        expression: source.expect("source type requires a source"),
                    });
                }
                None => (
                    append_graph_expression(program, texture_reference.clone()),
                    append_graph_expression(
                        program,
                        MaterialExpressionKind::Input(MaterialInput::Uv0),
                    ),
                ),
            };
            let ddx =
                append_graph_expression(program, MaterialExpressionKind::DerivativeX { value: uv });
            let ddy =
                append_graph_expression(program, MaterialExpressionKind::DerivativeY { value: uv });
            MaterialExpressionKind::SampleTextureGradient {
                texture,
                uv,
                ddx,
                ddy,
            }
        }
        MaterialGraphFunction::PixelOutline => {
            let from_texture = matches!(source_type, Some(MaterialValueType::Texture2D(_)));
            if let Some(expression) = source
                && !from_texture
                && source_type != Some(MaterialValueType::Vec2)
            {
                return Err(MaterialGraphNodeCreationError::IncompatibleSource {
                    kind: MaterialGraphCreateKind::Function(function),
                    expression,
                });
            }
            let texture = if from_texture {
                source.expect("source type requires a source")
            } else {
                let reference = program
                    .outline_texture_reference()
                    .ok_or(MaterialGraphNodeCreationError::TextureParameterMissing)?;
                append_graph_expression(program, reference)
            };
            let uv = if source_type == Some(MaterialValueType::Vec2) {
                source.expect("source type requires a source")
            } else {
                append_graph_expression(program, MaterialExpressionKind::Input(MaterialInput::Uv0))
            };
            MaterialExpressionKind::PixelOutline {
                texture,
                uv,
                width: append_graph_constant(program, MaterialValue::Float(1.0)),
                threshold: append_graph_constant(program, MaterialValue::Float(0.5)),
                frame_bounds: None,
            }
        }
        MaterialGraphFunction::PixelSnap
        | MaterialGraphFunction::Posterize
        | MaterialGraphFunction::PixelateUv
        | MaterialGraphFunction::Quantize
        | MaterialGraphFunction::Threshold => {
            use MaterialExpressionKind::{Constant, Input};
            let (default, value_type, control) = match function {
                MaterialGraphFunction::PixelSnap => (
                    Input(MaterialInput::EffectPositionXY),
                    MaterialValueType::Vec2,
                    MaterialValue::Float(1.0),
                ),
                MaterialGraphFunction::Posterize => (
                    Input(MaterialInput::ParticleColor),
                    MaterialValueType::Color,
                    MaterialValue::Float(4.0),
                ),
                MaterialGraphFunction::PixelateUv => (
                    Input(MaterialInput::Uv0),
                    MaterialValueType::Vec2,
                    MaterialValue::Vec2([16.0, 16.0]),
                ),
                MaterialGraphFunction::Quantize => (
                    Constant(MaterialValue::Float(0.0)),
                    MaterialValueType::Float,
                    MaterialValue::Float(0.25),
                ),
                _ => (
                    Constant(MaterialValue::Float(0.0)),
                    MaterialValueType::Float,
                    MaterialValue::Float(0.5),
                ),
            };
            if let Some(expression) = source
                && source_type != Some(value_type)
            {
                return Err(MaterialGraphNodeCreationError::IncompatibleSource {
                    kind: MaterialGraphCreateKind::Function(function),
                    expression,
                });
            }
            let value = source.unwrap_or_else(|| append_graph_expression(program, default));
            let control = append_graph_constant(program, control);
            match function {
                MaterialGraphFunction::PixelSnap => MaterialExpressionKind::PixelSnap {
                    position: value,
                    cell_size: control,
                },
                MaterialGraphFunction::Posterize => MaterialExpressionKind::Posterize {
                    color: value,
                    levels: control,
                },
                MaterialGraphFunction::PixelateUv => MaterialExpressionKind::PixelateUv {
                    uv: value,
                    cells: control,
                },
                MaterialGraphFunction::Quantize => MaterialExpressionKind::Quantize {
                    value,
                    step: control,
                },
                _ => MaterialExpressionKind::Threshold {
                    value,
                    threshold: control,
                },
            }
        }
        MaterialGraphFunction::BayerDither2x2
        | MaterialGraphFunction::BayerDither4x4
        | MaterialGraphFunction::BayerDither8x8
        | MaterialGraphFunction::BlueNoiseDither
        | MaterialGraphFunction::PatternDither => {
            let is_pattern = function == MaterialGraphFunction::PatternDither;
            let texture_source =
                is_pattern && matches!(source_type, Some(MaterialValueType::Texture2D(_)));
            if let Some(expression) = source
                && !texture_source
                && !matches!(
                    source_type,
                    Some(MaterialValueType::Float | MaterialValueType::Vec2)
                )
            {
                return Err(MaterialGraphNodeCreationError::IncompatibleSource {
                    kind: MaterialGraphCreateKind::Function(function),
                    expression,
                });
            }
            let alpha = if source_type == Some(MaterialValueType::Float) {
                source.expect("source type requires a source")
            } else {
                append_graph_constant(program, MaterialValue::Float(0.5))
            };
            let position = if source_type == Some(MaterialValueType::Vec2) {
                source.expect("source type requires a source")
            } else {
                append_graph_expression(
                    program,
                    MaterialExpressionKind::Input(MaterialInput::EffectPositionXY),
                )
            };
            let cell_size = append_graph_constant(program, MaterialValue::Float(1.0));
            if is_pattern {
                let texture = if texture_source {
                    source.expect("source type requires a source")
                } else {
                    let reference = program
                        .pattern_reference()
                        .ok_or(MaterialGraphNodeCreationError::TextureParameterMissing)?;
                    append_graph_expression(program, reference)
                };
                MaterialExpressionKind::PatternDither {
                    texture,
                    alpha,
                    position,
                    cell_size,
                }
            } else {
                MaterialExpressionKind::dither(
                    function.dither_pattern().unwrap(),
                    alpha,
                    position,
                    cell_size,
                )
            }
        }
        MaterialGraphFunction::NearestPaletteColor
        | MaterialGraphFunction::PaletteRamp
        | MaterialGraphFunction::PaletteShift => {
            let (input_type, default) = match function {
                MaterialGraphFunction::PaletteRamp => {
                    (MaterialValueType::Float, MaterialValue::Float(0.5))
                }
                _ => (MaterialValueType::Color, MaterialValue::ColorSrgb([1.0; 4])),
            };
            let from_steps = function == MaterialGraphFunction::PaletteShift
                && source_type == Some(MaterialValueType::Float);
            if let Some(expression) = source
                && !source_type.is_some_and(|ty| ty == input_type || is_srgb_texture(ty))
                && !from_steps
            {
                return Err(MaterialGraphNodeCreationError::IncompatibleSource {
                    kind: MaterialGraphCreateKind::Function(function),
                    expression,
                });
            }
            let palette = if source_type.is_some_and(is_srgb_texture) {
                source.expect("source type requires a source")
            } else {
                let reference = program
                    .palette_reference()
                    .ok_or(MaterialGraphNodeCreationError::PaletteParameterMissing)?;
                append_graph_expression(program, reference)
            };
            let input = if source_type == Some(input_type) {
                source.expect("source type requires a source")
            } else {
                append_graph_constant(program, default)
            };
            match function {
                MaterialGraphFunction::PaletteRamp => MaterialExpressionKind::PaletteRamp {
                    palette,
                    factor: input,
                },
                MaterialGraphFunction::PaletteShift => MaterialExpressionKind::PaletteShift {
                    palette,
                    color: input,
                    steps: if from_steps {
                        source.expect("source type requires a source")
                    } else {
                        append_graph_constant(program, MaterialValue::Float(0.0))
                    },
                },
                _ => MaterialExpressionKind::NearestPaletteColor {
                    palette,
                    color: input,
                },
            }
        }
        MaterialGraphFunction::PaletteSwap => {
            if let Some(expression) = source
                && source_type != Some(MaterialValueType::Color)
            {
                return Err(MaterialGraphNodeCreationError::IncompatibleSource {
                    kind: MaterialGraphCreateKind::Function(function),
                    expression,
                });
            }
            let [source_palette, target_palette] = program
                .palette_swap_references()
                .ok_or(MaterialGraphNodeCreationError::PaletteParameterMissing)?;
            MaterialExpressionKind::PaletteSwap {
                source_palette: append_graph_expression(program, source_palette),
                target_palette: append_graph_expression(program, target_palette),
                color: source.unwrap_or_else(|| {
                    append_graph_constant(program, MaterialValue::ColorSrgb([1.0; 4]))
                }),
            }
        }
        MaterialGraphFunction::ExtractComponent => {
            let value = source.unwrap_or_else(|| {
                append_graph_constant(program, MaterialValue::Vec4([0.0, 0.0, 0.0, 1.0]))
            });
            MaterialExpressionKind::ExtractComponent {
                value,
                component: MaterialVectorComponent::X,
            }
        }
        function => {
            let kind = graph_function_modifier(function)
                .expect("all non-special graph functions map to a stack modifier");
            let source = source.unwrap_or_else(|| default_modifier_source(program, kind));
            return append_default_modifier(program, kind, source).ok_or(
                MaterialGraphNodeCreationError::IncompatibleSource {
                    kind: MaterialGraphCreateKind::Function(function),
                    expression: source,
                },
            );
        }
    };
    Ok(append_graph_expression(program, expression))
}

fn graph_function_modifier(function: MaterialGraphFunction) -> Option<MaterialStackModifierKind> {
    Some(match function {
        MaterialGraphFunction::Remap => MaterialStackModifierKind::Remap,
        MaterialGraphFunction::Smoothstep => MaterialStackModifierKind::Smoothstep,
        MaterialGraphFunction::RadialMask => MaterialStackModifierKind::RadialMask,
        MaterialGraphFunction::Dissolve => MaterialStackModifierKind::Dissolve,
        MaterialGraphFunction::DissolveEdge => MaterialStackModifierKind::DissolveEdge,
        MaterialGraphFunction::SoftParticle => MaterialStackModifierKind::SoftParticle,
        MaterialGraphFunction::PanUv => MaterialStackModifierKind::PanUv,
        MaterialGraphFunction::RotateUv => MaterialStackModifierKind::RotateUv,
        MaterialGraphFunction::ScaleUv => MaterialStackModifierKind::ScaleUv,
        MaterialGraphFunction::Add
        | MaterialGraphFunction::Subtract
        | MaterialGraphFunction::Multiply
        | MaterialGraphFunction::Divide
        | MaterialGraphFunction::Lerp
        | MaterialGraphFunction::Clamp
        | MaterialGraphFunction::Select
        | MaterialGraphFunction::Fresnel
        | MaterialGraphFunction::NormalMap
        | MaterialGraphFunction::SpritePointIrradiance
        | MaterialGraphFunction::DepthFade
        | MaterialGraphFunction::DerivativeX
        | MaterialGraphFunction::DerivativeY
        | MaterialGraphFunction::SampleTexture
        | MaterialGraphFunction::SampleTextureLevel
        | MaterialGraphFunction::SampleTextureGradient
        | MaterialGraphFunction::NearestPaletteColor
        | MaterialGraphFunction::PaletteRamp
        | MaterialGraphFunction::PaletteShift
        | MaterialGraphFunction::PaletteSwap
        | MaterialGraphFunction::BayerDither4x4
        | MaterialGraphFunction::BayerDither2x2
        | MaterialGraphFunction::BayerDither8x8
        | MaterialGraphFunction::BlueNoiseDither
        | MaterialGraphFunction::PatternDither
        | MaterialGraphFunction::PixelSnap
        | MaterialGraphFunction::Posterize
        | MaterialGraphFunction::PixelateUv
        | MaterialGraphFunction::Quantize
        | MaterialGraphFunction::Threshold
        | MaterialGraphFunction::PixelOutline
        | MaterialGraphFunction::ExtractComponent => return None,
    })
}

fn default_modifier_source(
    program: &mut impl ExpressionSink,
    kind: MaterialStackModifierKind,
) -> MaterialExpressionId {
    match kind {
        MaterialStackModifierKind::PanUv
        | MaterialStackModifierKind::RotateUv
        | MaterialStackModifierKind::ScaleUv
        | MaterialStackModifierKind::RadialMask => {
            append_graph_expression(program, MaterialExpressionKind::Input(MaterialInput::Uv0))
        }
        MaterialStackModifierKind::Dissolve
        | MaterialStackModifierKind::DissolveEdge
        | MaterialStackModifierKind::SoftParticle => {
            append_graph_constant(program, MaterialValue::Float(1.0))
        }
        MaterialStackModifierKind::Remap | MaterialStackModifierKind::Smoothstep => {
            append_graph_constant(program, MaterialValue::Float(0.0))
        }
        MaterialStackModifierKind::BaseTexture
        | MaterialStackModifierKind::Fresnel
        | MaterialStackModifierKind::DepthFade => {
            unreachable!("generator modifiers are created by specialized graph factories")
        }
    }
}

fn default_value(value_type: MaterialValueType, one: bool) -> Option<MaterialValue> {
    let scalar = if one { 1.0 } else { 0.0 };
    match value_type {
        MaterialValueType::Float => Some(MaterialValue::Float(scalar)),
        MaterialValueType::Vec2 => Some(MaterialValue::Vec2([scalar; 2])),
        MaterialValueType::Vec3 => Some(MaterialValue::Vec3([scalar; 3])),
        MaterialValueType::Vec4 => Some(MaterialValue::Vec4([scalar; 4])),
        MaterialValueType::Color => Some(MaterialValue::ColorSrgb([scalar, scalar, scalar, 1.0])),
        MaterialValueType::Bool => Some(MaterialValue::Bool(one)),
        MaterialValueType::Texture2D(_) => None,
    }
}

fn append_graph_constant(
    program: &mut impl ExpressionSink,
    value: MaterialValue,
) -> MaterialExpressionId {
    append_graph_expression(program, MaterialExpressionKind::Constant(value))
}

fn append_graph_expression(
    program: &mut impl ExpressionSink,
    kind: MaterialExpressionKind,
) -> MaterialExpressionId {
    let mut id = MaterialExpressionId::new();
    while program
        .expressions_mut()
        .iter()
        .any(|expression| expression.id == id)
    {
        id = MaterialExpressionId::new();
    }
    program
        .expressions_mut()
        .push(MaterialExpression { id, kind });
    id
}

fn reachable_expressions(program: &MaterialProgram) -> BTreeSet<MaterialExpressionId> {
    let expressions = program
        .expressions
        .iter()
        .map(|expression| (expression.id, &expression.kind))
        .collect::<BTreeMap<_, _>>();
    let mut reachable = BTreeSet::new();
    let mut pending = program.outputs.roots().collect::<Vec<_>>();
    while let Some(id) = pending.pop() {
        if !reachable.insert(id) {
            continue;
        }
        if let Some(kind) = expressions.get(&id) {
            pending.extend(expression_inputs(kind).into_iter().map(|(_, id)| id));
        }
    }
    reachable
}

fn node_kind(kind: &MaterialExpressionKind) -> MaterialGraphNodeKind {
    use MaterialExpressionKind as E;
    let function = match kind {
        E::Constant(_) => return MaterialGraphNodeKind::Constant,
        E::Input(input) => return MaterialGraphNodeKind::Input(*input),
        E::Parameter(parameter) => return MaterialGraphNodeKind::Parameter(*parameter),
        E::FunctionInput(input) => return MaterialGraphNodeKind::FunctionInput(*input),
        E::FunctionCall { function, .. } => {
            return MaterialGraphNodeKind::FunctionCall(*function);
        }
        E::CustomWeslCall { function, .. } => {
            return MaterialGraphNodeKind::FunctionCall(MaterialFunctionRef::Project(*function));
        }
        E::Add(..) => MaterialGraphFunction::Add,
        E::Subtract(..) => MaterialGraphFunction::Subtract,
        E::Multiply(..) => MaterialGraphFunction::Multiply,
        E::Divide(..) => MaterialGraphFunction::Divide,
        E::Lerp { .. } => MaterialGraphFunction::Lerp,
        E::Clamp { .. } => MaterialGraphFunction::Clamp,
        E::Select { .. } => MaterialGraphFunction::Select,
        E::Remap { .. } => MaterialGraphFunction::Remap,
        E::Smoothstep { .. } => MaterialGraphFunction::Smoothstep,
        E::NormalMap { .. } => MaterialGraphFunction::NormalMap,
        E::SpritePointIrradiance { .. } => MaterialGraphFunction::SpritePointIrradiance,
        E::Fresnel { .. } => MaterialGraphFunction::Fresnel,
        E::RadialMask { .. } => MaterialGraphFunction::RadialMask,
        E::Dissolve { .. } => MaterialGraphFunction::Dissolve,
        E::DissolveEdge { .. } => MaterialGraphFunction::DissolveEdge,
        E::DepthFade { .. } => MaterialGraphFunction::DepthFade,
        E::SoftParticle { .. } => MaterialGraphFunction::SoftParticle,
        E::PanUv { .. } => MaterialGraphFunction::PanUv,
        E::RotateUv { .. } => MaterialGraphFunction::RotateUv,
        E::ScaleUv { .. } => MaterialGraphFunction::ScaleUv,
        E::DerivativeX { .. } => MaterialGraphFunction::DerivativeX,
        E::DerivativeY { .. } => MaterialGraphFunction::DerivativeY,
        E::SampleTexture { .. } => MaterialGraphFunction::SampleTexture,
        E::SampleTextureLevel { .. } => MaterialGraphFunction::SampleTextureLevel,
        E::SampleTextureGradient { .. } => MaterialGraphFunction::SampleTextureGradient,
        E::NearestPaletteColor { .. } => MaterialGraphFunction::NearestPaletteColor,
        E::PaletteRamp { .. } => MaterialGraphFunction::PaletteRamp,
        E::PaletteShift { .. } => MaterialGraphFunction::PaletteShift,
        E::PaletteSwap { .. } => MaterialGraphFunction::PaletteSwap,
        E::BayerDither4x4 { .. } => MaterialGraphFunction::BayerDither4x4,
        E::BayerDither2x2 { .. } => MaterialGraphFunction::BayerDither2x2,
        E::BayerDither8x8 { .. } => MaterialGraphFunction::BayerDither8x8,
        E::BlueNoiseDither { .. } => MaterialGraphFunction::BlueNoiseDither,
        E::PatternDither { .. } => MaterialGraphFunction::PatternDither,
        E::PixelSnap { .. } => MaterialGraphFunction::PixelSnap,
        E::Posterize { .. } => MaterialGraphFunction::Posterize,
        E::PixelateUv { .. } => MaterialGraphFunction::PixelateUv,
        E::Quantize { .. } => MaterialGraphFunction::Quantize,
        E::Threshold { .. } => MaterialGraphFunction::Threshold,
        E::PixelOutline { .. } => MaterialGraphFunction::PixelOutline,
        E::ExtractComponent { .. } => MaterialGraphFunction::ExtractComponent,
    };
    MaterialGraphNodeKind::Function(function)
}

fn node_label(
    kind: &MaterialExpressionKind,
    program: &MaterialProgram,
    functions: &MaterialFunctionLibrary,
) -> String {
    match kind {
        MaterialExpressionKind::Constant(value) => format!("Constant · {value:?}"),
        MaterialExpressionKind::NormalMap { .. } => "Normal Map".to_owned(),
        MaterialExpressionKind::SpritePointIrradiance { .. } => {
            "Sprite Point Irradiance".to_owned()
        }
        MaterialExpressionKind::Input(input) => format!("{input:?}"),
        MaterialExpressionKind::Parameter(id) => program
            .parameters
            .iter()
            .find(|parameter| parameter.id == *id)
            .map(|parameter| parameter.name.clone())
            .unwrap_or_else(|| format!("Parameter {id}")),
        MaterialExpressionKind::FunctionInput(id) => format!("Function input {id}"),
        MaterialExpressionKind::FunctionCall { function, .. } => functions
            .get(*function)
            .map(|function| function.name.clone())
            .unwrap_or_else(|| format!("Missing function {}", function.id())),
        MaterialExpressionKind::CustomWeslCall { entry_point, .. } => entry_point.clone(),
        MaterialExpressionKind::ExtractComponent { component, .. } => {
            format!("Extract {component:?}")
        }
        _ => format!(
            "{:?}",
            match node_kind(kind) {
                MaterialGraphNodeKind::Function(function) => function,
                _ => unreachable!(),
            }
        ),
    }
}

fn expression_ports(
    kind: &MaterialExpressionKind,
    functions: &MaterialFunctionLibrary,
) -> Vec<(
    String,
    MaterialExpressionId,
    Option<MaterialFunctionInputId>,
)> {
    if let MaterialExpressionKind::FunctionCall {
        function,
        arguments,
        ..
    } = kind
    {
        if let Some(function) = functions.get(*function) {
            return function
                .inputs
                .iter()
                .filter_map(|input| {
                    arguments
                        .get(&input.id)
                        .copied()
                        .map(|source| (input.name.clone(), source, Some(input.id)))
                })
                .collect();
        }
        return arguments
            .iter()
            .map(|(&input, &source)| (format!("Missing input {input}"), source, Some(input)))
            .collect();
    }
    expression_inputs(kind)
        .into_iter()
        .map(|(name, source)| (name.to_owned(), source, None))
        .chain(
            matches!(
                kind,
                MaterialExpressionKind::PixelOutline {
                    frame_bounds: None,
                    ..
                }
            )
            .then(|| {
                (
                    "frame_bounds".into(),
                    MaterialExpressionId::from_u128(0),
                    None,
                )
            }),
        )
        .collect()
}

pub(crate) fn expression_inputs(
    kind: &MaterialExpressionKind,
) -> Vec<(&'static str, MaterialExpressionId)> {
    use MaterialExpressionKind as E;
    match kind {
        E::Constant(_) | E::Input(_) | E::Parameter(_) | E::FunctionInput(_) => Vec::new(),
        E::FunctionCall { arguments, .. } => arguments
            .values()
            .copied()
            .map(|expression| ("argument", expression))
            .collect(),
        E::CustomWeslCall { arguments, .. } => arguments
            .iter()
            .map(|argument| ("argument", argument.expression))
            .collect(),
        E::Add(left, right)
        | E::Subtract(left, right)
        | E::Multiply(left, right)
        | E::Divide(left, right) => {
            vec![("left", *left), ("right", *right)]
        }
        E::Lerp { start, end, factor } => {
            vec![("start", *start), ("end", *end), ("factor", *factor)]
        }
        E::Clamp { value, min, max } => vec![("value", *value), ("min", *min), ("max", *max)],
        E::Select {
            condition,
            if_false,
            if_true,
        } => vec![
            ("condition", *condition),
            ("false", *if_false),
            ("true", *if_true),
        ],
        E::Remap {
            value,
            input_min,
            input_max,
            output_min,
            output_max,
        } => vec![
            ("value", *value),
            ("input_min", *input_min),
            ("input_max", *input_max),
            ("output_min", *output_min),
            ("output_max", *output_max),
        ],
        E::Smoothstep {
            edge_min,
            edge_max,
            value,
        } => vec![
            ("edge_min", *edge_min),
            ("edge_max", *edge_max),
            ("value", *value),
        ],
        E::NormalMap {
            sample,
            strength,
            flip_y,
            normal,
            tangent,
            bitangent,
        } => vec![
            ("sample", *sample),
            ("strength", *strength),
            ("flip_y", *flip_y),
            ("normal", *normal),
            ("tangent", *tangent),
            ("bitangent", *bitangent),
        ],
        E::SpritePointIrradiance { normal } => vec![("normal", *normal)],
        E::Fresnel {
            normal,
            view,
            power,
        } => vec![("normal", *normal), ("view", *view), ("power", *power)],
        E::RadialMask {
            uv,
            center,
            radius,
            softness,
            invert,
        } => vec![
            ("uv", *uv),
            ("center", *center),
            ("radius", *radius),
            ("softness", *softness),
            ("invert", *invert),
        ],
        E::Dissolve {
            source,
            threshold,
            edge_width,
            invert,
        }
        | E::DissolveEdge {
            source,
            threshold,
            edge_width,
            invert,
        } => vec![
            ("source", *source),
            ("threshold", *threshold),
            ("edge_width", *edge_width),
            ("invert", *invert),
        ],
        E::DepthFade {
            scene_depth,
            pixel_depth,
            fade_distance,
            invert,
        } => vec![
            ("scene_depth", *scene_depth),
            ("pixel_depth", *pixel_depth),
            ("fade_distance", *fade_distance),
            ("invert", *invert),
        ],
        E::SoftParticle {
            alpha,
            scene_depth,
            pixel_depth,
            fade_distance,
            invert,
        } => vec![
            ("alpha", *alpha),
            ("scene_depth", *scene_depth),
            ("pixel_depth", *pixel_depth),
            ("fade_distance", *fade_distance),
            ("invert", *invert),
        ],
        E::PanUv { uv, speed, time } => vec![("uv", *uv), ("speed", *speed), ("time", *time)],
        E::RotateUv { uv, center, angle } => {
            vec![("uv", *uv), ("center", *center), ("angle", *angle)]
        }
        E::ScaleUv { uv, center, scale } => {
            vec![("uv", *uv), ("center", *center), ("scale", *scale)]
        }
        E::DerivativeX { value } | E::DerivativeY { value } => vec![("value", *value)],
        E::SampleTexture { texture, uv } => vec![("texture", *texture), ("uv", *uv)],
        E::SampleTextureLevel { texture, uv, level } => {
            vec![("texture", *texture), ("uv", *uv), ("level", *level)]
        }
        E::SampleTextureGradient {
            texture,
            uv,
            ddx,
            ddy,
        } => vec![
            ("texture", *texture),
            ("uv", *uv),
            ("ddx", *ddx),
            ("ddy", *ddy),
        ],
        E::NearestPaletteColor { palette, color } => vec![("palette", *palette), ("color", *color)],
        E::PaletteRamp { palette, factor } => vec![("palette", *palette), ("factor", *factor)],
        E::PaletteShift {
            palette,
            color,
            steps,
        } => vec![("palette", *palette), ("color", *color), ("steps", *steps)],
        E::PaletteSwap {
            source_palette,
            target_palette,
            color,
        } => vec![
            ("source_palette", *source_palette),
            ("target_palette", *target_palette),
            ("color", *color),
        ],
        E::PixelSnap {
            position,
            cell_size,
        } => vec![("position", *position), ("cell_size", *cell_size)],
        E::Posterize { color, levels } => vec![("color", *color), ("levels", *levels)],
        E::PixelateUv { uv, cells } => vec![("uv", *uv), ("cells", *cells)],
        E::Quantize { value, step } => vec![("value", *value), ("step", *step)],
        E::Threshold { value, threshold } => vec![("value", *value), ("threshold", *threshold)],
        E::PixelOutline {
            texture,
            uv,
            width,
            threshold,
            frame_bounds,
        } => [
            ("texture", *texture),
            ("uv", *uv),
            ("width", *width),
            ("threshold", *threshold),
        ]
        .into_iter()
        .chain(frame_bounds.map(|bounds| ("frame_bounds", bounds)))
        .collect(),
        E::PatternDither {
            texture,
            alpha,
            position,
            cell_size,
        } => vec![
            ("texture", *texture),
            ("alpha", *alpha),
            ("position", *position),
            ("cell_size", *cell_size),
        ],
        E::BayerDither2x2 {
            alpha,
            position,
            cell_size,
        }
        | E::BayerDither8x8 {
            alpha,
            position,
            cell_size,
        }
        | E::BlueNoiseDither {
            alpha,
            position,
            cell_size,
        }
        | E::BayerDither4x4 {
            alpha,
            position,
            cell_size,
        } => vec![
            ("alpha", *alpha),
            ("position", *position),
            ("cell_size", *cell_size),
        ],
        E::ExtractComponent { value, .. } => vec![("value", *value)],
    }
}
